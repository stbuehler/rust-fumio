//! Generic implementation for "current" (thread-local) instances for "executor" handles.
//!
//! Requires a `futures_executor::Enter` reference to set.
//!
//! # Example
//!
//! ```
//! pub struct Handle;
//!
//! use futures_executor::Enter;
//! use fumio_utils::current::Current;
//!
//! thread_local! {
//!     static CURRENT: Current<Handle> = Current::new();
//! }
//!
//! pub fn enter<F, R>(handle: Handle, enter: &mut Enter, f: F) -> R
//! where
//!     F: FnOnce(&mut Enter) -> R
//! {
//!     Current::enter(&CURRENT, enter, handle, f)
//! }
//!
//! pub fn with_current_handle<F, R>(f: F) -> R
//! where
//!     F: FnOnce(Option<&Handle>) -> R,
//! {
//!     Current::with(&CURRENT, f)
//! }
//!
//! ```

use futures_executor::Enter;
use std::cell::RefCell;
use std::thread::LocalKey;

struct Reset<T: 'static> {
	current: &'static LocalKey<Current<T>>,
}

impl<T> Drop for Reset<T> {
	fn drop(&mut self) {
		// ignore error
		let _ = self.current.try_with(|c| *c.inner.borrow_mut() = None);
	}
}

/// Holds a value when entered or nothing when not.
#[derive(Debug)]
pub struct Current<T> {
	inner: RefCell<Option<T>>,
}

impl<T> Current<T> {
	/// Construct a new (empty) instance.
	pub const fn new() -> Self {
		Self {
			inner: RefCell::new(None),
		}
	}

	/// Set instance to `value` while running the callback.
	///
	/// On exit the instance is cleared.
	///
	/// # Panics
	///
	/// Panics if the instance already was entered.
	#[inline]
	pub fn enter<F, R>(this: &'static LocalKey<Self>, enter: &mut Enter, value: T, f: F) -> R
	where
		F: FnOnce(&mut Enter) -> R,
	{
		this.with(|c| {
			{
				let mut inner = c.inner.borrow_mut();
				assert!(inner.is_none(), "can't enter more than once at a time");
				*inner = Some(value);
			}
			let _reset = Reset { current: this };
			f(enter)
		})
	}

	/// Run callback with a reference to the current value (if there is one)
	///
	/// The callback will be called while holding a shareable lock to the inner value.
	///
	/// # Panics
	///
	/// Panics if the inner value is currently locked exclusively by a `with_mut` call.
	#[inline]
	pub fn with<F, R>(this: &'static LocalKey<Self>, f: F) -> R
	where
		F: FnOnce(Option<&T>) -> R,
	{
		this.with(|c| {
			f(c.inner.borrow().as_ref())
		})
	}

	/// Run callback with a reference to the current value (if there is one)
	///
	/// The callback will be called while holding an exclusive lock to the inner value.
	///
	/// # Panics
	///
	/// Panics if the inner value is currently locked by a `with` or a `with_mut` call.
	#[inline]
	pub fn with_mut<F, R>(this: &'static LocalKey<Self>, f: F) -> R
	where
		F: FnOnce(Option<&mut T>) -> R,
	{
		this.with(|c| {
			f(c.inner.borrow_mut().as_mut())
		})
	}
}
