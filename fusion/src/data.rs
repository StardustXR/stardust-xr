//! Module containing pulse senders/receivers, the way to send non-spatial data through 3D space.
//!
//! Uses include:
//! - Keyboard (virtual or physical) events
//! - Controller inputs
//! - Hardware mouse/trackball events (when mapping it to 3D space and back is impractical)
//! - Actions such as copy/paste, duplicate, etc.
//! - Quit requests
//!
//! Pulse senders and receivers both have a mask, a set of keys and values (using flexbuffers maps) that are used to filter specific protocols of information.
//!
//! Pulse senders can see all the pulse receivers that match their mask (contain at least the same keys/values).
//! Each receiver has its own UID to identify it for "connecting" the sender to it visually or such.
//! Pulse senders can send any message that matches the receiver's mask (contain at least the same keys/values).
//!
//! Pulse receivers have an attached field that can be used to make pulse senders aware of their bounds better, such as a panel with a box field and a pulse receiver for keyboard input.
//! The position/rotation of pulse receivers should be the exact point a visual indicator of connection would connect to, and the forward direction should be away from the body it's part of design-wise.
//! Pulse receivers cannot see the pulse senders, but any time data is sent to them they get the UID of the sender to allow keymap switching or such.

use crate::{
	fields::{Field, UnknownField},
	node::{HandledNodeType, NodeError},
	node::{Node, NodeType},
	spatial::Spatial,
	HandlerWrapper,
};
use mint::{Quaternion, Vector3};
use serde::Deserialize;
use stardust_xr::{
	schemas::flex::{deserialize_owned, flexbuffers},
	values::Transform,
};
use std::{ops::Deref, os::fd::OwnedFd, sync::Arc};
use tokio::sync::Mutex;

#[cfg(feature = "keymap")]
use crate::client::Client;
#[cfg(feature = "keymap")]
use stardust_xr::schemas::flex::serialize;
#[cfg(feature = "keymap")]
use std::future::Future;
#[cfg(feature = "keymap")]
use xkbcommon::xkb::{ffi::XKB_KEYMAP_FORMAT_TEXT_V1, Context, Keymap};

#[cfg(feature = "keymap")]
impl Client {
	pub fn register_keymap(
		&self,
		keymap: impl AsRef<str>,
	) -> Result<impl Future<Output = Result<String, NodeError>>, NodeError> {
		let test_keymap = Keymap::new_from_string(
			&Context::new(0),
			keymap.as_ref().to_string(),
			XKB_KEYMAP_FORMAT_TEXT_V1,
			0,
		);
		if test_keymap.is_none() {
			return Err(NodeError::ReturnedError {
				e: "Keymap is not valid".to_string(),
			});
		};
		let future = self
			.message_sender_handle
			.method(
				"/data",
				"register_keymap",
				&serialize(keymap.as_ref()).map_err(|_| NodeError::Serialization)?,
				Vec::new(),
			)
			.map_err(|e| NodeError::MessengerError { e })?;

		Ok(async move {
			let result = future.await.map_err(|e| NodeError::ReturnedError { e })?;
			deserialize(&result.into_message()).map_err(|e| NodeError::Deserialization { e })
		})
	}
	pub fn get_keymap_string(
		&self,
		keymap_id: &str,
	) -> Result<impl Future<Output = Result<String, NodeError>>, NodeError> {
		let future = self
			.message_sender_handle
			.method(
				"/data",
				"get_keymap",
				&serialize(keymap_id).map_err(|_| NodeError::Serialization)?,
				Vec::new(),
			)
			.map_err(|e| NodeError::MessengerError { e })?;

		Ok(async move {
			let result = future.await.map_err(|e| NodeError::ReturnedError { e })?;
			deserialize(&result.into_message()).map_err(|e| NodeError::Deserialization { e })
		})
	}
}

