use crate::helper::async_io;
use crate::reactor::{LazyHandle, PollEvented};
use mio::net::UdpSocket as MioUdpSocket;
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

/// A UDP socket
#[derive(Debug)]
#[must_use = "A UDP socket does nothing if not actually used"]
pub struct UdpSocket {
	pub(super) mio_socket: PollEvented<MioUdpSocket>,
}

impl UdpSocket {
	/// Create builder with default options, but doesn't bind yet.
	///
	/// To create a `UdpSocket` from a builder go through the `std::net::UdpSocket` created by
	/// `builder.bind(...)?`.
	pub fn default_builder_for(local: &SocketAddr) -> io::Result<net2::UdpBuilder> {
		let builder;
		match local {
			SocketAddr::V4(_) => {
				builder = net2::UdpBuilder::new_v4()?;
			}
			SocketAddr::V6(a) => {
				builder = net2::UdpBuilder::new_v6()?;
				if a.ip().is_unspecified() {
					// always try to disable only_v6
					let _ = builder.only_v6(false);
				}
			}
		}
		builder.reuse_address(true)?;
		Ok(builder)
	}

	/// Wraps an already bound tcp stream
	pub fn from_std(stream: std::net::UdpSocket, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_socket: PollEvented::new(MioUdpSocket::from_socket(stream)?, handle),
		})
	}

	/// Wraps an already bound tcp stream
	pub fn from_mio(stream: mio::net::UdpSocket, handle: LazyHandle) -> io::Result<Self> {
		Ok(Self {
			mio_socket: PollEvented::new(stream, handle),
		})
	}

	/// Binds a new UDP socket to IPv6 `[::]` with V6_ONLY=false (i.e. also listen on IPv4) and the
	/// given port.
	///
	/// If the port is 0 the OS will select a random port.
	pub fn bind_port(port: u16) -> io::Result<Self> {
		Self::bind(std::net::SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0).into())
	}

	/// Binds a new UDP socket to IPv4 `0.0.0.0` and the given port.
	///
	/// If the port is 0 the OS will select a random port.
	pub fn bind_ipv4_port(port: u16) -> io::Result<Self> {
		Self::bind(std::net::SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port).into())
	}

	/// Bind a new UDP socket to the specified address.
	pub fn bind(local: SocketAddr) -> io::Result<Self> {
		Self::bind_with(local, LazyHandle::new())
	}

	/// Bind a new UDP socket  to the specified address.
	pub fn bind_with(local: SocketAddr, handle: LazyHandle) -> io::Result<Self> {
		let builder = Self::default_builder_for(&local)?;
		Self::from_std(builder.bind(&local)?, handle)
	}

	/// Returns the local socket address of this socket.
	pub fn local_addr(&self) -> io::Result<SocketAddr> {
		self.mio_socket.io_ref().local_addr()
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
			mio_socket: PollEvented::new(self.mio_socket.io_ref().try_clone()?, handle),
		})
	}

	/// Receives data from the socket. On success, returns the number of bytes read and the address from whence the data came.
	pub fn poll_recv_from(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<(usize, SocketAddr)>> {
		// use mutable (although io.recv_from doesn't need it), because only one context can get registered;
		// shared ownership isn't useful.
		self.mio_socket.try_mut_read(cx, |io| {
			async_io(|| io.recv_from(buf))
		})
	}

	/// Receives data from the socket. On success, completes with the number of bytes read and the
	/// address from whence the data came.
	pub fn recv_from<'a>(&'a mut self, buf: &'a mut [u8]) -> UdpRecvFrom<'a> {
		UdpRecvFrom {
			socket: self,
			buf,
		}
	}

	/// Sends data on the socket to the given address. On success, returns the number of bytes written.
	pub fn poll_send_to(&mut self, cx: &mut Context<'_>, buf: &[u8], target: &SocketAddr) -> Poll<io::Result<usize>> {
		// use mutable (although io.send_to doesn't need it), because only one context can get registered;
		// shared ownership isn't useful.
		self.mio_socket.try_mut_write(cx, |io| {
			async_io(|| io.send_to(buf, target))
		})
	}

	/// Sends data on the socket to the given address. On success, completes with the number of
	/// bytes written.
	pub fn send_to<'a>(&'a mut self, buf: &'a [u8], target: &'a SocketAddr) -> UdpSendTo<'a> {
		UdpSendTo {
			socket: self,
			buf,
			target,
		}
	}

