use color::rgba_linear;
use glam::Quat;
use manifest_dir_macros::directory_relative_path;
use stardust_xr::values::Transform;
use stardust_xr_fusion::{
	client::{Client, ClientState, FrameInfo, RootHandler},
	drawable::{MaterialParameter, Model, ModelPart, ResourceID},
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
	tracing_subscriber::fmt::init();
	let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();
	client.set_base_prefixes(&[directory_relative_path!("res")]);

	let _root = client.wrap_root(SpatialDemo::new(&client));

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => (),
		e = event_loop => e.unwrap().unwrap(),
	};
}

struct SpatialDemo {
	_gyro: Model,
	gem: ModelPart,
	ring_inner: ModelPart,
	ring_middle: ModelPart,
	ring_outer: ModelPart,
}
impl SpatialDemo {
	fn new(client: &Arc<Client>) -> Self {
		let gyro = Model::create(
			&client.get_root(),
			Transform::default(),
			&ResourceID::new_namespaced("fusion", "gyro"),
		)
		.unwrap();
		gyro.set_zoneable(true).unwrap();

		SpatialDemo {
			gem: gyro.model_part("Gem").unwrap(),
			ring_inner: gyro.model_part("OuterRing/MiddleRing/InnerRing").unwrap(),
			ring_middle: gyro.model_part("OuterRing/MiddleRing").unwrap(),
			ring_outer: gyro.model_part("OuterRing").unwrap(),
			_gyro: gyro,
		}
	}
}
#[stardust_xr_fusion::handler]
impl RootHandler for SpatialDemo {
	async fn frame(&mut self, info: FrameInfo) {
		let elapsed = info.elapsed as f32;

		self.gem
			.set_material_parameter(
				"color",
				MaterialParameter::Color(rgba_linear!(0.0, 0.25, 1.0, elapsed.sin().abs())),
			)
			.unwrap();
		self.gem
			.set_rotation(None, Quat::from_rotation_y(elapsed))
			.unwrap();
		self.ring_inner
			.set_rotation(None, Quat::from_rotation_x(elapsed))
			.unwrap();
		self.ring_middle
			.set_rotation(None, Quat::from_rotation_z(elapsed))
			.unwrap();
		self.ring_outer
			.set_rotation(None, Quat::from_rotation_x(elapsed))
			.unwrap();
	}
	fn save_state(&mut self) -> ClientState {
		ClientState::default()
	}
}
