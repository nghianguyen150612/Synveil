//! Durable scheduled-maintenance claim/lease with a fenced
//! exactly-one-transition worker primitive.
//!
//! One explicit worker-step invocation claims or reconciles at most one
//! eligible scheduled step and executes or reconciles at most one canonical
//! Prompt 49 maintenance transition. There is no daemon, polling loop,
//! scheduler loop, heartbeat renewal, retry/backoff loop, queue, or public
//! API in this module.
//!
//! Discovery applies only to maintenance runs with a durable Prompt 63
//! handoff. Manual Prompt 49 runs are never claimed. A committed handoff is
//! execution authority: current schedule revision, misfire policy, and enabled
//! state are never rechecked to revoke handed-off work.
//!
//! Safety rests on fencing, not on workers promising to stop. Every
//! maintenance-state commit re-verifies the active claim
//! `(worker, token, generation, expected state, unexpired lease)` inside the
//! same authoritative transaction, so a stale generation can never commit a
//! transition even when its canonical child work already replayed.

use std::fmt;

use sqlx::{FromRow, Postgres, Transaction};
use synveil_core::{
    BackupMaintenanceRun, BackupMaintenanceRunPreflightIssue, BackupMaintenanceRunState,
    BackupScheduleId, BackupScheduleOccurrenceId, BackupScheduledMaintenanceClaim,
    BackupScheduledMaintenanceClaimId, BackupScheduledMaintenanceClaimOutcome,
    BackupScheduledMaintenanceLeaseToken, BackupScheduledMaintenanceStepResult,
    BackupScheduledMaintenanceWorkerId, BackupSetId, SnapshotId, Timestamp, UserId,
    is_claimable_scheduled_maintenance_state, scheduled_maintenance_resulting_state,
    validate_scheduled_maintenance_lease_duration,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    BackupError, DatabaseError, DatabasePool,
    backup::{load_maintenance_run_in_transaction, maintenance_has_foreign_active_expiry_plan},
};

/// Typed failures for the scheduled-maintenance worker boundary.
///
/// `LeaseLost` is a terminal fence outcome, not a retryable signal: the
/// caller must not commit a transition and must not loop. Stale and
/// inconsistent states fail closed with dedicated variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledMaintenanceWorkerError {
    NotFound,
    InvalidLeaseDuration,
    LeaseLost,
    RunStale,
    InconsistentState,
    MaintenancePreflight(BackupMaintenanceRunPreflightIssue),
    Database(DatabaseError),
    InvalidPersistedData,
    Backup(BackupError),
}

impl fmt::Display for ScheduledMaintenanceWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("scheduled maintenance claim was not found"),
            Self::InvalidLeaseDuration => {
                formatter.write_str("scheduled maintenance lease duration is out of bounds")
            }
            Self::LeaseLost => formatter.write_str("scheduled maintenance lease is lost or stale"),
            Self::RunStale => formatter.write_str("scheduled maintenance run became stale"),
            Self::InconsistentState => formatter
                .write_str("scheduled maintenance run state is inconsistent with its claim"),
            Self::MaintenancePreflight(issue) => issue.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
            Self::InvalidPersistedData => {
                formatter.write_str("scheduled maintenance persisted data is invalid")
            }
            Self::Backup(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ScheduledMaintenanceWorkerError {}

impl From<sqlx::Error> for ScheduledMaintenanceWorkerError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(DatabaseError::from(error))
    }
}

impl From<BackupError> for ScheduledMaintenanceWorkerError {
    fn from(error: BackupError) -> Self {
        match error {
            BackupError::NotFound => Self::NotFound,
            BackupError::InvalidPersistedData => Self::InvalidPersistedData,
            BackupError::Database(error) => Self::Database(error),
            BackupError::MaintenanceRunPreflight(issue) => Self::MaintenancePreflight(issue),
            other => Self::Backup(other),
        }
    }
}

/// Combined outcome of one explicit worker step: the discovery decision plus
/// the optional fenced execution decision. Recovery reconciliations stop
/// after the receipt and never execute a transition in the same invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledMaintenanceWorkerStepOutcome {
    discovery: BackupScheduledMaintenanceClaimOutcome,
    execution: Option<BackupScheduledMaintenanceStepResult>,
}

impl ScheduledMaintenanceWorkerStepOutcome {
    #[must_use]
    pub const fn discovery(&self) -> &BackupScheduledMaintenanceClaimOutcome {
        &self.discovery
    }

    #[must_use]
    pub const fn execution(&self) -> Option<&BackupScheduledMaintenanceStepResult> {
        self.execution.as_ref()
    }

    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self.discovery, BackupScheduledMaintenanceClaimOutcome::Idle)
    }
}

