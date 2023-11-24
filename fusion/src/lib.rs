//! A library for Stardust XR clients to use with abstractions over the client, nodes, and event loop.
//!
//! # Example
//! ```
//!use stardust_xr_fusion::client::Client;
//!
//!#[tokio::main(flavor="current_thread")]
//!async fn main() {
//!	let (_client, event_loop) = Client::connect_with_async_loop().await.unwrap();
//!
//!	tokio::select! {
//!		biased;
//!		_ = tokio::signal::ctrl_c() => (),
//!		e = event_loop => e.unwrap().unwrap(),
//!	}
//!}
//! ```

#![allow(dead_code)]

pub use stardust_xr as core;
use tokio::sync::{Mutex, MutexGuard};

#[macro_use]
pub mod node;

pub mod audio;
pub mod client;
pub mod data;
pub mod drawable;
pub mod fields;
pub mod input;
pub mod items;
mod scenegraph;
pub mod spatial;

use self::node::HandledNodeType;
use color_eyre::eyre::{anyhow, Result};
use node::NodeError;
use std::{os::fd::OwnedFd, sync::Arc};

pub use async_trait::async_trait as handler;

/// A wrapper around a node and a handler struct implementing the node's handler trait.
/// Necessary because the methods on the handler may be called at any time and bundling the 2 together makes it harder to screw up.
/// Can't be created directly, nodes that could use handlers have a `wrap()` and `wrap_raw()` method on them that consumes them and a handler and returns a `HandlerWrapper`.
///
/// # Example
/// ```
/// use stardust_xr_fusion::{HandlerWrapper, field::SphereField, zone::{Zone, ZoneHandler}};
///
/// struct ZoneHandlerTest;
/// impl ZoneHandler for ZoneHandlerTest {
/// 	fn enter(&mut self, uid: &str, spatial: Spatial) {}
/// 	fn capture(&mut self, uid: &str, spatial: Spatial) {}
/// 	fn release(&mut self, uid: &str) {}
/// 	fn leave(&mut self, uid: &str) {}
/// }
///
/// let sphere_field = SphereField::create(client.get_root(), Transform::none(), 0.5).unwrap();
/// let zone = Zone::create(client.get_root(), Transform::none(), &sphere_field).unwrap();
/// let zone_wrapped = zone.wrap(ZoneHandlerTest);
/// ```
#[derive(Debug)]
pub struct HandlerWrapper<N: HandledNodeType, H: Send + Sync + 'static> {
	node: Arc<N>,
	wrapped: Arc<Mutex<H>>,
}
impl<N: HandledNodeType, H: Send + Sync + 'static> HandlerWrapper<N, H> {
	pub(crate) fn new_raw(node: N, handler: Arc<Mutex<H>>) -> Self {
		Self {
			wrapped: handler,
			node: Arc::new(node),
		}
	}

	/// Get a reference to the node inside
	pub fn node(&self) -> &Arc<N> {
		&self.node
	}
	/// Convenience function to get the handler inside.
	///
	/// # Safety
	/// Since this is a mutex, it can deadlock.
	pub async fn lock_wrapped(&self) -> MutexGuard<H> {
		self.wrapped.lock().await
	}
	/// Get an `Arc<Mutex<_>>` of the handleNamespacedResourced type for portability.
	///
	/// # Safety
	/// Since this is a mutex, it can deadlock.
	pub fn wrapped(&self) -> &Arc<Mutex<H>> {
		&self.wrapped
	}

	pub(crate) fn add_handled_signal(
		&self,
		name: &str,
		parse: fn(Arc<N>, Arc<Mutex<H>>, Vec<u8>, Vec<OwnedFd>) -> Result<()>,
	) -> Result<(), NodeError> {
		let node = Arc::downgrade(&self.node);
		let handler = Arc::downgrade(&self.wrapped);
		self.node.node().add_local_signal(name, move |data, fds| {
			let Some(node) = node.upgrade() else {
				return Err(anyhow!("Node broken"));
			};
			let Some(handler) = handler.upgrade() else {
				return Err(anyhow!("Handler broken"));
			};
			parse(node, handler, data, fds)
		})
	}
	// #[allow(clippy::type_complexity)]
	pub(crate) fn add_handled_method(
		&self,
		name: &str,
		parse: fn(Arc<N>, Arc<Mutex<H>>, Vec<u8>, Vec<OwnedFd>) -> Result<(Vec<u8>, Vec<OwnedFd>)>,
	) -> Result<(), NodeError> {
		let node = Arc::downgrade(&self.node);
		let handler = Arc::downgrade(&self.wrapped);
		self.node.node().add_local_method(name, move |data, fds| {
			let Some(node) = node.upgrade() else {
				return Err(anyhow!("Node broken"));
			};
			let Some(handler) = handler.upgrade() else {
				return Err(anyhow!("Handler broken"));
			};
			parse(node, handler, data, fds)
		})
	}
}

#[macro_export]
macro_rules! handle_action {
    ($handler:ident, $action:ident) => {
        $handler
            .add_handled_signal(stringify!($action), |_, handler, _, _| {
                tokio::task::spawn(async move {
					handler.lock().await.$action().await;
				});  // No data deserialization
                Ok(())
            })
            .unwrap();
    };

    ($handler:ident, $action:ident, $name:ident) => {
        $handler
            .add_handled_signal(stringify!($action), |_, handler, data, _| {
				let $name = deserialize_owned(data)?;
                tokio::spawn(async move {
					handler.lock().await.$action($name).await;
				});
                Ok(())
            })
            .unwrap();
    };

    ($handler:ident, $action:ident, ($( $name:ident ),*)) => {
        $handler
            .add_handled_signal(stringify!($action), |_, handler, data, _| {
                let ($($name),*,) = deserialize_owned(data)?;
				tokio::spawn(async move {
					handler.lock().await.$action($(
						$name
					),*).await;
				});
                Ok(())
            })
            .unwrap();
    };
}
