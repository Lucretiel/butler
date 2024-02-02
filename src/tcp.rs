use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use mio::Interest;

use crate::Runtime;

impl Runtime {
    pub fn connect(&self, address: SocketAddr) -> Connect<'_> {
        let stream = mio::net::TcpStream::connect(address).and_then(|mut stream| {
            self.poll
                .registry()
                .register(&mut stream, crate::BUTLER_WAKER_TOKEN, Interest::WRITABLE)
                .map(|()| stream)
        });

        Connect {
            runtime: self,
            partial_connection: Some(stream),
        }
    }
}

/// Future representing creating a new connection
pub struct Connect<'a> {
    runtime: &'a Runtime,
    partial_connection: Option<io::Result<mio::net::TcpStream>>,
}

impl Future for Connect<'_> {
    type Output = io::Result<TcpStream>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let stream = match self.get_mut().partial_connection.take() {
            None => panic!("polled `connect` after it returned a value"),
            Some(Err(err)) => return Poll::Ready(Err(err)),
            Some(Ok(stream)) => stream,
        };

        match stream.peer_addr() {
            Ok(_) => Poll::Ready(Ok(TcpStream { mio_stream: stream })),
            Err(err)
                if err.kind() == io::ErrorKind::NotConnected
                    || err.raw_os_error() == Some(libc::EINPROGRESS) =>
            {
                // SET UP THE WAKER
            }
        }
    }
}

pub struct TcpStream {
    mio_stream: mio::net::TcpStream,
}