#[derive(Clone, Debug, FromRow)]
#[allow(dead_code)]
struct ScheduledMaintenanceClaimRow {
    claim_id: Uuid,
    owner_user_id: Uuid,
    backup_set_id: Uuid,
    schedule_id: Uuid,
    occurrence_id: Uuid,
    maintenance_run_id: Uuid,
    expected_state: String,
    resulting_state: Option<String>,
    lease_worker_id: Uuid,
    lease_token: Uuid,
    lease_generation: i64,
    lease_acquired_at: OffsetDateTime,
    lease_expires_at: OffsetDateTime,
    completed_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl ScheduledMaintenanceClaimRow {
    fn try_into_domain(
        self,
    ) -> Result<BackupScheduledMaintenanceClaim, ScheduledMaintenanceWorkerError> {
        let claim_id = BackupScheduledMaintenanceClaimId::try_from_uuid(self.claim_id)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let owner_user_id = UserId::try_from_uuid(self.owner_user_id)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let backup_set_id = BackupSetId::try_from_uuid(self.backup_set_id)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let schedule_id = BackupScheduleId::try_from_uuid(self.schedule_id)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let occurrence_id = BackupScheduleOccurrenceId::try_from_uuid(self.occurrence_id)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let maintenance_run_id =
            synveil_core::BackupMaintenanceRunId::try_from_uuid(self.maintenance_run_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let expected_state = self
            .expected_state
            .parse::<BackupMaintenanceRunState>()
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let resulting_state = self
            .resulting_state
            .map(|value| {
                value
                    .parse::<BackupMaintenanceRunState>()
                    .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)
            })
            .transpose()?;
        let lease_worker_id =
            BackupScheduledMaintenanceWorkerId::try_from_uuid(self.lease_worker_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let lease_token = BackupScheduledMaintenanceLeaseToken::try_from_uuid(self.lease_token)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let lease_generation = u64::try_from(self.lease_generation)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        BackupScheduledMaintenanceClaim::rehydrate(
            claim_id,
            owner_user_id,
            backup_set_id,
            schedule_id,
            occurrence_id,
            maintenance_run_id,
            expected_state,
            resulting_state,
            lease_worker_id,
            lease_token,
            lease_generation,
            Timestamp::from_offset_datetime(self.lease_acquired_at),
            Timestamp::from_offset_datetime(self.lease_expires_at),
            self.completed_at.map(Timestamp::from_offset_datetime),
            Timestamp::from_offset_datetime(self.created_at),
        )
        .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)
    }
}

const CLAIM_COLUMNS: &str = "claim_id, owner_user_id, backup_set_id, schedule_id, \
     occurrence_id, maintenance_run_id, expected_state, resulting_state, \
     lease_worker_id, lease_token, lease_generation, lease_acquired_at, \
     lease_expires_at, completed_at, created_at, updated_at";

#[derive(Clone, Debug, FromRow)]
struct CandidateRunRow {
    run_id: Uuid,
    owner_user_id: Uuid,
}

/// Manually invoked, test-driven scheduled-maintenance worker primitive.
///
/// Every method performs a bounded amount of durable work and returns; no
/// method loops, polls, sleeps, spawns, or renews a lease.
#[derive(Clone)]
pub struct ScheduledMaintenanceWorkerService {
    pool: DatabasePool,
}

impl ScheduledMaintenanceWorkerService {
    #[must_use]
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub const fn pool(&self) -> &DatabasePool {
        &self.pool
    }

