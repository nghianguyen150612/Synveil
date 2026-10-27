//! Read-only host inspection for the P008 wizard.
//!
//! This module has no installation adapter, journal, credential, listener,
//! privilege, or mutation API. Its only subprocesses are fixed, bounded
//! information queries. All host details are reduced to enums and short
//! presentation strings before they can reach QML.

use crate::installer_wizard::{ConnectionMode, DeviceRole, PlatformCode};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::fs::File;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::path::PathBuf;
use std::{
    ffi::OsString,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use synveil_install_engine::{
    ArtifactEvidenceKind, ArtifactSourceObservation, ArtifactSourceRequest,
    AuthoritativePlanBuilder, FindingSeverity, PlanArchitecture, PlanBuildBlocker,
    PlanBuildOutcome, PlanChannel, PlanPlatform, PrivilegeRequirement, ReadOnlyArtifactSource,
    ResourceClass, Reversibility, TargetScope,
};

const MAX_COMMAND_OUTPUT: usize = 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_millis(700);
#[cfg(target_os = "linux")]
const MAX_OS_RELEASE_BYTES: u64 = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallationChannel {
    LinuxDeb,
    LinuxRpm,
    LinuxAppImage,
    WindowsPerUserSetup,
    WindowsPortable,
    ExistingInstallation,
}

impl InstallationChannel {
    pub const fn code(self) -> &'static str {
        match self {
            Self::LinuxDeb => "linux_deb",
            Self::LinuxRpm => "linux_rpm",
            Self::LinuxAppImage => "linux_appimage",
            Self::WindowsPerUserSetup => "windows_setup",
            Self::WindowsPortable => "windows_portable",
            Self::ExistingInstallation => "existing_installation",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LinuxDeb => "Linux native package (DEB)",
            Self::LinuxRpm => "Linux native package (RPM)",
            Self::LinuxAppImage => "Linux AppImage / portable",
            Self::WindowsPerUserSetup => "Windows per-user Setup",
            Self::WindowsPortable => "Windows portable distribution",
            Self::ExistingInstallation => "Existing installation management",
        }
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "linux_deb" => Some(Self::LinuxDeb),
            "linux_rpm" => Some(Self::LinuxRpm),
            "linux_appimage" => Some(Self::LinuxAppImage),
            "windows_setup" => Some(Self::WindowsPerUserSetup),
            "windows_portable" => Some(Self::WindowsPortable),
            "existing_installation" => Some(Self::ExistingInstallation),
            _ => None,
        }
    }

    pub const fn belongs_to(self, platform: PlatformCode) -> bool {
        matches!(
            (self, platform),
            (
                Self::LinuxDeb | Self::LinuxRpm | Self::LinuxAppImage,
                PlatformCode::Linux
            ) | (
                Self::WindowsPerUserSetup | Self::WindowsPortable,
                PlatformCode::Windows
            ) | (Self::ExistingInstallation, _)
        )
    }

    pub fn choices_for(platform: PlatformCode) -> Vec<Self> {
        match platform {
            PlatformCode::Linux => vec![
                Self::LinuxDeb,
                Self::LinuxRpm,
                Self::LinuxAppImage,
                Self::ExistingInstallation,
            ],
            PlatformCode::Windows => vec![
                Self::WindowsPerUserSetup,
                Self::WindowsPortable,
                Self::ExistingInstallation,
            ],
            PlatformCode::MacOs | PlatformCode::Other => vec![Self::ExistingInstallation],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelAvailability {
    PreviewAvailable,
    PreviewPartiallyAvailable,
    Unsupported,
    RequiredArtifactMissing,
    InstallationStateUncertain,
}

impl ChannelAvailability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PreviewAvailable => "available",
            Self::PreviewPartiallyAvailable => "partially available",
            Self::Unsupported => "unsupported",
            Self::RequiredArtifactMissing => "required artifact missing",
            Self::InstallationStateUncertain => "installation state uncertain",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactEvidenceClass {
    VerifiedReleaseArtifact,
    LocallySuppliedUnverified,
    NativePackageManagerManaged,
    NoArtifact,
    ArtifactIdentityUnknown,
}

impl ArtifactEvidenceClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifiedReleaseArtifact => "verified release artifact",
            Self::LocallySuppliedUnverified => "locally supplied, unverified artifact",
            Self::NativePackageManagerManaged => "native package-manager-managed artifact",
            Self::NoArtifact => "no artifact supplied",
            Self::ArtifactIdentityUnknown => "artifact identity unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreflightRequest {
    pub generation: u64,
    pub role: DeviceRole,
    pub connection_mode: ConnectionMode,
    pub selected_channel: Option<InstallationChannel>,
    pub client_profile_available: bool,
    pub existing_installation_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightBinding {
    pub selection: PreflightRequest,
    /// The exact bounded host snapshot produced by this inspection. It is
    /// presentation evidence only; any later action must inspect again.
    pub platform: PlatformEvidence,
    pub channels: Vec<InstallationChannelEvidence>,
    pub private_lan_candidate_count: Option<u8>,
    pub target_scope: Option<TargetScope>,
    /// None until a trusted artifact acquisition path supplies an identity.
    pub artifact_identity: Option<String>,
}

impl PreflightBinding {
    pub fn matches_request(&self, request: PreflightRequest) -> bool {
        if self.selection != request
            || self.platform.platform != PlatformCode::current()
            || self.artifact_identity.is_some()
            || self
                .channels
                .iter()
                .any(|channel| !channel.channel.belongs_to(self.platform.platform))
        {
            return false;
        }

        let selected = request
            .selected_channel
            .and_then(|channel| self.channels.iter().find(|item| item.channel == channel))
            .or_else(|| {
                let mut active = self.channels.iter().filter(|item| {
                    item.active && item.channel != InstallationChannel::ExistingInstallation
                });
                let first = active.next()?;
                active.next().is_none().then_some(first)
            });
        self.target_scope == selected.and_then(|channel| channel.target_scope)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformEvidence {
    pub platform: PlatformCode,
    pub display_name: String,
    pub version: String,
    pub architecture: &'static str,
    pub managed_host_supported: bool,
    pub operating_system_evidence_complete: bool,
}

impl PlatformEvidence {
    pub fn summary(&self) -> String {
        format_platform_summary(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationChannelEvidence {
    pub channel: InstallationChannel,
    pub availability: ChannelAvailability,
    pub active: bool,
    pub target_scope: Option<TargetScope>,
    pub artifact_evidence: ArtifactEvidenceClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityFindingCode {
    UnsupportedPlatform,
    UnsupportedOperatingSystemVersion,
    UnsupportedInstallationChannel,
    OperatingSystemEvidenceUnavailable,
    MissingPackageManager,
    MissingNativeDependency(&'static str),
    InstallationChannelAmbiguous,
    ExistingInstallationOwnershipUncertain,
    ArtifactUnavailable,
    ArtifactVerificationIncomplete,
    AuthoritativePlanUnavailable,
    InvalidPlan,
    InspectionFailed,
    InspectionCancelled,
    StaleInspection,
    InsufficientCapabilityEvidence,
    PrivateLanUnavailable,
    PrivateLanChoiceRequired,
    RemoteEndpointIdentityUnverified,
    TailscaleUnavailable,
}

impl CapabilityFindingCode {
    pub fn label(self) -> String {
        match self {
            Self::UnsupportedPlatform => {
                "This platform has no supported installation channel.".to_owned()
            }
            Self::UnsupportedOperatingSystemVersion => {
                "This operating-system version is outside the managed Host support matrix."
                    .to_owned()
            }
            Self::UnsupportedInstallationChannel => {
                "The selected installation channel is not supported on this platform.".to_owned()
            }
            Self::OperatingSystemEvidenceUnavailable => {
                "The operating-system version could not be identified safely.".to_owned()
            }
            Self::MissingPackageManager => {
                "The required native package manager is unavailable.".to_owned()
            }
            Self::MissingNativeDependency(name) => {
                format!("Required native Host dependency is unavailable: {name}.")
            }
            Self::InstallationChannelAmbiguous => {
                "Select an installation channel before requesting a preview.".to_owned()
            }
            Self::ExistingInstallationOwnershipUncertain => {
                "Existing installation ownership could not be verified by this bounded inspection."
                    .to_owned()
            }
            Self::ArtifactUnavailable => {
                "The required installation artifact is not available on this device.".to_owned()
            }
            Self::ArtifactVerificationIncomplete => {
                "Artifact identity or publisher verification is incomplete.".to_owned()
            }
            Self::AuthoritativePlanUnavailable => {
                "No authoritative installation plan builder exists for this channel.".to_owned()
            }
            Self::InvalidPlan => {
                "The authoritative installation plan failed validation.".to_owned()
            }
            Self::InspectionFailed => {
                "Read-only capability inspection did not complete.".to_owned()
            }
            Self::InspectionCancelled => {
                "Read-only capability inspection was cancelled.".to_owned()
            }
            Self::StaleInspection => {
                "This inspection was discarded because the choices changed.".to_owned()
            }
            Self::InsufficientCapabilityEvidence => {
                "There is not enough verified capability evidence to continue.".to_owned()
            }
            Self::PrivateLanUnavailable => {
                "No suitable Private LAN candidate was observed.".to_owned()
            }
            Self::PrivateLanChoiceRequired => {
                "Several Private LAN candidates were observed; this preview cannot choose one."
                    .to_owned()
            }
            Self::RemoteEndpointIdentityUnverified => {
                "Network reachability does not verify a remote endpoint or authorize sign-in."
                    .to_owned()
            }
            Self::TailscaleUnavailable => {
                "Tailscale is unavailable until an approved integration exists.".to_owned()
            }
        }
    }

    pub const fn severity(self) -> FindingSeverity {
        match self {
            Self::RemoteEndpointIdentityUnverified => FindingSeverity::Warning,
            _ => FindingSeverity::Blocking,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityFinding {
    pub code: CapabilityFindingCode,
    pub severity: FindingSeverity,
}

impl CapabilityFinding {
    fn new(code: CapabilityFindingCode) -> Self {
        Self {
            code,
            severity: code.severity(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanPreviewUnavailableReason {
    VerifiedArtifactEvidenceRequired,
    AuthoritativePlanBuilderUnavailable,
    PlanValidationFailed,
    InsufficientCapabilityEvidence,
}

impl PlanPreviewUnavailableReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerifiedArtifactEvidenceRequired => {
                "Plan preview unavailable — no release source currently supplies independently authenticated artifact evidence; the production publisher trust root and approved channel effect catalog are unavailable. P004-D02, D11, and D14 remain pending."
            }
            Self::AuthoritativePlanBuilderUnavailable => {
                "Plan preview unavailable — no authoritative plan builder exists for this channel"
            }
            Self::PlanValidationFailed => "Plan preview unavailable — the plan failed validation",
            Self::InsufficientCapabilityEvidence => {
                "Plan preview unavailable — capability evidence is incomplete"
            }
        }
    }
}

/// Safe, engine-owned information that a future validated plan mapper can
/// publish. P008 does not construct this value because no authoritative,
/// channel-specific plan builder exists in the current source.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanPreview {
    pub intent: &'static str,
    pub target_scope: TargetScope,
    pub installation_channel: InstallationChannel,
    pub artifact_evidence: ArtifactEvidenceClass,
    pub ordered_operation_categories: Vec<&'static str>,
    pub resources_expected_to_touch: Vec<ResourceClass>,
    pub preservation_expectations: Vec<ResourceClass>,
    pub privilege: PrivilegeRequirement,
    pub reversibility: Reversibility,
    pub verification_requirements: Vec<&'static str>,
    pub blocking_findings: Vec<CapabilityFindingCode>,
    pub confirmation_prerequisites: Vec<&'static str>,
    pub validity: PlanValidity,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanValidity {
    Valid,
    Invalid,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanPreviewAvailability {
    Available(PlanPreview),
    PartiallyAvailable(PlanPreview),
    Unavailable(PlanPreviewUnavailableReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightOutcome {
    pub binding: PreflightBinding,
    pub findings: Vec<CapabilityFinding>,
    pub selected_channel_supported: bool,
    pub plan_preview: PlanPreviewAvailability,
    pub plan_build_blockers: Vec<PlanBuildBlocker>,
}

impl PreflightOutcome {
    pub fn platform_summary(&self) -> String {
        format_platform_summary(&self.binding.platform)
    }

    pub fn channel_summary(&self) -> String {
        self.binding
            .channels
            .iter()
            .map(|channel| {
                let active = if channel.active {
                    " (marker observed)"
                } else {
                    ""
                };
                format!(
                    "{}: {}{}",
                    channel.channel.label(),
                    channel.availability.label(),
                    active
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn finding_summary(&self) -> String {
        if self.findings.is_empty() {
            return "No additional capability findings.".to_owned();
        }
        self.findings
            .iter()
            .map(|finding| finding.code.label())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn artifact_summary(&self) -> String {
        self.selected_channel()
            .map_or(ArtifactEvidenceClass::NoArtifact, |channel| {
                channel.artifact_evidence
            })
            .label()
            .to_owned()
    }

    pub fn scope_summary(&self) -> String {
        self.selected_channel()
            .and_then(|channel| channel.target_scope)
            .map_or_else(
                || "Target scope unavailable until an installation channel is identified.".into(),
                |scope| match scope {
                    TargetScope::CurrentUser => "Current user".into(),
                    TargetScope::System => "System (not selected by privilege detection)".into(),
                    TargetScope::NativePackage => "Native package manager".into(),
                    TargetScope::PortableUser => "Portable, current user".into(),
                },
            )
    }

    pub fn plan_summary(&self) -> String {
        if !self.plan_build_blockers.is_empty() {
            return self
                .plan_build_blockers
                .iter()
                .map(|blocker| blocker.label())
                .collect::<Vec<_>>()
                .join("; ");
        }
        match &self.plan_preview {
            PlanPreviewAvailability::Unavailable(reason) => reason.label().to_owned(),
            PlanPreviewAvailability::Available(_)
            | PlanPreviewAvailability::PartiallyAvailable(_) => {
                "Plan preview unavailable — authoritative plan mapping is not enabled".to_owned()
            }
        }
    }

    pub fn selected_channel(&self) -> Option<&InstallationChannelEvidence> {
        self.binding
            .selection
            .selected_channel
            .and_then(|selected| {
                self.binding
                    .channels
                    .iter()
                    .find(|item| item.channel == selected)
            })
            .or_else(|| {
                let mut active = self.binding.channels.iter().filter(|item| {
                    item.active && item.channel != InstallationChannel::ExistingInstallation
                });
                let first = active.next()?;
                active.next().is_none().then_some(first)
            })
    }

    pub fn connection_summary(&self) -> String {
        match self.binding.selection.connection_mode {
            ConnectionMode::LocalOnly => {
                "Local-only selected; no listener or endpoint was changed.".into()
            }
            ConnectionMode::PrivateLan => match self.binding.private_lan_candidate_count {
                Some(0) => "No suitable Private LAN candidate was observed.".into(),
                Some(1) => {
                    "One suitable Private LAN candidate was observed; endpoint identity remains unverified.".into()
                }
                Some(_) => {
                    "Multiple suitable Private LAN candidates were observed; the preview does not choose one.".into()
                }
                None => {
                    "Private LAN capability was not observed for this role; remote identity remains unverified.".into()
                }
            },
            ConnectionMode::Manual => {
                "Advanced/manual mode uses the existing endpoint entry flow; no address was entered or saved.".into()
            }
            ConnectionMode::Tailscale => "Tailscale is unavailable.".into(),
        }
    }
}

pub fn detect_platform_evidence() -> PlatformEvidence {
    inspect_platform()
}

fn format_platform_summary(platform: &PlatformEvidence) -> String {
    if platform.version.is_empty() {
        format!(
            "{}; operating-system version unknown; {}",
            platform.display_name, platform.architecture
        )
    } else {
        format!(
            "{} {}; {}",
            platform.display_name, platform.version, platform.architecture
        )
    }
}

pub fn inspect(request: PreflightRequest) -> PreflightOutcome {
    let platform = detect_platform_evidence();
    let mut findings = Vec::new();
    let channels = inspect_channels(&platform, &mut findings);
    let mut private_lan_candidate_count = None;

    if !platform.operating_system_evidence_complete {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::OperatingSystemEvidenceUnavailable,
        ));
    }
    if matches!(platform.platform, PlatformCode::Other | PlatformCode::MacOs) {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::UnsupportedPlatform,
        ));
    }

    if request.connection_mode == ConnectionMode::PrivateLan && request.role == DeviceRole::Host {
        match synveil_server_host::inspect_private_lan_capability() {
            synveil_server_host::PrivateLanCapability::Available { candidate_count } => {
                private_lan_candidate_count = Some(candidate_count);
                if candidate_count > 1 {
                    findings.push(CapabilityFinding::new(
                        CapabilityFindingCode::PrivateLanChoiceRequired,
                    ));
                }
            }
            synveil_server_host::PrivateLanCapability::NoCandidate => {
                private_lan_candidate_count = Some(0);
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::PrivateLanUnavailable,
                ));
            }
            synveil_server_host::PrivateLanCapability::InspectionUnavailable
            | synveil_server_host::PrivateLanCapability::Unsupported => {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::InsufficientCapabilityEvidence,
                ));
            }
        }
    }

    match request.role {
        DeviceRole::Host => {
            if !platform.managed_host_supported {
                let finding = if matches!(platform.platform, PlatformCode::Windows) {
                    CapabilityFindingCode::UnsupportedPlatform
                } else {
                    CapabilityFindingCode::UnsupportedOperatingSystemVersion
                };
                findings.push(CapabilityFinding::new(finding));
            }
            #[cfg(target_os = "linux")]
            {
                for (dependency, label) in [
                    ("/usr/bin/pkexec", "pkexec authorization helper"),
                    ("/usr/bin/systemctl", "systemd control utility"),
                    ("/usr/bin/ip", "iproute interface inspection utility"),
                ] {
                    if !Path::new(dependency).is_file() {
                        findings.push(CapabilityFinding::new(
                            CapabilityFindingCode::MissingNativeDependency(label),
                        ));
                    }
                }
            }
        }
        DeviceRole::Client => {
            if !request.client_profile_available {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::InsufficientCapabilityEvidence,
                ));
            }
            if matches!(
                request.connection_mode,
                ConnectionMode::PrivateLan | ConnectionMode::Manual
            ) {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::RemoteEndpointIdentityUnverified,
                ));
                if request.connection_mode == ConnectionMode::PrivateLan {
                    findings.push(CapabilityFinding::new(
                        CapabilityFindingCode::InsufficientCapabilityEvidence,
                    ));
                }
            }
        }
        DeviceRole::Existing => {
            if !request.existing_installation_available
                && !channels.iter().any(|channel| channel.active)
            {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::ExistingInstallationOwnershipUncertain,
                ));
            }
        }
    }

    if request.connection_mode == ConnectionMode::Tailscale {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::TailscaleUnavailable,
        ));
    }

    let active_channel_count = channels
        .iter()
        .filter(|channel| {
            channel.active && channel.channel != InstallationChannel::ExistingInstallation
        })
        .count();
    let selected_channel_supported = request.selected_channel.map_or_else(
        || {
            active_channel_count == 1
                && channels.iter().any(|channel| {
                    channel.active
                        && channel.channel != InstallationChannel::ExistingInstallation
                        && matches!(
                            channel.availability,
                            ChannelAvailability::PreviewAvailable
                                | ChannelAvailability::PreviewPartiallyAvailable
                                | ChannelAvailability::RequiredArtifactMissing
                        )
                })
        },
        |selected| {
            channels
                .iter()
                .find(|channel| channel.channel == selected)
                .is_some_and(|channel| {
                    matches!(
                        channel.availability,
                        ChannelAvailability::PreviewAvailable
                            | ChannelAvailability::PreviewPartiallyAvailable
                            | ChannelAvailability::RequiredArtifactMissing
                    )
                })
        },
    );
    if request.selected_channel.is_none() && active_channel_count != 1 {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::InstallationChannelAmbiguous,
        ));
    }
    if let Some(selected) = request.selected_channel {
        if let Some(channel) = channels.iter().find(|item| item.channel == selected) {
            if channel.availability == ChannelAvailability::RequiredArtifactMissing {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::ArtifactUnavailable,
                ));
            }
            if matches!(
                channel.artifact_evidence,
                ArtifactEvidenceClass::LocallySuppliedUnverified
                    | ArtifactEvidenceClass::ArtifactIdentityUnknown
                    | ArtifactEvidenceClass::NativePackageManagerManaged
            ) {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::ArtifactVerificationIncomplete,
                ));
            }
            if channel.availability == ChannelAvailability::Unsupported {
                findings.push(CapabilityFinding::new(
                    CapabilityFindingCode::UnsupportedInstallationChannel,
                ));
            }
        }
    }

    findings.push(CapabilityFinding::new(
        CapabilityFindingCode::AuthoritativePlanUnavailable,
    ));
    let plan_preview = PlanPreviewAvailability::Unavailable(
        PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired,
    );

    let selected = request
        .selected_channel
        .and_then(|selected| channels.iter().find(|channel| channel.channel == selected))
        .or_else(|| {
            let mut active = channels.iter().filter(|channel| {
                channel.active && channel.channel != InstallationChannel::ExistingInstallation
            });
            let first = active.next()?;
            active.next().is_none().then_some(first)
        });
    let target_scope = selected.and_then(|channel| channel.target_scope);
    let plan_build_blockers = plan_build_blockers(&request, &platform, &channels);
    let binding = PreflightBinding {
        selection: request,
        platform,
        channels,
        private_lan_candidate_count,
        target_scope,
        artifact_identity: None,
    };

    PreflightOutcome {
        binding,
        findings,
        selected_channel_supported,
        plan_preview,
        plan_build_blockers,
    }
}

