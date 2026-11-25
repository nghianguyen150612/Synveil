//! Provisional, reference-only audit evidence types and verifier.
//!
//! Callers must construct records from trusted authentication and authority
//! state. This module does not authorize operations, persist evidence, or make
//! a database administrator unable to rewrite the history.

use std::collections::HashSet;

use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: u16 = 1;
pub const MAX_PAGE_SIZE: usize = 100;
pub const MAX_VERIFIED_RECORDS: u64 = 10_000;
pub const ZERO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    Authentication,
    Authorization,
    OwnershipChange,
    RoleAssignment,
    Delegation,
    ApprovalIssued,
    ApprovalConsumed,
    ApprovalRejected,
    GrantRevocation,
    CredentialRevocation,
    SessionRevocation,
    DeviceRevocation,
    PolicyRevision,
    PrivilegedOperationRejected,
}

impl EventType {
    fn tag(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Authorization => "authorization",
            Self::OwnershipChange => "ownership-change",
            Self::RoleAssignment => "role-assignment",
            Self::Delegation => "delegation",
            Self::ApprovalIssued => "approval-issued",
            Self::ApprovalConsumed => "approval-consumed",
            Self::ApprovalRejected => "approval-rejected",
            Self::GrantRevocation => "grant-revocation",
            Self::CredentialRevocation => "credential-revocation",
            Self::SessionRevocation => "session-revocation",
            Self::DeviceRevocation => "device-revocation",
            Self::PolicyRevision => "policy-revision",
            Self::PrivilegedOperationRejected => "privileged-operation-rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Authenticate,
    Read,
    Write,
    Delete,
    ChangeOwner,
    AssignRole,
    Delegate,
    Approve,
    Revoke,
    ChangePolicy,
    Privileged,
}

impl Operation {
    fn tag(self) -> &'static str {
        match self {
            Self::Authenticate => "authenticate",
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
            Self::ChangeOwner => "change-owner",
            Self::AssignRole => "assign-role",
            Self::Delegate => "delegate",
            Self::Approve => "approve",
            Self::Revoke => "revoke",
            Self::ChangePolicy => "change-policy",
            Self::Privileged => "privileged",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Allowed,
    Denied,
    Committed,
    Recovered,
    Rejected,
}

impl Outcome {
    fn tag(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Committed => "committed",
            Self::Recovered => "recovered",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenialCategory {
    Unauthenticated,
    Unauthorized,
    ForeignOrUnknownResource,
    Revoked,
    Expired,
    StalePolicy,
    ApprovalRequired,
    ApprovalInvalid,
    Unsupported,
    AuditUnavailable,
}

impl DenialCategory {
    fn tag(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::Unauthorized => "unauthorized",
            Self::ForeignOrUnknownResource => "foreign-or-unknown-resource",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::StalePolicy => "stale-policy",
            Self::ApprovalRequired => "approval-required",
            Self::ApprovalInvalid => "approval-invalid",
            Self::Unsupported => "unsupported",
            Self::AuditUnavailable => "audit-unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampProvenance {
    DatabaseClock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrincipalKind {
    Browser,
    Device,
    Service,
    Unauthenticated,
    Unknown,
}

impl PrincipalKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Device => "device",
            Self::Service => "service",
            Self::Unauthenticated => "unauthenticated",
            Self::Unknown => "unknown",
        }
    }
}

impl TimestampProvenance {
    fn tag(self) -> &'static str {
        match self {
            Self::DatabaseClock => "database-clock",
        }
    }
}

/// Bounded, secret-free event context. Identifiers are stable trusted references,
/// never credentials, arbitrary request data, or caller-asserted policy facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    pub schema_version: u16,
    pub event_id: String,
    pub event_type: EventType,
    pub actor_id: Option<String>,
    pub principal_kind: PrincipalKind,
    pub device_id: Option<String>,
    pub owner_id: String,
    pub resource_id: Option<String>,
    pub operation: Operation,
    pub policy_revision: Option<u64>,
    pub outcome: Outcome,
    pub denial: Option<DenialCategory>,
    /// Monotonic within one owner-scoped stream. Allocated in the authority txn.
    pub durable_sequence: u64,
    /// Stable global ordering reference into P027's existing audit sequence.
    pub authority_order: u64,
    pub operation_id: String,
    pub correlation_id: String,
    /// Unix nanoseconds read from the database clock inside the authority txn.
    pub observed_at_unix_ns: i64,
    pub timestamp_provenance: TimestampProvenance,
    pub related_reference: Option<String>,
    pub previous_digest: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    UnsupportedVersion,
    InvalidRecord,
    ScopeMismatch,
    IncompleteEvidence,
    DuplicateRecord,
    Ordering,
    Integrity,
    InvalidLimit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub owner_id: String,
    pub record_count: u64,
    pub head_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditPage {
    pub records: Vec<AuditRecord>,
    pub next_sequence: Option<u64>,
    pub verified_checkpoint: Checkpoint,
}

fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn append_field(output: &mut String, value: &str) {
    use std::fmt::Write;
    write!(output, "{}:", value.len()).expect("writing to String cannot fail");
    output.push_str(value);
}

impl AuditRecord {
    pub fn validate(&self) -> Result<(), VerifyError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(VerifyError::UnsupportedVersion);
        }
        if !valid_reference(&self.event_id)
            || !valid_reference(&self.owner_id)
            || !valid_reference(&self.operation_id)
            || !valid_reference(&self.correlation_id)
            || self
                .actor_id
                .as_deref()
                .is_some_and(|v| !valid_reference(v))
            || self
                .device_id
                .as_deref()
                .is_some_and(|v| !valid_reference(v))
            || (self.principal_kind == PrincipalKind::Device) != self.device_id.is_some()
            || (self.principal_kind == PrincipalKind::Unauthenticated) != self.actor_id.is_none()
            || self
                .resource_id
                .as_deref()
                .is_some_and(|v| !valid_reference(v))
            || self
                .related_reference
                .as_deref()
                .is_some_and(|v| !valid_reference(v))
            || self.durable_sequence == 0
            || self.durable_sequence > i64::MAX as u64
            || self.authority_order == 0
            || self.authority_order > i64::MAX as u64
            || self
                .policy_revision
                .is_some_and(|value| value > i64::MAX as u64)
            || self.observed_at_unix_ns < 0
            || self.previous_digest.len() != 64
            || !self.previous_digest.bytes().all(|b| b.is_ascii_hexdigit())
            || self.digest.len() != 64
            || !self.digest.bytes().all(|b| b.is_ascii_hexdigit())
            || matches!(self.outcome, Outcome::Denied | Outcome::Rejected) != self.denial.is_some()
        {
            return Err(VerifyError::InvalidRecord);
        }
        Ok(())
    }

