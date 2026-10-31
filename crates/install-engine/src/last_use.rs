//! Read-only last-use byte verification over crate-owned trust evidence.
//!
//! The release authentication implementation remains in the existing release
//! security tooling. This module accepts expected evidence only through a
//! crate-private handoff; it does not authenticate a publisher or make a path
//! trustworthy by itself. A returned handle proves byte integrity at the time
//! of hashing, not authorization or identity at a later consumer boundary.

#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;
use std::path::PathBuf;

pub const LAST_USE_VERIFICATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_SUPPORTED_ARTIFACT_SIZE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[cfg(target_os = "linux")]
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LastUseVerificationBlocker {
    MissingArtifactBytes,
    TrustedExpectedDigestUnavailable,
    AuthenticatedManifestUnavailable,
    ApprovedTrustPolicyUnavailable,
    ExpectedSizeUnavailable,
    UnsupportedArtifactSize,
    InvalidExpectedEvidence,
    UnsafePath,
    UnsafePathAncestry,
    UnsupportedFileType,
    OwnershipOrAccessRejected,
    ExactSizeMismatch,
    DigestMismatch,
    FileIdentityChanged,
    ConcurrentModification,
    PlatformFileIdentityUnavailable,
    UseTimeIdentityNotGuaranteed,
    StaleVerificationContext,
    IoUnavailable,
}