/// Trait for handling when pulse receivers matching the sender's mask are created/destroyed on the server.
#[crate::handler]
pub trait PulseSenderHandler: Send + Sync {
	async fn new_receiver(
		&mut self,
		info: NewReceiverInfo,
		receiver: PulseReceiver,
		field: UnknownField,
	);
	async fn drop_receiver(&mut self, uid: String);
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewReceiverInfo {
	pub uid: String,
	pub distance: f32,
	pub position: Vector3<f32>,
	pub rotation: Quaternion<f32>,
}

/// Node to send non-spatial data through 3D space.
///
/// # Example
/// ```
/// struct PulseSenderTest {
/// 	data: Vec<u8>,
/// 	node: PulseSender,
/// }
/// impl PulseSenderHandler for PulseSenderTest {
/// 	fn new_receiver(
/// 		&mut self,
/// 		info: NewReceiverInfo,
/// 		receiver: PulseReceiver,
/// 		field: UnknownField,
/// 	) {
/// 		println!(
/// 			"New pulse receiver {:?} with field {:?} and info {:?}",
/// 			receiver.node().get_path(),
/// 			field.node().get_path(),
/// 			info
/// 		);
/// 		self.node.send_data(&receiver, &self.data).unwrap();
/// 	}
/// 	fn drop_receiver(&mut self, uid: &str) {
/// 		println!("Pulse receiver {} dropped", uid);
/// 	}
/// }
///
/// let mask = {
/// 	let mut fbb = flexbuffers::Builder::default();
/// 	let mut map = fbb.start_map();
/// 	map.push("test", true);
/// 	map.end_map();
/// 	fbb.take_buffer()
/// };
///
/// let pulse_sender =
/// 	PulseSender::create(client.get_root(), None, None, &mask).unwrap();
/// let pulse_sender_handler = PulseSenderTest {
/// 	data: mask,
/// 	node: pulse_sender.alias(),
/// };
/// let _pulse_sender_handler = pulse_sender.wrap(pulse_sender_handler).unwrap();
/// ```
#[derive(Debug)]
pub struct PulseSender {
	spatial: Spatial,
}
impl PulseSender {
	/// Create a pulse receiver node. The mask must be a flexbuffers serialized map at its root.
	pub fn create(
		spatial_parent: &Spatial,
		transform: Transform,
		mask: &[u8],
	) -> Result<PulseSender, NodeError> {
		flexbuffers::Reader::get_root(mask)
			.and_then(|f| f.get_map())
			.map_err(|_| NodeError::MapInvalid)?;
		let id = nanoid::nanoid!();
		Ok(PulseSender {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/data",
					"create_pulse_sender",
					"/data/sender",
					true,
					&id.clone(),
					(id, spatial_parent.node().get_path()?, transform, mask),
				)?,
			},
		})
	}

	fn handle_new_receiver<H: PulseSenderHandler + 'static>(
		sender: Arc<PulseSender>,
		handler: Arc<Mutex<H>>,
		data: Vec<u8>,
		_fds: Vec<OwnedFd>,
	) -> color_eyre::eyre::Result<()> {
		let client = sender.client()?;
		let info: NewReceiverInfo = deserialize_owned(data)?;
		let receiver = PulseReceiver {
			spatial: Spatial::from_path(&client, sender.node().get_path()?, &info.uid, false),
		};
		let field = UnknownField {
			spatial: Spatial::from_path(&client, receiver.node().get_path()?, "field", false),
		};
		tokio::task::spawn(async move {
			handler
				.lock()
				.await
				.new_receiver(info, receiver, field)
				.await;
		});
		Ok(())
	}

	fn handle_drop_receiver<H: PulseSenderHandler + 'static>(
		_sender: Arc<PulseSender>,
		handler: Arc<Mutex<H>>,
		data: Vec<u8>,
		_fds: Vec<OwnedFd>,
	) -> color_eyre::eyre::Result<()> {
		let uid: String = deserialize_owned(data)?;
		tokio::task::spawn(async move {
			handler.lock().await.drop_receiver(uid).await;
		});
		Ok(())
	}

	/// Send data to the receiver.
	/// This message must be a flexbuffers serialized map with all the keys/values from the receiver's mask present.
	/// You can use `stardust_xr_fusion::core::schemas::flex::flexbuffers::FlexbufferSerializer` with the flexbuffers crate feature `serialize_human_readable` to serialize structs into maps.
	pub fn send_data(&self, receiver: &PulseReceiver, data: &[u8]) -> Result<(), NodeError> {
		flexbuffers::Reader::get_root(data)
			.and_then(|f| f.get_map())
			.map_err(|_| NodeError::MapInvalid)?;

		self.node
			.send_remote_signal("send_data", &(receiver.node().get_path()?, data))
	}

	/// Wrap this node and a pulse sender handler struct into a `HandlerWrapper` struct. You can use `PulseSender::receivers()` instead.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: PulseSenderHandler>(
		self,
		handler: H,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}

	/// Wrap this node and a pulse sender handler struct into a `HandlerWrapper` struct. You can use `PulseSender::receivers()` instead.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap_raw<H: PulseSenderHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);
		handler_wrapper.add_handled_signal("new_receiver", Self::handle_new_receiver)?;
		handler_wrapper.add_handled_signal("drop_receiver", Self::handle_drop_receiver)?;
		Ok(handler_wrapper)
	}
}
impl NodeType for PulseSender {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		PulseSender {
			spatial: self.spatial.alias(),
		}
	}
}
impl HandledNodeType for PulseSender {}
impl std::ops::Deref for PulseSender {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

/// Trait for handling when data is sent to this pulse receiver.
#[crate::handler]
pub trait PulseReceiverHandler: Send + Sync {
	/// `data` and `data_reader` point to the same data, so feel free to use one or the other.
	async fn data(&mut self, uid: String, data: Vec<u8>);
}

/// Node to receive non-spatial data through 3D space.
///
/// # Example
/// ```
/// use stardust_xr_fusion::data::PulseReceiverHandler;
/// struct PulseReceiverTest;
/// impl PulseReceiverHandler for PulseReceiverTest {
/// 	fn data(&mut self, uid: &str, data: Vec<u8>) {
/// 		println!(
/// 			"Pulse sender {} sent {}",
/// 			uid,
/// 			flexbuffers::Reader::get_root(data).unwrap()
/// 		);
/// 	}
/// }
///
/// let mask = {
/// 	let mut fbb = flexbuffers::Builder::default();
/// 	let mut map = fbb.start_map();
/// 	map.push("test", true);
/// 	map.end_map();
/// 	fbb.take_buffer()
/// };
///
/// use stardust_xr_fusion::fields::SphereField;
/// let field = SphereField::builder()
/// 	.spatial_parent(client.get_root())
/// 	.radius(0.1)
/// 	.build()
/// 	.unwrap();
///
/// use stardust_xr_fusion::data::PulseReceiver;
/// let _pulse_receiver =
/// 	PulseReceiver::create(client.get_root(), None, None, &field, &mask)
/// 		.unwrap()
/// 		.wrap(PulseReceiverTest)
/// 		.unwrap();
/// ```
#[derive(Debug)]
pub struct PulseReceiver {
	spatial: Spatial,
}
impl PulseReceiver {
	/// Create a pulse receiver node. The field will remain intact even if its node is dropped.
	pub fn create<Fi: Field>(
		spatial_parent: &Spatial,
		transform: Transform,
		field: &Fi,
		mask: &[u8],
	) -> Result<Self, NodeError> {
		flexbuffers::Reader::get_root(mask)
			.and_then(|f| f.get_map())
			.map_err(|_| NodeError::MapInvalid)?;

		let id = nanoid::nanoid!();
		Ok(PulseReceiver {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/data",
					"create_pulse_receiver",
					"/data/receiver",
					true,
					&id.clone(),
					(
						id,
						spatial_parent.node().get_path()?,
						transform,
						&field.node().get_path()?,
						mask,
					),
				)?,
			},
		})
	}

	/// Wrap this struct and a handler into a `HandlerWrapper` if possible.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: PulseReceiverHandler>(
		self,
		handler: H,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}
	/// Wrap this struct and a handler into a `HandlerWrapper` if possible.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap_raw<H: PulseReceiverHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);

		handler_wrapper.add_handled_signal("data", move |_receiver, handler, data, _fds| {
			#[derive(Deserialize)]
			struct SendDataInfo {
				uid: String,
				data: Vec<u8>,
			}
			let info: SendDataInfo = deserialize_owned(data)?;
			tokio::task::spawn(async move {
				handler.lock().await.data(info.uid, info.data).await;
			});
			Ok(())
		})?;

		Ok(handler_wrapper)
	}
}
impl NodeType for PulseReceiver {
	fn node(&self) -> &Node {
		self.spatial.node()
	}

