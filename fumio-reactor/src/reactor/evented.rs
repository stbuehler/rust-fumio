use crate::helper::async_io;
use crate::reactor::{LazyHandle, Registration};
use std::io;
use std::pin::Pin;
use std::sync::Once;
use std::task::{Context, Poll};

/// A wrapper for `Read` and `Write` based IO sources.
#[derive(Debug)]
pub struct PollEvented<E>
where
	E: mio::Evented,
{
	registration: Registration<E>,
	registered: Once,
	handle: LazyHandle,
}

impl<E> PollEvented<E>
where
	E: mio::Evented,
{
	/// Wrap io and lazily bind to `handle` on first use.
	pub fn new(io: E, handle: LazyHandle) -> Self {
		Self {
			registration: Registration::new(
				io,
				mio::Ready::all() - mio::Ready::writable(),
				mio::Ready::writable() | platform::hup(),
			),
			registered: Once::new(),
			handle,
		}
	}

	fn register(&self) {
		self.registered.call_once(|| {
			let _ = self.registration.register(
				&self.handle.bind().expect("no reactor present"),
				mio::Ready::all(),
				mio::PollOpt::edge(),
			);
		});
	}

	/// Try a read operation with mutable IO
	///
	/// If read operation fails make sure to get notified when read readiness is signalled.
	pub fn try_mut_read<F, T>(&mut self, context: &mut Context<'_>, mut read_op: F) -> Poll<io::Result<T>>
	where
		F: FnMut(&mut E) -> Poll<io::Result<T>>,
	{
		if let Poll::Ready(v) = read_op(self.io_mut()) {
			return Poll::Ready(v);
		}
		self.register();
		futures_util::ready!(self.registration.poll_read_ready(context))?;
		if let Poll::Ready(v) = read_op(self.io_mut()) {
			return Poll::Ready(v);
		}
		// registration said we're ready, but read_op failed
		// come back later to try again
		context.waker().wake_by_ref();
		Poll::Pending
	}

	/// Clears all pending read events (and returns them)
	///
	/// If no events were pending (and possibly even if there were) the waker in `context` is
	/// registered to be notified when new read events are pending.
	pub fn poll_read_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		self.register();
		self.registration.poll_read_ready(context)
	}

	/// Try a write operation with mutable IO
	///
	/// If write operation fails make sure to get notified when write readiness is signalled.
	pub fn try_mut_write<F, T>(&mut self, context: &mut Context<'_>, mut write_op: F) -> Poll<io::Result<T>>
	where
		F: FnMut(&mut E) -> Poll<io::Result<T>>,
	{
		if let Poll::Ready(v) = write_op(self.io_mut()) {
			return Poll::Ready(v);
		}
		self.register();
		futures_util::ready!(self.registration.poll_write_ready(context))?;
		if let Poll::Ready(v) = write_op(self.io_mut()) {
			return Poll::Ready(v);
		}
		// registration said we're ready, but write_op failed
		// come back later to try again
		context.waker().wake_by_ref();
		Poll::Pending
	}

	/// Clears all pending write events (and returns them)
	///
	/// If no events were pending (and possibly even if there were) the waker in `context` is
	/// registered to be notified when new write events are pending.
	pub fn poll_write_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		self.register();
		self.registration.poll_write_ready(context)
	}

	/// Retrieve reference to the contained IO
	pub fn io_ref(&self) -> &E {
		self.registration.io_ref()
	}

	/// Retrieve mutable reference to the contained IO
	pub fn io_mut(&mut self) -> &mut E {
		self.registration.io_mut()
	}

	/// Retrieve reactor handle this is (going to) be bound to.
	///
	/// Returns an unbound `LazyHandle` if not registered and not explicitly bound itself.
	pub fn handle(&self) -> LazyHandle {
		if self.handle.is_bound() {
			self.handle.clone()
		} else {
			self.registration.handle()
		}
	}

	/// Detach inner io from reactor and extract it.
	pub fn into_inner(self) -> E {
		self.registration.into_inner()
	}
}


#[cfg(unix)]
mod platform {
	pub fn hup() -> mio::Ready {
		mio::unix::UnixReady::hup().into()
	}
}

#[cfg(not(unix))]
mod platform {
	pub fn hup() -> mio::Ready {
		mio::Ready::empty()
	}
}

impl<R: mio::Evented + io::Read + Unpin> futures_io::AsyncRead for PollEvented<R> {
	fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
		self.try_mut_read(cx, |io| {
			async_io(|| io.read(buf))
		})
	}
}

impl<R: mio::Evented + io::Write + Unpin> futures_io::AsyncWrite for PollEvented<R> {
	fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
		self.try_mut_write(cx, |io| {
			async_io(|| io.write(buf))
		})
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		self.try_mut_write(cx, |io| {
			async_io(|| io.flush())
		})
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		type K = io::ErrorKind;
		if let Err(e) = futures_core::ready!(self.as_mut().poll_flush(cx)) {
			match e.kind() {
				K::BrokenPipe | K::ConnectionAborted | K::ConnectionReset | K::NotConnected | K::UnexpectedEof => (),
				_ => return Poll::Ready(Err(e)),
			}
		}
		Poll::Ready(Ok(()))
	}
}
