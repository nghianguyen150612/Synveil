//! PostgreSQL adapter used only by P029 qualification tests.
#[path = "p027.rs"]
#[allow(dead_code)]
pub mod p027;

use p027::{Ledger, Outcome as AuthorityOutcome, Request};
use sqlx::{PgPool, Postgres, Row, Transaction};
use synveil_auth::authorization_reference::{Decision, audit_v1::*};

fn protocol(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_owned())
}

fn event_type_tag(value: EventType) -> &'static str {
    match value {
        EventType::Authentication => "authentication",
        EventType::Authorization => "authorization",
        EventType::OwnershipChange => "ownership-change",
        EventType::RoleAssignment => "role-assignment",
        EventType::Delegation => "delegation",
        EventType::ApprovalIssued => "approval-issued",
        EventType::ApprovalConsumed => "approval-consumed",
        EventType::ApprovalRejected => "approval-rejected",
        EventType::GrantRevocation => "grant-revocation",
        EventType::CredentialRevocation => "credential-revocation",
        EventType::SessionRevocation => "session-revocation",
        EventType::DeviceRevocation => "device-revocation",
        EventType::PolicyRevision => "policy-revision",
        EventType::PrivilegedOperationRejected => "privileged-operation-rejected",
    }
}

fn operation_tag(value: Operation) -> &'static str {
    match value {
        Operation::Authenticate => "authenticate",
        Operation::Read => "read",
        Operation::Write => "write",
        Operation::Delete => "delete",
        Operation::ChangeOwner => "change-owner",
        Operation::AssignRole => "assign-role",
        Operation::Delegate => "delegate",
        Operation::Approve => "approve",
        Operation::Revoke => "revoke",
        Operation::ChangePolicy => "change-policy",
        Operation::Privileged => "privileged",
    }
}

fn outcome_tag(value: Outcome) -> &'static str {
    match value {
        Outcome::Allowed => "allowed",
        Outcome::Denied => "denied",
        Outcome::Committed => "committed",
        Outcome::Recovered => "recovered",
        Outcome::Rejected => "rejected",
    }
}

fn denial_tag(value: DenialCategory) -> &'static str {
    match value {
        DenialCategory::Unauthenticated => "unauthenticated",
        DenialCategory::Unauthorized => "unauthorized",
        DenialCategory::ForeignOrUnknownResource => "foreign-or-unknown-resource",
        DenialCategory::Revoked => "revoked",
        DenialCategory::Expired => "expired",
        DenialCategory::StalePolicy => "stale-policy",
        DenialCategory::ApprovalRequired => "approval-required",
        DenialCategory::ApprovalInvalid => "approval-invalid",
        DenialCategory::Unsupported => "unsupported",
        DenialCategory::AuditUnavailable => "audit-unavailable",
    }
}

fn parse_event_type(value: &str) -> Result<EventType, sqlx::Error> {
    match value {
        "authentication" => Ok(EventType::Authentication),
        "authorization" => Ok(EventType::Authorization),
        "ownership-change" => Ok(EventType::OwnershipChange),
        "role-assignment" => Ok(EventType::RoleAssignment),
        "delegation" => Ok(EventType::Delegation),
        "approval-issued" => Ok(EventType::ApprovalIssued),
        "approval-consumed" => Ok(EventType::ApprovalConsumed),
        "approval-rejected" => Ok(EventType::ApprovalRejected),
        "grant-revocation" => Ok(EventType::GrantRevocation),
        "credential-revocation" => Ok(EventType::CredentialRevocation),
        "session-revocation" => Ok(EventType::SessionRevocation),
        "device-revocation" => Ok(EventType::DeviceRevocation),
        "policy-revision" => Ok(EventType::PolicyRevision),
        "privileged-operation-rejected" => Ok(EventType::PrivilegedOperationRejected),
        _ => Err(protocol("unsupported P029 event type")),
    }
}