struct PreflightArtifactSource(ArtifactEvidenceKind);

impl ReadOnlyArtifactSource for PreflightArtifactSource {
    fn observe(&self, _request: ArtifactSourceRequest) -> ArtifactSourceObservation {
        ArtifactSourceObservation {
            evidence_kind: self.0,
            manifest_generation: None,
        }
    }
}

fn plan_build_blockers(
    request: &PreflightRequest,
    platform: &PlatformEvidence,
    channels: &[InstallationChannelEvidence],
) -> Vec<PlanBuildBlocker> {
    let plan_platform = match (
        platform.platform,
        platform.display_name.as_str(),
        platform.version.as_str(),
    ) {
        (PlatformCode::Linux, "Ubuntu", "24.04") => PlanPlatform::Ubuntu2404,
        (PlatformCode::Linux, "Fedora", "42") => PlanPlatform::Fedora42,
        (PlatformCode::Linux, _, _) => PlanPlatform::OtherLinux,
        (PlatformCode::Windows, _, _) => PlanPlatform::Windows,
        (PlatformCode::MacOs, _, _) => PlanPlatform::MacOs,
        (PlatformCode::Other, _, _) => PlanPlatform::Other,
    };
    let architecture = match platform.architecture {
        "x86-64" => PlanArchitecture::X86_64,
        "ARM64" => PlanArchitecture::Aarch64,
        _ => PlanArchitecture::Other,
    };
    let selected = request
        .selected_channel
        .and_then(|value| channels.iter().find(|item| item.channel == value))
        .or_else(|| {
            let mut active = channels.iter().filter(|item| {
                item.active && item.channel != InstallationChannel::ExistingInstallation
            });
            let first = active.next()?;
            active.next().is_none().then_some(first)
        });
    let channel = selected.map_or(PlanChannel::Unknown, |item| match item.channel {
        InstallationChannel::LinuxDeb => PlanChannel::UbuntuDeb,
        InstallationChannel::LinuxRpm => PlanChannel::FedoraRpm,
        InstallationChannel::LinuxAppImage => PlanChannel::LinuxAppImage,
        InstallationChannel::WindowsPerUserSetup => PlanChannel::WindowsPerUserSetup,
        InstallationChannel::WindowsPortable => PlanChannel::WindowsPortableZip,
        InstallationChannel::ExistingInstallation => PlanChannel::ExistingInstallationMaintenance,
    });
    let artifact_evidence = selected.map_or(
        ArtifactEvidenceKind::UnknownOrMissingArtifact,
        |item| match item.artifact_evidence {
            ArtifactEvidenceClass::VerifiedReleaseArtifact => {
                // P008 has not bound authenticated channel and manifest bytes
                // to this exact artifact; its marker is incomplete provenance.
                ArtifactEvidenceKind::IncompleteProvenance
            }
            ArtifactEvidenceClass::LocallySuppliedUnverified => {
                ArtifactEvidenceKind::LocallySuppliedUntrustedArtifact
            }
            ArtifactEvidenceClass::NativePackageManagerManaged => {
                ArtifactEvidenceKind::NativePackageManagerManagedIdentity
            }
            ArtifactEvidenceClass::NoArtifact | ArtifactEvidenceClass::ArtifactIdentityUnknown => {
                ArtifactEvidenceKind::UnknownOrMissingArtifact
            }
        },
    );
    let target_scope = selected
        .and_then(|item| item.target_scope)
        .unwrap_or(TargetScope::CurrentUser);
    let request = ArtifactSourceRequest {
        platform: plan_platform,
        architecture,
        channel,
        target_scope,
        intent: synveil_install_engine::InstallationIntent::Install,
        capability_generation: request.generation,
    };
    let PlanBuildOutcome::Blocked(blockers) =
        AuthoritativePlanBuilder.build(request, &PreflightArtifactSource(artifact_evidence));
    blockers
}

