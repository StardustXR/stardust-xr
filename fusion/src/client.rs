//! Your connection to the Stardust server and other essentials.

use crate::node::{Node, NodeType};
use crate::spatial::Spatial;
use crate::{node::NodeError, scenegraph::Scenegraph};

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use stardust_xr::schemas::flex::flexbuffers::DeserializationError;
use stardust_xr::{
	client,
	messenger::{self, MessengerError},
	messenger::{MessageReceiver, MessageSender, MessageSenderHandle},
	schemas::flex::{deserialize, serialize},
};
use std::any::TypeId;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Weak};
use thiserror::Error;
use tokio::net::UnixStream;
use tokio::sync::{watch, Notify, OnceCell};
use tokio::task::JoinHandle;

/// The persistent state of a Stardust client.
#[derive(Debug, Default)]
pub struct ClientState {
	/// Data specific to your client, put anything you like here and it'll be saved/restored intact.
	pub data: Option<Vec<u8>>,
	/// The root node of this client.
	pub root: Option<Spatial>,
	/// Spatials that will be in the same place you left them.
	pub spatial_anchors: FxHashMap<String, Spatial>,
}
impl ClientState {
	fn to_internal(&self) -> ClientStateInternal {
		ClientStateInternal {
			data: self.data.clone(),
			root: self
				.root
				.as_ref()
				.map(|r| &r.node)
				.map(Node::get_path)
				.map(Result::ok)
				.flatten(),
			spatial_anchors: self
				.spatial_anchors
				.iter()
				.filter_map(|(k, v)| Some((k.clone(), v.node.get_path().ok()?)))
				.collect(),
		}
	}
}

#[derive(Default, Serialize, Deserialize)]
struct ClientStateInternal {
	data: Option<Vec<u8>>,
	root: Option<String>,
	spatial_anchors: FxHashMap<String, String>,
}

/// Information on the frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameInfo {
	/// The time between this frame and last frame's display time, in seconds.
	pub delta: f64,
	/// The total time in seconds the client has been connected to the server.
	pub elapsed: f64,
}

/// Handle the events that apply to the whole client.
#[crate::handler]
pub trait RootHandler: Send + Sync + 'static {
	/// Runs every frame with information about the current frame, for animations and motion and a consistent update.
	async fn frame(&mut self, info: FrameInfo);
	/// The server needs your client to save its state so that it can be restored (through Client::get_state) on relaunch. This may happen for any reason.
	///
	/// Client root transform is always saved.
	fn save_state(&mut self) -> ClientState;
}
struct DummyHandler;
#[crate::handler]
impl RootHandler for DummyHandler {
	async fn frame(&mut self, _info: FrameInfo) {}
	fn save_state(&mut self) -> ClientState {
		ClientState::default()
	}
}

#[derive(Error, Debug)]
pub enum ClientError {
	#[error("Could not connect to the stardust server")]
	ConnectionFailure,
	#[error("Node error: {0}")]
	NodeError(NodeError),
}
impl From<NodeError> for ClientError {
	fn from(e: NodeError) -> Self {
		ClientError::NodeError(e)
	}
}
impl From<MessengerError> for ClientError {
	fn from(e: MessengerError) -> Self {
		ClientError::NodeError(NodeError::MessengerError { e })
	}
}
impl From<String> for ClientError {
	fn from(e: String) -> Self {
		ClientError::NodeError(NodeError::ReturnedError { e })
	}
}
impl From<DeserializationError> for ClientError {
	fn from(e: DeserializationError) -> Self {
		ClientError::NodeError(NodeError::Deserialization { e })
	}
}

/// Your connection to the Stardust server.
pub struct Client {
	/// A handle to the messenger, allows you to send messages to nodes on the server.
	pub message_sender_handle: MessageSenderHandle,
	/// A reference to the scenegraph.
	pub scenegraph: Arc<Scenegraph>,

	stop_notifier: Notify,

	root: OnceCell<Arc<Spatial>>,
	hmd: OnceCell<Spatial>,
	pub(crate) registered_item_uis: Mutex<FxHashSet<TypeId>>,

	elapsed_time: Mutex<f64>,
	life_cycle_handler: Mutex<Weak<tokio::sync::Mutex<dyn RootHandler>>>,

	state: watch::Receiver<ClientState>,
}

impl Client {
	/// Try to connect to the server, return messenger halves for manually setting up the event loop.
	pub async fn connect() -> Result<(Arc<Self>, MessageSender, MessageReceiver), ClientError> {
		let connection = client::connect()
			.await
			.map_err(|_| ClientError::ConnectionFailure)?;
		Client::from_connection(connection).await
	}

