mod http;
mod socks;

use anyhow::{anyhow, Error, Result};
use futures::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io;
use tokio::net::{lookup_host, TcpSocket, TcpStream, ToSocketAddrs};
use tower::util::BoxCloneService;

pub type Service = BoxCloneService<TcpStream, (), Error>;

pub fn create_service(provider: &str, dialer: Dialer) -> Result<Service> {
    match provider {
        "http" => Ok(Service::new(http::Service::new(dialer))),
        "socks" => Ok(Service::new(socks::provider::Service::new(dialer))),
        _ => Err(anyhow!("unknown provider: `{provider}`")),
    }
}

#[derive(Default)]
pub struct Dialer {
    bind_addr: Option<SocketAddr>,
}

impl Dialer {
    pub async fn bind(addr: impl AsRef<str>) -> io::Result<Self> {
        let bind_addr = lookup_host((addr.as_ref(), 0))
            .await?
            .next()
            .ok_or(io::ErrorKind::AddrNotAvailable)?;

        Ok(Self {
            bind_addr: Some(bind_addr),
        })
    }

    pub async fn dial(self: &Arc<Self>, host: impl ToSocketAddrs) -> io::Result<TcpStream> {
        let dials = lookup_host(host)
            .await?
            .map(move |addr| self.dial_one(addr).boxed());

        let (stream, _) = future::select_ok(dials).await?;
        Ok(stream)
    }

    async fn dial_one(self: &Arc<Self>, addr: SocketAddr) -> io::Result<TcpStream> {
        let sock = match addr {
            SocketAddr::V4(_) => TcpSocket::new_v4(),
            SocketAddr::V6(_) => TcpSocket::new_v6(),
        }?;

        if let Some(addr) = self.bind_addr {
            sock.bind(addr)?;
        }

        sock.connect(addr).await
    }
}
