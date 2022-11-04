use crate::Dialer;
use future::BoxFuture;
use futures::prelude::*;
use hyper::client::conn::Builder;
use hyper::header::{HeaderName, PROXY_AUTHORIZATION};
use hyper::server::conn::Http;
use hyper::{Body, Method, Request, Response, StatusCode};
use std::sync::Arc;
use std::task;
use tokio::net::TcpStream;
use tracing::error;

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

    fn call(&mut self, stream: TcpStream) -> Self::Future {
        Http::new()
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .serve_connection(stream, Session::new(&self.dialer))
            .with_upgrades()
            .err_into()
            .boxed()
    }
}

#[cfg_attr(test, derive(Default))]
struct Session {
    dialer: Arc<Dialer>,
}

impl Session {
    fn new(dialer: &Arc<Dialer>) -> Self {
        Self {
            dialer: Arc::clone(dialer),
        }
    }

    fn handle_connect(
        &self,
        req: Request<Body>,
    ) -> impl Future<Output = Result<Response<Body>, hyper::Error>> {
        let res = if let Some(authority) = req.uri().authority() {
            let addr = authority.to_string();
            let dialer = Arc::clone(&self.dialer);
            Ok((addr, dialer))
        } else {
            Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("CONNECT must be to a socket address"))
                .unwrap())
        };

        async move {
            let (addr, dialer) = match res {
                Ok(req) => req,
                Err(res) => return Ok(res),
            };

            let mut server = match dialer.dial(addr).await {
                Ok(server) => server,
                Err(e) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from(e.to_string()))
                        .unwrap());
                }
            };

            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(mut client) => {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                    }
                    Err(e) => {
                        error!("Failed to upgrade: {e}");
                    }
                };
            });

            Ok(Response::new(Body::empty()))
        }
    }

    #[allow(clippy::declare_interior_mutable_const)]
    const PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");

    fn transform_request<T>(&self, mut req: Request<T>) -> Request<T> {
        *req.uri_mut() = req
            .uri()
            .path_and_query()
            .map(Clone::clone)
            .map(Into::into)
            .unwrap_or_default();

        let map = req.headers_mut();
        map.remove(Self::PROXY_CONNECTION);
        map.remove(PROXY_AUTHORIZATION);

        req
    }

    fn handle_request(
        &self,
        req: Request<Body>,
    ) -> impl Future<Output = Result<Response<Body>, hyper::Error>> {
        let res = if let Some(authority) = req.uri().authority() {
            let addr = format!(
                "{}:{}",
                authority.host(),
                authority.port_u16().unwrap_or(80)
            );
            let dialer = Arc::clone(&self.dialer);
            let req = self.transform_request(req);
            Ok((addr, dialer, req))
        } else {
            Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap())
        };

        async move {
            let (addr, dialer, req) = match res {
                Ok(req) => req,
                Err(res) => return Ok(res),
            };

            let stream = match dialer.dial(addr).await {
                Ok(stream) => stream,
                Err(e) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from(e.to_string()))
                        .unwrap());
                }
            };

            match Builder::new()
                .http1_preserve_header_case(true)
                .http1_title_case_headers(true)
                .handshake(stream)
                .await
            {
                Ok((mut sender, conn)) => {
                    tokio::task::spawn(async move {
                        if let Err(err) = conn.await {
                            error!("Connection failed: {:?}", err);
                        }
                    });

                    sender.send_request(req).await
                }
                Err(e) => Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(e.to_string()))
                    .unwrap()),
            }
        }
    }
}

impl tower::Service<Request<Body>> for Session {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if Method::CONNECT == req.method() {
            self.handle_connect(req).boxed()
        } else {
            self.handle_request(req).boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let req = Request::builder()
            .method(Method::GET)
            .uri("https://example.org/index.html")
            .header("Proxy-Connection", "keep-alive")
            .body(())
            .unwrap();

        let req = Session::default().transform_request(req);
        assert_eq!(req.uri(), "/index.html");
        assert!(!req.headers().contains_key("Proxy-Connection"));
    }
}
