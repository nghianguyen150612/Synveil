//! Safe, ephemeral state for the P007 installer-wizard foundation.
//!
//! This model owns only bounded choices and presentation status. It has no
//! installation adapter, journal handle, path, credential, command, or effect
//! execution capability.

use synveil_install_engine::{FindingSeverity, PreflightFinding, TargetScope};

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
        details_open: bool,
        return_to: WizardStep,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightToken {
    generation: u64,
}

/// Read-only input to the asynchronous capability check. This deliberately
/// contains no path, artifact claim, credential, listener setting, or effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreviewProbe {
    generation: u64,
    platform: PlatformCode,
    host_setup_supported: bool,
    client_connect_supported: bool,
    role_supported: bool,
    connection_supported: bool,
}

impl PreviewProbe {
    pub fn inspect(self) -> PreflightReport {
        PreflightReport {
            generation: self.generation,
            platform: self.platform,
            host_setup_supported: self.host_setup_supported,
            client_connect_supported: self.client_connect_supported,
            selected_capabilities_supported: self.role_supported && self.connection_supported,
            installer_preflight_ran: false,
            verified_artifact_available: false,
            plan_preview_available: false,
            target_scope_detected: false,
            privilege_assessed: false,
            blocking_finding_count: 0,
        }
        .with_findings(&[])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightReport {
    generation: u64,
    platform: PlatformCode,
    host_setup_supported: bool,
    client_connect_supported: bool,
    selected_capabilities_supported: bool,
    /// P007 has no channel-specific engine adapter, so no installer preflight
    /// has been run even though the read-only capability check completed.
    installer_preflight_ran: bool,
    verified_artifact_available: bool,
    plan_preview_available: bool,
    target_scope_detected: bool,
    privilege_assessed: bool,
    blocking_finding_count: u8,
}

impl PreflightReport {
    /// Keep only the existence of blocking findings. Engine evidence strings,
    /// identifiers and capability labels never cross into the UI model.
    pub fn with_findings(mut self, findings: &[PreflightFinding]) -> Self {
        self.blocking_finding_count = findings
            .iter()
            .filter(|finding| {
                finding.blocks_planning || finding.severity == FindingSeverity::Blocking
            })
            .count()
            .min(8) as u8;
        self
    }
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
    pub preview_invalidated: bool,
    pub installer_preflight_ran: bool,
    pub verified_artifact_available: bool,
    pub plan_preview_available: bool,
    pub target_scope_detected: bool,
    pub privilege_assessed: bool,
    pub blocking_preflight_finding: bool,
    pub error_code: &'static str,
    pub unavailable_code: &'static str,
}

#[derive(Debug)]
pub struct InstallerWizard {
    capabilities: WizardCapabilities,
    state: ProcessState,
    device_role: Option<DeviceRole>,
    connection_mode: Option<ConnectionMode>,
    generation: u64,
    report: Option<PreflightReport>,
    preview_invalidated: bool,
}

impl InstallerWizard {
    pub fn new(capabilities: WizardCapabilities) -> Self {
        Self {
            capabilities,
            state: ProcessState::Closed,
            device_role: None,
            connection_mode: None,
            generation: 0,
            report: None,
            preview_invalidated: false,
        }
    }

    pub fn open(&mut self) {
        if self.state != ProcessState::Closed {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.report = None;
        self.preview_invalidated = false;
        self.device_role = self.capabilities.recommended_role();
        self.connection_mode = self.device_role.map(DeviceRole::default_connection);
        self.state = ProcessState::DeviceChoice;
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.report = None;
        self.preview_invalidated = false;
        self.device_role = None;
        self.connection_mode = None;
        self.state = ProcessState::Closed;
    }

    pub fn select_device_role(&mut self, role: DeviceRole) {
        if self.state != ProcessState::DeviceChoice {
            self.fail(WizardErrorCode::InvalidTransition);
            return;
        }
        if !self.capabilities.supports_role(role) {
            self.generation = self.generation.wrapping_add(1);
            self.report = None;
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
            self.report = None;
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
            self.report = None;
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
        let token = PreflightToken {
            generation: self.generation,
        };
        self.state = ProcessState::Preflight {
            generation: token.generation,
        };
        Some(token)
    }

    pub fn probe_for(&self, token: PreflightToken) -> Option<PreviewProbe> {
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
        Some(PreviewProbe {
            generation: token.generation,
            platform: self.capabilities.platform,
            host_setup_supported: self.capabilities.host_setup_supported,
            client_connect_supported: self.capabilities.client_connect_supported,
            role_supported: self.capabilities.supports_role(role),
            connection_supported: self.connection_supported(role, mode),
        })
    }

    pub fn complete_preflight(
        &mut self,
        token: PreflightToken,
        report: Result<PreflightReport, ()>,
    ) -> bool {
        if self.state
            != (ProcessState::Preflight {
                generation: token.generation,
            })
            || self.generation != token.generation
        {
            return false;
        }
        match report {
            Ok(report)
                if report.generation == token.generation
                    && report.selected_capabilities_supported =>
            {
                self.report = Some(report);
                self.preview_invalidated = false;
                self.state = ProcessState::Review;
            }
            Ok(report) if report.generation == token.generation => {
                self.report = Some(report);
                self.state = ProcessState::Unavailable {
                    code: UnavailableCode::MissingCapability,
                    return_to: WizardStep::Review,
                };
            }
            _ => {
                self.report = None;
                self.state = ProcessState::Error {
                    code: WizardErrorCode::SafePreflightFailed,
                    details_open: false,
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
                self.report = None;
                self.state = ProcessState::ConnectionChoice;
            }
            ProcessState::Review => self.state = ProcessState::ConnectionChoice,
            ProcessState::Unavailable { return_to, .. } | ProcessState::Error { return_to, .. } => {
                self.generation = self.generation.wrapping_add(1);
                self.report = None;
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
        if let ProcessState::Error {
            code, return_to, ..
        } = self.state
        {
            self.state = ProcessState::Error {
                code,
                details_open: open,
                return_to,
            };
        } else {
            self.fail(WizardErrorCode::InvalidTransition);
        }
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
        let details_open = matches!(
            self.state,
            ProcessState::Error {
                details_open: true,
                ..
            }
        );
        let error_code = match self.state {
            ProcessState::Error { code, .. } => code.code(),
            _ => "",
        };
        let unavailable_code = match self.state {
            ProcessState::Unavailable { code, .. } => code.code(),
            _ => "",
        };
        let report = self.report;
        let role = self.device_role;
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
            platform: report.map_or(self.capabilities.platform.code(), |value| {
                value.platform.code()
            }),
            host_supported: report.map_or(self.capabilities.host_setup_supported, |value| {
                value.host_setup_supported
            }),
            client_supported: report.map_or(self.capabilities.client_connect_supported, |value| {
                value.client_connect_supported
            }),
            existing_supported: self.capabilities.existing_installation_configurable,
            local_only_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::LocalOnly)),
            private_lan_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::PrivateLan)),
            manual_supported: role
                .is_some_and(|role| self.connection_supported(role, ConnectionMode::Manual)),
            report_ready: report.is_some(),
            preview_invalidated: self.preview_invalidated,
            installer_preflight_ran: report.is_some_and(|value| value.installer_preflight_ran),
            verified_artifact_available: report
                .is_some_and(|value| value.verified_artifact_available),
            plan_preview_available: report.is_some_and(|value| value.plan_preview_available),
            target_scope_detected: report.is_some_and(|value| value.target_scope_detected),
            privilege_assessed: report.is_some_and(|value| value.privilege_assessed),
            blocking_preflight_finding: report
                .is_some_and(|value| value.blocking_finding_count > 0),
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
        self.preview_invalidated |= self.report.is_some();
        self.generation = self.generation.wrapping_add(1);
        self.report = None;
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
        self.report = None;
        self.state = ProcessState::Error {
            code,
            details_open: false,
            return_to,
        };
    }
}

/// No target scope is actionable in P007. The package or platform owner must
/// provide verified channel evidence before a later plan can carry a scope.
pub const fn target_scope_supported(_scope: TargetScope) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> WizardCapabilities {
        WizardCapabilities::new(PlatformCode::Linux, true, true, false)
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
        let report = wizard.probe_for(token).expect("current probe").inspect();
        assert!(wizard.complete_preflight(token, Ok(report)));
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
        assert!(wizard.probe_for(token).is_none());
    }

    #[test]
    fn changing_device_role_invalidates_previous_report() {
        let (mut wizard, _) = reviewed_wizard();
        wizard.back();
        wizard.back();
        let generation = wizard.generation;
        wizard.select_device_role(DeviceRole::Host);
        assert!(wizard.generation > generation);
        assert!(wizard.report.is_none());
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
        assert!(wizard.report.is_none());
        assert_eq!(wizard.view().connection_mode, "private_lan");
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
    fn unsupported_installation_scopes_remain_unavailable() {
        for scope in [
            TargetScope::CurrentUser,
            TargetScope::System,
            TargetScope::NativePackage,
            TargetScope::PortableUser,
        ] {
            assert!(!target_scope_supported(scope));
        }
    }

    #[test]
    fn no_verified_artifact_or_authoritative_plan_is_claimed() {
        let (wizard, _) = reviewed_wizard();
        let view = wizard.view();
        assert!(!view.verified_artifact_available);
        assert!(!view.plan_preview_available);
        assert!(!view.target_scope_detected);
        assert!(!view.privilege_assessed);
    }

    #[test]
    fn blocking_engine_finding_is_reduced_to_a_bounded_boolean() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        let finding = PreflightFinding {
            finding_id: "/private/path/with-secret".into(),
            severity: FindingSeverity::Blocking,
            capability: "raw capability data".into(),
            redacted_evidence: "raw diagnostic with token=secret".into(),
            blocks_planning: true,
        };
        let report = wizard
            .probe_for(token)
            .expect("probe exists")
            .inspect()
            .with_findings(&[finding]);
        assert_eq!(report.blocking_finding_count, 1);
        assert!(!format!("{report:?}").contains("private/path"));
        assert!(!format!("{report:?}").contains("secret"));
        assert!(wizard.complete_preflight(token, Ok(report)));
        assert!(wizard.view().blocking_preflight_finding);
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
        let report = wizard.probe_for(token).expect("probe exists").inspect();
        wizard.cancel();
        assert_eq!(wizard.view().state, "closed");
        assert!(!wizard.complete_preflight(token, Ok(report)));
        assert_eq!(wizard.view().device_role, "");
    }

    #[test]
    fn out_of_order_result_is_ignored_after_selection_changes() {
        let mut wizard = open_at_connection();
        let token = wizard.continue_forward().expect("preflight starts");
        let report = wizard.probe_for(token).expect("probe exists").inspect();
        wizard.back();
        wizard.select_connection_mode(ConnectionMode::PrivateLan);
        assert!(!wizard.complete_preflight(token, Ok(report)));
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
        let report = wizard.probe_for(retry).expect("retry probe").inspect();
        assert!(wizard.complete_preflight(retry, Ok(report)));
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
    }
}
