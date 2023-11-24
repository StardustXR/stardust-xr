use super::Item;
use crate::{
	client::Client,
	drawable::ModelPart,
	handle_action,
	node::{HandledNodeType, Node, NodeError, NodeType},
	spatial::Spatial,
	HandlerWrapper,
};
use mint::Vector2;
use rustc_hash::FxHashMap;
use serde::{
	de::{Deserializer, Error, SeqAccess, Visitor},
	ser::Serializer,
	Deserialize,
};
use stardust_xr::schemas::flex::deserialize_owned;
use std::{ops::Deref, sync::Arc};
use tokio::sync::Mutex;

/// Handler for the `panel` item.
#[allow(unused_variables)]
#[crate::handler]
pub trait PanelItemHandler: Send + Sync {
	/// This is invoked when the parent of the top-level surface changes.
	async fn toplevel_parent_changed(&mut self, parent: String) {}
	/// The title of the top-level surface was changed.
	async fn toplevel_title_changed(&mut self, title: String) {}
	/// The app id of the top-level surface was updated.
	async fn toplevel_app_id_changed(&mut self, app_id: String) {}
	/// The fullscreen state of the top-level surface was updated. The parameter 'active' indicates whether fullscreen is now active or not.
	async fn toplevel_fullscreen_active(&mut self, active: bool) {}
	/// This receives a request to move the top-level surface.
	async fn toplevel_move_request(&mut self) {}
	/// This is invoked when there is a request to resize the top-level surface. The parameters up, down, left and right indicate which edges are supposed to resize.
	async fn toplevel_resize_request(&mut self, up: bool, down: bool, left: bool, right: bool) {}
	/// The size of the top-level surface changed.
	async fn toplevel_size_changed(&mut self, size: Vector2<u32>);

	/// The cursor's material will automatically update -- you just need to hide/show the cursor and account for the new size/hotspot. The hotspot is the offset in the geometry.
	async fn set_cursor(&mut self, geometry: Option<Geometry>) {}

	/// A new child was created. Children are drawn independently for efficiency or to exceed the boundaries of the toplevel.
	async fn new_child(&mut self, uid: String, info: ChildInfo);
	/// The child has moved or resized itself, update your UI accordingly.
	async fn reposition_child(&mut self, uid: String, geometry: Geometry);
	/// The child was destroyed.
	async fn drop_child(&mut self, uid: String);
}

/// The origin and size of the surface's "solid" part.
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Geometry {
	pub origin: Vector2<i32>,
	pub size: Vector2<u32>,
}
/// The state of the panel item's toplevel.
#[derive(Debug, Clone, Deserialize)]
pub struct ToplevelInfo {
	/// The UID of the panel item of the parent of this toplevel, if it exists
	pub parent: Option<String>,
	/// Equivalent to the window title
	pub title: Option<String>,
	/// Application identifier, see <https://standards.freedesktop.org/desktop-entry-spec/>
	pub app_id: Option<String>,
	/// Current size in pixels
	pub size: Vector2<u32>,
	/// Recommended minimum size in pixels
	pub min_size: Option<Vector2<u32>>,
	/// Recommended maximum size in pixels
	pub max_size: Option<Vector2<u32>>,
	/// Surface geometry
	pub logical_rectangle: Geometry,
}

/// Data on positioning a child
#[derive(Debug, Clone, Deserialize)]
pub struct ChildInfo {
	pub parent: SurfaceID,
	pub geometry: Geometry,
}

/// The init data for the panel item.
#[derive(Debug, Clone, Deserialize)]
pub struct PanelItemInitData {
	/// The cursor, if applicable. The origin is the hotspot, size is size. If this is Some, you can apply its surface material to anything.
	pub cursor: Option<Geometry>,
	/// Size of the toplevel surface in pixels.
	pub toplevel: ToplevelInfo,
	/// Children that already exist, in a hashmap with UID keys for your convenience
	pub children: FxHashMap<String, ChildInfo>,
	/// The surface, if any, that has exclusive input to the pointer.
	pub pointer_grab: Option<SurfaceID>,
	/// The surface, if any, that has exclusive input to the keyboard.
	pub keyboard_grab: Option<SurfaceID>,
}