	/// Create a client and messenger halves from an established tokio async `UnixStream` for manually setting up the event loop.
	pub async fn from_connection(
		connection: UnixStream,
	) -> Result<(Arc<Self>, MessageSender, MessageReceiver), ClientError> {
		let (message_tx, message_rx) = messenger::create(connection);
		let (state_tx, state_rx) = watch::channel(ClientState::default());
		let client = Arc::new(Client {
			scenegraph: Arc::new(Scenegraph::new()),
			message_sender_handle: message_tx.handle(),

			stop_notifier: Default::default(),

			root: OnceCell::new(),
			hmd: OnceCell::new(),
			registered_item_uis: Mutex::new(FxHashSet::default()),

			elapsed_time: Mutex::new(0.0),
			life_cycle_handler: Mutex::new(Weak::<tokio::sync::Mutex<DummyHandler>>::new()),

			state: state_rx,
		});
		let _ = client
			.root
			.set(Arc::new(Spatial::from_path(&client, "", "", false)));
		let _ = client
			.hmd
			.set(Spatial::from_path(&client, "", "hmd", false));
		client.get_root().node.add_local_signal("restore_state", {
			let client = client.clone();

			move |data, _| {
				let state: ClientStateInternal = deserialize(&data)?;
				let _ = state_tx.send(ClientState {
					data: state.data,
					root: Some(client.get_root().alias()),
					spatial_anchors: state
						.spatial_anchors
						.into_iter()
						.map(|(k, v)| (k, Spatial::from_path(&client, "/spatial/anchor", v, true)))
						.collect(),
				});
				Ok(())
			}
		})?;
		client.get_root().node.add_local_signal("frame", {
			let client = client.clone();
			move |data, _| {
				if let Some(handler) = client.life_cycle_handler.lock().upgrade() {
					#[derive(Deserialize)]
					struct LogicStepInfoInternal {
						delta: f64,
					}

					let info_internal: LogicStepInfoInternal = deserialize(&data)?;
					let mut elapsed = client.elapsed_time.lock();
					(*elapsed) += info_internal.delta;
					let info = FrameInfo {
						delta: info_internal.delta,
						elapsed: *elapsed,
					};
					tokio::spawn(async move {
						handler.lock().await.frame(info).await;
					});
				}
				Ok(())
			}
		})?;
		client.get_root().node.add_local_method("save_state", {
			let client = client.clone();
			move |_, _| {
				let state = client
					.life_cycle_handler
					.lock()
					.upgrade()
					.map(|h| h.blocking_lock().save_state())
					.as_ref()
					.map(ClientState::to_internal)
					.unwrap_or_default();
				Ok(serialize(state).map(|v| (v, vec![]))?)
			}
		})?;

		Ok((client, message_tx, message_rx))
	}

	/// Automatically set up the client with an async loop. This option is generally what you'll want to use.
	pub async fn connect_with_async_loop(
	) -> Result<(Arc<Self>, JoinHandle<Result<(), MessengerError>>), ClientError> {
		let (client, mut message_tx, mut message_rx) = Client::connect().await?;

		let event_loop = tokio::task::spawn({
			let client = client.clone();
			let scenegraph = client.scenegraph.clone();
			async move {
				let dispatch_loop = async move {
					loop {
						match message_rx.dispatch(&*scenegraph).await {
							Ok(_) => continue,
							Err(e) => break e,
						}
					}
				};
				let flush_loop = async move {
					loop {
						match message_tx.flush().await {
							Ok(_) => continue,
							Err(e) => break e,
						}
					}
				};

				tokio::select! {
					_ = client.stop_notifier.notified() => Ok(()),
					e = dispatch_loop => Err(e),
					e = flush_loop => Err(e),
				}
			}
		});

		client
			.state
			.clone()
			.changed()
			.await
			.map_err(|_| ClientError::ConnectionFailure)?;
		Ok((client, event_loop))
	}

	/// Get a reference to the client's root node, a spatial that exists where the client was spawned.
	pub fn get_root(&self) -> &Spatial {
		self.root.get().as_ref().unwrap()
	}
	/// Get a reference to the head mounted display's spatial.
	pub fn get_hmd(&self) -> &Spatial {
		self.hmd.get().as_ref().unwrap()
	}