impl LastUseVerificationBlocker {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingArtifactBytes => "missing_artifact_bytes",
            Self::TrustedExpectedDigestUnavailable => "trusted_expected_digest_unavailable",
            Self::AuthenticatedManifestUnavailable => "authenticated_manifest_unavailable",
            Self::ApprovedTrustPolicyUnavailable => "approved_trust_policy_unavailable",
            Self::ExpectedSizeUnavailable => "expected_size_unavailable",
            Self::UnsupportedArtifactSize => "unsupported_artifact_size",
            Self::InvalidExpectedEvidence => "invalid_expected_evidence",
            Self::UnsafePath => "unsafe_path",
            Self::UnsafePathAncestry => "unsafe_path_ancestry",
            Self::UnsupportedFileType => "unsupported_file_type",
            Self::OwnershipOrAccessRejected => "ownership_or_access_rejected",
            Self::ExactSizeMismatch => "exact_size_mismatch",
            Self::DigestMismatch => "digest_mismatch",
            Self::FileIdentityChanged => "file_identity_changed",
            Self::ConcurrentModification => "concurrent_modification",
            Self::PlatformFileIdentityUnavailable => "platform_file_identity_unavailable",
            Self::UseTimeIdentityNotGuaranteed => "use_time_identity_not_guaranteed",
            Self::StaleVerificationContext => "stale_verification_context",
            Self::IoUnavailable => "io_unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteIntegrityStatus {
    Verified,
    Unavailable,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublisherTrustStatus {
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UseTimeIdentityStatus {
    NotGuaranteed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LastUseVerificationStatus {
    Unavailable,
    ByteIntegrityVerified,
    Blocked,
}

/// Crate-private values handed off by a future authenticated release-source
/// integration. This bundle has no public constructor and is not serializable.
#[allow(dead_code)]
pub(crate) struct AuthenticatedManifestArtifact {
    pub(crate) artifact_id: String,
    pub(crate) artifact_type: String,
    pub(crate) product_version: String,
    pub(crate) source_commit: String,
    pub(crate) expected_sha256: Option<String>,
    pub(crate) expected_size_bytes: Option<u64>,
    pub(crate) manifest_identity: String,
    pub(crate) manifest_generation: Option<u64>,
    pub(crate) trust_policy_identity: String,
    pub(crate) source_provenance_reference: String,
}

/// Stable artifact metadata. Fields are private so ordinary caller data cannot
/// be promoted into trusted expected evidence or serialized as a credential.
pub struct ArtifactIdentity {
    artifact_id: String,
    artifact_type: String,
    product_version: String,
    source_commit: String,
    expected_sha256: Option<[u8; 32]>,
    expected_size_bytes: Option<u64>,
    manifest_identity: String,
    manifest_generation: Option<u64>,
    trust_policy_identity: String,
    source_provenance_reference: String,
}

impl ArtifactIdentity {
    /// Crate-private trust handoff for an authenticated release source. No
    /// current production module calls this; P036's approved runtime keyring
    /// and a Rust trust bridge remain unavailable.
    #[allow(dead_code)]
    pub(crate) fn from_authenticated_manifest(
        evidence: AuthenticatedManifestArtifact,
    ) -> Result<Self, LastUseVerificationBlocker> {
        let expected_sha256 = evidence
            .expected_sha256
            .as_deref()
            .map(parse_digest)
            .transpose()?;
        if evidence.artifact_id.is_empty()
            || evidence.artifact_id.len() > 128
            || evidence.artifact_type.is_empty()
            || evidence.artifact_type.len() > 64
            || evidence.product_version.is_empty()
            || evidence.product_version.len() > 64
            || evidence.source_commit.len() != 40
            || !evidence
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || evidence.manifest_identity.len() != 64
            || parse_digest(&evidence.manifest_identity).is_err()
            || evidence.trust_policy_identity.is_empty()
            || evidence.trust_policy_identity.len() > 128
            || evidence.source_provenance_reference.is_empty()
            || evidence.source_provenance_reference.len() > 256
        {
            return Err(LastUseVerificationBlocker::InvalidExpectedEvidence);
        }
        Ok(Self {
            artifact_id: evidence.artifact_id,
            artifact_type: evidence.artifact_type,
            product_version: evidence.product_version,
            source_commit: evidence.source_commit,
            expected_sha256,
            expected_size_bytes: evidence.expected_size_bytes,
            manifest_identity: evidence.manifest_identity,
            manifest_generation: evidence.manifest_generation,
            trust_policy_identity: evidence.trust_policy_identity,
            source_provenance_reference: evidence.source_provenance_reference,
        })
    }
}

/// Values from the exact current review snapshot to which byte evidence is
/// bound. This type intentionally does not implement Serde.
pub struct ArtifactVerificationContext {
    platform: String,
    architecture: String,
    installation_channel: String,
    plan_fingerprint_sha256: String,
    manifest_generation: Option<u64>,
    capability_generation: u64,
    installation_state_generation: u64,
    effect_catalog_version: Option<u32>,
    security_policy_version: Option<u32>,
}

impl ArtifactVerificationContext {
    /// Bind byte evidence to the exact engine review and all generations that
    /// can invalidate its adapter/use-time classification.
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub(crate) fn from_current_review(
        platform: String,
        architecture: String,
        installation_channel: String,
        plan_fingerprint_sha256: String,
        manifest_generation: Option<u64>,
        capability_generation: u64,
        installation_state_generation: u64,
        effect_catalog_version: Option<u32>,
        security_policy_version: Option<u32>,
    ) -> Result<Self, LastUseVerificationBlocker> {
        if platform.is_empty()
            || architecture.is_empty()
            || installation_channel.is_empty()
            || plan_fingerprint_sha256.len() != 64
            || capability_generation == 0
        {
            return Err(LastUseVerificationBlocker::StaleVerificationContext);
        }
        Ok(Self {
            platform,
            architecture,
            installation_channel,
            plan_fingerprint_sha256,
            manifest_generation,
            capability_generation,
            installation_state_generation,
            effect_catalog_version,
            security_policy_version,
        })
    }
}

/// A request can only be created by an in-crate trust integration. The current
/// v0.3 release verifier is Python-backed and has no authenticated Rust handoff,
/// so production code cannot create this request yet.
pub struct LastUseVerificationRequest {
    identity: ArtifactIdentity,
    context: ArtifactVerificationContext,
    artifact_path: PathBuf,
}

impl LastUseVerificationRequest {
    /// Only a crate-owned authenticated-source integration may bind the raw
    /// staging path to expected evidence. The path remains private.
    #[allow(dead_code)]
    pub(crate) fn from_authenticated_source(
        identity: ArtifactIdentity,
        context: ArtifactVerificationContext,
        artifact_path: PathBuf,
    ) -> Self {
        Self {
            identity,
            context,
            artifact_path,
        }
    }
}

/// Non-serializable open-file evidence. The file path, file descriptor, and
/// private staging location are never exposed through this API.
pub struct VerifiedArtifactHandle {
    #[cfg(target_os = "linux")]
    file: File,
    identity: ArtifactIdentity,
    context: ArtifactVerificationContext,
    #[cfg(target_os = "linux")]
    artifact_path: PathBuf,
    #[cfg(target_os = "linux")]
    platform_identity: PlatformFileIdentity,
    #[cfg(target_os = "linux")]
    parent_identity: DirectoryIdentity,
    verified_digest: [u8; 32],
    verified_size_bytes: u64,
}

pub enum LastUseVerificationResult {
    ByteIntegrityVerified(Box<VerifiedArtifactHandle>),
    Blocked(LastUseVerificationBlocker),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LastUseVerificationSessionStatus {
    Idle,
    Verifying,
    ByteIntegrityVerified,
    Blocked,
    Invalidated,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LastUseVerificationTicket {
    generation: u64,
}

/// Synchronous consumers can ignore this type. It defines the async boundary
/// so duplicate work, cancellation, and stale completions cannot restore an
/// older result if a future UI source adds worker scheduling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LastUseVerificationStateMachine {
    generation: u64,
    status: LastUseVerificationSessionStatus,
}

impl LastUseVerificationStateMachine {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            status: LastUseVerificationSessionStatus::Idle,
        }
    }

    pub const fn status(&self) -> LastUseVerificationSessionStatus {
        self.status
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn begin(&mut self) -> Option<LastUseVerificationTicket> {
        if self.status == LastUseVerificationSessionStatus::Verifying
            || self.status == LastUseVerificationSessionStatus::Cancelled
        {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.status = LastUseVerificationSessionStatus::Verifying;
        Some(LastUseVerificationTicket {
            generation: self.generation,
        })
    }

    pub fn complete(
        &mut self,
        ticket: LastUseVerificationTicket,
        result: &LastUseVerificationResult,
    ) -> bool {
        if ticket.generation != self.generation
            || self.status != LastUseVerificationSessionStatus::Verifying
        {
            return false;
        }
        self.status = match result {
            LastUseVerificationResult::ByteIntegrityVerified(_) => {
                LastUseVerificationSessionStatus::ByteIntegrityVerified
            }
            LastUseVerificationResult::Blocked(_) => LastUseVerificationSessionStatus::Blocked,
        };
        true
    }

    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.status = LastUseVerificationSessionStatus::Invalidated;
    }

    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.status = LastUseVerificationSessionStatus::Cancelled;
    }

    pub fn restart(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.status = LastUseVerificationSessionStatus::Idle;
    }
}

impl Default for LastUseVerificationStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl LastUseVerificationResult {
    pub const fn blocker(&self) -> Option<LastUseVerificationBlocker> {
        match self {
            Self::ByteIntegrityVerified(_) => None,
            Self::Blocked(blocker) => Some(*blocker),
        }
    }

    pub const fn byte_integrity_status(&self) -> ByteIntegrityStatus {
        match self {
            Self::ByteIntegrityVerified(_) => ByteIntegrityStatus::Verified,
            Self::Blocked(LastUseVerificationBlocker::DigestMismatch) => {
                ByteIntegrityStatus::Mismatch
            }
            Self::Blocked(_) => ByteIntegrityStatus::Unavailable,
        }
    }

    pub const fn status(&self) -> LastUseVerificationStatus {
        match self {
            Self::ByteIntegrityVerified(_) => LastUseVerificationStatus::ByteIntegrityVerified,
            Self::Blocked(_) => LastUseVerificationStatus::Blocked,
        }
    }
}

impl VerifiedArtifactHandle {
    pub const fn schema_version(&self) -> u32 {
        LAST_USE_VERIFICATION_SCHEMA_VERSION
    }

    pub const fn byte_integrity_status(&self) -> ByteIntegrityStatus {
        ByteIntegrityStatus::Verified
    }

    pub const fn publisher_trust_status(&self) -> PublisherTrustStatus {
        PublisherTrustStatus::Unavailable
    }

    pub const fn use_time_identity_status(&self) -> UseTimeIdentityStatus {
        UseTimeIdentityStatus::NotGuaranteed
    }

    pub const fn verified_size_bytes(&self) -> u64 {
        self.verified_size_bytes
    }

    pub fn verified_sha256(&self) -> String {
        hex_digest(&self.verified_digest)
    }

    pub(crate) fn matches_current_plan(
        &self,
        request: &crate::InstallationRequest,
        plan: &crate::InstallationPlan,
        review: &crate::PlanReviewSnapshot,
        context: &crate::PlanReviewContext,
    ) -> bool {
        if !review.matches_current(request, plan, context) {
            return false;
        }
        let Some(crate::ArtifactEvidence::VerifiedReleaseArtifact { evidence }) = &plan.artifact
        else {
            return false;
        };
        let Some(expected_generation) = self.identity.manifest_generation else {
            return false;
        };
        self.identity.artifact_id == evidence.artifact_id
            && self.identity.artifact_type == evidence.artifact_type
            && self.identity.product_version == evidence.product_version
            && self.identity.source_commit == evidence.source_commit
            && self.identity.manifest_identity == evidence.manifest_sha256
            && self.identity.source_provenance_reference == evidence.source_provenance_reference
            && self.identity.trust_policy_identity == evidence.trust_policy_identity
            && self.identity.manifest_generation == context.manifest_generation
            && self.identity.expected_sha256.is_some_and(|digest| {
                evidence.artifact_sha256 == hex_digest(&digest) && self.verified_digest == digest
            })
            && self.identity.expected_size_bytes == Some(evidence.artifact_size_bytes)
            && self.identity.expected_size_bytes == Some(self.verified_size_bytes)
            && self.context.platform == context.platform
            && self.context.architecture == context.architecture
            && self.context.installation_channel == context.channel
            && self.context.plan_fingerprint_sha256 == review.fingerprint().as_hex()
            && self.context.manifest_generation == Some(expected_generation)
            && self.context.manifest_generation == context.manifest_generation
            && self.context.capability_generation == context.capability_generation
            && self.context.installation_state_generation == context.installation_state_generation
            && self.context.effect_catalog_version == context.effect_catalog_version
            && self.context.security_policy_version == context.security_policy_version
            && self.file_identity_is_current()
    }

    pub(crate) fn context_matches(
        &self,
        request: &crate::InstallationRequest,
        plan: &crate::InstallationPlan,
        review: &crate::PlanReviewSnapshot,
        context: &crate::PlanReviewContext,
    ) -> bool {
        self.matches_current_plan(request, plan, review, context)
    }

    fn file_identity_is_current(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let Ok(metadata) = self.file.metadata() else {
                return false;
            };
            let Some(identity) = PlatformFileIdentity::from_metadata(&metadata) else {
                return false;
            };
            if identity != self.platform_identity {
                return false;
            }
            let Ok(opened) = open_artifact_no_follow(&self.artifact_path) else {
                return false;
            };
            let Ok(path_metadata) = opened.file.metadata() else {
                return false;
            };
            let Some(path_identity) = PlatformFileIdentity::from_metadata(&path_metadata) else {
                return false;
            };
            identity.same_object(&path_identity) && opened.parent_identity == self.parent_identity
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

/// Hashes bytes from the opened object with bounded memory. The returned handle
/// keeps that same Linux file description alive, but no current adapter can
/// transfer and consume it as the exact future installation input.
pub fn verify_last_use(request: LastUseVerificationRequest) -> LastUseVerificationResult {
    let Some(expected_digest) = request.identity.expected_sha256 else {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::TrustedExpectedDigestUnavailable,
        );
    };
    let Some(expected_size) = request.identity.expected_size_bytes else {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::ExpectedSizeUnavailable,
        );
    };
    if expected_size == 0 || expected_size > MAX_SUPPORTED_ARTIFACT_SIZE_BYTES {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::UnsupportedArtifactSize,
        );
    }
    if request.identity.manifest_identity.is_empty()
        || request.identity.manifest_generation.is_none()
        || request.identity.manifest_identity.len() != 64
        || request.identity.source_provenance_reference.is_empty()
    {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::AuthenticatedManifestUnavailable,
        );
    }
    if request.identity.trust_policy_identity.is_empty() {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::ApprovedTrustPolicyUnavailable,
        );
    }
    if request.context.plan_fingerprint_sha256.len() != 64
        || request.context.capability_generation == 0
        || request.context.manifest_generation != request.identity.manifest_generation
    {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::StaleVerificationContext,
        );
    }

    #[cfg(target_os = "linux")]
    {
        verify_linux(request, expected_digest, expected_size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (&request.artifact_path, expected_digest, expected_size);
        LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::PlatformFileIdentityUnavailable,
        )
    }
}