    /// Claim, replay, take over, or reconcile exactly one eligible scheduled
    /// step in deterministic global order
    /// (`scheduled_for_utc, schedule_id, maintenance_run_id`). An unexpired
    /// foreign lease skips that run without blocking newer claimable work.
    /// Completed-claim retries return the canonical receipt.
    pub async fn claim_next_scheduled_maintenance_step(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<BackupScheduledMaintenanceClaimOutcome, ScheduledMaintenanceWorkerError> {
        let lease_duration = validate_scheduled_maintenance_lease_duration(lease_duration_seconds)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidLeaseDuration)?;
        let lease_expires_at = observed_at_utc
            .checked_add_std(lease_duration)
            .ok_or(ScheduledMaintenanceWorkerError::InvalidLeaseDuration)?;

        let candidates = sqlx::query_as::<_, CandidateRunRow>(
            "SELECT run.id AS run_id, run.owner_user_id
              FROM backup_maintenance_runs AS run
              INNER JOIN backup_schedule_occurrence_handoffs AS handoff
                ON handoff.maintenance_run_id = run.id
              INNER JOIN backup_schedule_occurrences AS occurrence
                ON occurrence.id = handoff.occurrence_id
              WHERE run.state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED')
                 OR EXISTS (
                     SELECT 1
                     FROM backup_scheduled_maintenance_claims AS pending
                     WHERE pending.maintenance_run_id = run.id
                       AND pending.completed_at IS NULL
                 )
              ORDER BY occurrence.scheduled_for_utc ASC,
                       handoff.schedule_id ASC,
                       run.id ASC",
        )
        .fetch_all(self.pool.sqlx_pool())
        .await?;

        for candidate in candidates {
            let owner_user_id = UserId::try_from_uuid(candidate.owner_user_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
            let run_id = synveil_core::BackupMaintenanceRunId::try_from_uuid(candidate.run_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
            if let Some(outcome) = self
                .claim_candidate_step(
                    worker_id,
                    observed_at_utc,
                    lease_expires_at,
                    owner_user_id,
                    run_id,
                )
                .await?
            {
                return Ok(outcome);
            }
        }
        Ok(BackupScheduledMaintenanceClaimOutcome::Idle)
    }

    /// Execute exactly one fenced claimed step. At most one canonical Prompt
    /// 49 transition commits per invocation; a reconciled recovery stops
    /// without advancing to the next state.
    pub async fn execute_claimed_scheduled_maintenance_step(
        &self,
        claim_id: BackupScheduledMaintenanceClaimId,
        worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduledMaintenanceStepResult, ScheduledMaintenanceWorkerError> {
        let plan = self
            .load_execution_plan(
                claim_id,
                worker_id,
                lease_token,
                lease_generation,
                observed_at_utc,
            )
            .await?;
        match plan {
            ExecutionPlan::AlreadyCompleted { claim } => {
                Ok(BackupScheduledMaintenanceStepResult::AlreadyCompleted { claim })
            }
            ExecutionPlan::Recover { claim, run } => {
                let completed = self
                    .complete_claim_receipt(
                        &claim,
                        worker_id,
                        lease_token,
                        lease_generation,
                        observed_at_utc,
                    )
                    .await?;
                Ok(BackupScheduledMaintenanceStepResult::RecoveredCompletion {
                    claim: completed,
                    maintenance_run: run,
                })
            }
            ExecutionPlan::Advance { claim, run } => {
                self.advance_claimed_run_once(
                    &claim,
                    &run,
                    worker_id,
                    lease_token,
                    lease_generation,
                    observed_at_utc,
                )
                .await
            }
        }
    }

    /// Claim or reconcile one eligible scheduled step, then execute or
    /// reconcile at most one transition. Recovery outcomes stop before any
    /// execution so one invocation never performs two semantic actions.
    pub async fn run_scheduled_maintenance_worker_step(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_duration_seconds: u64,
    ) -> Result<ScheduledMaintenanceWorkerStepOutcome, ScheduledMaintenanceWorkerError> {
        let discovery = self
            .claim_next_scheduled_maintenance_step(
                worker_id,
                observed_at_utc,
                lease_duration_seconds,
            )
            .await?;
        let claim = match &discovery {
            BackupScheduledMaintenanceClaimOutcome::Idle => {
                return Ok(ScheduledMaintenanceWorkerStepOutcome {
                    discovery,
                    execution: None,
                });
            }
            BackupScheduledMaintenanceClaimOutcome::RecoveredCompletion(_) => {
                return Ok(ScheduledMaintenanceWorkerStepOutcome {
                    discovery,
                    execution: None,
                });
            }
            BackupScheduledMaintenanceClaimOutcome::Claimed(claim)
            | BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(claim)
            | BackupScheduledMaintenanceClaimOutcome::TakenOver(claim) => claim.clone(),
        };
        let execution = self
            .execute_claimed_scheduled_maintenance_step(
                claim.claim_id(),
                worker_id,
                claim.lease_token(),
                claim.lease_generation(),
                observed_at_utc,
            )
            .await?;
        Ok(ScheduledMaintenanceWorkerStepOutcome {
            discovery,
            execution: Some(execution),
        })
    }

    /// Resolve one candidate run to a discovery outcome, or `None` to skip to
    /// the next globally ordered run. Each candidate holds a short transaction
    /// with the canonical lock hierarchy:
    /// `backup_sets -> backup_schedules -> occurrences -> handoffs -> maintenance_runs -> claims`.
    async fn claim_candidate_step(
        &self,
        worker_id: BackupScheduledMaintenanceWorkerId,
        observed_at_utc: Timestamp,
        lease_expires_at: Timestamp,
        owner_user_id: UserId,
        run_id: synveil_core::BackupMaintenanceRunId,
    ) -> Result<Option<BackupScheduledMaintenanceClaimOutcome>, ScheduledMaintenanceWorkerError>
    {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        // Canonical discovery: learn top-level authority without holding row locks,
        // then acquire locks in hierarchy order. This eliminates the previous
        // `run -> claim` vs `claim -> run` and `run -> backup_set` vs
        // `backup_set -> run` inversions.
        let backup_set_uuid = sqlx::query_scalar::<_, Uuid>(
            "SELECT backup_set_id FROM backup_maintenance_runs WHERE id = $1 AND owner_user_id = $2",
        )
        .bind(run_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(backup_set_uuid) = backup_set_uuid else {
            transaction.commit().await?;
            return Ok(None);
        };
        let backup_set_id = BackupSetId::try_from_uuid(backup_set_uuid)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        crate::backup::lock_owned_backup_set(&mut transaction, owner_user_id, backup_set_id)
            .await
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;

        // Handoff is immutable; discovering it without lock before acquiring
        // parent locks preserves hierarchy and avoids holding run lock while
        // waiting for schedule/occurrence.
        let handoff_opt = load_handoff_scope(&mut transaction, owner_user_id, run_id).await?;
        let Some(handoff) = handoff_opt else {
            transaction.commit().await?;
            return Ok(None);
        };

        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM backup_schedules WHERE id = $1 AND owner_user_id = $2 FOR UPDATE",
        )
        .bind(handoff.schedule_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;

        crate::occurrences::load_occurrence_in_transaction(
            &mut transaction,
            owner_user_id,
            handoff.occurrence_id,
            true,
        )
        .await
        .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?
        .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;

        sqlx::query_scalar::<_, Uuid>(
            "SELECT occurrence_id FROM backup_schedule_occurrence_handoffs WHERE maintenance_run_id = $1 AND owner_user_id = $2 FOR SHARE",
        )
        .bind(run_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;

        let Some(run_row) =
            load_maintenance_run_in_transaction(&mut transaction, owner_user_id, run_id, true)
                .await?
        else {
            transaction.commit().await?;
            return Ok(None);
        };
        let run = run_row
            .try_into_domain()
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        if run.owner_user_id() != owner_user_id {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        }

        let claim_rows = load_incomplete_claims_for_run(&mut transaction, run_id).await?;
        if claim_rows.len() > 1 {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        }

        if let Some(claim_row) = claim_rows.into_iter().next() {
            let claim = claim_row.try_into_domain()?;
            if claim.owner_user_id() != run.owner_user_id()
                || claim.backup_set_id() != run.backup_set_id()
                || claim.maintenance_run_id() != run.id()
            {
                return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
            }
            if run.state() == claim.expected_state() {
                if claim.lease_expires_at() > observed_at_utc {
                    if claim.lease_worker_id() == worker_id {
                        transaction.commit().await?;
                        return Ok(Some(
                            BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(claim),
                        ));
                    }
                    transaction.commit().await?;
                    return Ok(None);
                }
                let taken = takeover_claim_in_transaction(
                    &mut transaction,
                    &claim,
                    worker_id,
                    observed_at_utc,
                    lease_expires_at,
                )
                .await?;
                transaction.commit().await?;
                return Ok(taken.map(BackupScheduledMaintenanceClaimOutcome::TakenOver));
            }
            if run.state() == claim.expected_resulting_state() {
                // Recovery completes an idempotent receipt for an already
                // committed transition, so it never steals execution
                // authority: it reconciles before newer work even while the
                // recorded lease is still active.
                let recovered =
                    complete_claim_in_transaction(&mut transaction, &claim, observed_at_utc)
                        .await?;
                transaction.commit().await?;
                return Ok(recovered.map(|completed| {
                    BackupScheduledMaintenanceClaimOutcome::RecoveredCompletion(completed)
                }));
            }
            if run.state().is_terminal() {
                transaction.commit().await?;
                return Ok(None);
            }
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        }

        if run.state().is_terminal() {
            transaction.commit().await?;
            return Ok(None);
        }
        if !is_claimable_scheduled_maintenance_state(run.state()) {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        }
        // Reuse the already-locked handoff discovered above.
        let inserted = insert_claim_in_transaction(
            &mut transaction,
            owner_user_id,
            run.backup_set_id(),
            handoff.schedule_id,
            handoff.occurrence_id,
            run_id,
            run.state(),
            worker_id,
            observed_at_utc,
            lease_expires_at,
        )
        .await?;
        transaction.commit().await?;
        match inserted {
            Some(claim) => Ok(Some(BackupScheduledMaintenanceClaimOutcome::Claimed(claim))),
            None => {
                let winner =
                    load_claim_by_run_state(&self.pool, owner_user_id, run_id, run.state()).await?;
                match winner {
                    Some(winner) if winner.lease_worker_id() == worker_id => Ok(Some(
                        BackupScheduledMaintenanceClaimOutcome::ExistingCurrentLease(winner),
                    )),
                    _ => Ok(None),
                }
            }
        }
    }

    /// Fence-checked load that decides between replay, recovery, and
    /// advancement without performing any maintenance side effect.
    /// Canonical lock order is `maintenance_runs -> claims` (parent before
    /// child) to match `claim_candidate_step` and avoid the previous
    /// `claims -> runs` inversion.
    async fn load_execution_plan(
        &self,
        claim_id: BackupScheduledMaintenanceClaimId,
        worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        observed_at_utc: Timestamp,
    ) -> Result<ExecutionPlan, ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        // Discover run identity without holding claim lock, then acquire
        // parent before child in hierarchy order.
        let Some(discovery_row) = load_claim_row(&mut transaction, claim_id).await? else {
            return Err(ScheduledMaintenanceWorkerError::NotFound);
        };
        let discovery_claim = discovery_row.try_into_domain()?;
        if discovery_claim.is_completed() {
            transaction.commit().await?;
            return Ok(ExecutionPlan::AlreadyCompleted {
                claim: discovery_claim,
            });
        }
        // Canonical parent first.
        let Some(run_row) = load_maintenance_run_in_transaction(
            &mut transaction,
            discovery_claim.owner_user_id(),
            discovery_claim.maintenance_run_id(),
            true,
        )
        .await?
        else {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        };
        // Then child.
        let Some(claim_row) = load_claim_for_update(&mut transaction, claim_id).await? else {
            return Err(ScheduledMaintenanceWorkerError::NotFound);
        };
        let claim = claim_row.try_into_domain()?;
        if claim.is_completed() {
            transaction.commit().await?;
            return Ok(ExecutionPlan::AlreadyCompleted { claim });
        }
        let fence = ClaimFence {
            worker_id,
            lease_token,
            lease_generation,
            observed_at_utc,
        };
        check_claim_fence(&claim, fence)?;
        let run = run_row
            .try_into_domain()
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        if run.backup_set_id() != claim.backup_set_id()
            || run.owner_user_id() != claim.owner_user_id()
        {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        }
        let plan = if run.state() == claim.expected_state() {
            ExecutionPlan::Advance { claim, run }
        } else if run.state() == claim.expected_resulting_state() {
            ExecutionPlan::Recover { claim, run }
        } else if run.state() == BackupMaintenanceRunState::Stale {
            return Err(ScheduledMaintenanceWorkerError::RunStale);
        } else {
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        };
        transaction.commit().await?;
        Ok(plan)
    }

    /// Complete a claim receipt with the same fence that guards transitions.
    async fn complete_claim_receipt(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduledMaintenanceClaim, ScheduledMaintenanceWorkerError> {
        self.try_complete_claim_receipt(
            claim,
            worker_id,
            lease_token,
            lease_generation,
            observed_at_utc,
        )
        .await
        .map(|(completed, _)| completed)
    }

    /// Complete a claim receipt and report whether this caller sealed it.
    /// A concurrent winner's canonical receipt is returned with `false` so
    /// racing executors can report recovery instead of a second advance.
    async fn try_complete_claim_receipt(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        observed_at_utc: Timestamp,
    ) -> Result<(BackupScheduledMaintenanceClaim, bool), ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        let Some(claim_row) = load_claim_for_update(&mut transaction, claim.claim_id()).await?
        else {
            return Err(ScheduledMaintenanceWorkerError::NotFound);
        };
        let current = claim_row.try_into_domain()?;
        if current.is_completed() {
            transaction.commit().await?;
            return Ok((current, false));
        }
        let fence = ClaimFence {
            worker_id,
            lease_token,
            lease_generation,
            observed_at_utc,
        };
        check_claim_fence(&current, fence)?;
        let Some(completed) =
            complete_claim_in_transaction(&mut transaction, &current, observed_at_utc).await?
        else {
            transaction.commit().await?;
            return Err(ScheduledMaintenanceWorkerError::LeaseLost);
        };
        transaction.commit().await?;
        Ok((completed, true))
    }

    /// Perform exactly one canonical Prompt 49 transition for a fenced claim.
    /// Child work reuses the canonical `BackupService` operations with the
    /// run's durable child-operation identities; only the final run-state and
    /// claim-receipt commits add the lease fence.
    async fn advance_claimed_run_once(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        run: &BackupMaintenanceRun,
        worker_id: BackupScheduledMaintenanceWorkerId,
        lease_token: BackupScheduledMaintenanceLeaseToken,
        lease_generation: u64,
        observed_at_utc: Timestamp,
    ) -> Result<BackupScheduledMaintenanceStepResult, ScheduledMaintenanceWorkerError> {
        let fence = ClaimFence {
            worker_id,
            lease_token,
            lease_generation,
            observed_at_utc,
        };
        let transition = match claim.expected_state() {
            BackupMaintenanceRunState::Created => {
                let snapshot = crate::BackupService::new(self.pool.clone())
                    .capture_snapshot(
                        run.owner_user_id(),
                        run.backup_set_id(),
                        SnapshotId::new(),
                        run.capture_operation_id().to_owned(),
                    )
                    .await?;
                let committed_at = snapshot
                    .committed_at()
                    .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
                self.fenced_transition_snapshot_captured(claim, fence, snapshot.id(), committed_at)
                    .await?
            }
            BackupMaintenanceRunState::SnapshotCaptured => {
                if maintenance_has_foreign_active_expiry_plan(
                    self.pool.sqlx_pool(),
                    run.backup_set_id(),
                    run.expiry_plan_operation_id(),
                )
                .await?
                {
                    self.fenced_mark_run_stale(claim, fence).await?;
                    return Err(ScheduledMaintenanceWorkerError::MaintenancePreflight(
                        BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                    ));
                }
                let plan = match crate::BackupService::new(self.pool.clone())
                    .create_snapshot_expiry_plan_with_expected_policy(
                        run.owner_user_id(),
                        run.expiry_plan_operation_id().to_owned(),
                        run.backup_set_id(),
                        Some((run.policy_revision_id(), run.policy_revision_number())),
                    )
                    .await
                {
                    Ok(plan) => plan,
                    Err(BackupError::ExpiryPreflight(
                        synveil_core::BackupSnapshotExpiryPreflightIssue::ExpiryAlreadyPlanned,
                    )) => {
                        self.fenced_mark_run_stale(claim, fence).await?;
                        return Err(ScheduledMaintenanceWorkerError::MaintenancePreflight(
                            BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                        ));
                    }
                    Err(BackupError::MaintenanceRunPreflight(
                        BackupMaintenanceRunPreflightIssue::PolicyChanged,
                    )) => {
                        self.fenced_mark_run_stale(claim, fence).await?;
                        return Err(ScheduledMaintenanceWorkerError::MaintenancePreflight(
                            BackupMaintenanceRunPreflightIssue::PolicyChanged,
                        ));
                    }
                    Err(error) => return Err(error.into()),
                };
                if plan.policy_revision_id() != run.policy_revision_id()
                    || plan.policy_revision_number() != run.policy_revision_number()
                {
                    self.fenced_mark_run_stale(claim, fence).await?;
                    return Err(ScheduledMaintenanceWorkerError::MaintenancePreflight(
                        BackupMaintenanceRunPreflightIssue::PolicyChanged,
                    ));
                }
                self.fenced_transition_expiry_planned(claim, fence, plan.id(), plan.created_at())
                    .await?
            }
            BackupMaintenanceRunState::ExpiryPlanned => {
                let plan_id = run
                    .expiry_plan_id()
                    .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
                let execution = match crate::BackupService::new(self.pool.clone())
                    .execute_snapshot_expiry_plan(run.owner_user_id(), plan_id)
                    .await
                {
                    Ok(execution) => execution,
                    Err(BackupError::ExpiryExecutionPreflight(
                        synveil_core::BackupSnapshotExpiryExecutionPreflightIssue::PlanStale,
                    )) => {
                        self.fenced_mark_run_stale(claim, fence).await?;
                        return Err(ScheduledMaintenanceWorkerError::MaintenancePreflight(
                            BackupMaintenanceRunPreflightIssue::ExpiryPlanStale,
                        ));
                    }
                    Err(error) => return Err(error.into()),
                };
                self.fenced_transition_completed(
                    claim,
                    fence,
                    execution.id(),
                    execution.executed_at(),
                )
                .await?
            }
            BackupMaintenanceRunState::Completed | BackupMaintenanceRunState::Stale => {
                return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
            }
        };
        let (completed, we_completed) = self
            .try_complete_claim_receipt(
                claim,
                worker_id,
                lease_token,
                lease_generation,
                observed_at_utc,
            )
            .await?;
        let run = crate::BackupService::new(self.pool.clone())
            .get_backup_maintenance_run(claim.owner_user_id(), claim.maintenance_run_id())
            .await?;
        if transition == TransitionOutcome::Committed && we_completed {
            Ok(BackupScheduledMaintenanceStepResult::Advanced {
                claim: completed,
                maintenance_run: run,
            })
        } else {
            Ok(BackupScheduledMaintenanceStepResult::RecoveredCompletion {
                claim: completed,
                maintenance_run: run,
            })
        }
    }

    /// Authoritative `CREATED -> SNAPSHOT_CAPTURED` commit with the active
    /// claim verified inside the same transaction. A stale generation commits
    /// zero maintenance state.
    async fn fenced_transition_snapshot_captured(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        fence: ClaimFence,
        snapshot_id: SnapshotId,
        captured_at: Timestamp,
    ) -> Result<TransitionOutcome, ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        if let Err(error) = verify_fenced_run_state(
            &mut transaction,
            claim,
            fence,
            BackupMaintenanceRunState::Created,
        )
        .await
        {
            let outcome = map_transition_verify_error(&mut transaction, claim, error).await?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        let affected = sqlx::query(
            "UPDATE backup_maintenance_runs
             SET state = 'SNAPSHOT_CAPTURED',
                 captured_snapshot_id = $3,
                 snapshot_captured_at = $4
             WHERE id = $1 AND owner_user_id = $2 AND state = 'CREATED'",
        )
        .bind(claim.maintenance_run_id().into_uuid())
        .bind(claim.owner_user_id().into_uuid())
        .bind(snapshot_id.into_uuid())
        .bind(captured_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        }
        transaction.commit().await?;
        Ok(TransitionOutcome::Committed)
    }

    /// Authoritative `SNAPSHOT_CAPTURED -> EXPIRY_PLANNED` commit with the
    /// active claim verified inside the same transaction.
    async fn fenced_transition_expiry_planned(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        fence: ClaimFence,
        plan_id: synveil_core::BackupSnapshotExpiryPlanId,
        planned_at: Timestamp,
    ) -> Result<TransitionOutcome, ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        if let Err(error) = verify_fenced_run_state(
            &mut transaction,
            claim,
            fence,
            BackupMaintenanceRunState::SnapshotCaptured,
        )
        .await
        {
            let outcome = map_transition_verify_error(&mut transaction, claim, error).await?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        let affected = sqlx::query(
            "UPDATE backup_maintenance_runs
             SET state = 'EXPIRY_PLANNED',
                 expiry_plan_id = $3,
                 expiry_planned_at = $4
             WHERE id = $1 AND owner_user_id = $2 AND state = 'SNAPSHOT_CAPTURED'",
        )
        .bind(claim.maintenance_run_id().into_uuid())
        .bind(claim.owner_user_id().into_uuid())
        .bind(plan_id.into_uuid())
        .bind(planned_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        }
        transaction.commit().await?;
        Ok(TransitionOutcome::Committed)
    }

    /// Authoritative `EXPIRY_PLANNED -> COMPLETED` commit with the active
    /// claim verified inside the same transaction.
    async fn fenced_transition_completed(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        fence: ClaimFence,
        execution_id: synveil_core::BackupSnapshotExpiryExecutionId,
        completed_at: Timestamp,
    ) -> Result<TransitionOutcome, ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        if let Err(error) = verify_fenced_run_state(
            &mut transaction,
            claim,
            fence,
            BackupMaintenanceRunState::ExpiryPlanned,
        )
        .await
        {
            let outcome = map_transition_verify_error(&mut transaction, claim, error).await?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        let affected = sqlx::query(
            "UPDATE backup_maintenance_runs
             SET state = 'COMPLETED',
                 expiry_execution_id = $3,
                 maintenance_completed_at = $4
             WHERE id = $1 AND owner_user_id = $2 AND state = 'EXPIRY_PLANNED'",
        )
        .bind(claim.maintenance_run_id().into_uuid())
        .bind(claim.owner_user_id().into_uuid())
        .bind(execution_id.into_uuid())
        .bind(completed_at.as_offset_datetime())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        }
        transaction.commit().await?;
        Ok(TransitionOutcome::Committed)
    }

    /// Prompt 49 staleness applied by the fenced worker: the run moves to
    /// `STALE` only while the presenting lease is still the active claim.
    /// The open claim is deliberately left incomplete; it never receives a
    /// normal-transition receipt. Canonical order is `runs -> claims`.
    async fn fenced_mark_run_stale(
        &self,
        claim: &BackupScheduledMaintenanceClaim,
        fence: ClaimFence,
    ) -> Result<(), ScheduledMaintenanceWorkerError> {
        let mut transaction = self.pool.sqlx_pool().begin().await?;
        let Some(run_row) = load_maintenance_run_in_transaction(
            &mut transaction,
            claim.owner_user_id(),
            claim.maintenance_run_id(),
            true,
        )
        .await?
        else {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        };
        let _run = run_row
            .try_into_domain()
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
        let Some(claim_row) = load_claim_for_update(&mut transaction, claim.claim_id()).await?
        else {
            return Err(ScheduledMaintenanceWorkerError::NotFound);
        };
        let current = claim_row.try_into_domain()?;
        if current.is_completed() {
            return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
        }
        check_claim_fence(&current, fence)?;
        let affected = sqlx::query(
            "UPDATE backup_maintenance_runs
             SET state = 'STALE', stale_at = clock_timestamp()
             WHERE id = $1 AND owner_user_id = $2
               AND state IN ('CREATED', 'SNAPSHOT_CAPTURED', 'EXPIRY_PLANNED')",
        )
        .bind(claim.maintenance_run_id().into_uuid())
        .bind(claim.owner_user_id().into_uuid())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(ScheduledMaintenanceWorkerError::InconsistentState);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Owner-scoped read of one claim receipt for audit and replay callers.
    pub async fn get_scheduled_maintenance_claim(
        &self,
        owner_user_id: UserId,
        claim_id: BackupScheduledMaintenanceClaimId,
    ) -> Result<Option<BackupScheduledMaintenanceClaim>, ScheduledMaintenanceWorkerError> {
        let row = sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
            "SELECT {CLAIM_COLUMNS} FROM backup_scheduled_maintenance_claims
              WHERE claim_id = $1 AND owner_user_id = $2"
        ))
        .bind(claim_id.into_uuid())
        .bind(owner_user_id.into_uuid())
        .fetch_optional(self.pool.sqlx_pool())
        .await?;
        row.map(ScheduledMaintenanceClaimRow::try_into_domain)
            .transpose()
    }
}

enum ExecutionPlan {
    AlreadyCompleted {
        claim: BackupScheduledMaintenanceClaim,
    },
    Recover {
        claim: BackupScheduledMaintenanceClaim,
        run: BackupMaintenanceRun,
    },
    Advance {
        claim: BackupScheduledMaintenanceClaim,
        run: BackupMaintenanceRun,
    },
}

/// The presented lease fence for one worker-step attempt. Bundling keeps the
/// fenced helpers within the repository argument bound while making every
/// fence verification take the same four values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClaimFence {
    worker_id: BackupScheduledMaintenanceWorkerId,
    lease_token: BackupScheduledMaintenanceLeaseToken,
    lease_generation: u64,
    observed_at_utc: Timestamp,
}

/// Whether the authoritative transition transaction committed the run-state
/// flip or found it already committed by a racing holder. Both outcomes
/// preserve exactly one semantic transition; the caller completes the single
/// shared receipt afterwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransitionOutcome {
    Committed,
    AlreadyAtResulting,
}

struct HandoffScope {
    schedule_id: BackupScheduleId,
    occurrence_id: BackupScheduleOccurrenceId,
}

fn check_claim_fence(
    claim: &BackupScheduledMaintenanceClaim,
    fence: ClaimFence,
) -> Result<(), ScheduledMaintenanceWorkerError> {
    if claim.lease_worker_id() != fence.worker_id
        || claim.lease_token() != fence.lease_token
        || claim.lease_generation() != fence.lease_generation
        || claim.lease_expires_at() <= fence.observed_at_utc
    {
        return Err(ScheduledMaintenanceWorkerError::LeaseLost);
    }
    Ok(())
}

/// A fenced transition that finds the run already at the claim's
/// predetermined resulting state commits nothing and reports the race: the
/// canonical child work replayed idempotently, so the caller reconciles the
/// shared receipt instead of advancing again. Any other mismatch fails
/// closed with zero maintenance state committed.
async fn map_transition_verify_error(
    transaction: &mut Transaction<'_, Postgres>,
    claim: &BackupScheduledMaintenanceClaim,
    error: ScheduledMaintenanceWorkerError,
) -> Result<TransitionOutcome, ScheduledMaintenanceWorkerError> {
    if !matches!(error, ScheduledMaintenanceWorkerError::InconsistentState) {
        return Err(error);
    }
    let Some(run_row) = load_maintenance_run_in_transaction(
        transaction,
        claim.owner_user_id(),
        claim.maintenance_run_id(),
        true,
    )
    .await?
    else {
        return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
    };
    let run = run_row
        .try_into_domain()
        .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
    if run.state() == claim.expected_resulting_state() {
        Ok(TransitionOutcome::AlreadyAtResulting)
    } else {
        Err(ScheduledMaintenanceWorkerError::InconsistentState)
    }
}

/// Re-verify the full fence plus the expected run state inside one
/// authoritative transition transaction. Any mismatch rolls back with zero
/// maintenance state committed. Canonical order is `runs -> claims` to
/// match `claim_candidate_step`.
async fn verify_fenced_run_state(
    transaction: &mut Transaction<'_, Postgres>,
    claim: &BackupScheduledMaintenanceClaim,
    fence: ClaimFence,
    expected_state: BackupMaintenanceRunState,
) -> Result<(), ScheduledMaintenanceWorkerError> {
    // Canonical parent before child: lock run first, then claim.
    let Some(run_row) = load_maintenance_run_in_transaction(
        transaction,
        claim.owner_user_id(),
        claim.maintenance_run_id(),
        true,
    )
    .await?
    else {
        return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
    };
    let run = run_row
        .try_into_domain()
        .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
    if run.state() != expected_state {
        return Err(ScheduledMaintenanceWorkerError::InconsistentState);
    }
    let Some(claim_row) = load_claim_for_update(transaction, claim.claim_id()).await? else {
        return Err(ScheduledMaintenanceWorkerError::NotFound);
    };
    let current = claim_row.try_into_domain()?;
    if current.is_completed() {
        // A concurrently sealed receipt is not a fence violation: the
        // mismatch mapper reconciles racers whose transition already
        // committed and fails closed otherwise.
        return Err(ScheduledMaintenanceWorkerError::InconsistentState);
    }
    check_claim_fence(&current, fence)?;
    // Re-validate after acquiring both locks to close the window between
    // run and claim acquisition.
    if run.state() != expected_state {
        return Err(ScheduledMaintenanceWorkerError::InconsistentState);
    }
    Ok(())
}

async fn load_claim_row(
    transaction: &mut Transaction<'_, Postgres>,
    claim_id: BackupScheduledMaintenanceClaimId,
) -> Result<Option<ScheduledMaintenanceClaimRow>, ScheduledMaintenanceWorkerError> {
    Ok(sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "SELECT {CLAIM_COLUMNS} FROM backup_scheduled_maintenance_claims WHERE claim_id = $1"
    ))
    .bind(claim_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn load_claim_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    claim_id: BackupScheduledMaintenanceClaimId,
) -> Result<Option<ScheduledMaintenanceClaimRow>, ScheduledMaintenanceWorkerError> {
    Ok(sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "SELECT {CLAIM_COLUMNS} FROM backup_scheduled_maintenance_claims
          WHERE claim_id = $1 FOR UPDATE"
    ))
    .bind(claim_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?)
}