/*
	// connected UDP sockets should get a separate type?

	/// Connects the UDP socket setting the default destination for `send` and limiting packets
	/// that are read via `recv` from the address specified in `target`.
	pub fn connect(self, target: SocketAddr) -> Result<ConnectedUdpSocket> { ... }

	/// Receives data from the socket previously bound with connect(). On success, returns the number of bytes read.
	pub fn poll_recv(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
		// use mutable (although io.recv doesn't need it), because only one context can get registered;
		// shared ownership isn't useful.
		self.mio_socket.try_mut_read(cx, |io| {
			async_io(|| io.recv(buf))
		})
	}

	/// Sends data on the socket to the address previously bound via connect(). On success, returns the number of bytes written.
	pub fn poll_send(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
		// use mutable (although io.send doesn't need it), because only one context can get registered;
		// shared ownership isn't useful.
		self.mio_socket.try_mut_write(cx, |io| {
			async_io(|| io.send(buf))
		})
	}
*/

	/// Sets the value of the `SO_BROADCAST` option for this socket.
	///
	/// When enabled, this socket is allowed to send packets to a broadcast address.
	pub fn set_broadcast(&self, on: bool) -> io::Result<()> {
		self.mio_socket.io_ref().set_broadcast(on)
	}

	/// Gets the value of the SO_BROADCAST option for this socket.
	///
	/// For more information about this option, see set_broadcast.
	pub fn broadcast(&self) -> io::Result<bool> {
		self.mio_socket.io_ref().broadcast()
	}

	/// Sets the value of the `IP_MULTICAST_LOOP` option for this socket.
	///
	/// If enabled, multicast packets will be looped back to the local socket.
	/// Note that this may not have any affect on IPv6 sockets.
	pub fn set_multicast_loop_v4(&self, on: bool) -> io::Result<()> {
		self.mio_socket.io_ref().set_multicast_loop_v4(on)
	}

	/// Gets the value of the `IP_MULTICAST_LOOP` option for this socket.
	///
	/// For more information about this option, see
	/// [`set_multicast_loop_v4`][link].
	///
	/// [link]: #method.set_multicast_loop_v4
	pub fn multicast_loop_v4(&self) -> io::Result<bool> {
		self.mio_socket.io_ref().multicast_loop_v4()
	}

	/// Sets the value of the `IP_MULTICAST_TTL` option for this socket.
	///
	/// Indicates the time-to-live value of outgoing multicast packets for
	/// this socket. The default value is 1 which means that multicast packets
	/// don't leave the local network unless explicitly requested.
	///
	/// Note that this may not have any affect on IPv6 sockets.
	pub fn set_multicast_ttl_v4(&self, ttl: u32) -> io::Result<()> {
		self.mio_socket.io_ref().set_multicast_ttl_v4(ttl)
	}

	/// Gets the value of the `IP_MULTICAST_TTL` option for this socket.
	///
	/// For more information about this option, see
	/// [`set_multicast_ttl_v4`][link].
	///
	/// [link]: #method.set_multicast_ttl_v4
	pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
		self.mio_socket.io_ref().multicast_ttl_v4()
	}

	/// Sets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
	///
	/// Controls whether this socket sees the multicast packets it sends itself.
	/// Note that this may not have any affect on IPv4 sockets.
	pub fn set_multicast_loop_v6(&self, on: bool) -> io::Result<()> {
		self.mio_socket.io_ref().set_multicast_loop_v6(on)
	}

	/// Gets the value of the `IPV6_MULTICAST_LOOP` option for this socket.
	///
	/// For more information about this option, see
	/// [`set_multicast_loop_v6`][link].
	///
	/// [link]: #method.set_multicast_loop_v6
	pub fn multicast_loop_v6(&self) -> io::Result<bool> {
		self.mio_socket.io_ref().multicast_loop_v6()
	}

	/// Sets the value for the `IP_TTL` option on this socket.
	///
	/// This value sets the time-to-live field that is used in every packet sent
	/// from this socket.
	pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
		self.mio_socket.io_ref().set_ttl(ttl)
	}

	/// Gets the value of the `IP_TTL` option for this socket.
	///
	/// For more information about this option, see [`set_ttl`][link].
	///
	/// [link]: #method.set_ttl
	pub fn ttl(&self) -> io::Result<u32> {
		self.mio_socket.io_ref().ttl()
	}

	/// Executes an operation of the `IP_ADD_MEMBERSHIP` type.
	///
	/// This function specifies a new multicast group for this socket to join.
	/// The address must be a valid multicast address, and `interface` is the
	/// address of the local interface with which the system should join the
	/// multicast group. If it's equal to `INADDR_ANY` then an appropriate
	/// interface is chosen by the system.
	pub fn join_multicast_v4(&self, multiaddr: Ipv4Addr, interface: Ipv4Addr) -> io::Result<()> {
		self.mio_socket.io_ref().join_multicast_v4(&multiaddr, &interface)
	}

	/// Executes an operation of the `IPV6_ADD_MEMBERSHIP` type.
	///
	/// This function specifies a new multicast group for this socket to join.
	/// The address must be a valid multicast address, and `interface` is the
	/// index of the interface to join/leave (or 0 to indicate any interface).
	pub fn join_multicast_v6(&self, multiaddr: Ipv6Addr, interface: u32) -> io::Result<()> {
		self.mio_socket.io_ref().join_multicast_v6(&multiaddr, interface)
	}

	/// Executes an operation of the `IP_DROP_MEMBERSHIP` type.
	///
	/// For more information about this option, see
	/// [`join_multicast_v4`][link].
	///
	/// [link]: #method.join_multicast_v4
	pub fn leave_multicast_v4(&self, multiaddr: Ipv4Addr, interface: Ipv4Addr) -> io::Result<()> {
		self.mio_socket.io_ref().leave_multicast_v4(&multiaddr, &interface)
	}

	/// Executes an operation of the `IPV6_DROP_MEMBERSHIP` type.
	///
	/// For more information about this option, see
	/// [`join_multicast_v6`][link].
	///
	/// [link]: #method.join_multicast_v6
	pub fn leave_multicast_v6(&self, multiaddr: Ipv6Addr, interface: u32) -> io::Result<()> {
		self.mio_socket.io_ref().leave_multicast_v6(&multiaddr, interface)
	}

	/// Get the value of the `SO_ERROR` option on this socket.
	///
	/// This will retrieve the stored error in the underlying socket, clearing
	/// the field in the process. This can be useful for checking errors between
	/// calls.
	pub fn take_error(&self) -> io::Result<Option<io::Error>> {
		self.mio_socket.io_ref().take_error()
	}
}