#[cfg(target_os = "linux")]
fn verify_linux(
    request: LastUseVerificationRequest,
    expected_digest: [u8; 32],
    expected_size: u64,
) -> LastUseVerificationResult {
    use std::io;

    let opened = match open_artifact_no_follow(&request.artifact_path) {
        Ok(opened) => opened,
        Err(blocker) => return LastUseVerificationResult::Blocked(blocker),
    };
    let before = match opened.file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::UnsupportedFileType,
            );
        }
        Err(_) => {
            return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::IoUnavailable);
        }
    };
    let Some(before_identity) = PlatformFileIdentity::from_metadata(&before) else {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::PlatformFileIdentityUnavailable,
        );
    };
    if !file_access_is_allowed(&before) {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::OwnershipOrAccessRejected,
        );
    }
    if before.len() != expected_size {
        return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExactSizeMismatch);
    }

    let mut file = opened.file;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; STREAM_CHUNK_BYTES];
    loop {
        let remaining_with_probe = expected_size.saturating_sub(total).saturating_add(1);
        let chunk_limit = usize::try_from(remaining_with_probe.min(STREAM_CHUNK_BYTES as u64))
            .unwrap_or(STREAM_CHUNK_BYTES);
        let count = match file.read(&mut buffer[..chunk_limit]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                return LastUseVerificationResult::Blocked(
                    LastUseVerificationBlocker::IoUnavailable,
                );
            }
        };
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > expected_size {
            return LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::ExactSizeMismatch,
            );
        }
        digest.update(&buffer[..count]);
    }
    if total != expected_size {
        return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExactSizeMismatch);
    }
    let actual_digest: [u8; 32] = digest.finalize().into();
    if actual_digest != expected_digest {
        return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::DigestMismatch);
    }

    let after = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::IoUnavailable);
        }
    };
    let Some(after_identity) = PlatformFileIdentity::from_metadata(&after) else {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::PlatformFileIdentityUnavailable,
        );
    };
    if before_identity != after_identity {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::ConcurrentModification,
        );
    }

    let current_path = match open_artifact_no_follow(&request.artifact_path) {
        Ok(opened) => opened,
        Err(_) => {
            return LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::FileIdentityChanged,
            );
        }
    };
    let current_path_metadata = match current_path.file.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::IoUnavailable);
        }
    };
    let Some(current_path_identity) = PlatformFileIdentity::from_metadata(&current_path_metadata)
    else {
        return LastUseVerificationResult::Blocked(
            LastUseVerificationBlocker::PlatformFileIdentityUnavailable,
        );
    };
    if !before_identity.same_object(&current_path_identity)
        || opened.parent_identity != current_path.parent_identity
    {
        return LastUseVerificationResult::Blocked(LastUseVerificationBlocker::FileIdentityChanged);
    }

    LastUseVerificationResult::ByteIntegrityVerified(Box::new(VerifiedArtifactHandle {
        file,
        identity: request.identity,
        context: request.context,
        artifact_path: request.artifact_path,
        platform_identity: before_identity,
        parent_identity: opened.parent_identity,
        verified_digest: actual_digest,
        verified_size_bytes: total,
    }))
}