fn inspect_platform() -> PlatformEvidence {
    #[cfg(target_os = "linux")]
    {
        let os_release = read_os_release();
        let (display_name, version, complete) = os_release.map_or_else(
            || ("Linux".to_owned(), String::new(), false),
            |(id, version)| {
                let display_name = match id.as_str() {
                    "ubuntu" => "Ubuntu",
                    "debian" => "Debian",
                    "fedora" => "Fedora",
                    "opensuse-tumbleweed" | "opensuse-leap" => "openSUSE",
                    "arch" => "Arch Linux",
                    "alpine" => "Alpine Linux",
                    _ => "Other Linux distribution",
                };
                let complete = !version.is_empty();
                (display_name.to_owned(), version, complete)
            },
        );
        let managed_host_supported = complete
            && std::env::consts::ARCH == "x86_64"
            && matches!(
                (display_name.as_str(), version.as_str()),
                ("Ubuntu", "24.04") | ("Fedora", "42")
            );
        PlatformEvidence {
            platform: PlatformCode::Linux,
            display_name,
            version,
            architecture: architecture_label(),
            managed_host_supported,
            operating_system_evidence_complete: complete,
        }
    }

    #[cfg(target_os = "windows")]
    {
        let version = inspect_windows_version().unwrap_or_default();
        PlatformEvidence {
            platform: PlatformCode::Windows,
            display_name: "Windows".to_owned(),
            operating_system_evidence_complete: !version.is_empty(),
            version,
            architecture: architecture_label(),
            managed_host_supported: false,
        }
    }

    #[cfg(target_os = "macos")]
    {
        PlatformEvidence {
            platform: PlatformCode::MacOs,
            display_name: "macOS".to_owned(),
            version: String::new(),
            architecture: architecture_label(),
            managed_host_supported: false,
            operating_system_evidence_complete: false,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        PlatformEvidence {
            platform: PlatformCode::Other,
            display_name: "Unsupported platform".to_owned(),
            version: String::new(),
            architecture: architecture_label(),
            managed_host_supported: false,
            operating_system_evidence_complete: false,
        }
    }
}

fn architecture_label() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86-64",
        "aarch64" => "ARM64",
        "x86" => "x86",
        "arm" => "ARM",
        _ => "other architecture",
    }
}