async fn load_incomplete_claims_for_run(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: synveil_core::BackupMaintenanceRunId,
) -> Result<Vec<ScheduledMaintenanceClaimRow>, ScheduledMaintenanceWorkerError> {
    Ok(sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "SELECT {CLAIM_COLUMNS} FROM backup_scheduled_maintenance_claims
          WHERE maintenance_run_id = $1 AND completed_at IS NULL
          ORDER BY claim_id ASC FOR UPDATE"
    ))
    .bind(run_id.into_uuid())
    .fetch_all(&mut **transaction)
    .await?)
}

async fn load_claim_by_run_state(
    pool: &DatabasePool,
    owner_user_id: UserId,
    run_id: synveil_core::BackupMaintenanceRunId,
    expected_state: BackupMaintenanceRunState,
) -> Result<Option<BackupScheduledMaintenanceClaim>, ScheduledMaintenanceWorkerError> {
    let row = sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "SELECT {CLAIM_COLUMNS} FROM backup_scheduled_maintenance_claims
          WHERE maintenance_run_id = $1 AND expected_state = $2
            AND owner_user_id = $3"
    ))
    .bind(run_id.into_uuid())
    .bind(expected_state.as_str())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(pool.sqlx_pool())
    .await?;
    row.map(ScheduledMaintenanceClaimRow::try_into_domain)
        .transpose()
}