fn parse_operation(value: &str) -> Result<Operation, sqlx::Error> {
    match value {
        "authenticate" => Ok(Operation::Authenticate),
        "read" => Ok(Operation::Read),
        "write" => Ok(Operation::Write),
        "delete" => Ok(Operation::Delete),
        "change-owner" => Ok(Operation::ChangeOwner),
        "assign-role" => Ok(Operation::AssignRole),
        "delegate" => Ok(Operation::Delegate),
        "approve" => Ok(Operation::Approve),
        "revoke" => Ok(Operation::Revoke),
        "change-policy" => Ok(Operation::ChangePolicy),
        "privileged" => Ok(Operation::Privileged),
        _ => Err(protocol("unsupported P029 operation")),
    }
}

fn parse_outcome(value: &str) -> Result<Outcome, sqlx::Error> {
    match value {
        "allowed" => Ok(Outcome::Allowed),
        "denied" => Ok(Outcome::Denied),
        "committed" => Ok(Outcome::Committed),
        "recovered" => Ok(Outcome::Recovered),
        "rejected" => Ok(Outcome::Rejected),
        _ => Err(protocol("unsupported P029 outcome")),
    }
}

fn parse_denial(value: Option<String>) -> Result<Option<DenialCategory>, sqlx::Error> {
    value
        .map(|value| match value.as_str() {
            "unauthenticated" => Ok(DenialCategory::Unauthenticated),
            "unauthorized" => Ok(DenialCategory::Unauthorized),
            "foreign-or-unknown-resource" => Ok(DenialCategory::ForeignOrUnknownResource),
            "revoked" => Ok(DenialCategory::Revoked),
            "expired" => Ok(DenialCategory::Expired),
            "stale-policy" => Ok(DenialCategory::StalePolicy),
            "approval-required" => Ok(DenialCategory::ApprovalRequired),
            "approval-invalid" => Ok(DenialCategory::ApprovalInvalid),
            "unsupported" => Ok(DenialCategory::Unsupported),
            "audit-unavailable" => Ok(DenialCategory::AuditUnavailable),
            _ => Err(protocol("unsupported P029 denial category")),
        })
        .transpose()
}

fn principal_kind_tag(value: PrincipalKind) -> &'static str {
    match value {
        PrincipalKind::Browser => "browser",
        PrincipalKind::Device => "device",
        PrincipalKind::Service => "service",
        PrincipalKind::Unauthenticated => "unauthenticated",
        PrincipalKind::Unknown => "unknown",
    }
}

