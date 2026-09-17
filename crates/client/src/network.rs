//! Best-effort network-availability hint sources.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

#[cfg(target_os = "linux")]
use std::fs;

use async_trait::async_trait;

use crate::DesktopAdapterError;

pub const DEFAULT_DESKTOP_NETWORK_HINT_INTERVAL: Duration = Duration::from_secs(30);
const MIN_NETWORK_HINT_INTERVAL: Duration = Duration::from_millis(100);
const MAX_NETWORK_HINT_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub(crate) fn validate_network_hint_interval(
    interval: Duration,
) -> Result<(), crate::DesktopClientConfigError> {
    if !(MIN_NETWORK_HINT_INTERVAL..=MAX_NETWORK_HINT_INTERVAL).contains(&interval) {
        return Err(crate::DesktopClientConfigError::InvalidNetworkHintInterval);
    }
    Ok(())
}

/// Narrow network hint source consumed by the process runner.
#[async_trait]
pub trait NetworkHintSource: Send {
    async fn next_hint(&mut self) -> Result<(), DesktopAdapterError>;

    async fn stop(&mut self) -> Result<(), DesktopAdapterError>;
}

/// Lightweight native availability monitor.
///
/// The adapter deliberately does not promise reachability. On Linux it
/// fingerprints interface names and operational state under `/sys/class/net`;
/// on Windows it fingerprints the local address selected by a UDP route
/// lookup. It emits only a positive hint when that fingerprint changes. If a
/// platform cannot provide the cheap fingerprint, the process uses the
/// periodic safety fallback below.
pub struct NativeNetworkHintSource {
    interval: tokio::time::Interval,
    last_fingerprint: u64,
    stopped: bool,
}

impl NativeNetworkHintSource {
    pub fn new(interval: Duration) -> Result<Self, DesktopAdapterError> {
        if !(MIN_NETWORK_HINT_INTERVAL..=MAX_NETWORK_HINT_INTERVAL).contains(&interval) {
            return Err(DesktopAdapterError::Initialization);
        }
        let last_fingerprint = network_fingerprint()?;
        Ok(Self {
            interval: tokio::time::interval(interval),
            last_fingerprint,
            stopped: false,
        })
    }
}

#[async_trait]
impl NetworkHintSource for NativeNetworkHintSource {
    async fn next_hint(&mut self) -> Result<(), DesktopAdapterError> {
        if self.stopped {
            return Err(DesktopAdapterError::Stopped);
        }
        loop {
            self.interval.tick().await;
            if self.stopped {
                return Err(DesktopAdapterError::Stopped);
            }
            let fingerprint = network_fingerprint()?;
            if fingerprint != self.last_fingerprint {
                self.last_fingerprint = fingerprint;
                return Ok(());
            }
        }
    }

    async fn stop(&mut self) -> Result<(), DesktopAdapterError> {
        self.stopped = true;
        Ok(())
    }
}

/// Correctness-preserving fallback used when the native fingerprint source is
/// unavailable. It is intentionally bounded and does not perform a network
/// request; each tick is merely a positive "connectivity may have improved"
/// hint for Prompt 92.
pub struct PeriodicNetworkHintSource {
    interval: tokio::time::Interval,
    stopped: bool,
}

impl PeriodicNetworkHintSource {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval: tokio::time::interval(interval),
            stopped: false,
        }
    }
}

#[async_trait]
impl NetworkHintSource for PeriodicNetworkHintSource {
    async fn next_hint(&mut self) -> Result<(), DesktopAdapterError> {
        if self.stopped {
            return Err(DesktopAdapterError::Stopped);
        }
        self.interval.tick().await;
        if self.stopped {
            Err(DesktopAdapterError::Stopped)
        } else {
            Ok(())
        }
    }

    async fn stop(&mut self) -> Result<(), DesktopAdapterError> {
        self.stopped = true;
        Ok(())
    }
}

fn network_fingerprint() -> Result<u64, DesktopAdapterError> {
    #[cfg(target_os = "linux")]
    {
        let mut interfaces = Vec::new();
        for entry in
            fs::read_dir("/sys/class/net").map_err(|_| DesktopAdapterError::Initialization)?
        {
            let entry = entry.map_err(|_| DesktopAdapterError::Initialization)?;
            let name = entry.file_name();
            let state = fs::read_to_string(entry.path().join("operstate"))
                .unwrap_or_else(|_| "unknown".to_owned());
            interfaces.push((name.to_string_lossy().into_owned(), state));
        }
        interfaces.sort();
        let mut hasher = DefaultHasher::new();
        interfaces.hash(&mut hasher);
        Ok(hasher.finish())
    }

    #[cfg(target_os = "windows")]
    {
        use std::net::UdpSocket;

        let socket =
            UdpSocket::bind("0.0.0.0:0").map_err(|_| DesktopAdapterError::Initialization)?;
        socket
            .connect("1.1.1.1:53")
            .map_err(|_| DesktopAdapterError::Initialization)?;
        let local = socket
            .local_addr()
            .map_err(|_| DesktopAdapterError::Initialization)?;
        let mut hasher = DefaultHasher::new();
        local.hash(&mut hasher);
        Ok(hasher.finish())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn periodic_fallback_emits_bounded_hints_and_stops_cleanly() {
        let mut source = PeriodicNetworkHintSource::new(Duration::from_millis(100));
        source.next_hint().await.expect("first periodic hint");
        source.stop().await.expect("stop fallback");
        assert_eq!(source.next_hint().await, Err(DesktopAdapterError::Stopped));
    }

    #[test]
    fn native_source_rejects_unbounded_intervals_without_touching_network_policy() {
        assert!(matches!(
            NativeNetworkHintSource::new(Duration::ZERO),
            Err(DesktopAdapterError::Initialization)
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_native_source_initializes_or_uses_the_bounded_fallback() {
        match NativeNetworkHintSource::new(Duration::from_millis(100)) {
            Ok(mut source) => source.stop().await.expect("stop native source"),
            Err(DesktopAdapterError::Initialization) => {
                let mut fallback = PeriodicNetworkHintSource::new(Duration::from_millis(100));
                fallback.next_hint().await.expect("fallback hint");
                fallback.stop().await.expect("stop fallback source");
            }
            Err(error) => panic!("unexpected native source error: {error:?}"),
        }
    }
}