#[cfg(target_os = "linux")]
struct OpenedArtifact {
    file: File,
    parent_identity: DirectoryIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Eq, PartialEq)]
struct DirectoryIdentity {
    device: u64,
    inode: u64,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Eq, PartialEq)]
struct PlatformFileIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    group: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(target_os = "linux")]
impl PlatformFileIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        metadata.is_file().then_some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            owner: metadata.uid(),
            group: metadata.gid(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }

    fn same_object(&self, other: &Self) -> bool {
        self.device == other.device && self.inode == other.inode && self.mode == other.mode
    }
}

#[cfg(target_os = "linux")]
fn file_access_is_allowed(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = metadata.uid();
    let current_uid = nix::unistd::geteuid().as_raw();
    (uid == 0 || uid == current_uid) && metadata.mode() & 0o022 == 0
}

#[cfg(target_os = "linux")]
fn open_artifact_no_follow(
    path: &std::path::Path,
) -> Result<OpenedArtifact, LastUseVerificationBlocker> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::{ffi::OsStr, os::fd::OwnedFd};

    if !path.is_absolute() {
        return Err(LastUseVerificationBlocker::UnsafePath);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(component) => components.push(component.to_owned()),
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => {
                return Err(LastUseVerificationBlocker::UnsafePath);
            }
        }
    }
    if components.len() < 2 {
        return Err(LastUseVerificationBlocker::UnsafePath);
    }

    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open("/", directory_flags, Mode::empty())
        .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?;
    check_directory_access(&directory)?;
    for component in &components[..components.len() - 1] {
        let name: &OsStr = component.as_os_str();
        directory = openat(&directory, name, directory_flags, Mode::empty())
            .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?;
        check_directory_access(&directory)?;
    }
    let parent_identity = directory_identity(&directory)?;
    let leaf = components
        .last()
        .expect("component count checked above")
        .as_os_str();
    let file_flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let opened: OwnedFd = openat(&directory, leaf, file_flags, Mode::empty())
        .map_err(|_| LastUseVerificationBlocker::MissingArtifactBytes)?;
    let file: File = opened.into();
    let metadata = file
        .metadata()
        .map_err(|_| LastUseVerificationBlocker::IoUnavailable)?;
    if !metadata.is_file() {
        return Err(LastUseVerificationBlocker::UnsupportedFileType);
    }
    if !file_access_is_allowed(&metadata) {
        return Err(LastUseVerificationBlocker::OwnershipOrAccessRejected);
    }
    Ok(OpenedArtifact {
        file,
        parent_identity,
    })
}

