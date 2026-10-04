#![forbid(unsafe_code)]

//! Composition boundary for the guided Host journey.
//!
//! This crate owns no server resource. Platform adapters inspect and invoke the
//! canonical configuration, storage, service, network and bootstrap owners.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use synveil_server_config::ReachabilityMode;
use synveil_server_network::MANAGED_BACKEND;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostSetupState {
    NotStarted,
    PreflightPassed,
    DependenciesReady,
    ConfigurationReady,
    StorageReady,
    ServicesReady,
    AdminBootstrapRequired,
    Ready,
    NeedsRepair,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    CheckingThisDevice,
    PreparingConfiguration,
    PreparingStorage,
    PreparingSystemDatabase,
    StartingSynveil,
    ConfiguringAccess,
    CreatingOwnerAccount,
    CheckingServer,
    Ready,
}

/// One fresh, non-secret observation assembled from P031--P035 owners.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OwnerObservation {
    pub supported_platform: bool,
    pub blocked: bool,
    pub needs_repair: bool,
    pub host_intent_recorded: bool,
    pub dependencies_ready: bool,
    pub configuration_ready: bool,
    pub storage_ready: bool,
    pub services_ready: bool,
    pub postgres_17_identity_matches: bool,
    pub migrations_current: bool,
    pub api_live: bool,
    pub api_ready: bool,
    pub worker_healthy: bool,
    pub network_ready: bool,
    pub backend_loopback: bool,
    pub postgres_private: bool,
    pub client_https_healthy: bool,
    pub bootstrap_closed: bool,
    pub explicit_admin_login_verified: bool,
    pub initial_library_root_coherent: bool,
    pub canonical_origin: Option<String>,
    pub reachability_mode: Option<ReachabilityMode>,
    pub server_installation_id: Option<String>,
}

impl OwnerObservation {
    #[must_use]
    pub fn derive_state(&self) -> HostSetupState {
        if self.blocked || (self.host_intent_recorded && !self.supported_platform) {
            return HostSetupState::Blocked;
        }
        if self.needs_repair {
            return HostSetupState::NeedsRepair;
        }
        if !self.host_intent_recorded {
            return HostSetupState::NotStarted;
        }
        if !self.dependencies_ready {
            return HostSetupState::PreflightPassed;
        }
        if !self.configuration_ready {
            return HostSetupState::DependenciesReady;
        }
        if !self.storage_ready {
            return HostSetupState::ConfigurationReady;
        }
        if !self.services_ready {
            return HostSetupState::StorageReady;
        }
        if !self.bootstrap_closed || !self.explicit_admin_login_verified {
            return if self.infrastructure_ready() {
                HostSetupState::AdminBootstrapRequired
            } else {
                HostSetupState::ServicesReady
            };
        }
        if self.full_ready() {
            HostSetupState::Ready
        } else {
            HostSetupState::NeedsRepair
        }
    }

    fn infrastructure_ready(&self) -> bool {
        self.postgres_17_identity_matches
            && self.migrations_current
            && self.api_live
            && self.worker_healthy
            && self.network_ready
            && self.backend_loopback
            && self.postgres_private
    }

