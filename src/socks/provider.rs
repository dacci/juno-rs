use anyhow::Error;
use futures::future::BoxFuture;
use futures::{FutureExt, TryFutureExt};
use std::task;
use tokio::net::TcpStream;

#[derive(Clone, Default)]
pub struct Service {}

impl Service {
    pub fn new() -> Self {
        Self::default()
    }
}

impl tower::Service<TcpStream> for Service {
    type Response = ();
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, stream: TcpStream) -> Self::Future {
        super::handle_request(stream).err_into().boxed()
    }
}