    fn canonical(&self) -> String {
        let mut value = String::new();
        for field in [
            self.schema_version.to_string(),
            self.event_id.clone(),
            self.event_type.tag().to_owned(),
            self.actor_id.clone().unwrap_or_default(),
            self.principal_kind.tag().to_owned(),
            self.device_id.clone().unwrap_or_default(),
            self.owner_id.clone(),
            self.resource_id.clone().unwrap_or_default(),
            self.operation.tag().to_owned(),
            self.policy_revision
                .map_or_else(String::new, |v| v.to_string()),
            self.outcome.tag().to_owned(),
            self.denial
                .map_or_else(String::new, |value| value.tag().to_owned()),
            self.durable_sequence.to_string(),
            self.authority_order.to_string(),
            self.operation_id.clone(),
            self.correlation_id.clone(),
            self.observed_at_unix_ns.to_string(),
            self.timestamp_provenance.tag().to_owned(),
            self.related_reference.clone().unwrap_or_default(),
        ] {
            append_field(&mut value, &field);
        }
        value
    }

    pub fn seal(&mut self, previous_digest: &str) {
        self.previous_digest = previous_digest.to_ascii_lowercase();
        let mut hasher = Sha256::new();
        hasher.update(self.previous_digest.as_bytes());
        hasher.update(self.canonical().as_bytes());
        self.digest = format!("{:x}", hasher.finalize());
    }
}