	fn alias(&self) -> Self {
		PulseReceiver {
			spatial: self.spatial.alias(),
		}
	}
}
impl HandledNodeType for PulseReceiver {}
impl Deref for PulseReceiver {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_pulses() {
	use super::client::Client;
	color_eyre::install().unwrap();
	let (client, event_loop) = Client::connect_with_async_loop()
		.await
		.expect("Couldn't connect");

	struct PulseReceiverTest(Arc<Client>);
	#[crate::handler]
	impl PulseReceiverHandler for PulseReceiverTest {
		async fn data(&mut self, uid: String, data: Vec<u8>) {
			println!(
				"Pulse sender {} sent {}",
				uid,
				flexbuffers::Reader::get_root(data.as_slice()).unwrap()
			);
			self.0.stop_loop();
		}
	}
	struct PulseSenderTest {
		data: Vec<u8>,
		node: PulseSender,
	}
	#[crate::handler]
	impl PulseSenderHandler for PulseSenderTest {
		async fn new_receiver(
			&mut self,
			info: NewReceiverInfo,
			receiver: PulseReceiver,
			field: UnknownField,
		) {
			println!(
				"New pulse receiver {:?} with field {:?} and info {:?}",
				receiver.node().get_path(),
				field.node().get_path(),
				info
			);
			self.node.send_data(&receiver, &self.data).unwrap();
		}
		async fn drop_receiver(&mut self, uid: String) {
			println!("Pulse receiver {} dropped", uid);
		}
	}

	let field = super::fields::SphereField::create(client.get_root(), Vector3::from([0.0; 3]), 0.1)
		.unwrap();

	let mask = {
		let mut fbb = flexbuffers::Builder::default();
		let mut map = fbb.start_map();
		map.push("test", true);
		map.end_map();
		fbb.take_buffer()
	};

	let pulse_sender = PulseSender::create(client.get_root(), Transform::default(), &mask).unwrap();
	let pulse_sender_handler = PulseSenderTest {
		data: mask.clone(),
		node: pulse_sender.alias(),
	};
	let _pulse_sender_handler = pulse_sender.wrap(pulse_sender_handler).unwrap();
	let _pulse_receiver =
		PulseReceiver::create(client.get_root(), Transform::default(), &field, &mask)
			.unwrap()
			.wrap(PulseReceiverTest(client.clone()))
			.unwrap();

	tokio::select! {
		_ = tokio::time::sleep(core::time::Duration::from_secs(1)) => panic!("Timed Out"),
		e = event_loop => e.unwrap().unwrap(),
	}
}
