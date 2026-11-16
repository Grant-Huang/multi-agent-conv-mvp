//! Shared startup coordination for real child-process HTTP tests.
use std::{path::Path, process::Child, time::Duration};
use tokio::sync::{Mutex, MutexGuard};

static STARTUP: Mutex<()> = Mutex::const_new(());

/// Serialize only the reserve/release/start handoff, not the test requests.
pub async fn startup_guard() -> MutexGuard<'static, ()> {
    STARTUP.lock().await
}

pub async fn wait_for_service(
    child: &mut Child,
    console: &Path,
    port: u16,
    mom: bool,
    timeout: Duration,
) -> Result<(), String> {
    let marker = if mom {
        format!("MOM simulation listening on 127.0.0.1:{port}.")
    } else {
        format!("event=\"server.started\" bind=127.0.0.1:{port}")
    };
    tokio::time::timeout(timeout, async {
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!(
                    "own child exited before readiness: {status}; {}",
                    std::fs::read_to_string(console).unwrap_or_default()
                ));
            }
            let output = std::fs::read_to_string(console).unwrap_or_default();
            if output.lines().any(|line| line.contains(&marker))
                && tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .is_ok()
                && child.try_wait().map_err(|e| e.to_string())?.is_none()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "own service readiness deadline exceeded".to_string())?
}
