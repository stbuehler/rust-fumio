use std::cell::Cell;
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

#[doc(hidden)]
#[derive(Debug)]
pub struct Head {
	tail: AtomicPtr<Link>, // non-null too
	head: Cell<NonNull<Link>>,
	stub: Box<Link>,
}

impl Head {
	// only internal use, no need for Default trait
	#[allow(clippy::new_without_default)]
	pub fn new() -> Self {
		let stub = Box::new(Link::new());
		Self {
			tail: AtomicPtr::new(&*stub as *const Link as *mut _),
			head: Cell::new(NonNull::from(&*stub)),
			stub,
		}
	}

	// multiple producers, thread-safe
	pub unsafe fn push(&self, link: *const Link) {
		self._push(link);
	}

	fn _push(&self, link: *const Link) {
		let link = link as *mut Link; // AtomicPtr wants *mut ...
		let prev = self.tail.swap(link, Ordering::Release);
		unsafe { &*prev }.next.store(link, Ordering::Release);
	}

	// single consumer, must be only called by a single thread!
	// at most one pop operation must be in progress at a time.
	#[allow(clippy::mut_from_ref)]
	pub unsafe fn start_pop(&self) -> impl Iterator<Item = *const Link> + '_ {
		PopAll { this: self, pos: self.head.get(), repushed_stub: false }
	}
}

impl Default for Head {
	fn default() -> Self {
		Self::new()
	}
}

impl Drop for Head {
	fn drop(&mut self) {
		assert_eq!(self.tail.load(Ordering::Relaxed) as *const Link, &*self.stub);
	}
}

struct PopAll<'a> {
	this: &'a Head,
	pos: NonNull<Link>,
	repushed_stub: bool,
}

impl Drop for PopAll<'_> {
	fn drop(&mut self) {
		self.this.head.set(self.pos);
	}
}

impl Iterator for PopAll<'_> {
	type Item = *const Link;

	fn next(&mut self) -> Option<Self::Item> {
		loop {
			let next = NonNull::new(unsafe { self.pos.as_ref() }.next.swap(null_mut(), Ordering::Acquire))?;
			// pop this.pos as we have another node after it to set as next head
			let item = self.pos.as_ptr() as *const Link;
			self.pos = next;
			if item == &*self.this.stub {
				// break the loop when pushing stub the second time to avoid starvation
				let break_loop = self.repushed_stub;
				self.repushed_stub = true;
				self.this._push(item);
				if break_loop {
					return None;
				}
			} else {
				return Some(item);
			}
		}
	}
}

#[doc(hidden)]
#[derive(Debug)]
pub struct Link {
	next: AtomicPtr<Link>,
}

impl Link {
	pub const fn new() -> Self {
		Self {
			next: AtomicPtr::new(null_mut()),
		}
	}
}

impl Default for Link {
	fn default() -> Self {
		Self::new()
	}
}

