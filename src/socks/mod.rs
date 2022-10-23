pub(super) mod provider;
mod v4;
mod v5;

use std::net::{SocketAddrV4, SocketAddrV6};
use thiserror::Error;
use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

#[derive(Debug, Error)]
pub enum Error {
    #[error("need more data")]
    NeedMoreData,

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("{0}")]
    Io(#[from] io::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
    Raw(String, u16),
}

impl SocketAddr {
    fn v4(ip: u32, port: u16) -> Self {
        Self::V4(SocketAddrV4::new(ip.into(), port))
    }

    fn v6(ip: u128, port: u16) -> Self {
        Self::V6(SocketAddrV6::new(ip.into(), port, 0, 0))
    }

    fn raw(domain: String, port: u16) -> Self {
        Self::Raw(domain, port)
    }
}

async fn handle_request(mut stream: TcpStream) -> Result<()> {
    match stream.read_u8().await? {
        4 => v4::handle_request(stream).await,
        5 => v5::handle_request(stream).await,
        ver => Err(Error::Protocol(format!("illegal protocol version `{ver}`"))),
    }
}
