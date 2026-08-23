/// Coarse health states safe to expose without paths, credentials, or provider
/// diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unavailable,
    Unknown,
}

impl HealthState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "HEALTHY",
            Self::Degraded => "DEGRADED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Unknown => "UNKNOWN",
        }
    }

    #[must_use]
    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy)
    }
}

/// Contract-level component names rather than OS service names.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HealthComponent {
    Runtime,
    Paths,
    StorageDiscovery,
    SecretStore,
    ServiceLifecycle,
}

impl HealthComponent {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Paths => "paths",
            Self::StorageDiscovery => "storage_discovery",
            Self::SecretStore => "secret_store",
            Self::ServiceLifecycle => "service_lifecycle",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentHealth {
    component: HealthComponent,
    state: HealthState,
}

impl ComponentHealth {
    #[must_use]
    pub const fn new(component: HealthComponent, state: HealthState) -> Self {
        Self { component, state }
    }

    #[must_use]
    pub const fn component(self) -> HealthComponent {
        self.component
    }

    #[must_use]
    pub const fn state(self) -> HealthState {
        self.state
    }
}

/// Bounded platform health information. Detailed diagnostics belong to a
/// restricted adapter/application surface and are not included here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthInfo {
    overall: HealthState,
    liveness: bool,
    readiness: bool,
    components: Vec<ComponentHealth>,
}

impl HealthInfo {
    #[must_use]
    pub fn new(
        overall: HealthState,
        liveness: bool,
        readiness: bool,
        components: impl IntoIterator<Item = ComponentHealth>,
    ) -> Self {
        Self {
            overall,
            liveness,
            readiness,
            components: components.into_iter().collect(),
        }
    }

    #[must_use]
    pub const fn overall(&self) -> HealthState {
        self.overall
    }

    #[must_use]
    pub const fn liveness(&self) -> bool {
        self.liveness
    }

    #[must_use]
    pub const fn readiness(&self) -> bool {
        self.readiness
    }

    #[must_use]
    pub fn component(&self, component: HealthComponent) -> Option<HealthState> {
        self.components
            .iter()
            .find(|health| health.component == component)
            .map(|health| health.state)
    }

    #[must_use]
    pub fn components(&self) -> &[ComponentHealth] {
        &self.components
    }
}