/// Create a list "link" and "head" datatype; the "link" type must be used in a data structure as
/// member; this data structure then can be pushed as `Arc<...>` to the list head.
///
/// A single thread can then collect popped nodes by iterating over `head.start_pop()`; the
/// returned iterator must be dropped before starting a new run.  Due to these restrictions the
/// function is `unsafe`.
///
/// You also need to ensure a node is only pushed to a single list at a time; such gate needs to be
/// an atomic.
///
/// # Example
///
/// Memory synchronization missing as it is only a single thread example.
///
/// ```
/// fn main() {
///     use std::sync::Arc;
///     mod example {
///         fumio_utils::mpsc! {
///             pub mod ex1 {
///                 link MyLink;
///                 head MyHead;
///                 member my_link of MyData;
///             }
///         }
///
///         pub struct MyData {
///             my_link: MyLink,
///             pub value: u32,
///         }
///
///         impl MyData {
///             pub fn new(value: u32) -> Self {
///                 MyData {
///                     my_link: MyLink::default(),
///                     value,
///                 }
///             }
///         }
///     }
///     use example::*;
///
///     let list = MyHead::new();
///     let data1 = Arc::new(MyData::new(1));
///     let data2 = Arc::new(MyData::new(2));
///     list.push(data1);
///     list.push(data2);
///     let mut counter = 1;
///     for item in unsafe { list.start_pop() } {
///         assert_eq!(counter, item.value);
///         counter += 1;
///     }
/// }
/// ```
#[macro_export]
macro_rules! mpsc {
	(mod $($tt:tt)*) => {
		$crate::_mpsc! {
			[pub(self)] [pub(super)] mod $($tt)*
		}
	};
	(pub mod $($tt:tt)*) => {
		$crate::_mpsc! {
			[pub] [pub] mod $($tt)*
		}
	};
	(pub(crate) mod $($tt:tt)*) => {
		$crate::_mpsc! {
			[pub(crate)] [pub(crate)] mod $($tt)*
		}
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! _mpsc {
	([$vis:vis] [$innervis:vis] mod $modname:ident {
		link $link_name:ident;
		head $head_name:ident;
		member $member:ident of $parent:ident;
	}) => {
		mod $modname {
			use std::sync::Arc;
			use $crate::mpsc::{Link, Head};
			use super::$parent;

			#[derive(Debug, Default)]
			$innervis struct $link_name {
				link: Link,
			}

			unsafe impl Sync for $link_name {}
			unsafe impl Send for $link_name {}

			impl $link_name {
				/// Create a new link for a list.
				#[allow(dead_code)]
				$innervis const fn new() -> Self {
					Self {
						link: Link::new(),
					}
				}

				fn __base_from_node(ptr: *const Link) -> *const $parent {
					let member_offset: usize = {
						let p = core::ptr::NonNull::<$parent>::dangling();
						let $parent { $member: member, .. } = unsafe { &*p.as_ptr() };
						let head: &Link = &(member as &Self).link;
						(head as *const _ as usize) - (p.as_ptr() as *const _ as usize)
					};
					unsafe { (ptr as *const u8).sub(member_offset) as *const $parent }
				}
			}

			#[derive(Debug)]
			$innervis struct $head_name {
				head: Head,
			}

			unsafe impl Sync for $head_name {}
			unsafe impl Send for $head_name {}

			impl $head_name {
				/// Create a new head of a list.
				#[allow(dead_code)]
				$innervis fn new() -> Self {
					Self {
						head: Head::new(),
					}
				}

				/// Push a node to the list.
				///
				/// Make sure the node is pushed to at most one list at a time! (This would be also
				/// an appropriate place to `Release` previous stores.)
				$innervis fn push(&self, node: Arc<$parent>) {
					use core::mem::ManuallyDrop;
					// push takes a reference
					let node = ManuallyDrop::new(node);
					let node_link: &$link_name = &(*node).$member;
					unsafe { self.head.push(&node_link.link); }
				}

				/// Must only be used from a single thread, and the resulting iterator must be
				/// dropped before the next call.
				$innervis unsafe fn start_pop(&self) -> impl Iterator<Item = Arc<$parent>> + '_ {
					self.head.start_pop().map(|item| {
						Arc::from_raw($link_name::__base_from_node(item))
					})
				}
			}

			impl Default for $head_name {
				fn default() -> Self {
					Self::new()
				}
			}

			impl Drop for $head_name {
				fn drop(&mut self) {
					for _ in unsafe { self.start_pop() } {}
				}
			}
		}

		$vis use self::$modname::{$link_name, $head_name};
	};
}

#[cfg(test)]
mod test {
	use std::sync::Arc;
	mpsc! {
		mod ex1 {
			link MyLink;
			head MyHead;
			member my_link of MyData;
		}
	}

	struct MyData {
		my_link: MyLink,
		value: u32,
	}

	impl MyData {
		fn new(value: u32) -> Self {
			Self {
				my_link: MyLink::default(),
				value,
			}
		}
	}

	#[test]
	fn twoitems() {
		let list = MyHead::new();
		let data1 = Arc::new(MyData::new(1));
		let data2 = Arc::new(MyData::new(2));
		list.push(data1);
		list.push(data2);
		let mut counter = 1;
		for item in unsafe { list.start_pop() } {
			assert_eq!(counter, item.value);
			counter += 1;
		}
	}
}
