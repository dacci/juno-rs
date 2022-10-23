mod socks;

use anyhow::{anyhow, Error, Result};
use tokio::net::TcpStream;
use tower::util::BoxCloneService;

pub type Service = BoxCloneService<TcpStream, (), Error>;

pub fn create_service(provider: &str) -> Result<Service> {
    match provider {
        "socks" => Ok(BoxCloneService::new(socks::provider::Service::new())),
        _ => Err(anyhow!("unknown provider: `{provider}`")),
    }
}