#[cfg(target_os = "linux")]
fn read_os_release() -> Option<(String, String)> {
    let file = File::open("/etc/os-release").ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_OS_RELEASE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 == MAX_OS_RELEASE_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix('=')?;
            let value = value.trim_matches(['"', '\'']);
            (!value.is_empty() && value.len() <= 32).then_some(value)
        })
    };
    let id = value("ID")?
        .to_ascii_lowercase()
        .chars()
        .take_while(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    if id.is_empty() {
        return None;
    }
    let version = value("VERSION_ID")
        .filter(|version| {
            version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        })
        .unwrap_or_default()
        .to_owned();
    Some((id, version))
}

fn inspect_channels(
    platform: &PlatformEvidence,
    findings: &mut Vec<CapabilityFinding>,
) -> Vec<InstallationChannelEvidence> {
    match platform.platform {
        PlatformCode::Linux => inspect_linux_channels(platform, findings),
        PlatformCode::Windows => inspect_windows_channels(platform, findings),
        PlatformCode::MacOs | PlatformCode::Other => {
            vec![InstallationChannelEvidence {
                channel: InstallationChannel::ExistingInstallation,
                availability: ChannelAvailability::Unsupported,
                active: false,
                target_scope: None,
                artifact_evidence: ArtifactEvidenceClass::NoArtifact,
            }]
        }
    }
}

