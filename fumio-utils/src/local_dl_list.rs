use std::cell::Cell;
use std::ptr;

#[doc(hidden)]
#[derive(Debug)]
pub struct LocalDLHead {
	prev: Cell<*const LocalDLHead>,
	next: Cell<*const LocalDLHead>,
}

impl LocalDLHead {
	// only internal use, no need for Default trait
	pub const fn new() -> Self {
		Self {
			prev: Cell::new(ptr::null()),
			next: Cell::new(ptr::null()),
		}
	}

	pub fn init(&self) {
		if self.next.get().is_null() {
			self.next.set(self);
			self.prev.set(self);
		}
	}

	pub fn is_unlinked(&self) -> bool {
		self.next.get().is_null() || self.next.get() == (self as _)
	}

	pub unsafe fn unlink(&self) {
		if !self.is_unlinked() {
			/* unsafe */ { &*self.prev.get() }.next.set(self.next.get());
			/* unsafe */ { &*self.next.get() }.prev.set(self.prev.get());
		}
		self.next.set(ptr::null());
		self.prev.set(ptr::null());
	}

	pub unsafe fn insert_after(&self, node: &Self) {
		debug_assert!(node.is_unlinked());
		assert_ne!(self as *const _, node as *const _);
		self.init();
		node.next.set(self.next.get());
		node.prev.set(self);
		/* unsafe */ { &*node.next.get() }.prev.set(node);
		self.next.set(node);
	}

	pub unsafe fn insert_before(&self, node: &Self) {
		debug_assert!(node.is_unlinked());
		assert_ne!(self as *const _, node as *const _);
		self.init();
		node.next.set(self);
		node.prev.set(self.prev.get());
		/* unsafe */ { &*node.prev.get() }.next.set(node);
		self.prev.set(node);
	}

	pub unsafe fn pop_front(&self) -> Option<*const Self> {
		if self.is_unlinked() {
			return None;
		}
		let node = self.next.get();
		/* unsafe */ { &*node }.unlink();
		Some(node)
	}

	pub unsafe fn pop_back(&self) -> Option<*const Self> {
		if self.is_unlinked() {
			return None;
		}
		let node = self.prev.get();
		/* unsafe */ { &*node }.unlink();
		Some(node)
	}

	pub unsafe fn take_from(&mut self, other: &Self) {
		debug_assert!(self.is_unlinked());
		if !other.is_unlinked() {
			other.insert_after(self);
			other.unlink();
		}
	}
}

impl Default for LocalDLHead {
	fn default() -> Self {
		Self::new()
	}
}

impl Drop for LocalDLHead {
	fn drop(&mut self) {
		assert!(self.is_unlinked());
	}
}

/// Create a "link" and "head" datatype for a non thread-safe double linked list; the "link" type
/// must be used in a data structure as member.
///
/// The list itself doesn't manage any ownership / reference counts, which is why most
/// modifications are `unsafe`, and returned nodes are raw pointers.
///
/// # Example
/// 
/// ```
/// fn main() {
///     mod example {
///         fumio_utils::local_dl_list! {
///             pub mod ex1 {
///                 link TestLink;
///                 head TestHead;
///                 member link of Test;
///             }
///         }
/// 
///         pub struct Test {
///             link: TestLink,
///             pub value: usize,
///         }
/// 
///         impl Test {
///             pub fn new(value: usize) -> Self {
///                 Test {
///                     link: TestLink::new(),
///                     value,
///                 }
///             }
///         }
///     }
///     use example::*;
/// 
///     let head = TestHead::new();
///     let node1 = Test::new(1);
///     let node2 = Test::new(2);
///     unsafe {
///         head.append(&node1);
///         head.append(&node2);
///         assert_eq!( { &*head.pop_front().unwrap() }.value, 1);
///         assert_eq!( { &*head.pop_front().unwrap() }.value, 2);
///     }
/// }
/// ```
#[macro_export]
macro_rules! local_dl_list {
	(mod $($tt:tt)*) => {
		$crate::_local_dl_list! {
			[pub(self)] [pub(super)] mod $($tt)*
		}
	};
	(pub mod $($tt:tt)*) => {
		$crate::_local_dl_list! {
			[pub] [pub] mod $($tt)*
		}
	};
	(pub(crate) mod $($tt:tt)*) => {
		$crate::_local_dl_list! {
			[pub(crate)] [pub(crate)] mod $($tt)*
		}
	};
}