/// An ID for a surface inside this panel item
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SurfaceID {
	Cursor,
	Toplevel,
	Child(String),
}

impl<'de> serde::Deserialize<'de> for SurfaceID {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_seq(SurfaceIDVisitor)
	}
}

struct SurfaceIDVisitor;

impl<'de> Visitor<'de> for SurfaceIDVisitor {
	type Value = SurfaceID;

	fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		f.write_str("idk")
	}

	fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
		let Some(discrim) = seq.next_element()? else {
			return Err(A::Error::missing_field("discrim"));
		};

		// idk if you wanna check for extraneous elements
		// I didn't bother

		match discrim {
			"Cursor" => Ok(SurfaceID::Cursor),
			"Toplevel" => Ok(SurfaceID::Toplevel),
			"Child" => {
				let Some(text) = seq.next_element()? else {
					return Err(A::Error::missing_field("child_text"));
				};
				Ok(SurfaceID::Child(text))
			}
			_ => Err(A::Error::unknown_variant(
				discrim,
				&["Cursor", "Toplevel", "Child"],
			)),
		}
	}
}

impl serde::Serialize for SurfaceID {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		match self {
			Self::Cursor => ["Cursor"].serialize(serializer),
			Self::Toplevel => ["Toplevel"].serialize(serializer),
			Self::Child(text) => ["Child", text].serialize(serializer),
		}
	}
}

/// An item that represents a toplevel wayland surface (base window) and all its childs (context menus, modals, etc.).
#[derive(Debug)]
pub struct PanelItem {
	spatial: Spatial,
}
impl PanelItem {
	/// Apply a surface's visuals as a material to a model.
	///
	/// This material is unlit with the [Simula text shader](https://github.com/SimulaVR/Simula/blob/master/addons/godot-haskell-plugin/TextShader.tres) ported on the server.
	/// The material index is global across the whole model for now, just play around with it a bit.
	pub fn apply_surface_material(
		&self,
		surface: &SurfaceID,
		model_part: &ModelPart,
	) -> Result<(), NodeError> {
		self.node.send_remote_signal(
			"apply_surface_material",
			&(surface, model_part.node().get_path()?),
		)
	}

	/// Try to close the toplevel, equivalent of ctrl+w or such.
	///
	/// The panel item UI handler or panel item acceptor will drop the panel item if this succeeds.
	pub fn close_toplevel(&self) -> Result<(), NodeError> {
		self.node().send_remote_signal("close_toplevel", &())
	}
	/// Request a resize of the surface (in pixels) to whatever size the 2D app wants.
	///
	/// The surface's actual size after being resized will be given if the panel item is wrapped as `PanelItemHandler::resize`.
	pub fn auto_size_toplevel(&self) -> Result<(), NodeError> {
		self.node().send_remote_signal("auto_size_toplevel", &())
	}
	/// Request a resize of the surface (in pixels).
	///
	/// The surface's actual size after being resized will be given if the panel item is wrapped as `PanelItemHandler::resize`.
	pub fn set_toplevel_size(&self, size: Vector2<u32>) -> Result<(), NodeError> {
		self.node().send_remote_signal("set_toplevel_size", &size)
	}
	pub fn set_toplevel_focused_visuals(&self, focused: bool) -> Result<(), NodeError> {
		self.node()
			.send_remote_signal("set_toplevel_focused_visuals", &focused)
	}