#[cfg(target_os = "linux")]
fn inspect_linux_channels(
    platform: &PlatformEvidence,
    findings: &mut Vec<CapabilityFinding>,
) -> Vec<InstallationChannelEvidence> {
    let dpkg_query = Path::new("/usr/bin/dpkg-query");
    let apt_get = Path::new("/usr/bin/apt-get");
    let rpm = Path::new("/usr/bin/rpm");
    let dnf = Path::new("/usr/bin/dnf");
    let deb_available = dpkg_query.is_file() && apt_get.is_file();
    let rpm_available = rpm.is_file() && dnf.is_file();
    let executable = std::env::current_exe().ok();
    let deb_ownership = if dpkg_query.is_file() {
        executable
            .as_deref()
            .map_or(OwnershipProbe::Unknown, |path| {
                package_owns_executable(dpkg_query, "-S", path, false)
            })
    } else {
        OwnershipProbe::Unknown
    };
    let rpm_ownership = if rpm.is_file() {
        executable
            .as_deref()
            .map_or(OwnershipProbe::Unknown, |path| {
                package_owns_executable(rpm, "-qf", path, true)
            })
    } else {
        OwnershipProbe::Unknown
    };
    let deb_active = deb_ownership == OwnershipProbe::Synveil;
    let rpm_active = rpm_ownership == OwnershipProbe::Synveil;
    let appimage = active_appimage();
    let supported_deb = deb_available
        && matches!(platform.display_name.as_str(), "Ubuntu" | "Debian")
        && std::env::consts::ARCH == "x86_64";
    let supported_rpm = rpm_available
        && platform.display_name == "Fedora"
        && platform.version == "42"
        && std::env::consts::ARCH == "x86_64";
    let appimage_supported = std::env::consts::ARCH == "x86_64";

    if !deb_available && matches!(platform.display_name.as_str(), "Ubuntu" | "Debian") {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::MissingPackageManager,
        ));
    }
    if !rpm_available && platform.display_name == "Fedora" {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::MissingPackageManager,
        ));
    }

    let mut channels = vec![
        InstallationChannelEvidence {
            channel: InstallationChannel::LinuxDeb,
            availability: if deb_active {
                ChannelAvailability::PreviewPartiallyAvailable
            } else if supported_deb {
                ChannelAvailability::RequiredArtifactMissing
            } else {
                ChannelAvailability::Unsupported
            },
            active: deb_active,
            target_scope: supported_deb.then_some(TargetScope::NativePackage),
            artifact_evidence: if deb_active {
                ArtifactEvidenceClass::NativePackageManagerManaged
            } else {
                ArtifactEvidenceClass::NoArtifact
            },
        },
        InstallationChannelEvidence {
            channel: InstallationChannel::LinuxRpm,
            availability: if rpm_active {
                ChannelAvailability::PreviewPartiallyAvailable
            } else if supported_rpm {
                ChannelAvailability::RequiredArtifactMissing
            } else {
                ChannelAvailability::Unsupported
            },
            active: rpm_active,
            target_scope: supported_rpm.then_some(TargetScope::NativePackage),
            artifact_evidence: if rpm_active {
                ArtifactEvidenceClass::NativePackageManagerManaged
            } else {
                ArtifactEvidenceClass::NoArtifact
            },
        },
        InstallationChannelEvidence {
            channel: InstallationChannel::LinuxAppImage,
            availability: if !appimage_supported {
                ChannelAvailability::Unsupported
            } else if appimage == Some(false) {
                ChannelAvailability::RequiredArtifactMissing
            } else {
                ChannelAvailability::InstallationStateUncertain
            },
            active: appimage == Some(true),
            target_scope: appimage_supported.then_some(TargetScope::PortableUser),
            artifact_evidence: match appimage {
                Some(true) => ArtifactEvidenceClass::LocallySuppliedUnverified,
                Some(false) => ArtifactEvidenceClass::NoArtifact,
                None => ArtifactEvidenceClass::ArtifactIdentityUnknown,
            },
        },
    ];

    let active_channels = channels.iter().filter(|channel| channel.active).count();
    let unique_active_channel = if active_channels == 1 {
        channels.iter().find(|channel| channel.active)
    } else {
        None
    };
    channels.push(InstallationChannelEvidence {
        channel: InstallationChannel::ExistingInstallation,
        availability: ChannelAvailability::InstallationStateUncertain,
        active: active_channels > 0,
        target_scope: unique_active_channel.and_then(|channel| channel.target_scope),
        artifact_evidence: unique_active_channel.map_or_else(
            || {
                if active_channels > 1 {
                    ArtifactEvidenceClass::ArtifactIdentityUnknown
                } else {
                    ArtifactEvidenceClass::NoArtifact
                }
            },
            |channel| channel.artifact_evidence,
        ),
    });
    findings.push(CapabilityFinding::new(
        CapabilityFindingCode::ExistingInstallationOwnershipUncertain,
    ));
    channels
}

