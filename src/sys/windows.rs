use tokio::io;

pub async fn recv_signal() -> io::Result<()> {
    use tokio::signal::windows::*;

    let mut ctrl_c = ctrl_c()?;
    let mut ctrl_break = ctrl_break()?;
    let mut ctrl_close = ctrl_close()?;
    let mut ctrl_logoff = ctrl_logoff()?;
    let mut ctrl_shutdown = ctrl_shutdown()?;

    tokio::select! {
        _ = ctrl_c.recv() => {},
        _ = ctrl_break.recv() => {},
        _ = ctrl_close.recv() => {},
        _ = ctrl_logoff.recv() => {},
        _ = ctrl_shutdown.recv() => {},
    }

    Ok(())
}
