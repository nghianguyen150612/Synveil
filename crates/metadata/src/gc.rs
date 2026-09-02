//! Metadata-only object-GC eligibility planning.
//!
//! This module owns the bounded candidate claim/lease/revalidation protocol.
//! It deliberately has no ObjectStore, filesystem, replica-deletion, or
//! transport dependency. A `READY` result is revocable metadata planning
//! evidence only; a later physical-GC phase must perform one more reference
//! check before deleting anything.

use std::{fmt, str::FromStr};

use sqlx::FromRow;
use synveil_core::{DedupDomainId, ObjectGcPolicy, ObjectId, Revision, Timestamp};
use uuid::Uuid;

use crate::{DatabaseError, DatabasePool, DomainRepository, MappingError, MetadataError};

const METADATA_PURGE_SOURCE: &str = "METADATA_PURGE";

/// Opaque capability identifying one worker lease. It has no public
/// constructor or UUID accessor; debug output also redacts its value.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct GcLeaseId(Uuid);

impl GcLeaseId {
    pub(crate) fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub(crate) fn from_uuid(value: Uuid) -> Result<Self, synveil_core::IdParseError> {
        let bytes = value.as_bytes();
        if bytes[6] >> 4 != 7 {
            return Err(synveil_core::IdParseError::NotUuidV7);
        }
        if bytes[8] & 0xc0 != 0x80 {
            return Err(synveil_core::IdParseError::InvalidVariant);
        }
        Ok(Self(value))
    }

    pub(crate) const fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Debug for GcLeaseId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<opaque-gc-lease>")
    }
}

/// The only candidate states supported by this phase. `READY` is still
/// revocable and does not authorize physical deletion.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectGcCandidateState {
    Eligible,
    Leased,
    Ready,
}

impl ObjectGcCandidateState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "ELIGIBLE",
            Self::Leased => "LEASED",
            Self::Ready => "READY",
        }
    }

    pub(crate) fn from_str(value: &str) -> Result<Self, MappingError> {
        match value {
            "ELIGIBLE" => Ok(Self::Eligible),
            "LEASED" => Ok(Self::Leased),
            "READY" => Ok(Self::Ready),
            _ => Err(MappingError::InvalidEnum {
                field: "object_gc_candidates.state",
            }),
        }
    }
}

/// A capability returned after an atomic candidate claim. The lease token and
/// generation must be presented for renewal, release, and revalidation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ObjectGcLease {
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    lease_id: GcLeaseId,
    lease_generation: u64,
    lease_acquired_at: Timestamp,
    lease_expires_at: Timestamp,
    state: ObjectGcCandidateState,
}

impl fmt::Debug for ObjectGcLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectGcLease")
            .field("object_id", &self.object_id)
            .field("dedup_domain_id", &self.dedup_domain_id)
            .field("lease_id", &self.lease_id)
            .field("lease_generation", &self.lease_generation)
            .field("lease_acquired_at", &self.lease_acquired_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .field("state", &self.state)
            .finish()
    }
}

impl ObjectGcLease {
    #[must_use]
    pub const fn object_id(self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn dedup_domain_id(self) -> DedupDomainId {
        self.dedup_domain_id
    }

    /// The lease identifier remains opaque to callers.
    #[must_use]
    pub const fn lease_id(self) -> GcLeaseId {
        self.lease_id
    }

    #[must_use]
    pub const fn lease_generation(self) -> u64 {
        self.lease_generation
    }

    #[must_use]
    pub const fn lease_acquired_at(self) -> Timestamp {
        self.lease_acquired_at
    }

    #[must_use]
    pub const fn lease_expires_at(self) -> Timestamp {
        self.lease_expires_at
    }

    #[must_use]
    pub const fn state(self) -> ObjectGcCandidateState {
        self.state
    }

    pub(crate) const fn from_parts(
        object_id: ObjectId,
        dedup_domain_id: DedupDomainId,
        lease_id: GcLeaseId,
        lease_generation: u64,
        lease_acquired_at: Timestamp,
        lease_expires_at: Timestamp,
        state: ObjectGcCandidateState,
    ) -> Self {
        Self {
            object_id,
            dedup_domain_id,
            lease_id,
            lease_generation,
            lease_acquired_at,
            lease_expires_at,
            state,
        }
    }
}

/// A canonical metadata candidate returned by revalidation. It contains no
/// physical location or deletion command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectGcCandidate {
    object_id: ObjectId,
    dedup_domain_id: DedupDomainId,
    unreferenced_at: Timestamp,
    state: ObjectGcCandidateState,
    lease_generation: u64,
    validated_at: Option<Timestamp>,
    lease: Option<ObjectGcLease>,
}

