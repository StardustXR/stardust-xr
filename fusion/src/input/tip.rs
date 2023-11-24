use crate::{
	node::{HandledNodeType, Node, NodeError, NodeType},
	spatial::Spatial,
	HandlerWrapper,
};
use stardust_xr::{
	schemas::flex::flexbuffers::{self},
	values::Transform,
};
use std::{ops::Deref, sync::Arc};
use tokio::sync::Mutex;

use super::{input_method_handler_wrapper, InputMethod, InputMethodHandler};

/// Virtual spatial input device representing a tool device with a single point of interaction (pen tip, controller tip, etc.)
#[derive(Debug)]
pub struct TipInputMethod {
	spatial: Spatial,
}
impl<'a> TipInputMethod {
	pub fn create(
		spatial_parent: &'a Spatial,
		transform: Transform,
		radius: f32,
		datamap: Option<Vec<u8>>,
	) -> Result<Self, NodeError> {
		let id = nanoid::nanoid!();
		if let Some(datamap) = &datamap {
			flexbuffers::Reader::get_root(datamap.as_slice())
				.and_then(|root| root.get_map())
				.map_err(|_| NodeError::MapInvalid)?;
		}
		let spatial = Spatial {
			node: Node::new(
				&spatial_parent.node.client()?,
				"/input",
				"create_input_method_tip",
				"/input/method/tip",
				true,
				&id.clone(),
				(
					id,
					spatial_parent.node().get_path()?,
					transform,
					radius,
					datamap,
				),
			)?,
		};
		Ok(TipInputMethod { spatial })
	}

	/// Set the radius of influence for the tip.
	pub fn set_radius(&self, radius: f32) -> Result<(), NodeError> {
		self.node.send_remote_signal("set_radius", &radius)
	}
	/// Wrap this node and an `InputMethodHandler` in a `HandlerWrapper` to run code ASAP.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: InputMethodHandler>(
		self,
		handler: H,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}
	/// Wrap this node and an `InputMethodHandler` in a `HandlerWrapper` to run code ASAP.
	#[must_use = "Dropping this handler wrapper would immediately drop the node"]
	pub fn wrap_raw<H: InputMethodHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);
		input_method_handler_wrapper(&handler_wrapper)?;
		Ok(handler_wrapper)
	}
}
impl NodeType for TipInputMethod {
	fn node(&self) -> &Node {
		self.spatial.node()
	}

	fn alias(&self) -> Self
	where
		Self: Sized,
	{
		TipInputMethod {
			spatial: self.spatial.alias(),
		}
	}
}
impl HandledNodeType for TipInputMethod {}
impl InputMethod for TipInputMethod {}
impl Deref for TipInputMethod {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_tip_input_method() {
	use crate::client::{Client, FrameInfo};
	use crate::drawable::Model;
	use serde::Serialize;

	color_eyre::install().unwrap();
	let (client, event_loop) = Client::connect_with_async_loop()
		.await
		.expect("Couldn't connect");
	client.set_base_prefixes(&[manifest_dir_macros::directory_relative_path!("res")]);

	let mut fbb = flexbuffers::Builder::default();
	fbb.start_map();
	let tip = TipInputMethod::create(
		client.get_root(),
		Transform::default(),
		0.5,
		Some(fbb.take_buffer()),
	)
	.unwrap();

	fn summon_model(parent: &Spatial, rotation: glam::Quat) -> Model {
		Model::create(
			parent,
			Transform::from_rotation_scale(rotation, [0.1; 3]),
			&crate::drawable::ResourceID::new_namespaced("fusion", "cursor_spike"),
		)
		.unwrap()
	}

	struct Cursor {
		top: Model,
		bottom: Model,
		left: Model,
		right: Model,
		forward: Model,
		backward: Model,
	}
	#[derive(Default, serde::Serialize, serde::Deserialize)]
	struct Datamap {
		grab: f32,
		select: f32,
	}
	struct TipDemo {
		tip: TipInputMethod,
		cursor: Cursor,
		datamap: Datamap,
	}
	#[crate::handler]
	impl crate::client::RootHandler for TipDemo {
		async fn frame(&mut self, info: FrameInfo) {
			let (sin, cos) = (info.elapsed as f32).sin_cos();
			self.tip
				.set_position(None, mint::Vector3::from([sin * 0.1, 0.0, cos * 0.1]))
				.unwrap();

			self.datamap.grab = sin;
			let mut serializer = flexbuffers::FlexbufferSerializer::new();
			self.datamap.serialize(&mut serializer).unwrap();
			self.tip.set_datamap(&serializer.take_buffer()).unwrap();
		}
		fn save_state(&mut self) -> crate::client::ClientState {
			crate::client::ClientState::default()
		}
	}

	let _wrapped_root = client.wrap_root(TipDemo {
		cursor: Cursor {
			top: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::X, 180.0f32.to_radians()),
			),
			bottom: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::X, 0.0f32.to_radians()),
			),
			left: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::Z, 90.0f32.to_radians()),
			),
			right: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::Z, -90.0f32.to_radians()),
			),
			forward: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::X, 90.0f32.to_radians()),
			),
			backward: summon_model(
				&tip,
				glam::Quat::from_axis_angle(glam::Vec3::X, -90.0f32.to_radians()),
			),
		},
		tip,
		datamap: Datamap::default(),
	});

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => (),
		e = event_loop => e.unwrap().unwrap(),
	};
}
