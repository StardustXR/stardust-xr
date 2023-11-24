use super::Spatial;
use crate::{
	fields::Field,
	node::{HandledNodeType, Node, NodeError, NodeType},
	HandlerWrapper,
};
use color_eyre::eyre::Result;
use stardust_xr::{schemas::flex::deserialize_owned, values::Transform};
use std::{ops::Deref, sync::Arc};
use tokio::sync::Mutex;

/// Hamdle spatials entering the zone's field, leaving it, being captured, and released.
#[crate::handler]
pub trait ZoneHandler: Send + Sync {
	async fn enter(&mut self, uid: String, spatial: Spatial);
	async fn capture(&mut self, uid: String);
	async fn release(&mut self, uid: String);
	async fn leave(&mut self, uid: String);
}

/// Node to manipulate spatial nodes across clients.
#[derive(Debug)]
pub struct Zone {
	spatial: Spatial,
}
impl<'a> Zone {
	/// Create a zone given a field, this zone will become inactive if the field is dropped.
	///
	/// Keep in mind the zone and its field are different spatials, they can move independently.
	pub fn create<Fi: Field>(
		spatial_parent: &'a Spatial,
		transform: Transform,
		field: &'a Fi,
	) -> Result<Zone, NodeError> {
		let id = nanoid::nanoid!();
		Ok(Zone {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/spatial",
					"create_zone",
					"/spatial/zone",
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
		})
	}

	/// Wrap this node and a `ZoneHandler` in a `HandlerWrapper` to run code ASAP. Instead, you can also get the `spatials()` and `captured()` hashmaps.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: ZoneHandler>(self, handler: H) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}
	/// Wrap this node and a `ZoneHandler` in a `HandlerWrapper` to run code ASAP. Instead, you can also get the `spatials()` and `captured()` hashmaps.
	pub fn wrap_raw<H: ZoneHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);
		handler_wrapper.add_handled_signal("enter", |zone, handler, data, _| {
			let uid: String = deserialize_owned(data)?;
			let spatial = Spatial::from_path(
				&zone.node().client()?,
				zone.node().get_path()?,
				uid.clone(),
				false,
			);
			tokio::task::spawn(async move { handler.lock().await.enter(uid, spatial).await });
			Ok(())
		})?;
		handler_wrapper.add_handled_signal("capture", |_zone, handler, data, _| {
			let uid: String = deserialize_owned(data)?;
			tokio::task::spawn(async move { handler.lock().await.capture(uid).await });
			Ok(())
		})?;
		handler_wrapper.add_handled_signal("release", |_zone, handler, data, _| {
			let uid: String = deserialize_owned(data)?;
			tokio::task::spawn(async move { handler.lock().await.release(uid).await });
			Ok(())
		})?;
		handler_wrapper.add_handled_signal("leave", |_zone, handler, data, _| {
			let uid: String = deserialize_owned(data)?;
			tokio::task::spawn(async move { handler.lock().await.leave(uid).await });
			Ok(())
		})?;
		Ok(handler_wrapper)
	}

	/// Check for new spatials. Any outdated spatials may not be transformable.
	pub fn update(&self) -> Result<(), NodeError> {
		self.node.send_remote_signal("update", &())
	}
	/// Try to capture a spatial.
	/// If you sucessfully capture it you'll get a `capture()` call to the handler if wrapped and it added to the `captured` hashmap.
	pub fn capture(&self, spatial: &Spatial) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("capture", &spatial.node().get_path()?)
	}
	/// Try to release a spatial.
	/// If the spatial was already released, this does nothing.
	pub fn release(&self, uid: &str) -> Result<(), NodeError> {
		let path = self.node().get_path()? + "/" + uid;
		self.node.send_remote_signal("release", &path)
	}
}
impl NodeType for Zone {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		Zone {
			spatial: self.spatial.alias(),
		}
	}
}
impl HandledNodeType for Zone {}
impl Deref for Zone {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_zone() {
	color_eyre::install().unwrap();
	use crate::client::Client;
	let (client, event_loop) = Client::connect_with_async_loop()
		.await
		.expect("Couldn't connect");
	client.set_base_prefixes(&[manifest_dir_macros::directory_relative_path!("res")]);

	let model_parent = Spatial::create(client.get_root(), Transform::default(), true).unwrap();

	let gyro_gem = crate::drawable::ResourceID::new_namespaced("fusion", "gyro_gem");
	let _model =
		crate::drawable::Model::create(&model_parent, Transform::default(), &gyro_gem).unwrap();

	let field =
		crate::fields::SphereField::create(client.get_root(), mint::Vector3::from([0.0; 3]), 0.1)
			.unwrap();

	struct ZoneTest {
		client: Arc<Client>,
		root: Spatial,
		zone: Zone,
	}
	#[crate::handler]
	impl ZoneHandler for ZoneTest {
		async fn enter(&mut self, uid: String, spatial: Spatial) {
			println!("Spatial {uid} entered zone");
			self.zone.capture(&spatial).unwrap();
		}
		async fn capture(&mut self, uid: String) {
			println!("Spatial {uid} was captured");
			self.zone.release(&uid).unwrap();
		}
		async fn release(&mut self, uid: String) {
			println!("Spatial {uid} was released");
			self.root.set_position(None, [0.0, 1.0, 0.0]).unwrap();
			self.zone.update().unwrap();
		}
		async fn leave(&mut self, uid: String) {
			println!("Spatial {uid} left zone");
			self.client.stop_loop();
		}
	}
	let zone = Zone::create(client.get_root(), Transform::default(), &field).unwrap();
	let zone_handler = ZoneTest {
		client,
		root: model_parent,
		zone: zone.alias(),
	};
	let zone = zone.wrap(zone_handler).unwrap();
	zone.node().update().unwrap();

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => (),
		e = event_loop => e.unwrap().unwrap(),
	};
}