impl ObjectGcCandidate {
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    #[must_use]
    pub const fn dedup_domain_id(&self) -> DedupDomainId {
        self.dedup_domain_id
    }

    #[must_use]
    pub const fn unreferenced_at(&self) -> Timestamp {
        self.unreferenced_at
    }

    #[must_use]
    pub const fn state(&self) -> ObjectGcCandidateState {
        self.state
    }

    #[must_use]
    pub const fn lease_generation(&self) -> u64 {
        self.lease_generation
    }

    #[must_use]
    pub const fn validated_at(&self) -> Option<Timestamp> {
        self.validated_at
    }

    #[must_use]
    pub const fn lease(&self) -> Option<ObjectGcLease> {
        self.lease
    }

    pub(crate) const fn from_parts(
        object_id: ObjectId,
        dedup_domain_id: DedupDomainId,
        unreferenced_at: Timestamp,
        state: ObjectGcCandidateState,
        lease_generation: u64,
        validated_at: Option<Timestamp>,
        lease: Option<ObjectGcLease>,
    ) -> Self {
        Self {
            object_id,
            dedup_domain_id,
            unreferenced_at,
            state,
            lease_generation,
            validated_at,
            lease,
        }
    }
}

/// Result of a reference revalidation or a metadata-only ready transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectGcPlanResult {
    Valid(ObjectGcCandidate),
    Invalidated,
}

/// Idempotent release outcomes. `AlreadyGone` means a concurrent or earlier
/// committed reference already cancelled the candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcLeaseReleaseResult {
    Released,
    AlreadyReleased,
    AlreadyGone,
}

/// Sanitized failures for the internal GC planning boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectGcError {
    NotFound,
    InvalidRequest,
    InvalidPolicy,
    CandidateInvalidated,
    StaleLease,
    LeaseExpired,
    Database(DatabaseError),
    InvalidPersistedData,
}

impl fmt::Display for ObjectGcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotFound => "object GC candidate was not found",
            Self::InvalidRequest => "object GC request is invalid",
            Self::InvalidPolicy => "object GC policy cannot be evaluated",
            Self::CandidateInvalidated => "object GC candidate was invalidated by a reference",
            Self::StaleLease => "object GC lease is stale",
            Self::LeaseExpired => "object GC lease has expired",
            Self::Database(error) => return error.fmt(formatter),
            Self::InvalidPersistedData => "object GC persisted metadata is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ObjectGcError {}

/// Persisted candidate shape used only inside the SQLx adapter mapping
/// boundary. Numeric generations are selected as decimal text.
#[derive(Debug, FromRow)]
pub(crate) struct ObjectGcCandidateRow {
    pub(crate) object_id: Uuid,
    pub(crate) object_dedup_domain_id: Uuid,
    pub(crate) unreferenced_at: time::OffsetDateTime,
    pub(crate) source: String,
    pub(crate) state: String,
    pub(crate) lease_id: Option<Uuid>,
    pub(crate) lease_generation: String,
    pub(crate) lease_acquired_at: Option<time::OffsetDateTime>,
    pub(crate) lease_expires_at: Option<time::OffsetDateTime>,
    pub(crate) validated_at: Option<time::OffsetDateTime>,
}

pub(crate) enum GcRenewalMutation {
    Renewed(ObjectGcLease),
    Invalidated,
    CandidateGone,
    StaleLease,
    LeaseExpired,
    GraceNotMature,
}

pub(crate) enum GcPlanMutation {
    Valid(ObjectGcCandidate),
    Invalidated,
    CandidateGone,
    StaleLease,
    LeaseExpired,
    GraceNotMature,
}

pub(crate) enum GcReleaseMutation {
    Released,
    AlreadyReleased,
    CandidateGone,
    StaleLease,
}

/// PostgreSQL-backed, transport-neutral object-GC planning service.
#[derive(Clone)]
pub struct ObjectGcPlanningService {
    pool: DatabasePool,
    policy: ObjectGcPolicy,
}