#[cfg(not(target_os = "linux"))]
fn inspect_linux_channels(
    _platform: &PlatformEvidence,
    _findings: &mut Vec<CapabilityFinding>,
) -> Vec<InstallationChannelEvidence> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn active_appimage() -> Option<bool> {
    let Some(path) = std::env::var_os("APPIMAGE").map(PathBuf::from) else {
        return Some(false);
    };
    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| metadata.file_type().is_file())
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnershipProbe {
    Synveil,
    Other,
    Unknown,
}

#[cfg(target_os = "linux")]
fn package_owns_executable(
    manager: &Path,
    operation: &str,
    executable: &Path,
    rpm: bool,
) -> OwnershipProbe {
    let args = if rpm {
        vec![
            OsString::from(operation),
            OsString::from("--qf"),
            OsString::from("%{NAME}"),
            executable.as_os_str().to_owned(),
        ]
    } else {
        vec![OsString::from(operation), executable.as_os_str().to_owned()]
    };
    // Fixed program paths and fixed flags; the executable path comes from the
    // OS process API and is passed as one argument without a shell.
    let Some((success, output)) = run_bounded_command(manager, &args) else {
        return OwnershipProbe::Unknown;
    };
    if !success {
        return OwnershipProbe::Unknown;
    }
    if package_owner_matches(&output, rpm) {
        OwnershipProbe::Synveil
    } else {
        OwnershipProbe::Other
    }
}

#[allow(dead_code)]
fn package_owner_matches(output: &[u8], rpm: bool) -> bool {
    let output = String::from_utf8_lossy(output);
    if rpm {
        output.trim() == "synveil"
    } else {
        output.lines().any(|line| {
            line.split_once(':')
                .is_some_and(|(package, _)| package.trim() == "synveil")
        })
    }
}

#[allow(dead_code)]
fn run_bounded_command(program: &Path, args: &[OsString]) -> Option<(bool, Vec<u8>)> {
    let mut command = Command::new(program);
    command.args(args).env_clear();
    #[cfg(target_os = "windows")]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        // reg.exe relies on this Windows system location. Preserve only this
        // required environment value; arbitrary caller variables stay cleared.
        command.env("SystemRoot", system_root);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .by_ref()
            .take((MAX_COMMAND_OUTPUT + 1) as u64)
            .read_to_end(&mut output)
            .ok()?;
        Some(output)
    });
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    }?;
    let output = reader.join().ok()??;
    if output.len() > MAX_COMMAND_OUTPUT {
        return None;
    }
    Some((status.success(), output))
}

