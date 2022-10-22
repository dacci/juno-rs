use futures::future::BoxFuture;
use futures::prelude::*;
use hyper::client::conn::Builder;
use hyper::header::{HeaderName, PROXY_AUTHORIZATION};
use hyper::server::conn::Http;
use hyper::{Body, Method, Request, Response, StatusCode};
use log::error;
use std::task;
use tokio::net::TcpStream;
use tower::service_fn;

#[derive(Clone, Default)]
pub struct Service {}

impl Service {
    pub fn new() -> Self {
        Self::default()
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
            .serve_connection(stream, service_fn(proxy))
            .with_upgrades()
            .err_into()
            .boxed()
    }
}

async fn proxy(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if Method::CONNECT == req.method() {
        let addr = if let Some(authority) = req.uri().authority() {
            authority.to_string()
        } else {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("CONNECT must be to a socket address"))
                .unwrap());
        };

        let mut server = match TcpStream::connect(addr).await {
            Ok(s) => s,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(e.to_string()))
                    .unwrap())
            }
        };

        tokio::task::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(mut client) => {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                }
                Err(e) => {
                    error!("Could not upgrade: {e}");
                }
            };
        });

        Ok(Response::new(Body::empty()))
    } else {
        let addr = if let Some(authority) = req.uri().authority() {
            format!(
                "{}:{}",
                authority.host(),
                authority.port_u16().unwrap_or(80)
            )
        } else {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap());
        };

        let stream = match TcpStream::connect(addr).await {
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

                let req = transform_request(req);
                sender.send_request(req).await
            }
            Err(e) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(e.to_string()))
                .unwrap()),
        }
    }
}

#[allow(clippy::declare_interior_mutable_const)]
const PROXY_CONNECTION: HeaderName = HeaderName::from_static("proxy-connection");

fn transform_request<T>(mut req: Request<T>) -> Request<T> {
    *req.uri_mut() = req
        .uri()
        .path_and_query()
        .map(Clone::clone)
        .map(Into::into)
        .unwrap_or_default();

    let map = req.headers_mut();
    map.remove(PROXY_CONNECTION);
    map.remove(PROXY_AUTHORIZATION);

    req
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

        let req = transform_request(req);
        assert_eq!(req.uri(), "/index.html");
        assert!(!req.headers().contains_key("Proxy-Connection"));
    }
}
