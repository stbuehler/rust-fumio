use crate::net::TcpStream;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A future completing when a stream is ready to use (or failed).
#[must_use = "futures do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct TcpConnectFuture {
	stream: Option<TcpStream>,
}

impl TcpConnectFuture {
	pub(super) fn new(stream: TcpStream) -> Self {
		Self {
			stream: Some(stream),
		}
	}
}

impl Future for TcpConnectFuture {
	type Output = io::Result<TcpStream>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		futures_core::ready!(self.stream.as_mut().expect("can't poll TcpConnectFuture twice").mio_stream.poll_write_ready(cx))?;
		let stream = self.stream.take().unwrap();
		if let Some(e) = stream.mio_stream.io_ref().take_error()? {
			return Poll::Ready(Err(e));
		}
		Poll::Ready(Ok(stream))
	}
}
