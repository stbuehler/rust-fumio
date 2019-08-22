use std::io;
use std::task::Poll;

pub(crate) fn async_io<F, T>(mut op: F) -> Poll<io::Result<T>>
where
	F: FnMut() -> io::Result<T>
{
	loop {
		match op() {
			Ok(v) => return Poll::Ready(Ok(v)),
			Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
			Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Poll::Pending,
			Err(e) => return Poll::Ready(Err(e)),
		}
	}
}