/// Verifies a complete owner stream against a trusted transactionally stored
/// checkpoint. A database-resident checkpoint is not an independent trust anchor.
pub fn verify_complete(
    owner_id: &str,
    records: &[AuditRecord],
    checkpoint: &Checkpoint,
) -> Result<(), VerifyError> {
    if !valid_reference(owner_id) || checkpoint.owner_id != owner_id {
        return Err(VerifyError::ScopeMismatch);
    }
    if checkpoint.record_count > MAX_VERIFIED_RECORDS {
        return Err(VerifyError::IncompleteEvidence);
    }
    if records.len() as u64 != checkpoint.record_count {
        return Err(VerifyError::IncompleteEvidence);
    }
    let mut previous = ZERO_DIGEST.to_owned();
    let mut previous_authority_order = 0;
    let mut event_ids = HashSet::new();
    let mut committed_operations = HashSet::new();
    for (index, record) in records.iter().enumerate() {
        record.validate()?;
        if record.owner_id != owner_id {
            return Err(VerifyError::ScopeMismatch);
        }
        if !event_ids.insert(record.event_id.as_str())
            || (record.event_type == EventType::Authorization
                && record.outcome == Outcome::Committed
                && !committed_operations.insert(record.operation_id.as_str()))
        {
            return Err(VerifyError::DuplicateRecord);
        }
        if record.durable_sequence != index as u64 + 1 {
            return Err(VerifyError::Ordering);
        }
        if record.authority_order <= previous_authority_order {
            return Err(VerifyError::Ordering);
        }
        if record.previous_digest != previous {
            return Err(VerifyError::Integrity);
        }
        let mut expected = record.clone();
        expected.seal(&previous);
        if expected.digest != record.digest {
            return Err(VerifyError::Integrity);
        }
        previous = record.digest.clone();
        previous_authority_order = record.authority_order;
    }
    if previous != checkpoint.head_digest {
        return Err(VerifyError::Integrity);
    }
    Ok(())
}