	/// Send an event to set the pointer's position (in pixels, relative to top-left of surface). This will activate the pointer.
	pub fn pointer_motion(
		&self,
		surface: &SurfaceID,
		position: impl Into<Vector2<f32>>,
	) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("pointer_motion", &(surface, position.into()))
	}
	/// Send an event to set a pointer button's state if the pointer's active.
	///
	/// The `button` is from the `input_event_codes` crate (e.g. BTN_LEFT for left click).
	pub fn pointer_button(
		&self,
		surface: &SurfaceID,
		button: u32,
		pressed: bool,
	) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("pointer_button", &(surface, button, pressed as u32))
	}
	/// Send an event to scroll the pointer if it's active.
	///
	/// Scroll distance is a value in pixels corresponding to the "distance" the surface should be scrolled.
	/// Scroll steps is a value in columns/rows corresponding to the wheel clicks of a mouse or such. This also supports fractions of a wheel click.
	///
	/// If both the distance and steps are `None` then the scroll will be considered stopped. Either one being `Some` just scrolls.
	pub fn pointer_scroll(
		&self,
		surface: &SurfaceID,
		scroll_distance: Option<Vector2<f32>>,
		scroll_steps: Option<Vector2<f32>>,
	) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("pointer_scroll", &(surface, scroll_distance, scroll_steps))
	}

	/// Send a series of key presses and releases (positive keycode for pressed, negative for released).
	///
	/// To get a keymap ID use `Client::register_keymap` with your keymap.
	pub fn keyboard_keys(
		&self,
		surface: &SurfaceID,
		keymap_id: &str,
		keys: Vec<i32>,
	) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("keyboard_key", &(surface, keymap_id, keys))
	}

	/// Put a touch down on this surface with the unique ID `uid` at `position`.
	pub fn touch_down(
		&self,
		surface: &SurfaceID,
		uid: u32,
		position: impl Into<Vector2<f32>>,
	) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("touch_down", &(surface, uid, position.into()))
	}
	/// Move an existing touch point.
	pub fn touch_move(&self, uid: u32, position: impl Into<Vector2<f32>>) -> Result<(), NodeError> {
		self.node
			.send_remote_signal("touch_move", &(uid, position.into()))
	}
	/// Release a touch from its surface.
	pub fn touch_up(&self, uid: u32) -> Result<(), NodeError> {
		self.node.send_remote_signal("touch_up", &uid)
	}
	/// Make sure nothing is touching any surface on this panel item.
	///
	/// Useful for when it's newly captured into an item acceptor to make sure no touches get stuck.
	pub fn reset_touches(&self) -> Result<(), NodeError> {
		self.node
			.send_remote_signal_raw("reset_touches", &[], Vec::new())
	}

	/// Wrap the panel item and `PanelItemHandler` in a `HandlerWrapper` to receive resize and cursor events.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap<H: PanelItemHandler>(
		self,
		handler: H,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		self.wrap_raw(Arc::new(Mutex::new(handler)))
	}
	/// Wrap the panel item and `PanelItemHandler` in a `HandlerWrapper` to receive resize and cursor events.
	#[must_use = "Dropping this handler wrapper would immediately drop the handler"]
	pub fn wrap_raw<H: PanelItemHandler>(
		self,
		handler: Arc<Mutex<H>>,
	) -> Result<HandlerWrapper<Self, H>, NodeError> {
		let handler_wrapper = HandlerWrapper::new_raw(self, handler);

		handle_action!(handler_wrapper, set_cursor, geometry);

		handle_action!(handler_wrapper, toplevel_parent_changed, parent);
		handle_action!(handler_wrapper, toplevel_title_changed, title);
		handle_action!(handler_wrapper, toplevel_app_id_changed, app_id);
		handle_action!(handler_wrapper, toplevel_fullscreen_active, active);
		handle_action!(handler_wrapper, toplevel_move_request);
		handle_action!(
			handler_wrapper,
			toplevel_resize_request,
			(up, down, left, right)
		);
		handle_action!(handler_wrapper, toplevel_size_changed, size);

		handle_action!(handler_wrapper, new_child, (uid, info));
		handle_action!(handler_wrapper, reposition_child, (uid, position));
		handle_action!(handler_wrapper, drop_child, uid);

		Ok(handler_wrapper)
	}
}
impl NodeType for PanelItem {
	fn node(&self) -> &Node {
		&self.spatial.node
	}

