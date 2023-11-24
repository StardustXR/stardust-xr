//! Spatial input using the Spatial Universal Interaction System (SUIS).
//!
//! Input methods are nodes that represent a 3D pointer, hand, or tip (single point of interaction, like a controller).
//! They contain a datamap which is a flexbuffer map with non-spatial data like button/trackpad/grip.
//!
//! Input handlers are nodes that represent an object that can react to spatial input.
//! They have a field attached that is important data for the SUIS to determine what should get input.
//! On input (`InputHandlerHandler::input`) the input data's spatial components is relative to the input handler itself.
//! The return value for `InputHandlerHandler::input` is `true` if you want to capture the input method.
//! Capturing an input method is useful to indicate that only that handler should get its input.
//! For example when grabbing you don't want your hand to press buttons if you're grabbing the object through them.
//! Input handlers should account for the occasional case where their field is closer than an input handler that captured a method by filtering out interactions that are triggered the same frame the input method first becomes visible.
//! Capturing an input method may be delayed a frame or 2.
//!
//! Every frame, for each input method, the server will:
//! - Sort the input handlers by the distance from the input method to their fields (often absolute value for onion skinning)
//! - Send out input events (`InputHandlerHandler::input`) in order of distance until an input handler has captured the method.
//! - The frame event is sent (`LifeCycle::frame`).
//!
//! You may want to use the `InputAction`-based structs in molecules for an easy way to parse and react to the raw input.

mod pointer;
mod tip;

pub use pointer::PointerInputMethod;
pub use stardust_xr::schemas::flat::*;
pub use tip::TipInputMethod;
use tokio::sync::Mutex;

use super::{
	fields::Field,
	node::{HandledNodeType, Node, NodeError, NodeType},
	spatial::Spatial,
	HandlerWrapper,
};
use crate::fields::UnknownField;
use color_eyre::eyre::{anyhow, bail};
use stardust_xr::{
	schemas::flex::{deserialize_owned, flexbuffers},
	values::Transform,
};
use std::{ops::Deref, os::fd::OwnedFd, sync::Arc};

pub(self) fn input_method_handler_wrapper<N: InputMethod, H: InputMethodHandler>(
	handler_wrapper: &HandlerWrapper<N, H>,
) -> Result<(), NodeError> {
	handler_wrapper.add_handled_signal("handler_created", |node, handler, data, _fds| {
		let Ok(client) = node.client() else {
			bail!("no client??")
		};
		let Ok(path) = node.node().get_path() else {
			bail!("no path??")
		};
		let uid: String = deserialize_owned(data)?;

		let node = Node::from_path(&client, &path, &uid, false);
		let spatial = Spatial { node };
		let field = UnknownField {
			spatial: Spatial {
				node: Node::from_path(&client, spatial.node().get_path().unwrap(), "field", false),
			},
		};
		let input_handler = InputHandler { spatial, field };
		tokio::spawn(async move {
			handler
				.lock()
				.await
				.create_handler(uid, input_handler)
				.await
		});
		Ok(())
	})?;
	handler_wrapper.add_handled_signal("handler_destroyed", |_node, handler, data, _fds| {
		let uid: String = deserialize_owned(data)?;
		tokio::spawn(async move { handler.lock().await.drop_handler(uid).await });
		Ok(())
	})?;

	Ok(())
}

/// Node representing a spatial input device.
pub trait InputMethod: HandledNodeType {
	fn set_datamap(&self, datamap: &[u8]) -> Result<(), NodeError> {
		flexbuffers::Reader::get_root(datamap)
			.and_then(|root| root.get_map())
			.map_err(|_| NodeError::MapInvalid)?;
		self.node()
			.send_remote_signal_raw("set_datamap", datamap, Vec::new())
	}

	fn set_handler_order(&self, handlers: &[&InputHandler]) -> Result<(), NodeError> {
		let handlers: Vec<_> = handlers
			.into_iter()
			.filter_map(|h| h.node().get_path().ok())
			.collect();
		self.node().send_remote_signal("set_handlers", &handlers)
	}
}

#[crate::handler]
pub trait InputMethodHandler: Send + Sync {
	async fn create_handler(&mut self, uid: String, handler: InputHandler);
	async fn drop_handler(&mut self, uid: String);
}

/// Handle raw input events.
#[crate::handler]
pub trait InputHandlerHandler: Send + Sync {
	/// An input method has sent an input event on this frame.
	async fn input(&mut self, input: UnknownInputMethod, data: InputData);
}