fn parse_principal_kind(value: &str) -> Result<PrincipalKind, sqlx::Error> {
    match value {
        "browser" => Ok(PrincipalKind::Browser),
        "device" => Ok(PrincipalKind::Device),
        "service" => Ok(PrincipalKind::Service),
        "unauthenticated" => Ok(PrincipalKind::Unauthenticated),
        "unknown" => Ok(PrincipalKind::Unknown),
        _ => Err(protocol("unsupported P029 principal kind")),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn append(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: &str,
    event_id: &str,
    event_type: EventType,
    actor_id: Option<&str>,
    resource_id: Option<&str>,
    operation: Operation,
    policy_revision: Option<u64>,
    outcome: Outcome,
    denial: Option<DenialCategory>,
    operation_id: &str,
    correlation_id: &str,
    related_reference: Option<&str>,
) -> Result<AuditRecord, sqlx::Error> {
    append_ordered(
        tx,
        owner_id,
        event_id,
        event_type,
        actor_id,
        resource_id,
        operation,
        policy_revision,
        outcome,
        denial,
        operation_id,
        correlation_id,
        related_reference,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn append_ordered(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: &str,
    event_id: &str,
    event_type: EventType,
    actor_id: Option<&str>,
    resource_id: Option<&str>,
    operation: Operation,
    policy_revision: Option<u64>,
    outcome: Outcome,
    denial: Option<DenialCategory>,
    operation_id: &str,
    correlation_id: &str,
    related_reference: Option<&str>,
    explicit_authority_order: Option<u64>,
) -> Result<AuditRecord, sqlx::Error> {
    let head = sqlx::query(
        "SELECT last_sequence,last_digest FROM p029_heads WHERE owner_id=$1 FOR UPDATE",
    )
    .bind(owner_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| protocol("P029 owner evidence scope is unavailable"))?;
    let sequence: i64 = head.get("last_sequence");
    let sequence = sequence
        .checked_add(1)
        .ok_or_else(|| protocol("P029 owner evidence sequence exhausted"))?;
    let previous_digest: String = head.get::<String, _>("last_digest").trim().to_owned();
    let observed_at_unix_ns: i64 =
        sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp()) * 1000000000)::bigint")
            .fetch_one(&mut **tx)
            .await?;
    let authority_order: i64 = match explicit_authority_order {
        Some(value) => i64::try_from(value)
            .map_err(|_| protocol("P027 ordering reference exceeds the supported range"))?,
        None => {
            sqlx::query_scalar("SELECT COALESCE(MAX(seq),0) FROM audit")
                .fetch_one(&mut **tx)
                .await?
        }
    };
    let (principal_kind, device_id) = if let Some(actor_id) = actor_id {
        let device: Option<String> = sqlx::query_scalar("SELECT device FROM actors WHERE id=$1")
            .bind(actor_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| {
                protocol("P029 actor reference is not trusted by the authority store")
            })?;
        match device {
            Some(device) => (PrincipalKind::Device, Some(device)),
            None => (PrincipalKind::Browser, None),
        }
    } else {
        (PrincipalKind::Unauthenticated, None)
    };
    let mut record = AuditRecord {
        schema_version: SCHEMA_VERSION,
        event_id: event_id.to_owned(),
        event_type,
        actor_id: actor_id.map(str::to_owned),
        principal_kind,
        device_id,
        owner_id: owner_id.to_owned(),
        resource_id: resource_id.map(str::to_owned),
        operation,
        policy_revision,
        outcome,
        denial,
        durable_sequence: sequence as u64,
        authority_order: authority_order as u64,
        operation_id: operation_id.to_owned(),
        correlation_id: correlation_id.to_owned(),
        observed_at_unix_ns,
        timestamp_provenance: TimestampProvenance::DatabaseClock,
        related_reference: related_reference.map(str::to_owned),
        previous_digest,
        digest: ZERO_DIGEST.to_owned(),
    };
    record
        .validate()
        .map_err(|_| protocol("invalid P029 audit record"))?;
    let previous = record.previous_digest.clone();
    record.seal(&previous);
    sqlx::query("INSERT INTO p029_events(owner_id,durable_sequence,authority_order,event_id,schema_version,event_type,actor_id,principal_kind,device_id,resource_id,operation,policy_revision,outcome,denial,operation_id,correlation_id,observed_at_unix_ns,timestamp_provenance,related_reference,previous_digest,digest) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,'database-clock',$18,$19,$20)")
        .bind(&record.owner_id)
        .bind(sequence)
        .bind(authority_order)
        .bind(&record.event_id)
        .bind(i16::try_from(record.schema_version).map_err(|_| protocol("invalid P029 version"))?)
        .bind(event_type_tag(record.event_type))
        .bind(&record.actor_id)
        .bind(principal_kind_tag(record.principal_kind))
        .bind(&record.device_id)
        .bind(&record.resource_id)
        .bind(operation_tag(record.operation))
        .bind(record.policy_revision.map(|v| v as i64))
        .bind(outcome_tag(record.outcome))
        .bind(record.denial.map(denial_tag))
        .bind(&record.operation_id)
        .bind(&record.correlation_id)
        .bind(record.observed_at_unix_ns)
        .bind(&record.related_reference)
        .bind(&record.previous_digest)
        .bind(&record.digest)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE p029_heads SET last_sequence=$2,last_digest=$3 WHERE owner_id=$1")
        .bind(owner_id)
        .bind(sequence)
        .bind(&record.digest)
        .execute(&mut **tx)
        .await?;
    Ok(record)
}

pub async fn read_complete(
    pool: &PgPool,
    trusted_owner_id: &str,
) -> Result<(Vec<AuditRecord>, Checkpoint), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let head = sqlx::query("SELECT last_sequence,last_digest FROM p029_heads WHERE owner_id=$1")
        .bind(trusted_owner_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| protocol("P029 owner evidence scope is unavailable"))?;
    let checkpoint = Checkpoint {
        owner_id: trusted_owner_id.to_owned(),
        record_count: head.get::<i64, _>("last_sequence") as u64,
        head_digest: head.get::<String, _>("last_digest").trim().to_owned(),
    };
    if checkpoint.record_count > MAX_VERIFIED_RECORDS {
        return Err(protocol("P029 evidence exceeds the bounded verifier limit"));
    }
    let rows = sqlx::query("SELECT * FROM p029_events WHERE owner_id=$1 ORDER BY durable_sequence")
        .bind(trusted_owner_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        if row.get::<i16, _>("schema_version") as u16 != SCHEMA_VERSION {
            return Err(protocol("unsupported P029 schema version"));
        }
        if row.get::<String, _>("timestamp_provenance") != "database-clock" {
            return Err(protocol("unsupported P029 timestamp provenance"));
        }
        records.push(AuditRecord {
            schema_version: SCHEMA_VERSION,
            event_id: row.get("event_id"),
            event_type: parse_event_type(&row.get::<String, _>("event_type"))?,
            actor_id: row.get("actor_id"),
            principal_kind: parse_principal_kind(&row.get::<String, _>("principal_kind"))?,
            device_id: row.get("device_id"),
            owner_id: row.get("owner_id"),
            resource_id: row.get("resource_id"),
            operation: parse_operation(&row.get::<String, _>("operation"))?,
            policy_revision: row
                .get::<Option<i64>, _>("policy_revision")
                .map(|v| v as u64),
            outcome: parse_outcome(&row.get::<String, _>("outcome"))?,
            denial: parse_denial(row.get("denial"))?,
            durable_sequence: row.get::<i64, _>("durable_sequence") as u64,
            authority_order: row.get::<i64, _>("authority_order") as u64,
            operation_id: row.get("operation_id"),
            correlation_id: row.get("correlation_id"),
            observed_at_unix_ns: row.get("observed_at_unix_ns"),
            timestamp_provenance: TimestampProvenance::DatabaseClock,
            related_reference: row.get("related_reference"),
            previous_digest: row.get::<String, _>("previous_digest").trim().to_owned(),
            digest: row.get::<String, _>("digest").trim().to_owned(),
        });
    }
    verify_complete(trusted_owner_id, &records, &checkpoint)
        .map_err(|_| protocol("P029 evidence is incomplete or inconsistent"))?;
    let inconsistent: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM (
         SELECT 1 FROM p029_events e WHERE e.owner_id=$1 AND (
          (e.event_type='authorization' AND e.outcome='committed' AND (
           NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='executed' AND a.identity=e.operation_id) OR
           NOT EXISTS (SELECT 1 FROM operations o JOIN grants g ON g.id=o.grant_id JOIN ownership own ON own.resource=g.resource WHERE o.id=e.operation_id AND o.actor=e.actor_id AND o.revision=e.policy_revision AND own.owner_id=e.owner_id AND own.resource=e.resource_id))) OR
          (e.event_type='authorization' AND e.outcome IN ('denied','rejected') AND NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='denied' AND a.identity=e.operation_id)) OR
          (e.event_type='authorization' AND e.outcome='recovered' AND NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='recovered' AND a.identity=e.operation_id)) OR
          (e.event_type='approval-consumed' AND NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='approval-consumed' AND a.identity=e.related_reference AND a.detail=e.operation_id)) OR
          (e.event_type='grant-revocation' AND NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='revoked' AND a.identity=e.related_reference)) OR
          (e.event_type='session-revocation' AND NOT EXISTS (SELECT 1 FROM audit a WHERE a.seq=e.authority_order AND a.event='session-revoked' AND e.related_reference='p027-audit-'||a.seq)) OR
          (e.event_type='device-revocation' AND NOT EXISTS (SELECT 1 FROM audit a JOIN actors act ON act.id=a.identity WHERE a.seq=e.authority_order AND a.event='device-revoked' AND act.device=e.related_reference))
         )
         UNION ALL
         SELECT 1 FROM operations o JOIN grants g ON g.id=o.grant_id JOIN ownership own ON own.resource=g.resource
          WHERE own.owner_id=$1 AND (NOT EXISTS (SELECT 1 FROM p029_events e WHERE e.owner_id=$1 AND e.event_type='authorization' AND e.outcome='committed' AND e.operation_id=o.id)
           OR (o.approval_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM p029_events e WHERE e.owner_id=$1 AND e.event_type='approval-consumed' AND e.outcome='committed' AND e.operation_id=o.id AND e.related_reference=o.approval_id)))
         UNION ALL
         SELECT 1 FROM audit a WHERE a.event IN ('executed','recovered','denied','approval-consumed','revoked','session-revoked','device-revoked')
          AND NOT EXISTS (SELECT 1 FROM p029_events e WHERE e.owner_id=$1 AND e.authority_order=a.seq AND (
           (a.event='executed' AND e.event_type='authorization' AND e.outcome='committed' AND e.operation_id=a.identity) OR
           (a.event='recovered' AND e.event_type='authorization' AND e.outcome='recovered' AND e.operation_id=a.identity) OR
           (a.event='denied' AND e.event_type='authorization' AND e.outcome IN ('denied','rejected') AND e.operation_id=a.identity) OR
           (a.event='approval-consumed' AND e.event_type='approval-consumed' AND e.related_reference=a.identity AND e.operation_id=a.detail) OR
           (a.event='revoked' AND e.event_type='grant-revocation' AND e.related_reference=a.identity) OR
           (a.event='session-revoked' AND e.event_type='session-revocation' AND e.related_reference='p027-audit-'||a.seq) OR
           (a.event='device-revoked' AND e.event_type='device-revocation' AND EXISTS (SELECT 1 FROM actors act WHERE act.id=a.identity AND act.device=e.related_reference))
          ))
         ) AS invalid_evidence"#,
    )
    .bind(trusted_owner_id)
    .fetch_one(&mut *tx)
    .await?;
    if inconsistent != 0 {
        return Err(protocol(
            "P029 evidence does not match the P027 committed authority history",
        ));
    }
    tx.commit().await?;
    Ok((records, checkpoint))
}

