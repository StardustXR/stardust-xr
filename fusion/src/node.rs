//! The base of all objects in Stardust.

use super::client::Client;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use serde::{de::DeserializeOwned, Serialize, Serializer};
use stardust_xr::{
	messenger::{Message, MessengerError},
	schemas::flex::{deserialize, flexbuffers::DeserializationError, serialize},
};
use std::{
	fmt::Debug,
	future::Future,
	os::fd::OwnedFd,
	pin::Pin,
	sync::{Arc, Weak},
	vec::Vec,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NodeError {
	#[error("client has been dropped")]
	ClientDropped,
	#[error("Messenger error: {e}")]
	MessengerError { e: MessengerError },
	#[error("Node does not exist anymore")]
	DoesNotExist,
	#[error("Node's signal/method isn't available because it is an alias node")]
	NotAliased,
	#[error("invalid path")]
	InvalidPath,
	#[error("Serialization failed")]
	Serialization,
	#[error("Deserialization failed with an error: {e}")]
	Deserialization { e: DeserializationError },
	/// The server returned an error on a method return.
	#[error("Server returned an error: {e}")]
	ReturnedError { e: String },
	#[error("Attempted to register to a singleton twice")]
	OverrideSingleton,
	/// The given data is not a valid flexbuffer map.
	#[error("Map is not a valid flexbuffer map at the root")]
	MapInvalid,
}

/// Common methods all nodes share, to make them easier to use.
// #[enum_dispatch(FieldType)]
pub trait NodeType: Send + Sync + 'static {
	/// Get a reference to the node.
	fn node(&self) -> &Node;
	/// Try to get the client
	fn client(&self) -> Result<Arc<Client>, NodeError> {
		self.node().client()
	}
	/// Set whether the node is active or not. This has different effects depending on the node.
	fn set_enabled(&self, enabled: bool) -> Result<(), NodeError> {
		self.node().send_remote_signal("set_enabled", &enabled)
	}
	/// Create an alias of this node.
	/// Not the same as node scenegraph aliases,
	/// they are useful instead for getting a weak handle to a node.
	/// If the original node is destroyed, then any messages to the server will fail instantly with `NodeError::DoesNotExist`.
	fn alias(&self) -> Self
	where
		Self: Sized;
}
/// A trait to ensure this node type could be put in a `HandlerWrapper`.
pub trait HandledNodeType: NodeType {}

type Signal = dyn Fn(Vec<u8>, Vec<OwnedFd>) -> color_eyre::eyre::Result<()> + Send + Sync + 'static;
type Method = dyn Fn(Vec<u8>, Vec<OwnedFd>) -> color_eyre::eyre::Result<(Vec<u8>, Vec<OwnedFd>)>
	+ Send
	+ Sync
	+ 'static;

pub type BoxedFuture<O> = Pin<Box<dyn Future<Output = O> + Send + Sync>>;

pub struct NodeInternals {
	client: Weak<Client>,
	self_ref: Weak<NodeInternals>,
	parent: String,
	name: String,
	pub(crate) local_signals: Mutex<FxHashMap<String, Arc<Signal>>>,
	pub(crate) local_methods: Mutex<FxHashMap<String, Arc<Method>>>,
	pub(crate) destroyable: bool,
}
impl NodeInternals {
	pub(crate) fn path(&self) -> String {
		self.parent.clone() + "/" + &self.name
	}
}
impl Serialize for NodeInternals {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		self.path().serialize(serializer)
	}
}

impl Drop for NodeInternals {
	fn drop(&mut self) {
		if let Some(client) = self.client.upgrade() {
			let path = self.path();
			if self.destroyable {
				let _ = client
					.message_sender_handle
					.signal(&path, "destroy", &[], Vec::new());
			}
			client.scenegraph.remove_node(&path);
		}
	}
}

