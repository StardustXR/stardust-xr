use std::{
	os::fd::OwnedFd,
	sync::{Arc, Weak},
};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use stardust_xr::scenegraph::{self, ScenegraphError};
use tokio::sync::oneshot;

use crate::node::NodeInternals;

/// Scenegraph full of aliases to nodes, needed so the `Messenger` can send messages to nodes.
#[derive(Default)]
pub struct Scenegraph {
	nodes: Mutex<FxHashMap<String, Weak<NodeInternals>>>,
}

impl Scenegraph {
	pub fn new() -> Self {
		Default::default()
	}

	pub fn add_node(&self, node_internals: &Arc<NodeInternals>) {
		self.nodes
			.lock()
			.insert(node_internals.path(), Arc::downgrade(&node_internals));
	}

	pub fn remove_node(&self, node_path: &str) {
		self.nodes.lock().remove(node_path);
	}

	// pub fn get_node(&self, path: &str) -> Option<Node> {
	// 	self.nodes.lock().get(path).cloned().unwrap_or_default()
	// }
}

impl scenegraph::Scenegraph for Scenegraph {
	fn send_signal(
		&self,
		path: &str,
		method: &str,
		data: &[u8],
		fds: Vec<OwnedFd>,
	) -> Result<(), ScenegraphError> {
		let node = self
			.nodes
			.lock()
			.get(path)
			.and_then(Weak::upgrade)
			.ok_or(ScenegraphError::NodeNotFound)?;
		let local_signals = node.local_signals.lock();
		let signal = local_signals
			.get(method)
			.ok_or(ScenegraphError::SignalNotFound)?
			.clone();
		signal(data.to_vec(), fds).map_err(|e| ScenegraphError::SignalError {
			error: e.to_string(),
		})
	}
	fn execute_method(
		&self,
		path: &str,
		method: &str,
		data: &[u8],
		fds: Vec<OwnedFd>,
		response: oneshot::Sender<Result<(Vec<u8>, Vec<OwnedFd>), ScenegraphError>>,
	) {
		let method_method = || {
			let node = self
				.nodes
				.lock()
				.get(path)
				.and_then(Weak::upgrade)
				.ok_or(ScenegraphError::NodeNotFound)?;
			let local_methods = node.local_methods.lock();
			let method = local_methods
				.get(method)
				.ok_or(ScenegraphError::MethodNotFound)?
				.clone();
			method(data.to_vec(), fds).map_err(|e| ScenegraphError::MethodError {
				error: e.to_string(),
			})
		};

		let _ = response.send(method_method());
	}
}
