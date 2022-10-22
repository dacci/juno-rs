use anyhow::{anyhow, Error, Result};
use tokio::net::TcpStream;
use tower::util::BoxCloneService;

pub type Service = BoxCloneService<TcpStream, (), Error>;

pub fn create_service(provider: &str) -> Result<Service> {
    #[allow(clippy::match_single_binding)]
    match provider {
        _ => Err(anyhow!("unknown provider: `{provider}`")),
    }
}
