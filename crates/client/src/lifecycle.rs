//! Foreground process lifecycle adapters.

use async_trait::async_trait;
use synveil_client_sync::DesktopLifecycleEvent;

use crate::DesktopAdapterError;

/// Narrow process lifecycle source shared by Linux and Windows.
#[async_trait]
pub trait ProcessLifecycleSource: Send {
    async fn next_event(&mut self) -> Result<DesktopLifecycleEvent, DesktopAdapterError>;

    async fn stop(&mut self) -> Result<(), DesktopAdapterError>;
}

/// Native foreground lifecycle source.
///
/// Linux listens for both SIGINT and SIGTERM. Windows uses Tokio's console
/// Ctrl-C support. No service manager, tray integration, or forced process
/// termination is involved.
pub struct NativeProcessLifecycleSource {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    stopped: bool,
}

impl NativeProcessLifecycleSource {
    pub fn new() -> Result<Self, DesktopAdapterError> {
        #[cfg(unix)]
        {
            let interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .map_err(|_| DesktopAdapterError::Initialization)?;
            let terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|_| DesktopAdapterError::Initialization)?;
            Ok(Self {
                interrupt,
                terminate,
                stopped: false,
            })
        }

        #[cfg(not(unix))]
        {
            Ok(Self { stopped: false })
        }
    }
}

#[async_trait]
impl ProcessLifecycleSource for NativeProcessLifecycleSource {
    async fn next_event(&mut self) -> Result<DesktopLifecycleEvent, DesktopAdapterError> {
        if self.stopped {
            return Err(DesktopAdapterError::Stopped);
        }

        #[cfg(unix)]
        {
            tokio::select! {
                signal = self.interrupt.recv() => signal
                    .map(|_| DesktopLifecycleEvent::ShutdownRequested)
                    .ok_or(DesktopAdapterError::Closed),
                signal = self.terminate.recv() => signal
                    .map(|_| DesktopLifecycleEvent::ShutdownRequested)
                    .ok_or(DesktopAdapterError::Closed),
            }
        }

        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .map(|_| DesktopLifecycleEvent::ShutdownRequested)
                .map_err(|_| DesktopAdapterError::Closed)
        }
    }

    async fn stop(&mut self) -> Result<(), DesktopAdapterError> {
        self.stopped = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_lifecycle_stop_is_idempotent_and_closes_the_source() {
        let mut source = NativeProcessLifecycleSource::new().expect("native lifecycle source");
        source.stop().await.expect("first stop");
        source.stop().await.expect("repeated stop");
        assert_eq!(source.next_event().await, Err(DesktopAdapterError::Stopped));
    }
}