pub async fn revoke_session_and_audit(pool: &PgPool, session_id: &str) -> Result<(), sqlx::Error> {
    for attempt in 0..3 {
        let result = async {
            let mut tx = p027::begin(pool).await?;
            let changed =
                sqlx::query("UPDATE sessions SET revoked=true WHERE id=$1 AND NOT revoked")
                    .bind(session_id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            if changed == 1 {
                p027::audit(
                    &mut tx,
                    "session-revoked",
                    session_id,
                    "P029 session revoke",
                )
                .await?;
                let authority_order: i64 = sqlx::query_scalar(
                    "SELECT seq FROM audit WHERE event='session-revoked' AND identity=$1 ORDER BY seq DESC LIMIT 1",
                )
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;
                let owner: String =
                    sqlx::query_scalar("SELECT owner_id FROM ownership WHERE resource='lib'")
                        .fetch_one(&mut *tx)
                        .await?;
                let revision: i64 = sqlx::query_scalar("SELECT revision FROM policy WHERE id=1")
                    .fetch_one(&mut *tx)
                    .await?;
                let reference = format!("p027-audit-{authority_order}");
                let operation_id = format!("revoke-session-{}", uuid::Uuid::now_v7().simple());
                append_ordered(
                    &mut tx,
                    &owner,
                    &format!("session-revoked-{}", uuid::Uuid::now_v7().simple()),
                    EventType::SessionRevocation,
                    Some("owner-actor"),
                    None,
                    Operation::Revoke,
                    Some(revision as u64),
                    Outcome::Committed,
                    None,
                    &operation_id,
                    &operation_id,
                    Some(&reference),
                    Some(authority_order as u64),
                )
                .await?;
            }
            tx.commit().await
        }
        .await;
        match result {
            Err(error) if p027::retryable(&error) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!("bounded P029 session revocation retry loop returns on the last attempt")
}

pub async fn revoke_device_and_audit(pool: &PgPool, actor_id: &str) -> Result<(), sqlx::Error> {
    for attempt in 0..3 {
        let result = async {
            let mut tx = p027::begin(pool).await?;
            let device: Option<String> = sqlx::query_scalar(
                "SELECT device FROM actors WHERE id=$1 AND device IS NOT NULL FOR UPDATE",
            )
            .bind(actor_id)
            .fetch_optional(&mut *tx)
            .await?;
            let device = device.ok_or_else(|| protocol("P029 device scope is unavailable"))?;
            let changed = sqlx::query("UPDATE actors SET revoked=true WHERE id=$1 AND NOT revoked")
                .bind(actor_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if changed == 1 {
                p027::audit(&mut tx, "device-revoked", actor_id, "P029 device revoke").await?;
                let owner: String =
                    sqlx::query_scalar("SELECT owner_id FROM ownership WHERE resource='lib'")
                        .fetch_one(&mut *tx)
                        .await?;
                let revision: i64 = sqlx::query_scalar("SELECT revision FROM policy WHERE id=1")
                    .fetch_one(&mut *tx)
                    .await?;
                append(
                    &mut tx,
                    &owner,
                    &format!("device-revoked-{}", uuid::Uuid::now_v7().simple()),
                    EventType::DeviceRevocation,
                    Some("owner-actor"),
                    None,
                    Operation::Revoke,
                    Some(revision as u64),
                    Outcome::Committed,
                    None,
                    &format!("revoke-{device}"),
                    &format!("revoke-{device}"),
                    Some(&device),
                )
                .await?;
            }
            tx.commit().await
        }
        .await;
        match result {
            Err(error) if p027::retryable(&error) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!("bounded P029 device revocation retry loop returns on the last attempt")
}

pub async fn read_owner_page(
    pool: &PgPool,
    trusted_owner_id: &str,
    requested_owner_id: &str,
    after_sequence: u64,
    limit: usize,
) -> Result<AuditPage, VerifyError> {
    if trusted_owner_id != requested_owner_id {
        return Err(VerifyError::ScopeMismatch);
    }
    let (records, checkpoint) = read_complete(pool, trusted_owner_id)
        .await
        .map_err(|_| VerifyError::IncompleteEvidence)?;
    query_page(
        trusted_owner_id,
        &records,
        &checkpoint,
        after_sequence,
        limit,
    )
}

pub async fn execute_and_audit(
    pool: &PgPool,
    request: &Request,
) -> Result<AuthorityOutcome, sqlx::Error> {
    for attempt in 0..3 {
        let result = async {
            let mut tx = p027::begin(pool).await?;
            let outcome = p027::execute_in(&mut tx, request).await?;
            match outcome {
                AuthorityOutcome::Executed => {
                    let row = sqlx::query("SELECT o.actor,o.revision,own.owner_id,own.resource,own.action,o.approval_id FROM operations o JOIN grants g ON g.id=o.grant_id JOIN ownership own ON own.resource=g.resource WHERE o.id=$1")
                        .bind(&request.operation).fetch_one(&mut *tx).await?;
                    let owner: String = row.get("owner_id");
                    let actor: String = row.get("actor");
                    let resource: String = row.get("resource");
                    let action: String = row.get("action");
                    let operation = parse_operation(&action)?;
                    let revision = row.get::<i64, _>("revision") as u64;
                    let approval: Option<String> = row.get("approval_id");
                    if let Some(approval_id) = approval.as_deref() {
                        let authority_order: i64 = sqlx::query_scalar("SELECT seq FROM audit WHERE event='approval-consumed' AND identity=$1 AND detail=$2 ORDER BY seq DESC LIMIT 1")
                            .bind(approval_id).bind(&request.operation).fetch_one(&mut *tx).await?;
                        append_ordered(
                            &mut tx,
                            &owner,
                            &format!("approval-consumed-{}", uuid::Uuid::now_v7().simple()),
                            EventType::ApprovalConsumed,
                            Some(&actor),
                            Some(&resource),
                            Operation::Approve,
                            Some(revision),
                            Outcome::Committed,
                            None,
                            &request.operation,
                            &request.operation,
                            Some(approval_id),
                            Some(authority_order as u64),
                        ).await?;
                    }
                    append(
                        &mut tx,
                        &owner,
                        &format!("event-{}", uuid::Uuid::now_v7().simple()),
                        EventType::Authorization,
                        Some(&actor),
                        Some(&resource),
                        operation,
                        Some(revision),
                        Outcome::Committed,
                        None,
                        &request.operation,
                        &request.operation,
                        approval.as_deref(),
                    ).await?;
                }
                AuthorityOutcome::Rejected(decision) => {
                    let auth = sqlx::query("SELECT s.actor,s.revoked AS session_revoked,a.revoked AS actor_revoked FROM sessions s JOIN actors a ON a.id=s.actor WHERE s.id=$1")
                        .bind(&request.session).fetch_optional(&mut *tx).await?;
                    let actor: Option<String> = auth.as_ref().map(|row| row.get("actor"));
                    let revoked = auth.as_ref().is_some_and(|row| {
                        row.get::<bool, _>("session_revoked") || row.get::<bool, _>("actor_revoked")
                    });
                    let category = if auth.is_none() {
                        DenialCategory::Unauthenticated
                    } else if revoked {
                        DenialCategory::Revoked
                    } else {
                        match decision {
                            Decision::Allowed => return Err(protocol("allowed decision reported rejected")),
                            Decision::Denied => DenialCategory::Unauthorized,
                            Decision::RequiresApproval => DenialCategory::ApprovalRequired,
                            Decision::Unsupported => DenialCategory::Unsupported,
                            Decision::StaleOrRevoked => DenialCategory::StalePolicy,
                        }
                    };
                    let owner: String = sqlx::query_scalar("SELECT owner_id FROM ownership WHERE resource='lib'")
                        .fetch_one(&mut *tx).await?;
                    let revision: u64 = sqlx::query_scalar::<_, i64>("SELECT revision FROM policy WHERE id=1")
                        .fetch_one(&mut *tx).await? as u64;
                    append(
                        &mut tx,
                        &owner,
                        &format!("denied-{}", uuid::Uuid::now_v7().simple()),
                        EventType::Authorization,
                        actor.as_deref(),
                        None,
                        Operation::Read,
                        Some(revision),
                        Outcome::Denied,
                        Some(category),
                        &request.operation,
                        &request.operation,
                        None,
                    ).await?;
                }
                AuthorityOutcome::Recovered => {
                    let row = sqlx::query("SELECT o.actor,o.revision,own.owner_id,own.resource,own.action FROM operations o JOIN grants g ON g.id=o.grant_id JOIN ownership own ON own.resource=g.resource WHERE o.id=$1")
                        .bind(&request.operation).fetch_one(&mut *tx).await?;
                    let owner: String = row.get("owner_id");
                    let actor: String = row.get("actor");
                    let resource: String = row.get("resource");
                    let operation = parse_operation(&row.get::<String, _>("action"))?;
                    append(
                        &mut tx,
                        &owner,
                        &format!("recovered-{}", uuid::Uuid::now_v7().simple()),
                        EventType::Authorization,
                        Some(&actor),
                        Some(&resource),
                        operation,
                        Some(row.get::<i64, _>("revision") as u64),
                        Outcome::Recovered,
                        None,
                        &request.operation,
                        &request.operation,
                        Some(&request.operation),
                    ).await?;
                }
                AuthorityOutcome::IdentityConflict => {
                    let actor: Option<String> = sqlx::query_scalar("SELECT actor FROM sessions WHERE id=$1")
                        .bind(&request.session).fetch_optional(&mut *tx).await?;
                    let owner: String = sqlx::query_scalar("SELECT owner_id FROM ownership WHERE resource='lib'")
                        .fetch_one(&mut *tx).await?;
                    let revision: u64 = sqlx::query_scalar::<_, i64>("SELECT revision FROM policy WHERE id=1")
                        .fetch_one(&mut *tx).await? as u64;
                    append(
                        &mut tx,
                        &owner,
                        &format!("conflict-{}", uuid::Uuid::now_v7().simple()),
                        EventType::Authorization,
                        actor.as_deref(),
                        None,
                        Operation::Read,
                        Some(revision),
                        Outcome::Rejected,
                        Some(DenialCategory::Unauthorized),
                        &request.operation,
                        &request.operation,
                        None,
                    ).await?;
                }
            }
            tx.commit().await?;
            Ok(outcome)
        }.await;
        match result {
            Err(error) if p027::retryable(&error) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!("bounded P029 retry loop returns on the last attempt")
}

pub async fn revoke_and_audit(pool: &PgPool, grant_id: &str) -> Result<(), sqlx::Error> {
    for attempt in 0..3 {
        let result = async {
            let mut tx = p027::begin(pool).await?;
            sqlx::query("SELECT id FROM grants ORDER BY id FOR UPDATE")
                .fetch_all(&mut *tx)
                .await?;
            let changed = sqlx::query("INSERT INTO revocations(grant_id,reason) VALUES ($1,'P029 qualified revocation') ON CONFLICT DO NOTHING")
                .bind(grant_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if changed == 1 {
                p027::audit(&mut tx, "revoked", grant_id, "P029 ordered grant revocation").await?;
                let trusted: sqlx::postgres::PgRow = sqlx::query("SELECT g.issuer,o.owner_id,g.resource,p.revision FROM grants g JOIN ownership o ON o.resource=g.resource CROSS JOIN policy p WHERE g.id=$1 AND p.id=1")
                    .bind(grant_id).fetch_one(&mut *tx).await?;
                let owner: String = trusted.get("owner_id");
                let resource: String = trusted.get("resource");
                let revision: u64 = trusted.get::<i64, _>("revision") as u64;
                append(
                    &mut tx,
                    &owner,
                    &format!("revoked-{}", uuid::Uuid::now_v7().simple()),
                    EventType::GrantRevocation,
                    Some("owner-actor"),
                    Some(&resource),
                    Operation::Revoke,
                    Some(revision),
                    Outcome::Committed,
                    None,
                    &format!("revoke-{grant_id}"),
                    &format!("revoke-{grant_id}"),
                    Some(grant_id),
                ).await?;
            }
            tx.commit().await
        }.await;
        match result {
            Err(error) if p027::retryable(&error) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!("bounded P029 revocation retry loop returns on the last attempt")
}

pub async fn new_fixture() -> Ledger {
    Ledger::new_p029().await
}
