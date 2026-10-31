//! Test-only durable reference. Nothing here is linked into the library.
use sqlx::{
    Connection, Row, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::{path::Path, time::Duration};
use synveil_pairing_contracts::PairingQrPayload;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Consumed,
    Issued,
    Cancelled,
    Denied,
    Expired,
}

// Deliberately bounded errors: never forward SQL/driver diagnostics or bindings.
#[derive(Debug, PartialEq, Eq)]
pub struct LedgerError;
impl From<sqlx::Error> for LedgerError {
    fn from(_: sqlx::Error) -> Self {
        Self
    }
}
impl From<serde_json::Error> for LedgerError {
    fn from(_: serde_json::Error) -> Self {
        Self
    }
}

pub async fn connect(path: &Path) -> Result<SqliteConnection, LedgerError> {
    Ok(SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(15)),
    )
    .await?)
}

pub async fn initialize(db: &mut SqliteConnection) -> Result<(), LedgerError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *db)
        .await?;
    if version != 0 {
        return Err(LedgerError);
    }
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::raw_sql(include_str!("schema_v1.sql"))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create(db: &mut SqliteConnection, qr: &PairingQrPayload) -> Result<(), LedgerError> {
    qr.validate().map_err(|_| LedgerError)?;
    i64::try_from(qr.policy_revision).map_err(|_| LedgerError)?;
    sqlx::query("INSERT INTO invitations(id,binding,state) VALUES (?,?,'CREATED')")
        .bind(&qr.invitation_id)
        .bind(serde_json::to_string(qr)?)
        .execute(db)
        .await?;
    Ok(())
}

async fn authorized(
    db: &mut SqliteConnection,
    qr: &PairingQrPayload,
    installation: &str,
) -> Result<bool, LedgerError> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM authority WHERE installation=? AND installation=? AND owner=? AND endpoint=? AND fingerprint=? AND policy=? AND active=1")
        .bind(installation).bind(&qr.installation_id).bind(&qr.issuer_owner_id)
        .bind(&qr.approved_endpoint).bind(&qr.server_identity_fingerprint)
        .bind(i64::try_from(qr.policy_revision).map_err(|_| LedgerError)?).fetch_one(db).await?;
    Ok(count == 1)
}

// Issue/cancel/redeem all acquire the writer fence BEFORE reading any authority.
// The harness clock is evaluated after that fence, as in production sve1_.
pub async fn transition(
    db: &mut SqliteConnection,
    id: &str,
    installation: &str,
    now: impl FnOnce() -> i64,
    cancel: bool,
) -> Result<Outcome, LedgerError> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query("SELECT binding,state FROM invitations WHERE id=?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Ok(Outcome::Denied);
    };
    let state: String = row.get("state");
    if state != "CREATED" && state != "ISSUED" {
        return Ok(Outcome::Denied);
    }
    let qr: PairingQrPayload = serde_json::from_str(row.get("binding"))?;
    if !authorized(&mut tx, &qr, installation).await? {
        return Ok(Outcome::Denied);
    }
    let now = now();
    let next = if cancel {
        "CANCELLED"
    } else if now >= qr.expires_at_unix {
        "EXPIRED"
    } else if now < qr.issued_at_unix || state != "CREATED" {
        return Ok(Outcome::Denied);
    } else {
        "ISSUED"
    };
    sqlx::query("UPDATE invitations SET state=? WHERE id=?")
        .bind(next)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(if next == "EXPIRED" {
        Outcome::Expired
    } else if next == "ISSUED" {
        Outcome::Issued
    } else {
        Outcome::Cancelled
    })
}

pub async fn redeem(
    db: &mut SqliteConnection,
    id: &str,
    installation: &str,
    device: &str,
    now: impl FnOnce() -> i64,
) -> Result<Outcome, LedgerError> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query("SELECT binding FROM invitations WHERE id=? AND state='ISSUED'")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Ok(Outcome::Denied);
    };
    let binding: String = row.get("binding");
    let qr: PairingQrPayload = serde_json::from_str(&binding)?;
    let now = now();
    if !authorized(&mut tx, &qr, installation).await? || device != qr.target_device_id {
        return Ok(Outcome::Denied);
    }
    if now < qr.issued_at_unix {
        return Ok(Outcome::Denied);
    }
    if now >= qr.expires_at_unix {
        sqlx::query("UPDATE invitations SET state='EXPIRED' WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Outcome::Expired);
    }
    let approved: i64 =
        sqlx::query_scalar("SELECT count(*) FROM approvals WHERE invitation=? AND binding=?")
            .bind(id)
            .bind(binding)
            .fetch_one(&mut *tx)
            .await?;
    if approved != 1 {
        return Ok(Outcome::Denied);
    }
    let changed = sqlx::query(
        "UPDATE invitations SET state='CONSUMED',verifier_id=id WHERE id=? AND state='ISSUED'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(LedgerError);
    }
    sqlx::query("INSERT INTO synthetic_verifiers VALUES (?,'NON_PRODUCTION_SYNTHETIC_V1')")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Outcome::Consumed)
}