impl std::convert::TryFrom<std::net::UdpSocket> for UdpSocket {
	type Error = io::Error;

	fn try_from(s: std::net::UdpSocket) -> io::Result<Self> {
		Self::from_std(s, LazyHandle::new())
	}
}

impl std::convert::TryFrom<mio::net::UdpSocket> for UdpSocket {
	type Error = io::Error;

	fn try_from(s: mio::net::UdpSocket) -> io::Result<Self> {
		Self::from_mio(s, LazyHandle::new())
	}
}

/// Pending `recv_from` operation
#[must_use = "futures do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct UdpRecvFrom<'a> {
	socket: &'a mut UdpSocket,
	buf: &'a mut [u8],
}

impl Future for UdpRecvFrom<'_> {
	type Output = io::Result<(usize, SocketAddr)>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = self.get_mut();
		this.socket.poll_recv_from(cx, this.buf)
	}
}

/// Pending `send_to` operation
#[must_use = "futures do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct UdpSendTo<'a> {
	socket: &'a mut UdpSocket,
	buf: &'a [u8],
	target: &'a SocketAddr,
}

impl Future for UdpSendTo<'_> {
	type Output = io::Result<usize>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = self.get_mut();
		this.socket.poll_send_to(cx, this.buf, this.target)
	}
}