    fn full_ready(&self) -> bool {
        self.infrastructure_ready()
            && self.api_ready
            && self.client_https_healthy
            && self.bootstrap_closed
            && self.explicit_admin_login_verified
            && self.initial_library_root_coherent
            && self.canonical_origin.is_some()
            && self.server_installation_id.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedHostPlan {
    pub generation: u64,
    pub server_installation_id: String,
    pub config_fingerprint: String,
    pub storage_plan_fingerprint: String,
    pub service_plan_fingerprint: String,
    pub network_plan_fingerprint: String,
    pub platform_qualification: String,
    pub artifact_fingerprints: Vec<String>,
    pub expected_privileged_effects: Vec<PrivilegedEffect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivilegedEffect {
    PrepareConfiguration,
    PrepareStorage,
    InstallVerifiedDependencies,
    ReconcileOwnedServices,
    ConfigureOwnedReachability,
}

impl ReviewedHostPlan {
    #[must_use]
    pub fn binding(&self) -> String {
        let mut digest = Sha256::new();
        for value in [
            self.generation.to_string(),
            self.server_installation_id.clone(),
            self.config_fingerprint.clone(),
            self.storage_plan_fingerprint.clone(),
            self.service_plan_fingerprint.clone(),
            self.network_plan_fingerprint.clone(),
            self.platform_qualification.clone(),
            self.artifact_fingerprints.join(","),
        ] {
            digest.update(value.as_bytes());
            digest.update([0]);
        }
        format!("{:x}", digest.finalize())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerReady {
    pub server_installation_id: String,
    pub canonical_origin: String,
    pub trust_descriptor: String,
    pub logical_library_available: bool,
    pub reachability_mode: ReachabilityMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorError {
    ExplicitHostIntentRequired,
    ReviewRequired,
    StalePlan,
    OwnerRejected,
    NotReady,
}

/// Narrow adapter implemented by the native privileged helper. Every effect is
/// purpose-specific; arbitrary shell or filesystem operations are impossible.
pub trait CanonicalOwners {
    fn inspect(&mut self) -> Result<OwnerObservation, CoordinatorError>;
    fn current_generation(&mut self) -> Result<u64, CoordinatorError>;
    fn apply(&mut self, effect: PrivilegedEffect) -> Result<(), CoordinatorError>;
    fn trust_descriptor(&mut self) -> Result<String, CoordinatorError>;
}

pub struct ServerBootstrapCoordinator<O> {
    owners: O,
    intent: bool,
    reviewed_binding: Option<String>,
}

impl<O: CanonicalOwners> ServerBootstrapCoordinator<O> {
    pub fn new(owners: O) -> Self {
        Self {
            owners,
            intent: false,
            reviewed_binding: None,
        }
    }

    pub fn begin_host(&mut self) {
        self.intent = true;
    }

    pub fn review(&mut self, plan: &ReviewedHostPlan) -> Result<String, CoordinatorError> {
        if !self.intent {
            return Err(CoordinatorError::ExplicitHostIntentRequired);
        }
        if self.owners.current_generation()? != plan.generation {
            return Err(CoordinatorError::StalePlan);
        }
        let binding = plan.binding();
        self.reviewed_binding = Some(binding.clone());
        Ok(binding)
    }

    pub fn confirm(
        &mut self,
        plan: &ReviewedHostPlan,
        binding: &str,
    ) -> Result<(), CoordinatorError> {
        if self.reviewed_binding.as_deref() != Some(binding) || plan.binding() != binding {
            return Err(CoordinatorError::ReviewRequired);
        }
        if self.owners.current_generation()? != plan.generation {
            return Err(CoordinatorError::StalePlan);
        }
        for effect in &plan.expected_privileged_effects {
            self.owners.apply(*effect)?;
            // Reinspect after every uncertain external effect. The owner, not
            // this coordinator, decides whether it is complete or resumable.
            let _ = self.owners.inspect()?;
        }
        Ok(())
    }

    pub fn status(&mut self) -> Result<HostSetupState, CoordinatorError> {
        Ok(self.owners.inspect()?.derive_state())
    }

    pub fn verify(&mut self) -> Result<ServerReady, CoordinatorError> {
        let observation = self.owners.inspect()?;
        if observation.derive_state() != HostSetupState::Ready {
            return Err(CoordinatorError::NotReady);
        }
        Ok(ServerReady {
            server_installation_id: observation
                .server_installation_id
                .ok_or(CoordinatorError::NotReady)?,
            canonical_origin: observation
                .canonical_origin
                .ok_or(CoordinatorError::NotReady)?,
            trust_descriptor: self.owners.trust_descriptor()?,
            logical_library_available: true,
            reachability_mode: observation
                .reachability_mode
                .ok_or(CoordinatorError::NotReady)?,
        })
    }

    pub fn cancel(&mut self) {
        // Before confirmation this drops only ephemeral consent. After mutation
        // canonical owner state remains intact and a later inspection resumes.
        self.reviewed_binding = None;
    }
}

#[must_use]
pub fn backend_is_canonical_loopback(endpoint: &str) -> bool {
    endpoint == MANAGED_BACKEND
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Owners {
        generation: u64,
        observation: OwnerObservation,
        effects: Vec<PrivilegedEffect>,
    }
    impl CanonicalOwners for Owners {
        fn inspect(&mut self) -> Result<OwnerObservation, CoordinatorError> {
            Ok(self.observation.clone())
        }
        fn current_generation(&mut self) -> Result<u64, CoordinatorError> {
            Ok(self.generation)
        }
        fn apply(&mut self, effect: PrivilegedEffect) -> Result<(), CoordinatorError> {
            self.effects.push(effect);
            Ok(())
        }
        fn trust_descriptor(&mut self) -> Result<String, CoordinatorError> {
            Ok("sha256:certificate".into())
        }
    }
    fn plan() -> ReviewedHostPlan {
        ReviewedHostPlan {
            generation: 7,
            server_installation_id: "server".into(),
            config_fingerprint: "config".into(),
            storage_plan_fingerprint: "storage".into(),
            service_plan_fingerprint: "service".into(),
            network_plan_fingerprint: "network".into(),
            platform_qualification: "ubuntu-24.04-x86_64".into(),
            artifact_fingerprints: vec!["artifact".into()],
            expected_privileged_effects: vec![
                PrivilegedEffect::PrepareConfiguration,
                PrivilegedEffect::PrepareStorage,
            ],
        }
    }
    #[test]
    fn mutation_requires_intent_review_and_current_generation() {
        let owners = Owners {
            generation: 7,
            observation: OwnerObservation::default(),
            effects: vec![],
        };
        let mut coordinator = ServerBootstrapCoordinator::new(owners);
        assert_eq!(
            coordinator.review(&plan()),
            Err(CoordinatorError::ExplicitHostIntentRequired)
        );
        coordinator.begin_host();
        let binding = coordinator.review(&plan()).unwrap();
        coordinator.confirm(&plan(), &binding).unwrap();
        assert_eq!(coordinator.owners.effects.len(), 2);
        coordinator.owners.generation = 8;
        assert_eq!(
            coordinator.confirm(&plan(), &binding),
            Err(CoordinatorError::StalePlan)
        );
    }
    #[test]
    fn infrastructure_is_not_server_ready_without_login() {
        let observation = OwnerObservation {
            supported_platform: true,
            host_intent_recorded: true,
            dependencies_ready: true,
            configuration_ready: true,
            storage_ready: true,
            services_ready: true,
            postgres_17_identity_matches: true,
            migrations_current: true,
            api_live: true,
            worker_healthy: true,
            network_ready: true,
            backend_loopback: true,
            postgres_private: true,
            ..OwnerObservation::default()
        };
        assert_eq!(
            observation.derive_state(),
            HostSetupState::AdminBootstrapRequired
        );
    }
}
