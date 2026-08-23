use std::{fmt, time::Duration};

/// Stable service states exposed above a host service manager.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Failed,
    Maintenance,
    Unsupported,
}

impl ServiceState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "STARTING",
            Self::Ready => "READY",
            Self::Degraded => "DEGRADED",
            Self::Stopping => "STOPPING",
            Self::Failed => "FAILED",
            Self::Maintenance => "MAINTENANCE",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
}

impl fmt::Display for ServiceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Lifecycle operation names independent of an OS manager or exit code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleAction {
    Install,
    Configure,
    Start,
    Stop,
    Restart,
    GracefulShutdown,
    Upgrade,
    Uninstall,
}

/// A bounded shutdown request. The default is graceful and gives adapters a
/// chance to drain work without dropping durable jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownRequest {
    graceful: bool,
    deadline: Option<Duration>,
}

impl ShutdownRequest {
    #[must_use]
    pub const fn graceful() -> Self {
        Self {
            graceful: true,
            deadline: None,
        }
    }

    #[must_use]
    pub const fn forced() -> Self {
        Self {
            graceful: false,
            deadline: None,
        }
    }

    #[must_use]
    pub const fn with_deadline(self, deadline: Duration) -> Self {
        Self {
            graceful: self.graceful,
            deadline: Some(deadline),
        }
    }

    #[must_use]
    pub const fn is_graceful(self) -> bool {
        self.graceful
    }

    #[must_use]
    pub const fn deadline(self) -> Option<Duration> {
        self.deadline
    }
}

impl Default for ShutdownRequest {
    fn default() -> Self {
        Self::graceful()
    }
}

/// A restart request composed from a bounded shutdown phase.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RestartRequest {
    shutdown: ShutdownRequest,
}

impl RestartRequest {
    #[must_use]
    pub const fn graceful() -> Self {
        Self {
            shutdown: ShutdownRequest::graceful(),
        }
    }

    #[must_use]
    pub const fn forced() -> Self {
        Self {
            shutdown: ShutdownRequest::forced(),
        }
    }

    #[must_use]
    pub const fn with_deadline(self, deadline: Duration) -> Self {
        Self {
            shutdown: self.shutdown.with_deadline(deadline),
        }
    }

    #[must_use]
    pub const fn shutdown(self) -> ShutdownRequest {
        self.shutdown
    }
}

/// A manager-neutral result for an accepted lifecycle request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleReceipt {
    action: LifecycleAction,
    resulting_state: ServiceState,
}

impl LifecycleReceipt {
    #[must_use]
    pub const fn new(action: LifecycleAction, resulting_state: ServiceState) -> Self {
        Self {
            action,
            resulting_state,
        }
    }

    #[must_use]
    pub const fn action(self) -> LifecycleAction {
        self.action
    }

    #[must_use]
    pub const fn resulting_state(self) -> ServiceState {
        self.resulting_state
    }
}

/// Lifecycle failures never expose an OS process exit code as a domain value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    Unsupported,
    Unavailable,
    InvalidRequest,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported => "service lifecycle is unsupported",
            Self::Unavailable => "service lifecycle is unavailable",
            Self::InvalidRequest => "service lifecycle request is invalid",
        })
    }
}

impl std::error::Error for LifecycleError {}

/// A snapshot of lifecycle support and the stable service state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceLifecycleStatus {
    state: ServiceState,
    supported: bool,
}

impl ServiceLifecycleStatus {
    #[must_use]
    pub const fn new(state: ServiceState, supported: bool) -> Self {
        Self { state, supported }
    }

    #[must_use]
    pub const fn unsupported() -> Self {
        Self::new(ServiceState::Unsupported, false)
    }

    #[must_use]
    pub const fn state(self) -> ServiceState {
        self.state
    }

    #[must_use]
    pub const fn is_supported(self) -> bool {
        self.supported
    }
}

/// Service lifecycle port implemented by a future OS/runtime adapter.
pub trait ServiceLifecycle: Send + Sync {
    fn status(&self) -> ServiceLifecycleStatus;

    fn install(&self) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn configure(&self) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn start(&self) -> Result<LifecycleReceipt, LifecycleError>;
    fn stop(&self, request: ShutdownRequest) -> Result<LifecycleReceipt, LifecycleError>;
    fn restart(&self, request: RestartRequest) -> Result<LifecycleReceipt, LifecycleError>;

    fn upgrade(&self) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn uninstall(&self) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn shutdown(&self, request: ShutdownRequest) -> Result<LifecycleReceipt, LifecycleError> {
        self.stop(request)
    }

    fn graceful_shutdown(&self) -> Result<LifecycleReceipt, LifecycleError> {
        self.shutdown(ShutdownRequest::graceful())
    }
}

/// Safe placeholder until service-manager mutation is implemented and
/// separately reviewed for each host.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedServiceLifecycle;

impl UnsupportedServiceLifecycle {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ServiceLifecycle for UnsupportedServiceLifecycle {
    fn status(&self) -> ServiceLifecycleStatus {
        ServiceLifecycleStatus::unsupported()
    }

    fn start(&self) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn stop(&self, _request: ShutdownRequest) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }

    fn restart(&self, _request: RestartRequest) -> Result<LifecycleReceipt, LifecycleError> {
        Err(LifecycleError::Unsupported)
    }
}