#[doc(hidden)]
#[macro_export]
macro_rules! _local_dl_list {
	([$vis:vis] [$innervis:vis] mod $modname:ident {
		link $link_name:ident;
		head $head_name:ident;
		member $member:ident of $parent:ident;
	}) => {
		mod $modname {
			use super::$parent;
			use $crate::local_dl_list::LocalDLHead;

			#[derive(Debug)]
			$innervis struct $link_name {
				head: LocalDLHead,
			}

			#[allow(dead_code)]
			impl $link_name {
				fn __base_from_node(ptr: *const LocalDLHead) -> *const $parent {
					let member_offset: usize = {
						let p = core::ptr::NonNull::<$parent>::dangling();
						let $parent { $member: member, .. } = unsafe { &*p.as_ptr() };
						let head: &LocalDLHead = &member.head;
						(head as *const _ as usize) - (p.as_ptr() as *const _ as usize)
					};
					unsafe { (ptr as *const u8).sub(member_offset) as *const $parent }
				}

				$innervis const fn new() -> Self {
					Self {
						head: LocalDLHead::new(),
					}
				}

				$innervis fn is_unlinked(&self) -> bool {
					self.head.is_unlinked()
				}

				$innervis unsafe fn unlink(&self) {
					self.head.unlink();
				}

				$innervis unsafe fn insert_after(&self, node: &$parent) {
					let node_link: &Self = &node.$member;
					self.head.insert_after(&node_link.head);
				}

				$innervis unsafe fn insert_before(&self, node: &$parent) {
					let node_link: &Self = &node.$member;
					self.head.insert_before(&node_link.head);
				}
			}

			#[derive(Debug)]
			$innervis struct $head_name {
				head: LocalDLHead,
			}

			#[allow(dead_code)]
			impl $head_name {
				$innervis const fn new() -> Self {
					Self {
						head: LocalDLHead::new(),
					}
				}

				$innervis fn is_empty(&self) -> bool {
					self.head.is_unlinked()
				}

				$innervis unsafe fn prepend(&self, node: &$parent) {
					let node_link: &$link_name = &node.$member;
					self.head.insert_after(&node_link.head);
				}

				$innervis unsafe fn append(&self, node: &$parent) {
					let node_link: &$link_name = &node.$member;
					self.head.insert_before(&node_link.head);
				}

				$innervis unsafe fn pop_front(&self) -> Option<*const $parent> {
					let node_link = self.head.pop_front()?;
					Some($link_name::__base_from_node(node_link))
				}

				$innervis unsafe fn pop_back(&self) -> Option<*const $parent> {
					let node_link = self.head.pop_back()?;
					Some($link_name::__base_from_node(node_link))
				}

				$innervis unsafe fn take_from(&mut self, other: &Self) {
					self.head.take_from(&other.head);
				}
			}
		}
		$vis use self::$modname::{$link_name, $head_name};
	};
}

#[cfg(test)]
mod test {
	local_dl_list! {
		mod ex1 {
			link TestLink;
			head TestHead;
			member link of Test;
		}
	}

	struct Test {
		link: TestLink,
		value: usize,
	}

	impl Test {
		const fn new(value: usize) -> Self {
			Self {
				link: TestLink::new(),
				value,
			}
		}
	}

	#[test]
	fn run() {
		let head = TestHead::new();
		let node1 = Test::new(1);
		let node2 = Test::new(2);
		unsafe {
			head.append(&node1);
			head.append(&node2);
			assert_eq!( { &*head.pop_front().unwrap() }.value, 1);
			assert_eq!( { &*head.pop_front().unwrap() }.value, 2);
		}
	}
}