impl ObjectGcPlanningService {
    #[must_use]
    pub fn new(pool: DatabasePool, policy: ObjectGcPolicy) -> Self {
        Self { pool, policy }
    }

    #[must_use]
    pub const fn policy(&self) -> ObjectGcPolicy {
        self.policy
    }

    /// Atomically claim at most the configured maximum number of mature,
    /// unreferenced metadata candidates.
    pub async fn claim_eligible_candidates(&self) -> Result<Vec<ObjectGcLease>, ObjectGcError> {
        self.claim_candidates(self.policy.max_batch_size()).await
    }

    /// Claim a caller-selected bounded batch for a worker loop.
    pub async fn claim_candidates(&self, limit: u32) -> Result<Vec<ObjectGcLease>, ObjectGcError> {
        if !(1..=self.policy.max_batch_size()).contains(&limit) {
            return Err(ObjectGcError::InvalidRequest);
        }
        DomainRepository::new(&self.pool)
            .claim_object_gc_candidates(self.policy, limit)
            .await
            .map_err(map_metadata_error)
    }

    /// Renew a matching, non-expired worker lease. Re-reference cancellation
    /// is returned explicitly rather than being reported as success.
    pub async fn renew_lease(&self, lease: ObjectGcLease) -> Result<ObjectGcLease, ObjectGcError> {
        match DomainRepository::new(&self.pool)
            .renew_object_gc_lease(self.policy, lease)
            .await
            .map_err(map_metadata_error)?
        {
            GcRenewalMutation::Renewed(lease) => Ok(lease),
            GcRenewalMutation::Invalidated => Err(ObjectGcError::CandidateInvalidated),
            GcRenewalMutation::CandidateGone => Err(ObjectGcError::NotFound),
            GcRenewalMutation::StaleLease => Err(ObjectGcError::StaleLease),
            GcRenewalMutation::LeaseExpired => Err(ObjectGcError::LeaseExpired),
            GcRenewalMutation::GraceNotMature => Err(ObjectGcError::InvalidRequest),
        }
    }

