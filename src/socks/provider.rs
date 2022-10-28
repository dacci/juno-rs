use crate::Dialer;
use anyhow::Error;
use future::BoxFuture;
use futures::prelude::*;
use std::sync::Arc;
use std::task;
use tokio::net::TcpStream;

#[derive(Clone, Default)]
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
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, stream: TcpStream) -> Self::Future {
        super::handle_request(stream, Arc::clone(&self.dialer))
            .err_into()
            .boxed()
    }
}
