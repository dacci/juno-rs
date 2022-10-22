use anyhow::{Context as _, Result};
use clap::Parser;
use futures::prelude::*;
use juno::Service;
use log::{debug, info, warn};
use tokio::net::TcpListener;
use tower::Service as _;

#[cfg(unix)]
async fn recv_signal() -> Result<()> {
    use tokio::signal::unix::*;

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = interrupt.recv() => {},
        _ = terminate.recv() => {},
    }

    Ok(())
}

#[cfg(windows)]
async fn recv_signal() -> Result<()> {
    use tokio::signal::windows::*;

    let mut ctrl_c = ctrl_c()?;
    let mut ctrl_break = ctrl_break()?;
    let mut ctrl_close = ctrl_close()?;
    let mut ctrl_logoff = ctrl_logoff()?;
    let mut ctrl_shutdown = ctrl_shutdown()?;

    tokio::select! {
        _ = ctrl_c.recv() = {},
        _ = ctrl_break.recv() = {},
        _ = ctrl_close.recv() = {},
        _ = ctrl_logoff.recv() = {},
        _ = ctrl_shutdown.recv() = {},
    }

    Ok(())
}

#[cfg(not(any(unix, windows)))]
async fn recv_signal() -> Result<()> {
    tokio::signal::ctrl_c().err_into().await
}

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Specifies an address to listen on for a stream.
    #[arg(short, long, value_name = "ADDRESS", required = true)]
    listen_stream: Vec<String>,

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
        r = recv_signal() => r?,
    }

    Ok(())
}

async fn bind_all(args: &Args) -> Result<Vec<TcpListener>> {
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
        tokio::task::spawn(service.call(client));
    }
}
