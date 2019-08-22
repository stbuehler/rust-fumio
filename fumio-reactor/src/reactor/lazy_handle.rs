use super::Handle;

/// Refers to a specific handle or to the [`current`](fn.current.html) handle.
///
/// Use `LazyHandle::from(handle)` to initialize with a specific handle.
#[derive(Debug, Clone)]
pub struct LazyHandle {
	handle: Option<Handle>,
}

impl LazyHandle {
	/// Use the [`current`](fn.current.html) handle (at the time [`bind`](#method.bind) is called).
	pub const fn new() -> Self {
		Self {
			handle: None,
		}
	}

	/// Whether this is bound to a specific `Handle` or `bind` will return the
	/// [`current`](fn.current.html) `Handle`.
	pub fn is_bound(&self) -> bool {
		self.handle.is_some()
	}

	/// Return the `Handle` this was created with or [`current`](fn.current.html) if no specific
	/// handle was specified.
	pub fn bind(&self) -> Option<Handle> {
		match &self.handle {
			Some(handle) => Some(handle.clone()),
			None => super::current(),
		}
	}
}

impl Default for LazyHandle {
	fn default() -> Self {
		Self::new()
	}
}

impl From<Handle> for LazyHandle {
	fn from(handle: Handle) -> Self {
		Self {
			handle: Some(handle),
		}
	}
}