#[cfg(target_os = "linux")]
fn check_directory_access(
    directory: &impl std::os::fd::AsFd,
) -> Result<(), LastUseVerificationBlocker> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::File::from(
        directory
            .as_fd()
            .try_clone_to_owned()
            .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?,
    )
    .metadata()
    .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?;
    if !metadata.is_dir() {
        return Err(LastUseVerificationBlocker::UnsafePathAncestry);
    }
    let mode = metadata.mode();
    let owner = metadata.uid();
    let group_writable = mode & 0o022 != 0;
    let root_sticky = owner == 0 && mode & 0o1000 != 0;
    let current_uid = nix::unistd::geteuid().as_raw();
    if (owner != 0 && owner != current_uid) || group_writable && !root_sticky {
        return Err(LastUseVerificationBlocker::UnsafePathAncestry);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn directory_identity(
    directory: &impl std::os::fd::AsFd,
) -> Result<DirectoryIdentity, LastUseVerificationBlocker> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::File::from(
        directory
            .as_fd()
            .try_clone_to_owned()
            .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?,
    )
    .metadata()
    .map_err(|_| LastUseVerificationBlocker::UnsafePathAncestry)?;
    Ok(DirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn parse_digest(value: &str) -> Result<[u8; 32], LastUseVerificationBlocker> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return Err(LastUseVerificationBlocker::InvalidExpectedEvidence);
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        let start = index * 2;
        let high = digest_nibble(bytes[start])?;
        let low = digest_nibble(bytes[start + 1])?;
        *byte = (high << 4) | low;
    }
    Ok(digest)
}

