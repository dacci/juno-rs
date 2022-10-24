use anyhow::{Context as _, Result};
use std::os::unix::prelude::*;
use tokio::io;
use tokio::net::TcpListener;

pub async fn recv_signal() -> io::Result<()> {
    use tokio::signal::unix::*;

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = interrupt.recv() => {},
        _ = terminate.recv() => {},
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn activate_socket(name: &str) -> Result<Vec<TcpListener>> {
    launchd::activate_socket(name)
        .context("failed to activate from launchd")?
        .into_iter()
        .map(convert_socket)
        .collect()
}

#[cfg(target_os = "macos")]
mod launchd {
    use super::*;
    use libc::{c_char, c_int, size_t};
    use std::ffi::CString;
    use std::ptr::null_mut;
    use std::slice::from_raw_parts;

    pub fn activate_socket(name: impl Into<Vec<u8>>) -> io::Result<Vec<RawFd>> {
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
                let vec = unsafe { from_raw_parts(fds, cnt) }.to_vec();
                unsafe { libc::free(fds as _) };
                Ok(vec)
            }
            code => Err(io::Error::from_raw_os_error(code)),
        }
    }
}

#[cfg(all(target_os = "linux", feature = "systemd"))]
pub fn activate_socket() -> Result<Vec<TcpListener>> {
    systemd::daemon::listen_fds(true)
        .context("failed to activate from systemd")?
        .iter()
        .map(convert_socket)
        .collect()
}

trait FromStd<S> {
    fn from_std(s: S) -> io::Result<Self>
    where
        Self: Sized;
}

macro_rules! map_tokio_net {
    ($name:ident) => {
        impl FromStd<std::net::$name> for tokio::net::$name {
            fn from_std(s: std::net::$name) -> io::Result<Self> {
                tokio::net::$name::from_std(s)
            }
        }
    };
}

map_tokio_net!(TcpListener);

fn convert_socket<T, S>(fd: RawFd) -> Result<T>
where
    T: FromStd<S>,
    S: FromRawFd,
{
    set_non_blocking(fd)
        .context("failed to set in non-blocking mode")
        .map(|_| unsafe { FromRawFd::from_raw_fd(fd) })
        .and_then(|std| FromStd::from_std(std).context("failed to convert socket"))
}

fn set_non_blocking(fd: RawFd) -> io::Result<()> {
    let flags = match unsafe { libc::fcntl(fd, libc::F_GETFL) } {
        -1 => return Err(io::Error::last_os_error()),
        flags => flags,
    };

    if (flags & libc::O_NONBLOCK) == libc::O_NONBLOCK {
        return Ok(());
    }

    match unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } {
        -1 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}
