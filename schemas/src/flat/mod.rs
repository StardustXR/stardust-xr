//! Contains From/Into impls for flatbuffers types to mint types.

pub use flatbuffers;

pub mod message {
	pub use self::stardust_xr::*;
	include!("message.rs");
}
