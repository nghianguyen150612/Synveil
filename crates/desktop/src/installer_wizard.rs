//! Safe, ephemeral state for the P007 installer-wizard foundation.
//!
//! This model owns only bounded choices and presentation status. It has no
//! installation adapter, journal handle, path, credential, command, or effect
//! execution capability.

use crate::installer_preflight::{InstallationChannel, PreflightOutcome, PreflightRequest};
use synveil_install_engine::PlanReviewStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformCode {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl PlatformCode {
    pub const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WizardCapabilities {
    pub platform: PlatformCode,
    pub host_setup_supported: bool,
    pub client_connect_supported: bool,
    pub existing_installation_configurable: bool,
}

impl WizardCapabilities {
    pub const fn new(
        platform: PlatformCode,
        host_setup_supported: bool,
        client_connect_supported: bool,
        existing_installation_configurable: bool,
    ) -> Self {
        Self {
            platform,
            host_setup_supported,
            client_connect_supported,
            existing_installation_configurable,
        }
    }

    const fn supports_role(self, role: DeviceRole) -> bool {
        match role {
            DeviceRole::Host => self.host_setup_supported,
            DeviceRole::Client => self.client_connect_supported,
            DeviceRole::Existing => self.existing_installation_configurable,
        }
    }

    fn recommended_role(self) -> Option<DeviceRole> {
        if self.client_connect_supported {
            Some(DeviceRole::Client)
        } else if self.host_setup_supported {
            Some(DeviceRole::Host)
        } else if self.existing_installation_configurable {
            Some(DeviceRole::Existing)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceRole {
    Host,
    Client,
    Existing,
}

impl DeviceRole {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Client => "client",
            Self::Existing => "existing",
        }
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "host" => Some(Self::Host),
            "client" => Some(Self::Client),
            "existing" => Some(Self::Existing),
            _ => None,
        }
    }

    const fn default_connection(self) -> ConnectionMode {
        match self {
            Self::Host => ConnectionMode::LocalOnly,
            Self::Client | Self::Existing => ConnectionMode::Manual,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionMode {
    LocalOnly,
    PrivateLan,
    Tailscale,
    Manual,
}

impl ConnectionMode {
    pub const fn code(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::PrivateLan => "private_lan",
            Self::Tailscale => "tailscale",
            Self::Manual => "manual",
        }
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "local_only" => Some(Self::LocalOnly),
            "private_lan" => Some(Self::PrivateLan),
            "tailscale" => Some(Self::Tailscale),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WizardStep {
    Device,
    Connection,
    Review,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnavailableCode {
    UnsupportedPlatform,
    MissingCapability,
    TailscalePlanned,
}

impl UnavailableCode {
    const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::MissingCapability => "missing_capability",
            Self::TailscalePlanned => "tailscale_planned",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WizardErrorCode {
    InvalidTransition,
    InvalidSelection,
    SafePreflightFailed,
}

impl WizardErrorCode {
    const fn code(self) -> &'static str {
        match self {
            Self::InvalidTransition => "invalid_transition",
            Self::InvalidSelection => "invalid_selection",
            Self::SafePreflightFailed => "safe_preflight_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessState {
    Closed,
    DeviceChoice,
    ConnectionChoice,
    Preflight {
        generation: u64,
    },
    Review,
    Unavailable {
        code: UnavailableCode,
        return_to: WizardStep,
    },
    Error {
        code: WizardErrorCode,
        return_to: WizardStep,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightToken {
    generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelOptionView {
    pub code: &'static str,
    pub label: &'static str,
    pub availability: String,
    pub active: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WizardView {
    pub visible: bool,
    pub state: &'static str,
    pub step: i32,
    pub busy: bool,
    pub can_continue: bool,
    pub can_back: bool,
    pub can_retry: bool,
    pub details_open: bool,
    pub device_role: &'static str,
    pub connection_mode: &'static str,
    pub platform: &'static str,
    pub host_supported: bool,
    pub client_supported: bool,
    pub existing_supported: bool,
    pub local_only_supported: bool,
    pub private_lan_supported: bool,
    pub manual_supported: bool,
    pub report_ready: bool,
    pub evidence_complete: bool,
    pub preview_invalidated: bool,
    pub installer_preflight_ran: bool,
    pub verified_artifact_available: bool,
    pub plan_preview_available: bool,
    pub confirmation_status: &'static str,
    pub acknowledgement_available: bool,
    pub acknowledgement_reason: String,
    pub target_scope_detected: bool,
    pub privilege_assessed: bool,
    pub blocking_preflight_finding: bool,
    pub selected_channel: &'static str,
    pub channel_options: Vec<ChannelOptionView>,
    pub platform_summary: String,
    pub channel_summary: String,
    pub connection_summary: String,
    pub finding_summary: String,
    pub artifact_summary: String,
    pub plan_summary: String,
    pub scope_summary: String,
    pub capability_supported: bool,
    pub error_code: &'static str,
    pub unavailable_code: &'static str,
}

#[derive(Debug)]
pub struct InstallerWizard {
    capabilities: WizardCapabilities,
    initial_platform_summary: String,
    state: ProcessState,
    device_role: Option<DeviceRole>,
    connection_mode: Option<ConnectionMode>,
    generation: u64,
    outcome: Option<PreflightOutcome>,
    selected_channel: Option<InstallationChannel>,
    preview_invalidated: bool,
    details_open: bool,
}

impl InstallerWizard {
    pub fn new(capabilities: WizardCapabilities) -> Self {
        let initial_platform_summary = format!(
            "{}; distribution and version inspection pending",
            capabilities.platform.code()
        );
        Self::with_platform_summary(capabilities, initial_platform_summary)
    }

    pub fn with_platform_summary(
        capabilities: WizardCapabilities,
        initial_platform_summary: String,
    ) -> Self {
        Self {
            capabilities,
            initial_platform_summary,
            state: ProcessState::Closed,
            device_role: None,
            connection_mode: None,
            generation: 0,
            outcome: None,
            selected_channel: None,
            preview_invalidated: false,
            details_open: false,
        }
    }

    pub fn open(&mut self) {
        if self.state != ProcessState::Closed {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.outcome = None;
        self.selected_channel = None;
        self.preview_invalidated = false;
        self.details_open = false;
        self.device_role = self.capabilities.recommended_role();
        self.connection_mode = self.device_role.map(DeviceRole::default_connection);
        self.state = ProcessState::DeviceChoice;
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.outcome = None;
        self.preview_invalidated = false;
        self.details_open = false;
        self.device_role = None;
        self.connection_mode = None;
        self.selected_channel = None;
        self.state = ProcessState::Closed;
    }

    pub fn select_device_role(&mut self, role: DeviceRole) {
        if self.state != ProcessState::DeviceChoice {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        if !self.capabilities.supports_role(role) {
            self.generation = self.generation.wrapping_add(1);
            self.outcome = None;
            self.state = ProcessState::Unavailable {
                code: if self.capabilities.host_setup_supported
                    || self.capabilities.client_connect_supported
                    || self.capabilities.existing_installation_configurable
                {
                    UnavailableCode::MissingCapability
                } else {
                    UnavailableCode::UnsupportedPlatform
                },
                return_to: WizardStep::Device,
            };
            return;
        }
        if self.device_role != Some(role) {
            self.invalidate_preview();
            self.device_role = Some(role);
            self.connection_mode = Some(role.default_connection());
        }
    }

    pub fn select_connection_mode(&mut self, mode: ConnectionMode) {
        if self.state != ProcessState::ConnectionChoice {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        if mode == ConnectionMode::Tailscale {
            self.generation = self.generation.wrapping_add(1);
            self.outcome = None;
            self.state = ProcessState::Unavailable {
                code: UnavailableCode::TailscalePlanned,
                return_to: WizardStep::Connection,
            };
            return;
        }
        let supported = self
            .device_role
            .is_some_and(|role| self.connection_supported(role, mode));
        if !supported {
            self.generation = self.generation.wrapping_add(1);
            self.outcome = None;
            self.state = ProcessState::Unavailable {
                code: UnavailableCode::MissingCapability,
                return_to: WizardStep::Connection,
            };
            return;
        }
        if self.connection_mode != Some(mode) {
            self.invalidate_preview();
            self.connection_mode = Some(mode);
        }
    }

    pub fn select_installation_channel(&mut self, channel: InstallationChannel) {
        if self.state != ProcessState::ConnectionChoice
            || !channel.belongs_to(self.capabilities.platform)
        {
            self.fail(WizardErrorCode::InvalidSelection);
            return;
        }
        if self.selected_channel != Some(channel) {
            self.invalidate_preview();
            self.selected_channel = Some(channel);
        }
    }

    /// Future artifact acquisition paths must invalidate any inspected
    /// evidence before presenting a replacement artifact choice.
    pub fn artifact_changed(&mut self) {
        if matches!(
            self.state,
            ProcessState::ConnectionChoice | ProcessState::Review
        ) {
            self.invalidate_preview();
            if self.state == ProcessState::Review {
                self.state = ProcessState::ConnectionChoice;
            }
        }
    }

    /// Continue from the current page. Returns a token only when a new
    /// asynchronous read-only availability check should be launched.
    pub fn continue_forward(&mut self) -> Option<PreflightToken> {
        match self.state {
            ProcessState::DeviceChoice => {
                let supported = self
                    .device_role
                    .is_some_and(|role| self.capabilities.supports_role(role));
                if supported {
                    self.state = ProcessState::ConnectionChoice;
                } else {
                    self.fail(WizardErrorCode::InvalidSelection);
                }
                None
            }
            ProcessState::ConnectionChoice => self.begin_preflight(),
            ProcessState::Preflight { .. } => None,
            _ => {
                self.fail(WizardErrorCode::InvalidTransition);
                None
            }
        }
    }

    pub fn begin_preflight(&mut self) -> Option<PreflightToken> {
        if self.state != ProcessState::ConnectionChoice {
            return None;
        }
        let Some(role) = self.device_role else {
            self.fail(WizardErrorCode::InvalidSelection);
            return None;
        };
        let Some(mode) = self.connection_mode else {
            self.fail(WizardErrorCode::InvalidSelection);
            return None;
        };
        if !self.capabilities.supports_role(role) || !self.connection_supported(role, mode) {
            self.state = ProcessState::Unavailable {
                code: UnavailableCode::MissingCapability,
                return_to: WizardStep::Connection,
            };
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.outcome = None;
        let token = PreflightToken {
            generation: self.generation,
        };
        self.state = ProcessState::Preflight {
            generation: token.generation,
        };
        Some(token)
    }

    pub fn request_for(&self, token: PreflightToken) -> Option<PreflightRequest> {
        if self.state
            != (ProcessState::Preflight {
                generation: token.generation,
            })
            || self.generation != token.generation
        {
            return None;
        }
        let role = self.device_role?;
        let mode = self.connection_mode?;
        Some(PreflightRequest {
            generation: token.generation,
            role,
            connection_mode: mode,
            selected_channel: self.selected_channel,
            client_profile_available: self.capabilities.client_connect_supported,
            existing_installation_available: self.capabilities.existing_installation_configurable,
        })
    }

    pub fn complete_preflight(
        &mut self,
        token: PreflightToken,
        outcome: Result<PreflightOutcome, ()>,
    ) -> bool {
        if self.state
            != (ProcessState::Preflight {
                generation: token.generation,
            })
            || self.generation != token.generation
        {
            return false;
        }
        let Some(request) = self.request_for(token) else {
            return false;
        };
        match outcome {
            Ok(outcome) if outcome.binding.matches_request(request) => {
                self.outcome = Some(outcome);
                self.preview_invalidated = false;
                self.state = ProcessState::Review;
            }
            Ok(_) => {
                self.outcome = None;
                self.state = ProcessState::Error {
                    code: WizardErrorCode::SafePreflightFailed,
                    return_to: WizardStep::Connection,
                };
            }
            Err(()) => {
                self.outcome = None;
                self.state = ProcessState::Error {
                    code: WizardErrorCode::SafePreflightFailed,
                    return_to: WizardStep::Connection,
                };
            }
        }
        true
    }

    pub fn retry_safe_preflight(&mut self) -> Option<PreflightToken> {
        match self.state {
            ProcessState::Error {
                code: WizardErrorCode::SafePreflightFailed,
                ..
            } => {
                self.state = ProcessState::ConnectionChoice;
                self.begin_preflight()
            }
            _ => {
                self.fail(WizardErrorCode::InvalidTransition);
                None
            }
        }
    }

    pub fn back(&mut self) {
        match self.state {
            ProcessState::ConnectionChoice => self.state = ProcessState::DeviceChoice,
            ProcessState::Preflight { .. } => {
                self.generation = self.generation.wrapping_add(1);
                self.outcome = None;
                self.state = ProcessState::ConnectionChoice;
            }
            ProcessState::Review => self.state = ProcessState::ConnectionChoice,
            ProcessState::Unavailable { return_to, .. } | ProcessState::Error { return_to, .. } => {
                self.generation = self.generation.wrapping_add(1);
                self.outcome = None;
                self.state = match return_to {
                    WizardStep::Device => ProcessState::DeviceChoice,
                    WizardStep::Connection => ProcessState::ConnectionChoice,
                    WizardStep::Review => ProcessState::Review,
                };
            }
            _ => self.fail(WizardErrorCode::InvalidTransition),
        }
    }

    pub fn set_error_details_open(&mut self, open: bool) {
        if !matches!(
            self.state,
            ProcessState::Error { .. } | ProcessState::Review
        ) {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        self.details_open = open;
    }

    pub fn reject_invalid_selection(&mut self) {
        self.fail(WizardErrorCode::InvalidSelection);
    }

    pub fn view(&self) -> WizardView {
        let state = match self.state {
            ProcessState::Closed => "closed",
            ProcessState::DeviceChoice => "device_choice",
            ProcessState::ConnectionChoice => "connection_choice",
            ProcessState::Preflight { .. } => "preflight",
            ProcessState::Review => "review",
            ProcessState::Unavailable { .. } => "unavailable",
            ProcessState::Error { .. } => "error",
        };
        let step = match self.state {
            ProcessState::Closed | ProcessState::DeviceChoice => 0,
            ProcessState::ConnectionChoice => 1,
            ProcessState::Unavailable { return_to, .. } | ProcessState::Error { return_to, .. } => {
                match return_to {
                    WizardStep::Device => 0,
                    WizardStep::Connection => 1,
                    WizardStep::Review => 2,
                }
            }
            ProcessState::Preflight { .. } | ProcessState::Review => 2,
        };
        let can_continue = match self.state {
            ProcessState::DeviceChoice => self
                .device_role
                .is_some_and(|role| self.capabilities.supports_role(role)),
            ProcessState::ConnectionChoice => self
                .device_role
                .zip(self.connection_mode)
                .is_some_and(|(role, mode)| self.connection_supported(role, mode)),
            _ => false,
        };
        let can_back = matches!(
            self.state,
            ProcessState::ConnectionChoice
                | ProcessState::Preflight { .. }
                | ProcessState::Review
                | ProcessState::Unavailable { .. }
                | ProcessState::Error { .. }
        );
        let can_retry = matches!(
            self.state,
            ProcessState::Error {
                code: WizardErrorCode::SafePreflightFailed,
                ..
            }
        );
        let details_open = self.details_open;
        let error_code = match self.state {
            ProcessState::Error { code, .. } => code.code(),
            _ => "",
        };
        let unavailable_code = match self.state {
            ProcessState::Unavailable { code, .. } => code.code(),
            _ => "",
        };
        let outcome = self.outcome.as_ref();
        let role = self.device_role;
        let channel_options = InstallationChannel::choices_for(self.capabilities.platform)
            .into_iter()
            .map(|channel| {
                let evidence = outcome.and_then(|outcome| {
                    outcome
                        .binding
                        .channels
                        .iter()
                        .find(|item| item.channel == channel)
                });
                ChannelOptionView {
                    code: channel.code(),
                    label: channel.label(),
                    availability: evidence.map_or_else(
                        || "not inspected".to_owned(),
                        |item| item.availability.label().to_owned(),
                    ),
                    active: evidence.is_some_and(|item| item.active),
                    selected: self.selected_channel == Some(channel),
                }
            })
            .collect();
        let selected_channel = self.selected_channel.map_or("", InstallationChannel::code);
        let platform_summary = outcome.map_or_else(
            || self.initial_platform_summary.clone(),
            PreflightOutcome::platform_summary,
        );
        let channel_summary = outcome.map_or_else(
            || "Installation channels have not been inspected.".to_owned(),
            PreflightOutcome::channel_summary,
        );
        let connection_summary = outcome.map_or_else(
            || {
                "This is a preference only. The read-only check will not alter network settings."
                    .to_owned()
            },
            PreflightOutcome::connection_summary,
        );
        let finding_summary = outcome.map_or_else(
            || "Read-only capability inspection has not run.".to_owned(),
            PreflightOutcome::finding_summary,
        );
        let artifact_summary = outcome.map_or_else(
            || "No artifact was supplied or verified.".to_owned(),
            PreflightOutcome::artifact_summary,
        );
        let plan_summary = outcome.map_or(
            crate::installer_preflight::PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired
                .label()
                .to_owned(),
            PreflightOutcome::plan_summary,
        );
        let confirmation_status = match outcome.map(|value| &value.plan_preview) {
            None => PlanReviewStatus::NoPlanAvailable,
            Some(crate::installer_preflight::PlanPreviewAvailability::Unavailable(
                crate::installer_preflight::PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired,
            )) => PlanReviewStatus::ArtifactEvidenceUnavailable,
            Some(crate::installer_preflight::PlanPreviewAvailability::Unavailable(
                crate::installer_preflight::PlanPreviewUnavailableReason::PlanValidationFailed,
            )) => PlanReviewStatus::PlanValidationFailed,
            Some(crate::installer_preflight::PlanPreviewAvailability::Unavailable(
                crate::installer_preflight::PlanPreviewUnavailableReason::InsufficientCapabilityEvidence,
            )) => PlanReviewStatus::ReviewDataIncomplete,
            Some(crate::installer_preflight::PlanPreviewAvailability::Unavailable(
                crate::installer_preflight::PlanPreviewUnavailableReason::AuthoritativePlanBuilderUnavailable,
            ))
            | Some(crate::installer_preflight::PlanPreviewAvailability::Available(_))
            | Some(crate::installer_preflight::PlanPreviewAvailability::PartiallyAvailable(_)) => {
                // A preview DTO is not a validated engine snapshot.
                PlanReviewStatus::NoPlanAvailable
            }
        };
        let scope_summary = outcome.map_or_else(
            || "Target scope unavailable until a channel is identified.".to_owned(),
            PreflightOutcome::scope_summary,
        );
        let blocking = outcome.is_some_and(|outcome| {
            outcome.findings.iter().any(|finding| {
                finding.severity == synveil_install_engine::FindingSeverity::Blocking
            })
        });
        let verified_artifact_available = outcome.is_some_and(|outcome| {
            outcome.selected_channel().is_some_and(|channel| {
                channel.artifact_evidence
                    == crate::installer_preflight::ArtifactEvidenceClass::VerifiedReleaseArtifact
            })
        });
        let plan_available = outcome.is_some_and(|outcome| {
            matches!(
                &outcome.plan_preview,
                crate::installer_preflight::PlanPreviewAvailability::Available(_)
                    | crate::installer_preflight::PlanPreviewAvailability::PartiallyAvailable(_)
            )
        });
        WizardView {
            visible: self.state != ProcessState::Closed,
            state,
            step,
            busy: matches!(self.state, ProcessState::Preflight { .. }),
            can_continue,
            can_back,
            can_retry,
            details_open,
            device_role: role.map_or("", DeviceRole::code),
            connection_mode: self.connection_mode.map_or("", ConnectionMode::code),
            platform: self.capabilities.platform.code(),
            host_supported: outcome.map_or(self.capabilities.host_setup_supported, |value| {
                value.binding.platform.managed_host_supported
            }),
            client_supported: self.capabilities.client_connect_supported,
            existing_supported: self.capabilities.existing_installation_configurable
                || outcome.is_some_and(|value| {
                    value.binding.channels.iter().any(|channel| {
                        channel.channel == InstallationChannel::ExistingInstallation
                            && channel.active
                    })
                }),
            local_only_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::LocalOnly)),
            private_lan_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::PrivateLan)),
            manual_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::Manual)),
            report_ready: outcome.is_some(),
            evidence_complete: outcome
                .is_some_and(|value| value.binding.platform.operating_system_evidence_complete),
            preview_invalidated: self.preview_invalidated,
            installer_preflight_ran: false,
            verified_artifact_available,
            plan_preview_available: plan_available,
            confirmation_status: match confirmation_status {
                PlanReviewStatus::NoPlanAvailable => "no_plan_available",
                PlanReviewStatus::PlanValidationFailed => "plan_validation_failed",
                PlanReviewStatus::ArtifactEvidenceUnavailable => "artifact_evidence_unavailable",
                PlanReviewStatus::AvailableForReview => "available_for_review",
                PlanReviewStatus::ReviewDataIncomplete => "review_data_incomplete",
            },
            // P008 has no acquisition-to-authoritative-plan path. A preview
            // marker cannot enable acknowledgement or imply an execution gate.
            acknowledgement_available: false,
            acknowledgement_reason: plan_summary.to_owned(),
            target_scope_detected: outcome
                .and_then(PreflightOutcome::selected_channel)
                .is_some_and(|channel| channel.target_scope.is_some()),
            privilege_assessed: false,
            blocking_preflight_finding: blocking,
            selected_channel,
            channel_options,
            platform_summary,
            channel_summary,
            connection_summary,
            finding_summary,
            artifact_summary,
            plan_summary: plan_summary.to_owned(),
            scope_summary,
            capability_supported: outcome.is_some_and(|value| value.selected_channel_supported),
            error_code,
            unavailable_code,
        }
    }

    fn connection_supported(&self, role: DeviceRole, mode: ConnectionMode) -> bool {
        if !self.capabilities.supports_role(role) {
            return false;
        }
        match mode {
            ConnectionMode::Tailscale => false,
            ConnectionMode::LocalOnly | ConnectionMode::PrivateLan | ConnectionMode::Manual => {
                match role {
                    DeviceRole::Host => self.capabilities.host_setup_supported,
                    DeviceRole::Client => self.capabilities.client_connect_supported,
                    DeviceRole::Existing => self.capabilities.existing_installation_configurable,
                }
            }
        }
    }

    fn invalidate_preview(&mut self) {
        self.preview_invalidated |= self.outcome.is_some();
        self.generation = self.generation.wrapping_add(1);
        self.outcome = None;
    }

    fn fail(&mut self, code: WizardErrorCode) {
        let return_to = match self.state {
            ProcessState::Closed | ProcessState::DeviceChoice => WizardStep::Device,
            ProcessState::ConnectionChoice | ProcessState::Preflight { .. } => {
                WizardStep::Connection
            }
            ProcessState::Review => WizardStep::Review,
            ProcessState::Unavailable { return_to, .. } | ProcessState::Error { return_to, .. } => {
                return_to
            }
        };
        self.generation = self.generation.wrapping_add(1);
        self.outcome = None;
        self.state = ProcessState::Error { code, return_to };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> WizardCapabilities {
        WizardCapabilities::new(PlatformCode::current(), true, true, false)
    }

    fn open_at_connection() -> InstallerWizard {
        let mut wizard = InstallerWizard::new(capabilities());
        wizard.open();
        wizard.continue_forward();
        wizard
    }

    fn reviewed_wizard() -> (InstallerWizard, PreflightToken) {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts once");
        let request = wizard.request_for(token).expect("current request");
        let outcome = crate::installer_preflight::inspect(request);
        assert!(wizard.complete_preflight(token, Ok(outcome)));
        (wizard, token)
    }

    #[test]
    fn default_supported_path_is_client_with_manual_connection_and_review_only() {
        let (wizard, _) = reviewed_wizard();
        let view = wizard.view();
        assert_eq!(view.state, "review");
        assert_eq!(view.device_role, "client");
        assert_eq!(view.connection_mode, "manual");
        assert!(view.report_ready);
        assert!(!view.installer_preflight_ran);
        assert!(!view.plan_preview_available);
        assert_eq!(view.confirmation_status, "artifact_evidence_unavailable");
        assert!(!view.acknowledgement_available);
        assert_eq!(
            view.acknowledgement_reason,
            "Plan preview unavailable — verified artifact evidence required"
        );
        assert!(!view.verified_artifact_available);
        assert!(!view.target_scope_detected);
        assert!(!view.privilege_assessed);
    }

    #[test]
    fn back_navigation_tracks_process_state_not_page_numbers() {
        let mut wizard = open_at_connection();
        assert_eq!(wizard.view().step, 1);
        wizard.back();
        assert_eq!(wizard.view().state, "device_choice");
        assert_eq!(wizard.view().step, 0);
        wizard.continue_forward();
        let token = wizard.continue_forward().expect("preflight starts");
        assert_eq!(wizard.view().state, "preflight");
        wizard.back();
        assert_eq!(wizard.view().state, "connection_choice");
        assert!(wizard.request_for(token).is_none());
    }

    #[test]
    fn changing_device_role_invalidates_previous_report() {
        let (mut wizard, _) = reviewed_wizard();
        wizard.back();
        wizard.back();
        let generation = wizard.generation;
        wizard.select_device_role(DeviceRole::Host);
        assert!(wizard.generation > generation);
        assert!(wizard.outcome.is_none());
        assert_eq!(wizard.view().device_role, "host");
        assert_eq!(wizard.view().connection_mode, "local_only");
    }

    #[test]
    fn changing_connection_mode_invalidates_previous_report() {
        let (mut wizard, _) = reviewed_wizard();
        wizard.back();
        let generation = wizard.generation;
        wizard.select_connection_mode(ConnectionMode::PrivateLan);
        assert!(wizard.generation > generation);
        assert!(wizard.outcome.is_none());
        assert_eq!(wizard.view().connection_mode, "private_lan");
    }

    #[test]
    fn changing_channel_invalidates_the_bound_preview() {
        let (mut wizard, _) = reviewed_wizard();
        wizard.back();
        let generation = wizard.generation;
        let channel = InstallationChannel::choices_for(PlatformCode::current())[0];
        wizard.select_installation_channel(channel);
        assert!(wizard.generation > generation);
        assert!(wizard.outcome.is_none());
        assert_eq!(wizard.view().selected_channel, channel.code());
    }

    #[test]
    fn artifact_change_invalidates_the_preview_before_a_new_check() {
        let (mut wizard, old_token) = reviewed_wizard();
        let old_outcome = wizard.outcome.clone().expect("completed inspection");
        wizard.back();
        wizard.artifact_changed();
        assert!(wizard.outcome.is_none());
        assert!(wizard.view().preview_invalidated);
        assert!(!wizard.complete_preflight(old_token, Ok(old_outcome)));
    }

    #[test]
    fn mismatched_preflight_binding_is_ignored() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        let request = wizard.request_for(token).expect("current request");
        let mut outcome = crate::installer_preflight::inspect(request);
        outcome.binding.selection.generation = outcome.binding.selection.generation.wrapping_add(1);
        assert!(wizard.complete_preflight(token, Ok(outcome)));
        assert_eq!(wizard.view().state, "error");
        assert!(!wizard.view().report_ready);
    }

    #[test]
    fn unsupported_platform_does_not_offer_an_executable_path() {
        let mut wizard = InstallerWizard::new(WizardCapabilities::new(
            PlatformCode::Other,
            false,
            false,
            false,
        ));
        wizard.open();
        let view = wizard.view();
        assert_eq!(view.state, "device_choice");
        assert!(!view.host_supported);
        assert!(!view.client_supported);
        assert!(!view.can_continue);
        assert!(!view.manual_supported);
    }

    #[test]
    fn unsupported_role_fails_closed_without_retaining_the_choice() {
        let mut wizard = InstallerWizard::new(WizardCapabilities::new(
            PlatformCode::Windows,
            false,
            true,
            false,
        ));
        wizard.open();
        wizard.select_device_role(DeviceRole::Host);
        assert_eq!(wizard.view().state, "unavailable");
        assert_ne!(wizard.view().device_role, "host");
        assert_eq!(wizard.view().unavailable_code, "missing_capability");
    }

    #[test]
    fn no_verified_artifact_or_authoritative_plan_is_claimed() {
        let (wizard, _) = reviewed_wizard();
        let view = wizard.view();
        assert!(!view.verified_artifact_available);
        assert!(!view.plan_preview_available);
        assert!(!view.target_scope_detected);
        assert!(!view.privilege_assessed);
        assert!(view.plan_summary.contains("publisher trust root"));
        assert!(view
            .plan_summary
            .contains("approved channel effect catalog"));
        assert!(view
            .plan_summary
            .contains("P004-D02, D11, and D14 remain pending"));
        assert!(wizard
            .outcome
            .as_ref()
            .unwrap()
            .plan_build_blockers
            .contains(&synveil_install_engine::PlanBuildBlocker::TrustRootUnavailable));
    }

    #[test]
    fn preview_dto_never_enables_acknowledgement_without_an_engine_snapshot() {
        let (mut wizard, _) = reviewed_wizard();
        wizard.outcome.as_mut().unwrap().plan_preview =
            crate::installer_preflight::PlanPreviewAvailability::Available(
                crate::installer_preflight::PlanPreview {
                    intent: "INSTALL",
                    target_scope: synveil_install_engine::TargetScope::PortableUser,
                    installation_channel: InstallationChannel::LinuxAppImage,
                    artifact_evidence:
                        crate::installer_preflight::ArtifactEvidenceClass::VerifiedReleaseArtifact,
                    ordered_operation_categories: vec!["install_payload"],
                    resources_expected_to_touch: vec![
                        synveil_install_engine::ResourceClass::PackageOwned,
                    ],
                    preservation_expectations: vec![
                        synveil_install_engine::ResourceClass::CredentialState,
                    ],
                    privilege: synveil_install_engine::PrivilegeRequirement::CurrentUser,
                    reversibility: synveil_install_engine::Reversibility::PartiallyReversible,
                    verification_requirements: vec!["payload_present"],
                    blocking_findings: vec![],
                    confirmation_prerequisites: vec!["publisher_policy"],
                    validity: crate::installer_preflight::PlanValidity::Valid,
                },
            );
        let view = wizard.view();
        assert!(view.plan_preview_available);
        assert_eq!(view.confirmation_status, "no_plan_available");
        assert!(!view.acknowledgement_available);
    }

    #[test]
    fn findings_are_safe_categories_and_never_carry_raw_evidence() {
        let (wizard, _) = reviewed_wizard();
        let view = wizard.view();
        assert!(!view.finding_summary.contains("/private/path"));
        assert!(!view.finding_summary.contains("secret"));
        assert!(!view.finding_summary.contains("token="));
    }

    #[test]
    fn tailscale_is_unavailable_and_does_not_change_the_connection_selection() {
        let mut wizard = open_at_connection();
        wizard.select_connection_mode(ConnectionMode::Tailscale);
        assert_eq!(wizard.view().state, "unavailable");
        assert_eq!(wizard.view().unavailable_code, "tailscale_planned");
        assert_eq!(wizard.view().connection_mode, "manual");
    }

    #[test]
    fn repeated_continue_does_not_create_duplicate_preflight_work() {
        let mut wizard = open_at_connection();
        let first = wizard.continue_forward();
        assert!(first.is_some());
        assert!(wizard.continue_forward().is_none());
        assert_eq!(wizard.view().state, "preflight");
    }

    #[test]
    fn cancellation_invalidates_pending_work_and_only_clears_ephemeral_state() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        let request = wizard.request_for(token).expect("request exists");
        let outcome = crate::installer_preflight::inspect(request);
        wizard.cancel();
        assert_eq!(wizard.view().state, "closed");
        assert!(!wizard.complete_preflight(token, Ok(outcome)));
        assert_eq!(wizard.view().device_role, "");
    }

    #[test]
    fn out_of_order_result_is_ignored_after_selection_changes() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        let request = wizard.request_for(token).expect("request exists");
        let outcome = crate::installer_preflight::inspect(request);
        wizard.back();
        wizard.select_connection_mode(ConnectionMode::PrivateLan);
        assert!(!wizard.complete_preflight(token, Ok(outcome)));
        assert_eq!(wizard.view().state, "connection_choice");
        assert!(!wizard.view().report_ready);
    }

    #[test]
    fn safe_preflight_error_can_be_reentered_and_retried() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        assert!(wizard.complete_preflight(token, Err(())));
        assert_eq!(wizard.view().state, "error");
        wizard.set_error_details_open(true);
        assert!(wizard.view().details_open);
        let retry = wizard.retry_safe_preflight().expect("safe retry");
        let request = wizard.request_for(retry).expect("retry request");
        let outcome = crate::installer_preflight::inspect(request);
        assert!(wizard.complete_preflight(retry, Ok(outcome)));
        assert_eq!(wizard.view().state, "review");
        wizard.cancel();
        wizard.open();
        assert_eq!(wizard.view().state, "device_choice");
    }

    #[test]
    fn invalid_strings_are_not_retained_as_selection_or_command_text() {
        let mut wizard = InstallerWizard::new(capabilities());
        wizard.open();
        assert!(DeviceRole::from_code("/bin/sh -c secret").is_none());
        wizard.reject_invalid_selection();
        let view = wizard.view();
        assert_eq!(view.state, "error");
        assert_eq!(view.error_code, "invalid_selection");
        assert!(!format!("{wizard:?}").contains("/bin/sh"));
    }

    #[test]
    fn qml_smoke_startup_and_existing_first_run_routes_remain_present() {
        let main_qml = include_str!("../qml/Main.qml");
        let wizard_qml = include_str!("../qml/InstallerWizard.qml");
        assert!(main_qml.contains("bridge.installTray()"));
        assert!(main_qml.contains("bridge.startController()"));
        assert!(main_qml.contains("bridge.chooseHost()"));
        assert!(main_qml.contains("bridge.chooseConnect()"));
        assert!(wizard_qml.contains("Preview only — installation is not started"));
        assert!(wizard_qml.contains("id: acknowledgeReviewedPlan"));
        assert!(wizard_qml.contains("enabled: root.wizardView.acknowledgementAvailable"));
        assert!(!wizard_qml.contains("acknowledgeReviewedPlan.onClicked"));
        assert!(!wizard_qml.contains("InstallerEngine::execute"));
        assert!(!wizard_qml.contains("InstallationJournal"));
        assert!(!wizard_qml.contains("sudo "));
        assert!(!wizard_qml.contains("runas"));
        assert!(!wizard_qml.contains("systemctl"));
        assert!(!wizard_qml.contains("pkexec"));
        assert!(wizard_qml.contains("acknowledgementReason"));
        assert!(!wizard_qml.contains("qsTr(\"Install\")"));
        assert!(!wizard_qml.contains("qsTr(\"Apply\")"));
        assert!(!wizard_qml.contains("qsTr(\"Repair\")"));
        assert!(!wizard_qml.contains("qsTr(\"Configure\")"));
        assert!(!wizard_qml.contains("hostConfirm("));
        assert!(!wizard_qml.contains("hostReview("));
        assert!(!wizard_qml.contains("systemctl"));
        assert!(!wizard_qml.contains("pkexec"));
        let state_source = include_str!("installer_wizard.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("state implementation exists");
        assert!(!state_source.contains("InstallationJournal"));
        assert!(!state_source.contains("InstallerEngine::execute"));
        assert!(!state_source.contains("InstallationEffect"));
        assert!(!state_source.contains("std::fs::"));
        assert!(!state_source.contains("std::process::"));
        let review_contract = include_str!("../../install-engine/src/review_contract.rs");
        assert!(!review_contract.contains("InstallationJournal"));
        assert!(!review_contract.contains("apply_effect("));
        assert!(!review_contract.contains("std::net::"));
        assert!(!review_contract.contains("InstallerEngine::execute"));
    }
}