/// An object in the client's scenegraph on the server. Almost all calls to a node are IPC calls and so have several microseconds of delay, be aware.
pub enum Node {
	Owned(Arc<NodeInternals>),
	Aliased(Weak<NodeInternals>),
}
impl Node {
	pub(crate) fn new<'a, S: Serialize>(
		client: &Arc<Client>,
		interface_path: &'a str,
		interface_method: &'a str,
		parent_path: &'a str,
		destroyable: bool,
		id: &str,
		data: S,
	) -> Result<Node, NodeError> {
		let node = Node::from_path(client, parent_path, id, destroyable);

		client
			.message_sender_handle
			.signal(
				interface_path,
				interface_method,
				&serialize(data).map_err(|_| NodeError::Serialization)?,
				Vec::new(),
			)
			.map_err(|e| NodeError::MessengerError { e })?;

		Ok(node)
	}
	/// Create a node from path, this is only needed when fusion does not have a proper node struct.
	pub fn from_path(
		client: &Arc<Client>,
		parent: impl ToString,
		name: impl ToString,
		destroyable: bool,
	) -> Node {
		let node = Arc::new_cyclic(|self_ref| NodeInternals {
			client: Arc::downgrade(client),
			self_ref: self_ref.clone(),
			parent: parent.to_string(),
			name: name.to_string(),
			local_signals: Mutex::new(FxHashMap::default()),
			local_methods: Mutex::new(FxHashMap::default()),
			destroyable,
		});
		client.scenegraph.add_node(&node);
		Node::Owned(node)
	}

	/// Add a signal to the node so that the server can send a message to it. Not needed unless implementing functionality Fusion does not already have.
	pub fn add_local_signal<F>(&self, name: impl ToString, signal: F) -> Result<(), NodeError>
	where
		F: Fn(Vec<u8>, Vec<OwnedFd>) -> color_eyre::eyre::Result<()> + Send + Sync + 'static,
	{
		self.internals()?
			.local_signals
			.lock()
			.insert(name.to_string(), Arc::new(signal));
		Ok(())
	}

	/// Add a signal to the node so that the server can send a message to it and get a response back. Not needed unless implementing functionality Fusion does not already have.
	pub fn add_local_method<F>(&self, name: impl ToString, method: F) -> Result<(), NodeError>
	where
		F: Fn(Vec<u8>, Vec<OwnedFd>) -> color_eyre::eyre::Result<(Vec<u8>, Vec<OwnedFd>)>
			+ Send
			+ Sync
			+ 'static,
	{
		self.internals()?
			.local_methods
			.lock()
			.insert(name.to_string(), Arc::new(method));
		Ok(())
	}

	pub(crate) fn internals(&self) -> Result<Arc<NodeInternals>, NodeError> {
		match self {
			Node::Owned(node) => Ok(node.clone()),
			Node::Aliased(node) => node.upgrade().ok_or(NodeError::DoesNotExist),
		}
	}

	/// Try to get the client from the node, it's a result because that makes it work a lot better with `?` in internal functions.
	pub fn client(&self) -> Result<Arc<Client>, NodeError> {
		self.internals()?
			.client
			.upgrade()
			.ok_or(NodeError::ClientDropped)
	}

	pub fn get_name(&self) -> Result<String, NodeError> {
		Ok(self.internals()?.parent.to_string())
	}
	/// Get the entire path of the node including the name.
	pub fn get_path(&self) -> Result<String, NodeError> {
		Ok(self.internals()?.path())
	}

	/// Check if this node is still alive.
	pub fn alive(&self) -> bool {
		match self {
			Node::Owned(_) => true,
			Node::Aliased(a) => a.strong_count() > 0,
		}
	}

	/// Send a signal to the node on the server. Not needed unless implementing functionality Fusion does not already have.
	pub fn send_remote_signal<S: Serialize>(
		&self,
		signal_name: &str,
		data: &S,
	) -> Result<(), NodeError> {
		self.send_remote_signal_raw(
			signal_name,
			&serialize(data).map_err(|_| NodeError::Serialization)?,
			Vec::new(),
		)
	}
	/// Send a signal to the node on the server with raw data (like when sending flatbuffers over). Not needed unless implementing functionality Fusion does not already have.
	pub fn send_remote_signal_raw(
		&self,
		signal_name: &str,
		data: &[u8],
		fds: Vec<OwnedFd>,
	) -> Result<(), NodeError> {
		self.client()?
			.message_sender_handle
			.signal(&self.get_path()?, signal_name, data, fds)
			.map_err(|e| NodeError::MessengerError { e })
	}
	/// Execute a method on the node on the server. Not needed unless implementing functionality Fusion does not already have.
	pub fn execute_remote_method<S: Serialize, D: DeserializeOwned>(
		&self,
		method_name: &str,
		send_data: &S,
	) -> Result<impl Future<Output = Result<D, NodeError>>, NodeError> {
		let send_data = serialize(send_data).map_err(|_| NodeError::Serialization)?;
		let future = self.execute_remote_method_raw(method_name, &send_data, Vec::new())?;
		Ok(async move {
			future.await.and_then(|data| {
				deserialize(&data.into_message()).map_err(|e| NodeError::Deserialization { e })
			})
		})
	}
	/// Execute a method on the node on the server. This will return a trait safe future, pinned and boxed. Not needed unless implementing functionality Fusion does not already have.
	pub fn execute_remote_method_trait<S: Serialize, D: DeserializeOwned>(
		&self,
		method_name: &str,
		send_data: &S,
	) -> Result<Pin<Box<dyn Future<Output = Result<D, NodeError>> + Send + Sync>>, NodeError> {
		let send_data = serialize(send_data).map_err(|_| NodeError::Serialization)?;
		let future = self.execute_remote_method_raw(method_name, &send_data, Vec::new())?;
		Ok(Box::pin(async move {
			future.await.and_then(|data| {
				deserialize(&data.into_message()).map_err(|e| NodeError::Deserialization { e })
			})
		}))
	}
	/// Execute a method on the node on the server with raw data (like when sending over flatbuffers). Not needed unless implementing functionality Fusion does not already have.
	pub fn execute_remote_method_raw(
		&self,
		method_name: &str,
		data: &[u8],
		fds: Vec<OwnedFd>,
	) -> Result<impl Future<Output = Result<Message, NodeError>>, NodeError> {
		let future = self
			.client()?
			.message_sender_handle
			.method(&self.get_path()?, method_name, data, fds)
			.map_err(|e| NodeError::MessengerError { e })?;

		Ok(async move { future.await.map_err(|e| NodeError::ReturnedError { e }) })
	}
	fn set_enabled(&self, enabled: bool) -> Result<(), NodeError> {
		self.send_remote_signal("set_enabled", &enabled)
	}
}
impl NodeType for Node {
	fn node(&self) -> &Node {
		self
	}

	fn alias(&self) -> Self {
		match self {
			Node::Owned(internals) => Node::Aliased(Arc::downgrade(internals)),
			Node::Aliased(internals) => Node::Aliased(internals.clone()),
		}
	}
}

impl Debug for Node {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut dbg = f.debug_struct("Node");

		if let Ok(internals) = self.internals() {
			dbg.field("path", &internals.path())
				.field(
					"local_signals",
					&internals
						.local_signals
						.lock()
						.iter()
						.map(|(key, _)| key)
						.collect::<Vec<_>>(),
				)
				.field(
					"local_methods",
					&internals
						.local_methods
						.lock()
						.iter()
						.map(|(key, _)| key)
						.collect::<Vec<_>>(),
				);
		} else {
			dbg.field("node", &"broken");
		}

		dbg.finish()
	}
}