/// An input method on the server, but the type is unknown.
pub struct UnknownInputMethod {
	spatial: Spatial,
	handler: Arc<InputHandler>,
}
impl UnknownInputMethod {
	fn from_path(handler: Arc<InputHandler>, uid: &str) -> Result<Self, NodeError> {
		Ok(UnknownInputMethod {
			spatial: Spatial {
				node: Node::from_path(&handler.client()?, handler.node().get_path()?, uid, false),
			},
			handler,
		})
	}
	/// Have the input handler that this method reference came from capture the method for the next frame.
	pub fn capture(&self) -> Result<(), NodeError> {
		self.node()
			.send_remote_signal("capture", &self.handler.node().get_path()?)
	}
}
impl NodeType for UnknownInputMethod {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		UnknownInputMethod {
			spatial: self.spatial.alias(),
			handler: self.handler.clone(),
		}
	}
}
impl Deref for UnknownInputMethod {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

/// Node that can receive spatial input.
#[derive(Debug)]
pub struct InputHandler {
	spatial: Spatial,
	field: UnknownField,
}
impl<'a> InputHandler {
	/// Create an input handler given a field, this will become inactive if the field is dropped.
	///
	/// Keep in mind the handler and its field are different spatials, they can move independently.
	pub fn create<Fi: Field>(
		spatial_parent: &'a Spatial,
		transform: Transform,
		field: &'a Fi,
	) -> Result<Self, NodeError> {
		let id = nanoid::nanoid!();
		Ok(InputHandler {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/input",
					"create_input_handler",
					"/input/handler",
					true,
					&id.clone(),
					(
						id,
						spatial_parent.node().get_path()?,
						transform,
						field.node().get_path()?,
					),
				)?,
			},
			field: field.alias_unknown_field(),
		})
	}

	/// Wrap this node and an `InputHandlerHandler` in a `HandlerWrapper`. This is necessary to get any input events.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: InputHandlerHandler>(
		self,
		handler: H,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}
	/// Wrap this node and an `InputHandlerHandler` in a `HandlerWrapper`. This is necessary to get any input events.
	#[must_use = "Dropping this handler wrapper would immediately drop the node"]
	pub fn wrap_raw<H: InputHandlerHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);
		handler_wrapper.add_handled_signal("input", Self::handle_input)?;
		Ok(handler_wrapper)
	}

	fn handle_input<H: InputHandlerHandler + 'static>(
		input_handler: Arc<InputHandler>,
		handler: Arc<Mutex<H>>,
		data: Vec<u8>,
		_fds: Vec<OwnedFd>,
	) -> color_eyre::eyre::Result<()> {
		let data = InputData::deserialize(&data).map_err(|e| anyhow!(e))?;

		let input_method = UnknownInputMethod::from_path(input_handler, &data.uid)?;
		tokio::task::spawn(async move {
			handler.lock().await.input(input_method, data).await;
		});
		Ok(())
	}

	pub fn field(&self) -> Option<UnknownField> {
		self.field.node().alive().then(|| self.field.alias())
	}
}
impl NodeType for InputHandler {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		InputHandler {
			spatial: self.spatial.alias(),
			field: self.field.alias(),
		}
	}
}
impl HandledNodeType for InputHandler {}
impl Deref for InputHandler {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_input_handler() {
	use super::client::Client;
	color_eyre::install().unwrap();
	let (client, event_loop) = Client::connect_with_async_loop()
		.await
		.expect("Couldn't connect");

	let field =
		super::fields::SphereField::create(client.get_root(), mint::Vector3::from([0.0; 3]), 0.1)
			.unwrap();

	struct InputHandlerTest;
	#[crate::handler]
	impl InputHandlerHandler for InputHandlerTest {
		async fn input(&mut self, _input: UnknownInputMethod, data: InputData) {
			dbg!(data.uid);
			dbg!(data.distance);
			match &data.input {
				InputDataType::Pointer(_) => {
					println!("Pointer input");
				}
				InputDataType::Hand(_) => {
					println!("Hand input");
					let _ = data.datamap.with_data(|datamap| {
						dbg!(datamap
							.iter_keys()
							.zip(datamap.iter_values())
							.collect::<Vec<_>>());
						let _ = dbg!(datamap.idx("right").get_bool());
					});
				}
				InputDataType::Tip(_) => {
					println!("Tip input");
				}
			}
		}
	}

	// let input_handler = InputHandler::builder()
	// 	.spatial_parent(client.get_root())
	// 	.field(&field)
	// 	.wrapped_init(|_| InputHandlerTest)
	// 	.build()
	// 	.await
	// 	.unwrap();
	let _input_handler = InputHandler::create(client.get_root(), Transform::default(), &field)
		.unwrap()
		.wrap(InputHandlerTest)
		.unwrap();

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => (),
		e = event_loop => e.unwrap().unwrap(),
	};
}
