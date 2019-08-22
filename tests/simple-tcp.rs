#![feature(async_await)]

use fumio::net::{TcpListener, TcpStream};
use futures::prelude::*;

#[test]
fn main() {
	fumio::run(async {
		let mut l = TcpListener::bind_port(0)?;
		let server_addr = l.local_addr()?;
		println!("Serving on {}", server_addr);

		let client_task = async {
			let mut s = TcpStream::connect(server_addr)?.await?;
			s.write_all(b"hello server\n").await?;
			s.close().await?;
			let mut buf = String::new();
			s.read_to_string(&mut buf).await?;
			assert_eq!(buf, "hello client\n");
			Ok::<_, std::io::Error>(())
		};

		let serv_task = async {
			let (mut conn, clientaddr) = l.incoming().await?;
			println!("Incoming connection from {}", clientaddr);
			let mut buf = String::new();
			conn.read_to_string(&mut buf).await?;
			assert_eq!(buf, "hello server\n");
			conn.write_all(b"hello client\n").await?;
			conn.close().await?;
			Ok::<_, std::io::Error>(())
		};

		futures::try_join!(client_task, serv_task)
	}).unwrap();
}
