use super::Item;
use crate::client::Client;
use crate::node::{Node, NodeError, NodeType};
use crate::spatial::Spatial;
use mint::{RowMatrix4, Vector2};
use stardust_xr::values::Transform;
use std::ops::Deref;
use std::sync::Arc;

/// Item that contains the path to an equirectangular `.hdr` file.
pub struct CameraItem {
	spatial: Spatial,
}

impl CameraItem {
	/// Create a new environment item from a file path.
	pub fn create(
		spatial_parent: &Spatial,
		transform: Transform,
		proj_matrix: impl Into<RowMatrix4<f32>>,
		px_size: impl Into<Vector2<u32>>,
	) -> Result<Self, NodeError> {
		let id = nanoid::nanoid!();
		Ok(CameraItem {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/item",
					"create_environment_item",
					"/item/environment/item",
					true,
					&id.clone(),
					(
						id,
						spatial_parent.node().get_path()?,
						transform,
						proj_matrix.into(),
						px_size.into(),
					),
				)?,
			},
		})
	}
}
impl NodeType for CameraItem {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		CameraItem {
			spatial: self.spatial.alias(),
		}
	}
}
impl Item for CameraItem {
	type InitData = ();
	const TYPE_NAME: &'static str = "camera";

	fn from_path(
		client: &Arc<Client>,
		parent_path: impl ToString,
		name: impl ToString,
		_init_data: &(),
	) -> Self {
		CameraItem {
			spatial: Spatial {
				node: Node::from_path(client, parent_path, name, false),
			},
		}
	}
	// fn alias
}
impl Deref for CameraItem {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_camera_ui() {
	color_eyre::install().unwrap();
	let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();

	let environment_item = CameraItem::create(
		client.get_root(),
		Transform::default(),
		glam::Mat4::perspective_infinite_rh(
			std::f32::consts::PI * 0.5,
			std::f32::consts::PI * 0.5,
			0.01,
		),
		[512, 512],
	)
	.unwrap();

	struct CameraUIManager(Arc<Client>);
	#[crate::handler]
	impl crate::items::ItemUIHandler<CameraItem> for CameraUIManager {
		async fn item_created(&mut self, item_uid: String, _item: CameraItem, _data: ()) {
			println!("Camera item {item_uid} created");
		}
		async fn item_captured(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Capturing environment item {item_uid} in acceptor {acceptor_uid}");
		}
		async fn item_released(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Released environment item {item_uid} from acceptor {acceptor_uid}");
		}
		async fn item_destroyed(&mut self, _uid: String) {}
		async fn acceptor_created(
			&mut self,
			_uid: String,
			_acceptor: crate::items::ItemAcceptor<CameraItem>,
			_field: crate::fields::UnknownField,
		) {
		}
		async fn acceptor_destroyed(&mut self, _uid: String) {}
	}
	#[crate::handler]
	impl crate::items::ItemAcceptorHandler<CameraItem> for CameraUIManager {
		async fn captured(&mut self, uid: String, item: CameraItem, _data: ()) {
			println!("Item {uid} accepted sucessfully!");
			item.release().unwrap();
		}
		async fn released(&mut self, uid: String) {
			println!("Got {uid} released sucessfully!");
			self.0.stop_loop();
		}
	}

	let _item_ui = crate::items::ItemUI::register(&client)
		.unwrap()
		.wrap(CameraUIManager(client.clone()))
		.unwrap();

	let item_acceptor_field =
		crate::fields::SphereField::create(client.get_root(), mint::Vector3::from([0.0; 3]), 0.5)
			.unwrap();
	let item_acceptor = crate::items::ItemAcceptor::create(
		client.get_root(),
		Transform::default(),
		&item_acceptor_field,
	)
	.unwrap()
	.wrap(CameraUIManager(client.clone()))
	.unwrap();

	item_acceptor.node().capture(&environment_item).unwrap();

	tokio::select! {
		_ = tokio::time::sleep(core::time::Duration::from_secs(1)) => panic!("Timed Out"),
		e = event_loop => e.unwrap().unwrap(),
	}
}