async fn load_handoff_scope(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    run_id: synveil_core::BackupMaintenanceRunId,
) -> Result<Option<HandoffScope>, ScheduledMaintenanceWorkerError> {
    struct ScopeRow {
        schedule_id: Uuid,
        occurrence_id: Uuid,
    }
    impl<'r> FromRow<'r, sqlx::postgres::PgRow> for ScopeRow {
        fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
            use sqlx::Row;
            Ok(Self {
                schedule_id: row.try_get("schedule_id")?,
                occurrence_id: row.try_get("occurrence_id")?,
            })
        }
    }
    let row = sqlx::query_as::<_, ScopeRow>(
        "SELECT schedule_id, occurrence_id
          FROM backup_schedule_occurrence_handoffs
          WHERE maintenance_run_id = $1 AND owner_user_id = $2",
    )
    .bind(run_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(|row| {
        Ok::<_, ScheduledMaintenanceWorkerError>(HandoffScope {
            schedule_id: BackupScheduleId::try_from_uuid(row.schedule_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?,
            occurrence_id: BackupScheduleOccurrenceId::try_from_uuid(row.occurrence_id)
                .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?,
        })
    })
    .transpose()
}

#[allow(clippy::too_many_arguments)]
async fn insert_claim_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    owner_user_id: UserId,
    backup_set_id: BackupSetId,
    schedule_id: BackupScheduleId,
    occurrence_id: BackupScheduleOccurrenceId,
    run_id: synveil_core::BackupMaintenanceRunId,
    expected_state: BackupMaintenanceRunState,
    worker_id: BackupScheduledMaintenanceWorkerId,
    observed_at_utc: Timestamp,
    lease_expires_at: Timestamp,
) -> Result<Option<BackupScheduledMaintenanceClaim>, ScheduledMaintenanceWorkerError> {
    if scheduled_maintenance_resulting_state(expected_state).is_none() {
        return Err(ScheduledMaintenanceWorkerError::InvalidPersistedData);
    }
    let claim_id = BackupScheduledMaintenanceClaimId::new();
    let lease_token = BackupScheduledMaintenanceLeaseToken::new();
    let row = sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "INSERT INTO backup_scheduled_maintenance_claims
            (claim_id, owner_user_id, backup_set_id, schedule_id, occurrence_id,
             maintenance_run_id, expected_state, resulting_state, lease_worker_id,
             lease_token, lease_generation, lease_acquired_at, lease_expires_at,
             completed_at, created_at, updated_at)
          VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, $9, 1, $10, $11, NULL,
                  clock_timestamp(), clock_timestamp())
          ON CONFLICT (maintenance_run_id, expected_state) DO NOTHING
          RETURNING {CLAIM_COLUMNS}"
    ))
    .bind(claim_id.into_uuid())
    .bind(owner_user_id.into_uuid())
    .bind(backup_set_id.into_uuid())
    .bind(schedule_id.into_uuid())
    .bind(occurrence_id.into_uuid())
    .bind(run_id.into_uuid())
    .bind(expected_state.as_str())
    .bind(worker_id.into_uuid())
    .bind(lease_token.into_uuid())
    .bind(observed_at_utc.as_offset_datetime())
    .bind(lease_expires_at.as_offset_datetime())
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(ScheduledMaintenanceClaimRow::try_into_domain)
        .transpose()
}

