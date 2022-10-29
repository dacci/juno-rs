use super::*;
use crate::Dialer;
use anyhow::anyhow;
use future::BoxFuture;
use futures::prelude::*;
use std::sync::Arc;
use std::task;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct Service {
    dialer: Arc<Dialer>,
}

impl Service {
    pub fn new(dialer: Dialer) -> Self {
        Self {
            dialer: Arc::new(dialer),
        }
    }
}

impl tower::Service<TcpStream> for Service {
    type Response = ();
    type Error = anyhow::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut stream: TcpStream) -> Self::Future {
        let dialer = Arc::clone(&self.dialer);

        async move {
            match stream.read_u8().await? {
                4 => v4::handle_request(stream, dialer).err_into().await,
                5 => v5::handle_request(stream, dialer).err_into().await,
                ver => Err(anyhow!("illegal protocol version `{ver}`")),
            }
        }
        .boxed()
    }
}