	fn alias(&self) -> Self {
		PanelItem {
			spatial: self.spatial.alias(),
		}
	}
}
impl HandledNodeType for PanelItem {}
impl Item for PanelItem {
	type InitData = PanelItemInitData;
	const TYPE_NAME: &'static str = "panel";

	fn from_path(
		client: &Arc<Client>,
		parent_path: impl ToString,
		name: impl ToString,
		_init_data: &PanelItemInitData,
	) -> Self {
		// let handler_wrapper = HandlerWrapper::new(item, |handler: Weak<Mutex<T>>, item| {
		// 	ui_init_fn(init_data, handler, item)
		// });
		// handler_wrapper
		// 	.add_handled_signal("resize", Self::handle_resize)
		// 	.unwrap();
		// handler_wrapper
		// 	.add_handled_signal("set_cursor", Self::handle_set_cursor)
		// 	.unwrap();
		PanelItem {
			spatial: Spatial {
				node: Node::from_path(client, parent_path, name, false),
			},
		}
	}
}
impl Deref for PanelItem {
	type Target = Spatial;

	fn deref(&self) -> &Self::Target {
		&self.spatial
	}
}

#[tokio::test]
async fn fusion_panel_ui() {
	color_eyre::install().unwrap();
	use manifest_dir_macros::directory_relative_path;
	use rustc_hash::FxHashMap;
	let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();
	client.set_base_prefixes(&[directory_relative_path!("res")]);

	struct PanelItemManager(FxHashMap<String, HandlerWrapper<PanelItem, PanelItemUI>>);
	#[crate::handler]
	impl crate::items::ItemUIHandler<PanelItem> for PanelItemManager {
		async fn item_created(
			&mut self,
			uid: String,
			item: PanelItem,
			init_data: PanelItemInitData,
		) {
			item.set_toplevel_focused_visuals(true).unwrap();
			item.auto_size_toplevel().unwrap();
			self.0.insert(
				uid.to_string(),
				item.wrap(PanelItemUI::new(init_data)).unwrap(),
			);
		}
		async fn item_captured(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Acceptor {acceptor_uid} captured panel item {item_uid}");
		}
		async fn item_released(&mut self, item_uid: String, acceptor_uid: String) {
			println!("Acceptor {acceptor_uid} released panel item {item_uid}");
		}
	}
	struct PanelItemUI;
	impl PanelItemUI {
		fn new(init_data: PanelItemInitData) -> Self {
			println!("Panel item created with {:?}", init_data);
			PanelItemUI
		}
	}
	#[crate::handler]
	impl PanelItemHandler for PanelItemUI {
		async fn set_cursor(&mut self, cursor_info: Option<Geometry>) {
			dbg!(cursor_info);
		}

		async fn toplevel_size_changed(&mut self, size: Vector2<u32>) {
			dbg!(size);
		}

		async fn new_child(&mut self, uid: String, info: ChildInfo) {
			dbg!(uid);
			dbg!(info);
		}
		async fn reposition_child(&mut self, uid: String, geometry: Geometry) {
			dbg!(uid);
			dbg!(geometry);
		}
		async fn drop_child(&mut self, uid: String) {
			dbg!(uid);
		}
	}
	impl Drop for PanelItemUI {
		fn drop(&mut self) {
			println!("Panel item destroyed");
		}
	}

	let _item_ui = crate::items::ItemUI::<PanelItem>::register(&client)
		.unwrap()
		.wrap(PanelItemManager(FxHashMap::default()))
		.unwrap();

	tokio::select! {
		_ = tokio::time::sleep(core::time::Duration::from_secs(60)) => panic!("Timed Out"),
		e = event_loop => e.unwrap().unwrap(),
	}
}