#[cfg(target_os = "windows")]
fn inspect_windows_channels(
    platform: &PlatformEvidence,
    findings: &mut Vec<CapabilityFinding>,
) -> Vec<InstallationChannelEvidence> {
    let supported_architecture = std::env::consts::ARCH == "x86_64";
    let setup_registered = windows_setup_registration_present();
    let portable_manifest = windows_portable_manifest_present();
    let setup = InstallationChannelEvidence {
        channel: InstallationChannel::WindowsPerUserSetup,
        availability: if !supported_architecture {
            ChannelAvailability::Unsupported
        } else if setup_registered.is_none() || setup_registered == Some(true) {
            ChannelAvailability::InstallationStateUncertain
        } else {
            ChannelAvailability::RequiredArtifactMissing
        },
        active: setup_registered == Some(true),
        target_scope: supported_architecture.then_some(TargetScope::CurrentUser),
        artifact_evidence: match setup_registered {
            Some(true) => ArtifactEvidenceClass::ArtifactIdentityUnknown,
            Some(false) => ArtifactEvidenceClass::NoArtifact,
            None => ArtifactEvidenceClass::ArtifactIdentityUnknown,
        },
    };
    let portable = InstallationChannelEvidence {
        channel: InstallationChannel::WindowsPortable,
        availability: if !supported_architecture {
            ChannelAvailability::Unsupported
        } else if portable_manifest.is_none() || portable_manifest == Some(true) {
            ChannelAvailability::InstallationStateUncertain
        } else {
            ChannelAvailability::RequiredArtifactMissing
        },
        active: portable_manifest == Some(true),
        target_scope: supported_architecture.then_some(TargetScope::PortableUser),
        artifact_evidence: match portable_manifest {
            Some(true) => ArtifactEvidenceClass::LocallySuppliedUnverified,
            Some(false) => ArtifactEvidenceClass::NoArtifact,
            None => ArtifactEvidenceClass::ArtifactIdentityUnknown,
        },
    };
    if setup_registered != Some(false) || portable_manifest != Some(false) {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::ExistingInstallationOwnershipUncertain,
        ));
    }
    let active = setup_registered == Some(true) || portable_manifest == Some(true);
    let existing = InstallationChannelEvidence {
        channel: InstallationChannel::ExistingInstallation,
        availability: if setup_registered == Some(false) && portable_manifest == Some(false) {
            ChannelAvailability::Unsupported
        } else {
            ChannelAvailability::InstallationStateUncertain
        },
        active,
        target_scope: match (setup_registered, portable_manifest) {
            (Some(true), Some(false)) => Some(TargetScope::CurrentUser),
            (Some(false), Some(true)) => Some(TargetScope::PortableUser),
            _ => None,
        },
        artifact_evidence: match (setup_registered, portable_manifest) {
            (Some(true), Some(false)) => ArtifactEvidenceClass::ArtifactIdentityUnknown,
            (Some(false), Some(true)) => ArtifactEvidenceClass::LocallySuppliedUnverified,
            (Some(false), Some(false)) => ArtifactEvidenceClass::NoArtifact,
            _ => ArtifactEvidenceClass::ArtifactIdentityUnknown,
        },
    };
    if !supported_architecture {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::UnsupportedPlatform,
        ));
    } else if !platform.operating_system_evidence_complete {
        findings.push(CapabilityFinding::new(
            CapabilityFindingCode::InsufficientCapabilityEvidence,
        ));
    }
    vec![setup, portable, existing]
}

#[cfg(not(target_os = "windows"))]
fn inspect_windows_channels(
    _platform: &PlatformEvidence,
    _findings: &mut Vec<CapabilityFinding>,
) -> Vec<InstallationChannelEvidence> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_registry_query(args: &[OsString]) -> Option<(bool, Vec<u8>)> {
    let system_root = std::env::var_os("SystemRoot")?;
    let reg = PathBuf::from(&system_root).join("System32").join("reg.exe");
    run_bounded_command(&reg, args)
}

#[cfg(target_os = "windows")]
fn windows_setup_registration_present() -> Option<bool> {
    let args = [
        OsString::from("query"),
        OsString::from(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1",
        ),
        OsString::from("/v"),
        OsString::from("SynveilManifestSha256"),
        OsString::from("/reg:64"),
    ];
    windows_registry_query(&args).map(|(success, _)| success)
}

#[cfg(target_os = "windows")]
fn windows_portable_manifest_present() -> Option<bool> {
    let path = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("SYNVEIL-MANIFEST.txt"))
    })?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    let mut header = Vec::new();
    if file.take(128).read_to_end(&mut header).is_err() {
        return None;
    }
    Some(
        header
            .split(|byte| *byte == b'\n')
            .next()
            .is_some_and(|line| line == b"Synveil Windows portable package manifest"),
    )
}

