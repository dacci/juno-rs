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

    let listeners = activate_or_bind_all(&args)
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

#[cfg(target_os = "macos")]
async fn activate_or_bind_all(args: &Args) -> Result<Vec<TcpListener>> {
    if let Some(name) = &args.launchd {
        launchd::activate_socket::<std::net::TcpListener>(name.as_str())
            .context("failed to activate from launchd")?
            .into_iter()
            .map(|l| TcpListener::from_std(l).map_err(|e| e.into()))
            .collect()
    } else {
        bind_all(args).await
    }
}

#[cfg(all(target_os = "linux", feature = "systemd"))]
async fn activate_or_bind_all(args: &Args) -> Result<Vec<TcpListener>> {
    use std::os::unix::io::FromRawFd;

    if args.systemd {
        systemd::daemon::listen_fds(true)
            .context("failed to activate from systemd")?
            .iter()
            .map(|fd| unsafe { std::net::TcpListener::from_raw_fd(fd) })
            .map(|l| TcpListener::from_std(l).map_err(|e| e.into()))
            .collect()
    } else {
        bind_all(args).await
    }
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", feature = "systemd"))))]
async fn activate_or_bind_all(args: &Args) -> Result<Vec<TcpListener>> {
    bind_all(args).await
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

#[cfg(target_os = "macos")]
mod launchd {
    use libc::{c_char, c_int, size_t};
    use std::ffi::CString;
    use std::io;
    use std::os::unix::io::FromRawFd;
    use std::ptr::null_mut;
    use std::slice::from_raw_parts;

    pub fn activate_socket<T: FromRawFd>(name: impl Into<Vec<u8>>) -> io::Result<Vec<T>> {
        extern "C" {
            fn launch_activate_socket(
                name: *const c_char,
                fds: *mut *mut c_int,
                cnt: *mut size_t,
            ) -> c_int;
        }

        let name = CString::new(name)?;
        let mut fds = null_mut();
        let mut cnt = 0;
        match unsafe { launch_activate_socket(name.as_ptr(), &mut fds, &mut cnt) } {
            0 => {
                let listeners = unsafe { from_raw_parts(fds, cnt) }
                    .iter()
                    .map(|fd| unsafe { T::from_raw_fd(*fd) })
                    .collect();
                unsafe { libc::free(fds as _) };
                Ok(listeners)
            }
            code => Err(io::Error::from_raw_os_error(code)),
        }
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
