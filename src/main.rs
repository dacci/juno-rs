mod sys;

use anyhow::{Context as _, Result};
use clap::Parser;
use futures::prelude::*;
use juno::Service;
use log::{debug, info, warn};
use tokio::net::TcpListener;
use tower::{Service as _, ServiceExt};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Specifies an address to listen on for a stream.
    #[arg(short, long, value_name = "ADDRESS")]
    #[cfg_attr(target_os = "macos", arg(required_unless_present = "launchd"))]
    #[cfg_attr(
        all(target_os = "linux", feature = "systemd"),
        arg(required_unless_present = "systemd")
    )]
    #[cfg_attr(
        not(any(target_os = "macos", all(target_os = "linux", feature = "systemd"))),
        arg(required = true)
    )]
    listen_stream: Vec<String>,

    /// Specifies the name of the socket entry in the service's Sockets dictionary.
    #[cfg(target_os = "macos")]
    #[arg(long, value_name = "NAME", conflicts_with = "listen_stream")]
    launchd: Option<String>,

    /// Runs in systemd socket activation mode.
    #[cfg(all(target_os = "linux", feature = "systemd"))]
    #[arg(long, value_name = "NAME", conflicts_with = "listen_stream")]
    systemd: bool,

    /// Specifies the name of the service provider.
    #[arg(short, long, value_name = "NAME", required = true)]
    provider: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let args: Args = Args::parse();

    let service = juno::create_service(&args.provider)?;

    let listeners = bind_all(&args)
        .await?
        .into_iter()
        .map(|l| listen(l, service.clone()));

    tokio::select! {
        r = future::try_join_all(listeners) => {
            r?;
        },
        r = sys::recv_signal() => r?,
    }

    Ok(())
}

async fn bind_all(args: &Args) -> Result<Vec<TcpListener>> {
    #[cfg(target_os = "macos")]
    if let Some(name) = &args.launchd {
        return sys::activate_socket(name);
    }

    #[cfg(all(target_os = "linux", feature = "systemd"))]
    if args.systemd {
        return sys::activate_socket();
    }

    stream::iter(&args.listen_stream)
        .then(|addr| {
            TcpListener::bind(addr)
                .map(move |r| r.with_context(|| format!("failed to bind to `{addr}`")))
        })
        .try_collect()
        .await
}

async fn listen(listener: TcpListener, mut service: Service) -> Result<()> {
    match listener.local_addr() {
        Ok(addr) => {
            info!("listening on {addr}");
        }
        Err(e) => {
            warn!("failed to get local address: {e}")
        }
    }

    loop {
        let (client, addr) = listener
            .accept()
            .map(|r| r.context("failed to accept connection"))
            .await?;
        debug!("connected from {addr}");
        tokio::task::spawn(service.ready().await?.call(client));
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_launchd() {
        assert!(Args::try_parse_from(["", "-p", "provider", "-l", "host:port"]).is_ok());
        assert!(Args::try_parse_from(["", "-p", "provider", "--launchd", "name"]).is_ok());
        assert!(Args::try_parse_from([
            "",
            "-p",
            "provider",
            "-l",
            "host:port",
            "--launchd",
            "name"
        ])
        .is_err());
    }

    #[cfg(all(target_os = "linux", feature = "systemd"))]
    #[test]
    fn test_systemd() {
        assert!(Args::try_parse_from(["", "-p", "provider", "-l", "host:port"]).is_ok());
        assert!(Args::try_parse_from(["", "-p", "provider", "--systemd"]).is_ok());
        assert!(
            Args::try_parse_from(["", "-p", "provider", "-l", "host:port", "--systemd"]).is_err()
        );
        assert!(Args::try_parse_from(["", "-p", "provider"]).is_err());
    }
}
