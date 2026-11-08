//! Test-private PostgreSQL adapter. This is never compiled into product code.
use sqlx::{Connection, PgConnection, PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use synveil_auth::authorization_reference::{rbac_v2::*, *};

pub struct Ledger {
    pub pool: PgPool,
    pub schema: String,
    pub url: String,
}
impl Ledger {
    pub async fn new() -> Self {
        Self::modified(&[]).await
    }
    pub async fn modified(replacements: &[(&str, &str)]) -> Self {
        let url = std::env::var("SYNVEIL_P027_DATABASE_URL")
            .expect("P027 requires an explicitly disposable PostgreSQL database");
        assert_eq!(std::env::var("SYNVEIL_P027_DISPOSABLE").as_deref(), Ok("1"));
        let schema = format!("p027_{}", uuid::Uuid::now_v7().simple());
        let mut admin = PgConnection::connect(&url).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&mut admin)
            .await
            .unwrap();
        admin.close().await.unwrap();
        let pool = Self::connect(&url, &schema).await;
        let mut sql = include_str!("p027.sql").to_string();
        for (from, to) in replacements {
            assert!(sql.contains(from), "fixture replacement missing: {from}");
            sql = sql.replace(from, to);
        }
        sqlx::raw_sql(&sql).execute(&pool).await.unwrap();
        Self { pool, schema, url }
    }
    pub async fn connect(url: &str, schema: &str) -> PgPool {
        assert!(schema.starts_with("p027_") && schema[5..].chars().all(|x| x.is_ascii_hexdigit()));
        let statement = format!("SET search_path TO {schema}, pg_catalog");
        PgPoolOptions::new()
            .max_connections(8)
            .after_connect(move |c, _| {
                let statement = statement.clone();
                Box::pin(async move {
                    sqlx::query(&statement).execute(c).await?;
                    Ok(())
                })
            })
            .connect(url)
            .await
            .unwrap()
    }
    pub async fn close(self) {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.url).await.unwrap();
        // Only our generated schema is ever dropped, with explicit disposable opt-in.
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&mut admin)
            .await
            .unwrap();
        admin.close().await.unwrap();
    }
    pub async fn scalar(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }
}
pub fn browser(id: &str) -> Principal {
    Principal::Browser { id: id.into() }
}
fn principal(id: String, device: Option<String>) -> Principal {
    match device {
        Some(device) => Principal::Device { id, device },
        None => browser(&id),
    }
}
fn action(value: &str) -> Action {
    match value {
        "read" => Action::Read,
        "write" => Action::Write,
        "delete" => Action::Delete,
        _ => Action::Unknown,
    }
}
fn scope(row: &sqlx::postgres::PgRow) -> Scope {
    Scope {
        owner: OwnerId(row.get("owner_id")),
        host: InstallationId(row.get("host")),
        resource: ResourceId(row.get("resource")),
        resource_type: if row.get::<String, _>("resource_type") == "library" {
            ResourceType::Library
        } else {
            ResourceType::Unknown
        },
        action: action(&row.get::<String, _>("action")),
        device: row.get("device"),
    }
}
#[derive(Clone, Debug)]
pub struct Request {
    /// Fixture authenticated session handle; not a bearer secret or public API.
    pub session: String,
    pub context: Context,
    pub grant: String,
    pub operation: String,
}
impl Request {
    pub fn normal() -> Self {
        Self {
            session: "session-bob".into(),
            grant: "child".into(),
            operation: "operation".into(),
            context: Context {
                principal: browser("bob"),
                authentication_method: "browser".into(),
                audience: "host".into(),
                binding: Binding {
                    owner: "owner-record".into(),
                    host: "host".into(),
                    resource_type: "library".into(),
                    resource_id: "lib".into(),
                    device: None,
                    action: "read".into(),
                },
                revision: 7,
                principal_revoked: false,
                device_revoked: false,
                approval_required: false,
            },
        }
    }
    fn payload(&self) -> String {
        // Length-prefixed fields avoid concatenation ambiguity. Reference-only format.
        [
            self.session.clone(),
            self.grant.clone(),
            format!("{:?}", self.context),
        ]
        .iter()
        .map(|x| format!("{}:{x}", x.len()))
        .collect()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Executed,
    Recovered,
    Rejected(Decision),
    IdentityConflict,
}
pub fn retryable(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|s| s == "40001" || s == "40P01")
}
pub async fn begin(pool: &PgPool) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await?;
    // Global reference fence is also a write: stale SERIALIZABLE snapshots fail
    // even when the conflicting authority change only inserts a tombstone.
    sqlx::query("UPDATE policy SET epoch=epoch+1 WHERE id=1")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    event: &str,
    id: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit(event,identity,detail) VALUES ($1,$2,$3)")
        .bind(event)
        .bind(id)
        .bind(detail)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn load(
    tx: &mut Transaction<'_, Postgres>,
    resource: &str,
) -> Result<Option<(Policy, bool)>, sqlx::Error> {
    let policy = sqlx::query("SELECT * FROM policy WHERE id=1 FOR UPDATE")
        .fetch_one(&mut **tx)
        .await?;
    let actors = sqlx::query("SELECT * FROM actors ORDER BY id FOR UPDATE")
        .fetch_all(&mut **tx)
        .await?;
    let Some(owner) = sqlx::query("SELECT * FROM ownership WHERE resource=$1 FOR UPDATE")
        .bind(resource)
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(None);
    };
    let ownership = scope(&owner);
    let actors: Vec<Actor> = actors
        .iter()
        .map(|r| Actor {
            principal: principal(r.get("id"), r.get("device")),
            human: r.get("human"),
            revoked: r.get("revoked"),
        })
        .collect();
    let resolve = |id: String| {
        actors
            .iter()
            .find(|a| match &a.principal {
                Principal::Browser { id: p } | Principal::Device { id: p, .. } => p == &id,
                _ => false,
            })
            .map(|a| a.principal.clone())
            .unwrap_or(Principal::Unknown)
    };
    let rows = sqlx::query("SELECT g.*, r.grant_id IS NOT NULL AS revoked FROM grants g LEFT JOIN revocations r ON g.id=r.grant_id ORDER BY g.id FOR UPDATE OF g").fetch_all(&mut **tx).await?;
    let records = rows
        .iter()
        .map(|r| Record {
            id: r.get("id"),
            recipient: resolve(r.get("recipient")),
            grantor: resolve(r.get("issuer")),
            scope: scope(r),
            role: match r.get::<String, _>("role").as_str() {
                "viewer" => Role::FixtureViewer,
                "editor" => Role::FixtureEditor,
                _ => Role::Unknown,
            },
            role_revision: RoleRevision(r.get::<i64, _>("role_revision") as u64),
            revision: PolicyRevision(r.get::<i64, _>("revision") as u64),
            issued: r.get::<i64, _>("issued") as u64,
            expires: r.get::<i64, _>("expires") as u64,
            revoked: r.get("revoked"),
            deny: r.get("deny"),
            parent: r.get("parent"),
            maximum: action(&r.get::<String, _>("maximum")),
        })
        .collect();
    let rows = sqlx::query("SELECT * FROM approvals ORDER BY id FOR UPDATE")
        .fetch_all(&mut **tx)
        .await?;
    let approvals = rows
        .iter()
        .map(|r| Approval {
            id: r.get("id"),
            operation: r.get("operation"),
            requester: resolve(r.get("requester")),
            approver: resolve(r.get("approver")),
            scope: scope(r),
            revision: PolicyRevision(r.get::<i64, _>("revision") as u64),
            issued: r.get::<i64, _>("issued") as u64,
            expires: r.get::<i64, _>("expires") as u64,
            withdrawn: r.get("withdrawn"),
            consumed: r.get::<Option<String>, _>("consumed_by").is_some(),
        })
        .collect();
    Ok(Some((
        Policy {
            version: policy.get::<i32, _>("version") as u32,
            revision: PolicyRevision(policy.get::<i64, _>("revision") as u64),
            role_revision: RoleRevision(policy.get::<i64, _>("role_revision") as u64),
            ownership: ownership.clone(),
            owner_actor: resolve(owner.get("owner_actor")),
            actors: actors.clone(),
            records,
            approvers: vec![(resolve(owner.get("approver")), ownership)],
            approval_actions: if policy.get("approval_required") {
                vec![Action::Read, Action::Write, Action::Delete]
            } else {
                vec![]
            },
            approvals,
            recursive: policy.get("recursive"),
            max_depth: 8,
        },
        owner.get("legacy_allow"),
    )))
}
pub async fn execute_in(
    tx: &mut Transaction<'_, Postgres>,
    r: &Request,
) -> Result<Outcome, sqlx::Error> {
    if r.operation.is_empty() {
        audit(
            tx,
            "denied",
            "empty-operation",
            "operation identity required",
        )
        .await?;
        return Ok(Outcome::Rejected(Decision::Denied));
    }
    let auth = sqlx::query("SELECT s.actor,s.revoked AS session_revoked,a.revoked,a.device FROM sessions s JOIN actors a ON s.actor=a.id WHERE s.id=$1 FOR UPDATE OF s").bind(&r.session).fetch_optional(&mut **tx).await?;
    let Some(auth) = auth else {
        audit(tx, "denied", &r.operation, "unknown session").await?;
        return Ok(Outcome::Rejected(Decision::Denied));
    };
    let actor: String = auth.get("actor");
    if auth.get::<bool, _>("session_revoked")
        || auth.get::<bool, _>("revoked")
        || principal(actor.clone(), auth.get("device")) != r.context.principal
    {
        audit(tx, "denied", &r.operation, "untrusted principal").await?;
        return Ok(Outcome::Rejected(Decision::Denied));
    }
    if let Some(previous) = sqlx::query("SELECT * FROM operations WHERE id=$1")
        .bind(&r.operation)
        .fetch_optional(&mut **tx)
        .await?
    {
        if previous.get::<String, _>("payload") != r.payload()
            || previous.get::<String, _>("actor") != actor
        {
            audit(tx, "denied", &r.operation, "idempotency payload conflict").await?;
            return Ok(Outcome::IdentityConflict);
        }
        audit(
            tx,
            "recovered",
            &r.operation,
            "status only; no new effect or approval",
        )
        .await?;
        return Ok(Outcome::Recovered);
    }
    let Some((p, legacy)) = load(tx, &r.context.binding.resource_id).await? else {
        audit(tx, "denied", &r.operation, "unknown resource").await?;
        return Ok(Outcome::Rejected(Decision::Denied));
    };
    // Fresh DB time AFTER authority locks; transaction_timestamp would be stale
    // after lock waits. No caller-supplied clock or approval requirement.
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&mut **tx)
            .await?;
    let d = qualify(&r.context, &p, &r.grant, &r.operation, now as u64, legacy);
    if d != Decision::Allowed {
        audit(tx, "denied", &r.operation, &format!("{d:?}")).await?;
        return Ok(Outcome::Rejected(d));
    }
    let approval = if p.approval_actions.contains(&p.ownership.action) {
        // qualify against exactly one candidate, rather than consuming a different
        // (possibly invalid) evidence row from the full policy's positive result.
        p.approvals
            .iter()
            .find(|a| {
                let mut one = p.clone();
                one.approvals = vec![(*a).clone()];
                qualify(&r.context, &one, &r.grant, &r.operation, now as u64, legacy)
                    == Decision::Allowed
            })
            .map(|a| a.id.clone())
    } else {
        None
    };
    if let Some(id) = &approval {
        let n=sqlx::query("UPDATE approvals SET consumed_by=$1 WHERE id=$2 AND consumed_by IS NULL AND NOT withdrawn AND issued<=$3 AND expires>$3 AND revision=$4")
            .bind(&r.operation).bind(id).bind(now).bind(p.revision.0 as i64).execute(&mut **tx).await?.rows_affected();
        if n != 1 {
            return Err(sqlx::Error::Protocol(
                "approval consumption invariant".into(),
            ));
        }
        audit(tx, "approval-consumed", id, &r.operation).await?;
    }
    sqlx::query("INSERT INTO operations(id,payload,actor,revision,grant_id,approval_id,outcome) VALUES ($1,$2,$3,$4,$5,$6,'synthetic-success')")
        .bind(&r.operation).bind(r.payload()).bind(actor).bind(p.revision.0 as i64).bind(&r.grant).bind(approval).execute(&mut **tx).await?;
    audit(
        tx,
        "executed",
        &r.operation,
        "P026 allowed; legacy allowed; synthetic database effect only",
    )
    .await?;
    sqlx::query("INSERT INTO outbox VALUES ($1,'synthetic-success')")
        .bind(&r.operation)
        .execute(&mut **tx)
        .await?;
    Ok(Outcome::Executed)
}
pub async fn execute(pool: &PgPool, r: &Request) -> Result<Outcome, sqlx::Error> {
    for attempt in 0..3 {
        let result = async {
            let mut tx = begin(pool).await?;
            let result = execute_in(&mut tx, r).await?;
            tx.commit().await?;
            Ok(result)
        }
        .await;
        match result {
            Err(e) if retryable(&e) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!("bounded loop returns on last attempt")
}
pub async fn revoke(pool: &PgPool, id: &str) -> Result<(), sqlx::Error> {
    for attempt in 0..3 {
        let result=async {
            let mut tx=begin(pool).await?;
            sqlx::query("SELECT id FROM grants ORDER BY id FOR UPDATE").fetch_all(&mut *tx).await?;
            sqlx::query("INSERT INTO revocations(grant_id,reason) VALUES ($1,'fixture revoke') ON CONFLICT DO NOTHING").bind(id).execute(&mut *tx).await?;
            audit(&mut tx,"revoked",id,"ancestor tombstone; no child rewrite").await?;tx.commit().await
        }.await;
        match result {
            Err(e) if retryable(&e) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!()
}
pub async fn delegate(pool: &PgPool, id: &str) -> Result<Decision, sqlx::Error> {
    for attempt in 0..3 {
        let result=async {
            let mut tx=begin(pool).await?;
            let (mut p,legacy)=load(&mut tx,"lib").await?.unwrap();
            let mut r=Request::normal();r.context.principal=browser("alice");r.grant="root".into();
            // Grant issuance checks parent capability; execution approval is a
            // distinct lifecycle, and cannot be reused as delegation authority.
            p.approval_actions.clear();
            let now:i64=sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint").fetch_one(&mut *tx).await?;
            let d=qualify(&r.context,&p,&r.grant,"delegation",now as u64,legacy);
            if d==Decision::Allowed {
                sqlx::query("INSERT INTO grants SELECT $1,'bob','alice','root',owner_id,host,resource,resource_type,action,device,role,revision,role_revision,issued,expires,deny,maximum FROM grants WHERE id='root'").bind(id).execute(&mut *tx).await?;
                audit(&mut tx,"delegated",id,"validated persisted parent").await?;
            } else {audit(&mut tx,"denied",id,"delegation rejected").await?;}
            tx.commit().await?; Ok(d)
        }.await;
        match result {
            Err(e) if retryable(&e) && attempt < 2 => continue,
            other => return other,
        }
    }
    unreachable!()
}