#[cfg(target_os = "windows")]
fn inspect_windows_version() -> Option<String> {
    let args = [
        OsString::from("query"),
        OsString::from(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
        OsString::from("/v"),
        OsString::from("ProductName"),
        OsString::from("/reg:64"),
    ];
    let (success, output) = windows_registry_query(&args)?;
    if !success {
        return None;
    }
    let value = String::from_utf8_lossy(&output);
    let product = value.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let product = value.trim();
        if product.starts_with("Windows 11") {
            Some("11")
        } else if product.starts_with("Windows 10") {
            Some("10")
        } else {
            None
        }
    })?;
    Some(format!("{product} (version identified)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(role: DeviceRole, mode: ConnectionMode) -> PreflightRequest {
        PreflightRequest {
            generation: 7,
            role,
            connection_mode: mode,
            selected_channel: None,
            client_profile_available: true,
            existing_installation_available: false,
        }
    }

    #[test]
    fn only_read_only_channel_queries_are_part_of_preflight() {
        let source = include_str!("installer_preflight.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("preflight implementation exists");
        let bridge = include_str!("bridge.rs");
        let bridge_preflight = bridge
            .split("fn run_installer_wizard_preflight")
            .nth(1)
            .and_then(|source| source.split("impl cxx_qt::Initialize").next())
            .expect("bridge preflight boundary exists");
        assert!(!source.contains("InstallationJournal::"));
        assert!(!source.contains("InstallerEngine::execute"));
        assert!(!source.contains("InstallationAdapter"));
        assert!(!source.contains("TcpListener::bind"));
        assert!(!source.contains("Command::new(\"pkexec\")"));
        assert!(!source.contains("Command::new(\"systemctl\")"));
        assert!(!source.contains("OpenOptions"));
        assert!(!bridge_preflight.contains("InstallerEngine::execute"));
        assert!(!bridge_preflight.contains("InstallationJournal"));
        assert!(bridge_preflight.contains("installer_preflight::inspect(request)"));
        assert!(source.contains("Stdio::null()"));
        assert!(source.contains("COMMAND_TIMEOUT"));
    }

    #[test]
    fn user_request_cannot_supply_verified_artifact_evidence() {
        let request = request(DeviceRole::Client, ConnectionMode::Manual);
        assert!(!format!("{request:?}").contains("VerifiedArtifactEvidence"));
        assert!(!format!("{request:?}").contains("artifact_sha256"));
        let outcome = inspect(request);
        assert_eq!(outcome.binding.artifact_identity, None);
        assert_eq!(
            outcome.plan_preview,
            PlanPreviewAvailability::Unavailable(
                PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired
            )
        );
    }

    #[test]
    fn package_manager_output_requires_the_exact_synveil_owner() {
        assert!(package_owner_matches(
            b"synveil: /usr/bin/synveil-desktop\n",
            false
        ));
        assert!(!package_owner_matches(
            b"other-package: /usr/bin/synveil-desktop\n",
            false
        ));
        assert!(package_owner_matches(b"synveil", true));
        assert!(!package_owner_matches(b"untrusted-synveil", true));
    }

    #[test]
    fn no_verified_artifact_or_authoritative_builder_means_unavailable_preview() {
        let outcome = inspect(request(DeviceRole::Client, ConnectionMode::Manual));
        assert_eq!(
            outcome.plan_preview,
            PlanPreviewAvailability::Unavailable(
                PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired
            )
        );
        assert!(!matches!(
            outcome
                .binding
                .channels
                .iter()
                .find(|channel| channel.active)
                .map(|channel| channel.artifact_evidence),
            Some(ArtifactEvidenceClass::VerifiedReleaseArtifact)
        ));
    }

    #[test]
    fn unsupported_scope_is_never_inferred_from_process_privilege() {
        let outcome = inspect(request(DeviceRole::Client, ConnectionMode::Manual));
        assert!(outcome
            .binding
            .channels
            .iter()
            .all(|channel| { channel.target_scope != Some(TargetScope::System) }));
        assert_ne!(outcome.binding.target_scope, Some(TargetScope::System));
    }

    #[test]
    fn user_cannot_manufacture_artifact_identity_or_system_scope_into_a_binding() {
        let request = request(DeviceRole::Client, ConnectionMode::Manual);
        let mut outcome = inspect(request);
        outcome.binding.artifact_identity = Some("user-supplied-fake-id".to_owned());
        assert!(!outcome.binding.matches_request(request));

        let mut outcome = inspect(request);
        outcome.binding.target_scope = Some(TargetScope::System);
        assert!(!outcome.binding.matches_request(request));
    }

    #[test]
    fn qml_summaries_do_not_include_raw_paths_or_command_output() {
        let outcome = inspect(request(DeviceRole::Client, ConnectionMode::Manual));
        let text = format!(
            "{}\n{}\n{}\n{}",
            outcome.platform_summary(),
            outcome.channel_summary(),
            outcome.finding_summary(),
            outcome.artifact_summary()
        );
        assert!(!text.contains("/home/"));
        assert!(!text.contains("/usr/"));
        assert!(!text.contains("APPIMAGE="));
    }

    #[test]
    fn tailscale_is_typed_unavailable_and_no_enrollment_is_called() {
        let mut request = request(DeviceRole::Client, ConnectionMode::Tailscale);
        let outcome = inspect(request);
        assert!(outcome
            .findings
            .iter()
            .any(|finding| finding.code == CapabilityFindingCode::TailscaleUnavailable));
        request.generation = request.generation.wrapping_add(1);
        assert_ne!(outcome.binding.selection.generation, request.generation);
    }

    #[test]
    fn windows_setup_and_portable_scope_types_are_distinct() {
        assert_eq!(
            InstallationChannel::WindowsPerUserSetup.label(),
            "Windows per-user Setup"
        );
        assert_eq!(
            InstallationChannel::WindowsPortable.label(),
            "Windows portable distribution"
        );
        assert_ne!(TargetScope::CurrentUser, TargetScope::PortableUser);
    }

    #[test]
    fn active_portable_markers_are_unverified_not_publisher_verified() {
        let class = ArtifactEvidenceClass::LocallySuppliedUnverified;
        assert_ne!(class, ArtifactEvidenceClass::VerifiedReleaseArtifact);
        let unavailable = PlanPreviewUnavailableReason::VerifiedArtifactEvidenceRequired.label();
        assert!(unavailable.contains("independently authenticated artifact evidence"));
        assert!(unavailable.contains("production publisher trust root"));
        assert!(unavailable.contains("approved channel effect catalog"));
    }

    #[test]
    fn preflight_request_binds_role_connection_channel_and_generation() {
        let mut request = request(DeviceRole::Host, ConnectionMode::LocalOnly);
        let expected_scope = match PlatformCode::current() {
            PlatformCode::Linux => {
                request.selected_channel = Some(InstallationChannel::LinuxDeb);
                Some(TargetScope::NativePackage)
            }
            PlatformCode::Windows => {
                request.selected_channel = Some(InstallationChannel::WindowsPerUserSetup);
                Some(TargetScope::CurrentUser)
            }
            PlatformCode::MacOs | PlatformCode::Other => {
                request.selected_channel = Some(InstallationChannel::ExistingInstallation);
                None
            }
        };
        let outcome = inspect(request);
        assert_eq!(outcome.binding.selection.role, DeviceRole::Host);
        assert_eq!(
            outcome.binding.selection.connection_mode,
            ConnectionMode::LocalOnly
        );
        assert_eq!(
            outcome.binding.selection.selected_channel,
            request.selected_channel
        );
        assert_eq!(outcome.binding.selection.generation, 7);
        assert_eq!(outcome.binding.target_scope, expected_scope);
        assert_eq!(outcome.binding.platform.platform, PlatformCode::current());
        assert!(!outcome.binding.platform.architecture.is_empty());
        assert!(!outcome.binding.channels.is_empty());
        assert_eq!(outcome.binding.artifact_identity, None);
        assert!(outcome.binding.matches_request(request));
    }
}
