use super::Item;
use crate::client::Client;
use crate::node::{Node, NodeError, NodeType};
use crate::spatial::Spatial;
use stardust_xr::values::Transform;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Item that contains the path to an equirectangular `.hdr` file.
pub struct EnvironmentItem {
	spatial: Spatial,
	pub path: PathBuf,
}

impl EnvironmentItem {
	/// Create a new environment item from a file path.
	pub fn create<P: AsRef<Path>>(
		spatial_parent: &Spatial,
		transform: Transform,
		file_path: P,
	) -> Result<Self, NodeError> {
		let path = file_path.as_ref();
		if path.is_relative() || !path.exists() {
			return Err(NodeError::InvalidPath);
		}

		let id = nanoid::nanoid!();
		Ok(EnvironmentItem {
			spatial: Spatial {
				node: Node::new(
					&spatial_parent.node.client()?,
					"/item",
					"create_environment_item",
					"/item/environment/item",
					true,
					&id.clone(),
					(id, spatial_parent.node().get_path()?, transform, path),
				)?,
			},
			path: path.to_path_buf(),
		})
	}
}
impl NodeType for EnvironmentItem {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		EnvironmentItem {
			spatial: self.spatial.alias(),
			path: self.path.clone(),
		}
	}
}
impl Item for EnvironmentItem {
	type InitData = PathBuf;
	const TYPE_NAME: &'static str = "environment";

	fn from_path(
		client: &Arc<Client>,
		parent_path: impl ToString,
		name: impl ToString,
		init_data: &PathBuf,
	) -> Self {
		EnvironmentItem {
			spatial: Spatial {
				node: Node::from_path(client, parent_path, name, false),
			},
			path: init_data.clone(),
		}
	}
	// fn alias
}
impl Deref for EnvironmentItem {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_environment_ui() {
	color_eyre::install().unwrap();
	use manifest_dir_macros::file_relative_path;
	let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();

	let environment_item = EnvironmentItem::create(
		client.get_root(),
		Transform::default(),
		file_relative_path!("res/fusion/sky.hdr"),
	)
	.unwrap();

	struct EnvironmentUIManager(Arc<Client>);
	#[crate::handler]
	impl crate::items::ItemUIHandler<EnvironmentItem> for EnvironmentUIManager {
		async fn item_created(&mut self, item_uid: String, _item: EnvironmentItem, path: PathBuf) {
			println!(
				"Environment item {item_uid} created with path {}",
				path.display()
			);
		}
		async fn item_captured(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Capturing environment item {item_uid} in acceptor {acceptor_uid}");
		}
		async fn item_released(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Released environment item {item_uid} from acceptor {acceptor_uid}");
		}
		async fn item_destroyed(&mut self, _item_uid: String) {}
		async fn acceptor_created(
			&mut self,
			_uid: String,
			_acceptor: crate::items::ItemAcceptor<EnvironmentItem>,
			_field: crate::fields::UnknownField,
		) {
		}
		async fn acceptor_destroyed(&mut self, _uid: String) {}
	}
	#[crate::handler]
	impl crate::items::ItemAcceptorHandler<EnvironmentItem> for EnvironmentUIManager {
		async fn captured(&mut self, uid: String, item: EnvironmentItem, path: PathBuf) {
			println!(
				"Item {uid} accepted sucessfully with path {}!",
				path.display()
			);
			item.release().unwrap();
		}
		async fn released(&mut self, uid: String) {
			println!("Got {uid} released sucessfully!");
			self.0.stop_loop();
		}
	}

	let _item_ui = crate::items::ItemUI::register(&client)
		.unwrap()
		.wrap(EnvironmentUIManager(client.clone()))
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
	.wrap(EnvironmentUIManager(client.clone()))
	.unwrap();

	item_acceptor.node().capture(&environment_item).unwrap();

	tokio::select! {
		_ = tokio::time::sleep(core::time::Duration::from_secs(1)) => panic!("Timed Out"),
		e = event_loop => e.unwrap().unwrap(),
	}
}
