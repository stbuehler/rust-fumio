use crate::net::TcpConnectFuture;
use crate::reactor::{LazyHandle, PollEvented};
use mio::net::TcpStream as MioTcpStream;
use std::io;
use std::net::{Shutdown, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

/// A TCP connection
#[derive(Debug)]
#[must_use = "A TCP stream does nothing if not actually used"]
pub struct TcpStream {
	pub(super) mio_stream: PollEvented<MioTcpStream>,
}

impl TcpStream {
	/// Wraps an already connected tcp stream
	pub fn from_std(stream: std::net::TcpStream, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_stream: PollEvented::new(MioTcpStream::from_stream(stream)?, handle),
		})
	}

	/// Wraps an already connected tcp stream
	pub fn from_mio(stream: mio::net::TcpStream, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_stream: PollEvented::new(stream, handle),
		})
	}

	/// Create a new TCP connection to the given target.
	pub fn connect(target: SocketAddr) -> io::Result<TcpConnectFuture> {
		Self::connect_with(target, LazyHandle::new())
	}

	/// Create a new TCP connection to the given target.
	pub fn connect_with(target: SocketAddr, handle: LazyHandle) -> io::Result<TcpConnectFuture> {
		let builder;
		match target {
			SocketAddr::V4(_) => {
				builder = net2::TcpBuilder::new_v4()?;
				#[cfg(windows)]
				builder.bind(std::net::SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0))?;
			}
			SocketAddr::V6(_) => {
				builder = net2::TcpBuilder::new_v6()?;
				#[cfg(windows)]
				builder.bind(std::net::SocketAddrV6::new(std::net::Ipv6Addr::UNSPECIFIED, 0, 0, 0))?;
			},
		};
		Self::connect_builder(builder, target, handle)
	}

	/// Create a new TCP connection to the given target using a prepared socket.
	#[allow(clippy::needless_pass_by_value)] // builders should actually be consumed, even if net2 screwed this up
	pub fn connect_builder(builder: net2::TcpBuilder, target: SocketAddr, handle: LazyHandle) -> io::Result<TcpConnectFuture> {
		let stream = Self {
			mio_stream: PollEvented::new(MioTcpStream::connect_stream(builder.to_tcp_stream()?, &target)?, handle),
		};
		Ok(TcpConnectFuture::new(stream))
	}
}

impl std::convert::TryFrom<std::net::TcpStream> for TcpStream {
	type Error = io::Error;

	fn try_from(s: std::net::TcpStream) -> io::Result<Self> {
		Self::from_std(s, LazyHandle::new())
	}
}

impl std::convert::TryFrom<mio::net::TcpStream> for TcpStream {
	type Error = io::Error;

	fn try_from(s: mio::net::TcpStream) -> io::Result<Self> {
		Self::from_mio(s, LazyHandle::new())
	}
}

impl futures_io::AsyncRead for TcpStream {
	fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
		Pin::new(&mut self.mio_stream).poll_read(cx, buf)
	}
}

impl futures_io::AsyncWrite for TcpStream {
	fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
		Pin::new(&mut self.mio_stream).poll_write(cx, buf)
	}

	fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		Pin::new(&mut self.mio_stream).poll_flush(cx)
	}

	fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		futures_core::ready!(Pin::new(&mut self.mio_stream).poll_close(cx))?;
		self.mio_stream.io_mut().shutdown(Shutdown::Write)?;
		Poll::Ready(Ok(()))
	}
}
