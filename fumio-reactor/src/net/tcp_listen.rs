use crate::helper::async_io;
use crate::net::TcpStream;
use crate::reactor::{LazyHandle, PollEvented};
use futures_core::Stream;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A TCP listening socket.
#[derive(Debug)]
#[must_use = "A TCP listener does nothing if not actually used"]
pub struct TcpListener {
	mio_listener: PollEvented<mio::net::TcpListener>,
}

impl TcpListener {
	/// Create builder with default options, but doesn't bind yet.
	///
	/// To create a `TcpListener` from a builder go through the `std::net::TcpListener` created by
	/// `builder.listen(...)?`.
	pub fn default_builder_for(local: &SocketAddr) -> io::Result<net2::TcpBuilder> {
		let builder;
		match local {
			SocketAddr::V4(_) => {
				builder = net2::TcpBuilder::new_v4()?;
			}
			SocketAddr::V6(a) => {
				builder = net2::TcpBuilder::new_v6()?;
				if a.ip().is_unspecified() {
					// always try to disable only_v6
					let _ = builder.only_v6(false);
				}
			}
		}
		builder.reuse_address(true)?;
		Ok(builder)
	}

	/// Binds a new listener to IPv6 `[::]` with V6_ONLY=false (i.e. also listen on IPv4) to the
	/// given port.
	///
	/// If the port is 0 the OS will select a random port.
	pub fn bind_port(port: u16) -> io::Result<Self> {
		Self::bind(std::net::SocketAddrV6::new(std::net::Ipv6Addr::UNSPECIFIED, port, 0, 0).into())
	}

	/// Binds a new listener to IPv4 `0.0.0.0` to the given port.
	///
	/// If the port is 0 the OS will select a random port.
	pub fn bind_ipv4_port(port: u16) -> io::Result<Self> {
		Self::bind(std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, port).into())
	}

	/// Bind a new listener to the specified address
	///
	/// Uses `default_builder_for(addr)` to construct a builder, binds the address and listens with
	/// a backlog of up to 1024 connections.
	pub fn bind(local: SocketAddr) -> io::Result<Self> {
		Self::bind_with(local, LazyHandle::new())
	}

	/// Bind a new listener to the specified address
	///
	/// Uses `default_builder_for(addr)` to construct a builder, binds the address and listens with
	/// a backlog of up to 1024 connections.
	pub fn bind_with(local: SocketAddr, handle: LazyHandle) -> io::Result<Self> {
		let builder = Self::default_builder_for(&local)?;
		builder.bind(&local)?;
		Self::from_std(builder.listen(1024)?, handle)
	}

	/// Wraps a `std` listener
	pub fn from_std(listener: std::net::TcpListener, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_listener: PollEvented::new(mio::net::TcpListener::from_std(listener)?, handle),
		})
	}

	/// Wraps a `mio` listener
	pub fn from_mio(listener: mio::net::TcpListener, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_listener: PollEvented::new(listener, handle),
		})
	}

	/// Returns the local socket address of this listener.
	pub fn local_addr(&self) -> io::Result<SocketAddr> {
		self.mio_listener.io_ref().local_addr()
	}

	/// Creates a new independently owned handle to the underlying socket.
	///
	/// The new listener isn't registered to a reactor yet.
	pub fn try_clone(&self) -> io::Result<Self> {
		self.try_clone_with(LazyHandle::new())
	}

	/// Creates a new independently owned handle to the underlying socket.
	pub fn try_clone_with(&self, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_listener: PollEvented::new(self.mio_listener.io_ref().try_clone()?, handle),
		})
	}

	/// Stream of incoming `(TcpStream, SocketAddr)` connections.
	pub fn incoming(&mut self) -> TcpIncoming<'_> {
		TcpIncoming { listener: self }
	}

	/// Stream of incoming `(std::net::TcpStream, SocketAddr)` connections.
	pub fn incoming_std(&mut self) -> TcpIncomingStd<'_> {
		TcpIncomingStd { listener: self }
	}

	/// Accept a new connection or register context.
	pub fn poll_accept(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<(TcpStream, SocketAddr)>> {
		let (stream, addr) = futures_core::ready!(self.mio_listener.try_mut_read(cx, |io| {
				async_io(|| io.accept())
		}))?;
		let stream = TcpStream { mio_stream: PollEvented::new(stream, LazyHandle::new()) };
		Poll::Ready(Ok((stream, addr)))
	}

	/// Accept a new `std` connection or register context.
	pub fn poll_accept_std(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<(std::net::TcpStream, SocketAddr)>> {
		self.mio_listener.try_mut_read(cx, |io| {
				async_io(|| io.accept_std())
		})
	}
}

impl std::convert::TryFrom<std::net::TcpListener> for TcpListener {
	type Error = io::Error;

	fn try_from(l: std::net::TcpListener) -> io::Result<Self> {
		Self::from_std(l, LazyHandle::new())
	}
}

impl std::convert::TryFrom<mio::net::TcpListener> for TcpListener {
	type Error = io::Error;

	fn try_from(l: mio::net::TcpListener) -> io::Result<Self> {
		Self::from_mio(l, LazyHandle::new())
	}
}

/// Stream of incoming connections (can also be polled as single future to get the next connection,
/// as the stream never ends).
#[must_use = "futures and streams do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct TcpIncoming<'a> {
	listener: &'a mut TcpListener,
}

impl Future for TcpIncoming<'_> {
	type Output = io::Result<(TcpStream, SocketAddr)>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.listener.poll_accept(cx)
	}
}

impl Stream for TcpIncoming<'_> {
	type Item = io::Result<(TcpStream, SocketAddr)>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.listener.poll_accept(cx).map(Some)
	}
}

/// Stream of incoming `std` connections (can also be polled as single future to get the next
/// connection, as the stream never ends).
#[must_use = "futures and streams do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct TcpIncomingStd<'a> {
	listener: &'a mut TcpListener,
}

impl Future for TcpIncomingStd<'_> {
	type Output = io::Result<(std::net::TcpStream, SocketAddr)>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		self.listener.poll_accept_std(cx)
	}
}

impl Stream for TcpIncomingStd<'_> {
	type Item = io::Result<(std::net::TcpStream, SocketAddr)>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.listener.poll_accept_std(cx).map(Some)
	}
}