fn digest_nibble(byte: u8) -> Result<u8, LastUseVerificationBlocker> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(LastUseVerificationBlocker::InvalidExpectedEvidence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{fs, path::Path};
    use tempfile::tempdir;

    fn digest(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }

    fn request(
        path: &Path,
        bytes: &[u8],
        expected_size: Option<u64>,
    ) -> LastUseVerificationRequest {
        LastUseVerificationRequest {
            identity: ArtifactIdentity {
                artifact_id: "linux-appimage-x86_64".into(),
                artifact_type: "appimage".into(),
                product_version: "0.3.0".into(),
                source_commit: "a".repeat(40),
                expected_sha256: Some(digest(bytes)),
                expected_size_bytes: expected_size,
                manifest_identity: "b".repeat(64),
                manifest_generation: Some(7),
                trust_policy_identity: "production-policy-unavailable-fixture".into(),
                source_provenance_reference: "release-artifact:linux-appimage-x86_64".into(),
            },
            context: ArtifactVerificationContext {
                platform: "ubuntu-24.04".into(),
                architecture: "x86_64".into(),
                installation_channel: "linux_appimage".into(),
                plan_fingerprint_sha256: "c".repeat(64),
                manifest_generation: Some(7),
                capability_generation: 3,
                installation_state_generation: 4,
                effect_catalog_version: Some(1),
                security_policy_version: Some(1),
            },
            artifact_path: path.to_path_buf(),
        }
    }

    #[cfg(target_os = "linux")]
    fn write_fixture(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("artifact.bin");
        fs::write(&path, bytes).expect("fixture bytes");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("private file");
        (directory, path)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exact_size_and_digest_return_non_serializable_open_handle() {
        let bytes = b"synthetic artifact bytes";
        let (_directory, path) = write_fixture(bytes);
        let result = verify_last_use(request(&path, bytes, Some(bytes.len() as u64)));
        let LastUseVerificationResult::ByteIntegrityVerified(handle) = result else {
            panic!("expected byte-integrity handle");
        };
        assert_eq!(
            handle.byte_integrity_status(),
            ByteIntegrityStatus::Verified
        );
        assert_eq!(
            handle.publisher_trust_status(),
            PublisherTrustStatus::Unavailable
        );
        assert_eq!(
            handle.use_time_identity_status(),
            UseTimeIdentityStatus::NotGuaranteed
        );
        assert_eq!(handle.verified_size_bytes(), bytes.len() as u64);
        assert_eq!(handle.verified_sha256(), hex_digest(&digest(bytes)));
        assert_eq!(
            handle.schema_version(),
            LAST_USE_VERIFICATION_SCHEMA_VERSION
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_digest_size_mismatch_truncation_append_and_same_size_mutation_block() {
        let bytes = b"payload";
        let (_directory, path) = write_fixture(bytes);

        let mut wrong_expected_digest = request(&path, bytes, Some(bytes.len() as u64));
        wrong_expected_digest.identity.expected_sha256 = Some(digest(b"different"));
        assert!(matches!(
            verify_last_use(wrong_expected_digest),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::DigestMismatch)
        ));

        let mut missing_digest = request(&path, bytes, Some(bytes.len() as u64));
        missing_digest.identity.expected_sha256 = None;
        assert!(matches!(
            verify_last_use(missing_digest),
            LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::TrustedExpectedDigestUnavailable
            )
        ));
        let mut missing_size = request(&path, bytes, Some(bytes.len() as u64));
        missing_size.identity.expected_size_bytes = None;
        assert!(matches!(
            verify_last_use(missing_size),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExpectedSizeUnavailable)
        ));
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some((bytes.len() + 1) as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExactSizeMismatch)
        ));

        fs::write(&path, b"pay").expect("truncate fixture");
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExactSizeMismatch)
        ));
        fs::write(&path, b"payload!").expect("append fixture");
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::ExactSizeMismatch)
        ));
        fs::write(&path, b"payloAd").expect("same-size mutation");
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::DigestMismatch)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsafe_symlink_wrong_file_type_and_staging_replacement_block() {
        use std::os::unix::fs::symlink;
        let bytes = b"artifact";
        let (directory, path) = write_fixture(bytes);
        let outside = directory.path().join("outside.bin");
        fs::write(&outside, bytes).expect("outside fixture");
        fs::remove_file(&path).expect("remove original");
        symlink(&outside, &path).expect("symlink fixture");
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::MissingArtifactBytes)
                | LastUseVerificationResult::Blocked(
                    LastUseVerificationBlocker::UnsafePathAncestry
                )
        ));

        let fifo = directory.path().join("named-pipe");
        nix::unistd::mkfifo(
            &fifo,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )
        .expect("fifo fixture");
        assert!(matches!(
            verify_last_use(request(&fifo, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::UnsupportedFileType)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_artifact_and_symlinked_parent_are_rejected() {
        use std::os::unix::fs::symlink;

        let bytes = b"artifact";
        let directory = tempdir().expect("temporary directory");
        let missing = directory.path().join("missing.bin");
        assert!(matches!(
            verify_last_use(request(&missing, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::MissingArtifactBytes)
        ));

        let actual_directory = directory.path().join("actual");
        fs::create_dir(&actual_directory).expect("actual directory");
        let actual_path = actual_directory.join("artifact.bin");
        fs::write(&actual_path, bytes).expect("fixture bytes");
        let linked_directory = directory.path().join("linked");
        symlink(&actual_directory, &linked_directory).expect("symlink parent");
        let linked_path = linked_directory.join("artifact.bin");
        assert!(matches!(
            verify_last_use(request(&linked_path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::UnsafePathAncestry)
        ));
    }

    #[test]
    fn absent_manifest_policy_stale_context_and_platform_identity_are_blockers() {
        let bytes = b"fixture";
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("artifact.bin");
        fs::write(&path, bytes).expect("fixture bytes");

        let mut missing_manifest = request(&path, bytes, Some(bytes.len() as u64));
        missing_manifest.identity.manifest_generation = None;
        assert!(matches!(
            verify_last_use(missing_manifest),
            LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::AuthenticatedManifestUnavailable
            )
        ));
        let mut missing_policy = request(&path, bytes, Some(bytes.len() as u64));
        missing_policy.identity.trust_policy_identity.clear();
        assert!(matches!(
            verify_last_use(missing_policy),
            LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::ApprovedTrustPolicyUnavailable
            )
        ));
        let mut stale = request(&path, bytes, Some(bytes.len() as u64));
        stale.context.manifest_generation = Some(8);
        assert!(matches!(
            verify_last_use(stale),
            LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::StaleVerificationContext
            )
        ));
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64))),
            LastUseVerificationResult::Blocked(
                LastUseVerificationBlocker::PlatformFileIdentityUnavailable
            )
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unsafe_relative_path_and_unexpected_size_limit_fail_closed() {
        let bytes = b"fixture";
        let mut relative = request(
            Path::new("relative/artifact.bin"),
            bytes,
            Some(bytes.len() as u64),
        );
        relative.artifact_path = PathBuf::from("relative/artifact.bin");
        assert!(matches!(
            verify_last_use(relative),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::UnsafePath)
        ));
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("artifact.bin");
        fs::write(&path, bytes).expect("fixture bytes");
        assert!(matches!(
            verify_last_use(request(
                &path,
                bytes,
                Some(MAX_SUPPORTED_ARTIFACT_SIZE_BYTES + 1)
            )),
            LastUseVerificationResult::Blocked(LastUseVerificationBlocker::UnsupportedArtifactSize)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn artifact_path_replacement_after_verification_invalidates_the_open_handle_binding() {
        let bytes = b"replacement fixture";
        let (directory, path) = write_fixture(bytes);
        let LastUseVerificationResult::ByteIntegrityVerified(handle) =
            verify_last_use(request(&path, bytes, Some(bytes.len() as u64)))
        else {
            panic!("expected byte-integrity handle");
        };
        let previous = directory.path().join("previous.bin");
        fs::rename(&path, previous).expect("rename original fixture");
        fs::write(&path, bytes).expect("replace staged name");
        assert!(!handle.file_identity_is_current());
    }

    #[test]
    fn byte_verification_has_no_publisher_assertion_or_effectful_boundary() {
        let source = include_str!("last_use.rs");
        let publisher_assertion_name = ["publisher", "_verified"].concat();
        let companion_path_name = ["checksum", "_path"].concat();
        let process_namespace = ["std", "::", "process"].concat();
        let journal_name = ["Installation", "Journal"].concat();
        let effect_method = ["apply_", "effect("].concat();
        assert!(!source.contains(&publisher_assertion_name));
        assert!(!source.contains(&companion_path_name));
        assert!(!source.contains(&process_namespace));
        assert!(!source.contains(&journal_name));
        assert!(!source.contains(&effect_method));
    }

    #[test]
    fn duplicate_cancelled_and_stale_async_verification_results_are_ignored() {
        let mut state = LastUseVerificationStateMachine::new();
        let first = state.begin().expect("begin verification");
        assert_eq!(state.status(), LastUseVerificationSessionStatus::Verifying);
        assert!(state.begin().is_none(), "duplicate request is rejected");
        state.cancel();
        assert_eq!(state.status(), LastUseVerificationSessionStatus::Cancelled);
        let stale = LastUseVerificationResult::Blocked(LastUseVerificationBlocker::DigestMismatch);
        assert!(!state.complete(first, &stale));
        assert!(
            state.begin().is_none(),
            "cancelled session cannot restart implicitly"
        );

        state.restart();
        let old = state.begin().expect("new generation");
        state.invalidate();
        let current = state.begin().expect("revalidation generation");
        assert!(!state.complete(old, &stale));
        assert!(state.complete(current, &stale));
        assert_eq!(state.status(), LastUseVerificationSessionStatus::Blocked);
    }
}