async fn takeover_claim_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    claim: &BackupScheduledMaintenanceClaim,
    worker_id: BackupScheduledMaintenanceWorkerId,
    observed_at_utc: Timestamp,
    lease_expires_at: Timestamp,
) -> Result<Option<BackupScheduledMaintenanceClaim>, ScheduledMaintenanceWorkerError> {
    let next_generation = claim
        .lease_generation()
        .checked_add(1)
        .ok_or(ScheduledMaintenanceWorkerError::InvalidPersistedData)?;
    let next_token = BackupScheduledMaintenanceLeaseToken::new();
    let row = sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "UPDATE backup_scheduled_maintenance_claims
         SET lease_worker_id = $2, lease_token = $3, lease_generation = $4,
             lease_acquired_at = $5, lease_expires_at = $6,
             updated_at = clock_timestamp()
         WHERE claim_id = $1 AND completed_at IS NULL
           AND lease_generation = $7 AND lease_token = $8
         RETURNING {CLAIM_COLUMNS}"
    ))
    .bind(claim.claim_id().into_uuid())
    .bind(worker_id.into_uuid())
    .bind(next_token.into_uuid())
    .bind(
        i64::try_from(next_generation)
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?,
    )
    .bind(observed_at_utc.as_offset_datetime())
    .bind(lease_expires_at.as_offset_datetime())
    .bind(
        i64::try_from(claim.lease_generation())
            .map_err(|_| ScheduledMaintenanceWorkerError::InvalidPersistedData)?,
    )
    .bind(claim.lease_token().into_uuid())
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(ScheduledMaintenanceClaimRow::try_into_domain)
        .transpose()
}

async fn complete_claim_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    claim: &BackupScheduledMaintenanceClaim,
    observed_at_utc: Timestamp,
) -> Result<Option<BackupScheduledMaintenanceClaim>, ScheduledMaintenanceWorkerError> {
    let resulting_state = claim.expected_resulting_state();
    let row = sqlx::query_as::<_, ScheduledMaintenanceClaimRow>(&format!(
        "UPDATE backup_scheduled_maintenance_claims
         SET completed_at = $2, resulting_state = $3,
             updated_at = clock_timestamp()
         WHERE claim_id = $1 AND completed_at IS NULL
         RETURNING {CLAIM_COLUMNS}"
    ))
    .bind(claim.claim_id().into_uuid())
    .bind(observed_at_utc.as_offset_datetime())
    .bind(resulting_state.as_str())
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(ScheduledMaintenanceClaimRow::try_into_domain)
        .transpose()
}
