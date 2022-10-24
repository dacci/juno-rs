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
    launchd::activate_socket::<std::net::TcpListener>(name)
        .context("failed to activate from launchd")?
        .into_iter()
        .map(|l| TcpListener::from_std(l).map_err(|e| e.into()))
        .collect()
}

#[cfg(target_os = "macos")]
mod launchd {
    use super::*;
    use libc::{c_char, c_int, size_t};
    use std::ffi::CString;
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

#[cfg(all(target_os = "linux", feature = "systemd"))]
pub fn activate_socket() -> Result<Vec<TcpListener>> {
    systemd::daemon::listen_fds(true)
        .context("failed to activate from systemd")?
        .iter()
        .map(|fd| unsafe { std::net::TcpListener::from_raw_fd(fd) })
        .map(|l| TcpListener::from_std(l).map_err(|e| e.into()))
        .collect()
}