	/// Wrap the root in a handler and return an `Arc` to the handler.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler, and you want to check for errors too"]
	pub fn wrap_root<H: RootHandler>(
		&self,
		wrapped: H,
	) -> Result<Arc<tokio::sync::Mutex<H>>, NodeError> {
		self.get_root()
			.node
			.send_remote_signal_raw("subscribe_frame", &[], Vec::new())?;
		let wrapped = Arc::new(tokio::sync::Mutex::new(wrapped));
		*self.life_cycle_handler.lock() =
			Arc::downgrade(&(wrapped.clone() as Arc<tokio::sync::Mutex<dyn RootHandler>>));
		Ok(wrapped)
	}
	/// Wrap the root in an already wrapped handler
	pub fn wrap_root_raw<H: RootHandler>(
		&self,
		wrapped: &Arc<tokio::sync::Mutex<H>>,
	) -> Result<(), NodeError> {
		self.get_root()
			.node
			.send_remote_signal_raw("subscribe_frame", &[], Vec::new())?;
		*self.life_cycle_handler.lock() =
			Arc::downgrade(&(wrapped.clone() as Arc<tokio::sync::Mutex<dyn RootHandler>>));
		Ok(())
	}

	/// Set the prefixes for any `NamespacedResource`s.
	pub fn set_base_prefixes<H: AsRef<Path>>(&self, prefixes: &[H]) {
		let env_prefixes = option_env!("STARDUST_RES_PREFIXES");

		let prefix_vec: Vec<&Path> = if let Some(env_prefixes) = env_prefixes {
			env_prefixes.split(":").map(|p| Path::new(p)).collect()
		} else {
			prefixes
				.iter()
				.map(|p| p.as_ref())
				.filter(|p| p.is_absolute() && p.exists())
				.collect()
		};

		self.message_sender_handle
			.signal(
				"/",
				"set_base_prefixes",
				&serialize(prefix_vec).unwrap(),
				Vec::new(),
			)
			.unwrap();
	}

	pub fn get_connection_environment(
		&self,
	) -> Result<impl Future<Output = Result<FxHashMap<String, String>, NodeError>>, NodeError> {
		let future = self
			.message_sender_handle
			.method("/", "get_connection_environment", &[], Vec::new())
			.map_err(|e| NodeError::MessengerError { e })?;

		Ok(async move {
			let result = future.await.map_err(|e| NodeError::ReturnedError { e })?;
			deserialize(&result.into_message()).map_err(|e| NodeError::Deserialization { e })
		})
	}

	/// Generate a client state token and return it back.
	///
	/// When launching a new client, set the environment variable `STARDUST_STARTUP_TOKEN` to the returned string.
	/// Make sure the environment variable shows in `/proc/{pid}/environ` as that's the only reliable way to pass the value to the server (suggestions welcome).
	///
	/// # Example
	/// ```
	/// let state_token = client.state_token(ClientState::default()).unwrap().await.unwrap();
	///
	/// std::env::set_var("STARDUST_STARTUP_TOKEN", state_token); // to make sure it ends up in `/proc/{pid}/environ` of the new process
	/// nix::unistd::execvp(ustr(&program).as_cstr(), &args).unwrap(); // make sure to use execv here to get the environment variable there properly.
	/// ```
	pub fn state_token(
		&self,
		state: &ClientState,
	) -> Result<impl Future<Output = Result<String, NodeError>>, NodeError> {
		self.get_root()
			.node
			.execute_remote_method("state_token", &state.to_internal())
	}

	/// Stop the event loop if created with async loop. Equivalent to a graceful disconnect.
	pub fn stop_loop(&self) {
		self.stop_notifier.notify_one();
	}
}
impl Drop for Client {
	fn drop(&mut self) {
		self.stop_loop();
		let _ = self
			.message_sender_handle
			.signal("/", "disconnect", &[0_u8; 0], Vec::new());
	}
}

#[tokio::test]
async fn fusion_client_connect() {
	color_eyre::install().unwrap();
	let (_client, event_loop) = Client::connect_with_async_loop().await.unwrap();

	tokio::select! {
		biased;
		_ = tokio::time::sleep(core::time::Duration::from_secs(1)) => (),
		e = event_loop => e.unwrap().unwrap(),
	}
}

#[tokio::test]
async fn fusion_client_life_cycle() {
	color_eyre::install().unwrap();
	let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();

	struct RootHandlerDummy(Arc<Client>);
	#[crate::handler]
	impl RootHandler for RootHandlerDummy {
		async fn frame(&mut self, _info: FrameInfo) {
			self.0.stop_loop();
		}
		fn save_state(&mut self) -> ClientState {
			ClientState::default()
		}
	}

	let _wrapper = client.wrap_root(RootHandlerDummy(client.clone())).unwrap();

	tokio::select! {
		biased;
		_ = tokio::time::sleep(core::time::Duration::from_secs(5)) => panic!("Timed Out"),
		e = event_loop => e.unwrap().unwrap(),
	};
}