    /// Release a lease back to `ELIGIBLE`. Releasing the matching token after
    /// an earlier release is idempotent; a missing row is safe if rereference
    /// cancellation already won the race.
    pub async fn release_lease(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcLeaseReleaseResult, ObjectGcError> {
        match DomainRepository::new(&self.pool)
            .release_object_gc_lease(lease)
            .await
            .map_err(map_metadata_error)?
        {
            GcReleaseMutation::Released => Ok(ObjectGcLeaseReleaseResult::Released),
            GcReleaseMutation::AlreadyReleased => Ok(ObjectGcLeaseReleaseResult::AlreadyReleased),
            GcReleaseMutation::CandidateGone => Ok(ObjectGcLeaseReleaseResult::AlreadyGone),
            GcReleaseMutation::StaleLease => Err(ObjectGcError::StaleLease),
        }
    }

    /// Revalidate the candidate's committed FileVersion reference relation
    /// under its canonical object lock without changing it to `READY`.
    pub async fn revalidate_candidate(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcPlanResult, ObjectGcError> {
        self.plan(lease, false).await
    }

    /// Revalidate and mark metadata as `READY`. This remains revocable and is
    /// not an instruction to delete an Object, ObjectReplica, or byte.
    pub async fn mark_ready_for_deletion(
        &self,
        lease: ObjectGcLease,
    ) -> Result<ObjectGcPlanResult, ObjectGcError> {
        self.plan(lease, true).await
    }

    async fn plan(
        &self,
        lease: ObjectGcLease,
        mark_ready: bool,
    ) -> Result<ObjectGcPlanResult, ObjectGcError> {
        match DomainRepository::new(&self.pool)
            .revalidate_object_gc_candidate(self.policy, lease, mark_ready)
            .await
            .map_err(map_metadata_error)?
        {
            GcPlanMutation::Valid(candidate) => Ok(ObjectGcPlanResult::Valid(candidate)),
            GcPlanMutation::Invalidated => Ok(ObjectGcPlanResult::Invalidated),
            GcPlanMutation::CandidateGone => Err(ObjectGcError::NotFound),
            GcPlanMutation::StaleLease => Err(ObjectGcError::StaleLease),
            GcPlanMutation::LeaseExpired => Err(ObjectGcError::LeaseExpired),
            GcPlanMutation::GraceNotMature => Err(ObjectGcError::InvalidRequest),
        }
    }
}

pub(crate) fn map_object_gc_candidate_row(
    row: ObjectGcCandidateRow,
) -> Result<ObjectGcCandidate, MetadataError> {
    if row.source != METADATA_PURGE_SOURCE {
        return Err(MetadataError::Mapping(MappingError::InvalidEnum {
            field: "object_gc_candidates.source",
        }));
    }

    let object_id =
        ObjectId::try_from_uuid(row.object_id).map_err(|reason| MappingError::InvalidId {
            field: "object_gc_candidates.object_id",
            reason,
        })?;
    let dedup_domain_id =
        DedupDomainId::try_from_uuid(row.object_dedup_domain_id).map_err(|reason| {
            MappingError::InvalidId {
                field: "object_gc_candidates.object_dedup_domain_id",
                reason,
            }
        })?;
    let state = ObjectGcCandidateState::from_str(&row.state)?;
    let lease_generation = Revision::from_str(&row.lease_generation)
        .map(Revision::get)
        .map_err(|_| MappingError::InvalidDecimal {
            field: "object_gc_candidates.lease_generation",
        })?;
    let unreferenced_at = Timestamp::from_offset_datetime(row.unreferenced_at);
    let validated_at = row.validated_at.map(Timestamp::from_offset_datetime);

    let lease_fields_present = row.lease_id.is_some()
        && row.lease_acquired_at.is_some()
        && row.lease_expires_at.is_some()
        && validated_at.is_some();
    let lease_fields_absent = row.lease_id.is_none()
        && row.lease_acquired_at.is_none()
        && row.lease_expires_at.is_none()
        && validated_at.is_none();

    let lease = match state {
        ObjectGcCandidateState::Eligible if lease_fields_absent => None,
        ObjectGcCandidateState::Leased | ObjectGcCandidateState::Ready if lease_fields_present => {
            let lease_id = GcLeaseId::from_uuid(row.lease_id.expect("presence checked")).map_err(
                |reason| MappingError::InvalidId {
                    field: "object_gc_candidates.lease_id",
                    reason,
                },
            )?;
            let acquired_at =
                Timestamp::from_offset_datetime(row.lease_acquired_at.expect("presence checked"));
            let expires_at =
                Timestamp::from_offset_datetime(row.lease_expires_at.expect("presence checked"));
            if expires_at <= acquired_at {
                return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                    relation: "object_gc_candidates.lease_lifecycle",
                }));
            }
            Some(ObjectGcLease::from_parts(
                object_id,
                dedup_domain_id,
                lease_id,
                lease_generation,
                acquired_at,
                expires_at,
                state,
            ))
        }
        _ => {
            return Err(MetadataError::Mapping(MappingError::RelationMismatch {
                relation: "object_gc_candidates.lifecycle",
            }));
        }
    };

    Ok(ObjectGcCandidate::from_parts(
        object_id,
        dedup_domain_id,
        unreferenced_at,
        state,
        lease_generation,
        validated_at,
        lease,
    ))
}

fn map_metadata_error(error: MetadataError) -> ObjectGcError {
    match error {
        MetadataError::Database(error) => ObjectGcError::Database(error),
        MetadataError::Mapping(MappingError::Domain(_)) => ObjectGcError::InvalidRequest,
        MetadataError::Mapping(MappingError::InvalidTimestamp { field })
            if field.starts_with("object_gc_policy.") =>
        {
            ObjectGcError::InvalidPolicy
        }
        MetadataError::Mapping(_) => ObjectGcError::InvalidPersistedData,
        MetadataError::CapacityUnavailable => ObjectGcError::InvalidRequest,
    }
}

#[cfg(test)]
mod tests {
    use super::{GcLeaseId, ObjectGcCandidateState};

    #[test]
    fn lease_ids_are_debug_redacted_and_states_are_canonical() {
        let lease_id = GcLeaseId::new();
        assert_eq!(format!("{lease_id:?}"), "<opaque-gc-lease>");
        assert_eq!(ObjectGcCandidateState::Eligible.as_str(), "ELIGIBLE");
        assert_eq!(ObjectGcCandidateState::Leased.as_str(), "LEASED");
        assert_eq!(ObjectGcCandidateState::Ready.as_str(), "READY");
    }
}