/// Bounded owner-scoped pagination. The complete stream is verified before a
/// page is returned; truncation or a caller-supplied foreign scope fails closed.
pub fn query_page(
    trusted_owner_id: &str,
    records: &[AuditRecord],
    checkpoint: &Checkpoint,
    after_sequence: u64,
    limit: usize,
) -> Result<AuditPage, VerifyError> {
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(VerifyError::InvalidLimit);
    }
    verify_complete(trusted_owner_id, records, checkpoint)?;
    let start = usize::try_from(after_sequence).map_err(|_| VerifyError::Ordering)?;
    if start > records.len() {
        return Err(VerifyError::Ordering);
    }
    let end = start.saturating_add(limit).min(records.len());
    let page = records[start..end].to_vec();
    Ok(AuditPage {
        records: page,
        next_sequence: (end < records.len()).then_some(end as u64),
        verified_checkpoint: checkpoint.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(sequence: u64, event_id: &str, outcome: Outcome) -> AuditRecord {
        let mut record = AuditRecord {
            schema_version: SCHEMA_VERSION,
            event_id: event_id.into(),
            event_type: EventType::Authorization,
            actor_id: Some("actor-1".into()),
            principal_kind: PrincipalKind::Browser,
            device_id: None,
            owner_id: "owner-1".into(),
            resource_id: Some("library-1".into()),
            operation: Operation::Write,
            policy_revision: Some(7),
            outcome,
            denial: (outcome == Outcome::Denied).then_some(DenialCategory::Revoked),
            durable_sequence: sequence,
            authority_order: sequence,
            operation_id: format!("operation-{sequence}"),
            correlation_id: "request-1".into(),
            observed_at_unix_ns: 1_800_000_000_000_000_000 + sequence as i64,
            timestamp_provenance: TimestampProvenance::DatabaseClock,
            related_reference: None,
            previous_digest: String::new(),
            digest: String::new(),
        };
        let previous = if sequence == 1 { ZERO_DIGEST } else { "unused" };
        record.seal(previous);
        record
    }

    fn stream() -> (Vec<AuditRecord>, Checkpoint) {
        let mut first = record(1, "event-1", Outcome::Committed);
        first.seal(ZERO_DIGEST);
        let mut second = record(2, "event-2", Outcome::Denied);
        second.seal(&first.digest);
        let checkpoint = Checkpoint {
            owner_id: "owner-1".into(),
            record_count: 2,
            head_digest: second.digest.clone(),
        };
        (vec![first, second], checkpoint)
    }

    #[test]
    fn complete_stream_verifies_and_pages_are_bounded() {
        let (records, checkpoint) = stream();
        assert_eq!(verify_complete("owner-1", &records, &checkpoint), Ok(()));
        let page = query_page("owner-1", &records, &checkpoint, 0, 1).unwrap();
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.next_sequence, Some(1));
    }

    #[test]
    fn missing_duplicate_reordered_or_tampered_evidence_fails() {
        let (records, checkpoint) = stream();
        assert_eq!(
            verify_complete("owner-1", &records[..1], &checkpoint),
            Err(VerifyError::IncompleteEvidence)
        );
        let duplicate = vec![records[0].clone(), records[0].clone()];
        assert_eq!(
            verify_complete("owner-1", &duplicate, &checkpoint),
            Err(VerifyError::DuplicateRecord)
        );
        let reordered = vec![records[1].clone(), records[0].clone()];
        assert_eq!(
            verify_complete("owner-1", &reordered, &checkpoint),
            Err(VerifyError::Ordering)
        );
        let mut changed = records;
        changed[0].actor_id = Some("other-actor".into());
        assert_eq!(
            verify_complete("owner-1", &changed, &checkpoint),
            Err(VerifyError::Integrity)
        );
    }

    #[test]
    fn owner_scope_versions_and_secret_shaped_values_fail_closed() {
        let (mut records, checkpoint) = stream();
        assert_eq!(
            query_page("foreign-owner", &records, &checkpoint, 0, 10),
            Err(VerifyError::ScopeMismatch)
        );
        records[0].schema_version = 2;
        assert_eq!(
            verify_complete("owner-1", &records, &checkpoint),
            Err(VerifyError::UnsupportedVersion)
        );
        let mut invalid = record(1, "event-1", Outcome::Committed);
        invalid.actor_id = Some("Bearer secret token".into());
        invalid.seal(ZERO_DIGEST);
        assert_eq!(invalid.validate(), Err(VerifyError::InvalidRecord));
    }

    #[test]
    fn unsupported_limits_and_duplicate_successful_operation_fail() {
        let (records, checkpoint) = stream();
        assert_eq!(
            query_page("owner-1", &records, &checkpoint, 0, MAX_PAGE_SIZE + 1),
            Err(VerifyError::InvalidLimit)
        );
        let mut duplicates = records.clone();
        duplicates[1].outcome = Outcome::Committed;
        duplicates[1].denial = None;
        duplicates[1].operation_id = duplicates[0].operation_id.clone();
        let prior_digest = duplicates[0].digest.clone();
        duplicates[1].seal(&prior_digest);
        let checkpoint = Checkpoint {
            owner_id: "owner-1".into(),
            record_count: 2,
            head_digest: duplicates[1].digest.clone(),
        };
        assert_eq!(
            verify_complete("owner-1", &duplicates, &checkpoint),
            Err(VerifyError::DuplicateRecord)
        );
    }
}
