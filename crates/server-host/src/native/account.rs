use super::*;
use std::{net::IpAddr, time::Duration};
use synveil_auth::{AuthenticationService, PlaintextPassword};
use synveil_core::{LoginIdentifier, Timestamp, UserId};
use synveil_metadata::{
    AuthRepository, BootstrapState, DatabaseConfig, DatabasePool, MigrationRunner,
};

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Host metadata runtime unavailable")
}
fn bounded<T>(future: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    runtime()?.block_on(async {
        tokio::time::timeout(Duration::from_secs(90), future)
            .await
            .map_err(|_| "Host metadata operation timed out; inspect before retry")?
    })
}
async fn connect(owner: &NativeOwners) -> Result<DatabasePool> {
    let credential = owner
        .store
        .host_database_url()
        .map_err(|_| "protected database credential unavailable")?;
    let config = credential
        .with_secret(|value| DatabaseConfig::from_url(value.to_owned()))
        .map_err(|_| "invalid protected database credential")?;
    tokio::time::timeout(Duration::from_secs(15), DatabasePool::connect(&config))
        .await
        .map_err(|_| "database connection timed out")?
        .map_err(|_| "database unavailable")
}
pub fn database_evidence(owner: &NativeOwners) -> Result<(i32, bool, bool)> {
    bounded(async {
        let pool = connect(owner).await?;
        let result = async {
            let (major, address, listeners) = pool
                .host_database_boundary()
                .await
                .map_err(|_| "database identity unavailable")?;
            let private = |address: &str| {
                if address == "localhost" {
                    return true;
                }
                address.parse::<IpAddr>().is_ok_and(|ip| {
                    ip.is_loopback() || matches!(ip, IpAddr::V4(ip) if ip.is_private())
                })
            };
            let private_listener =
                private(&address) && listeners.split(',').all(|v| private(v.trim()));
            let current = MigrationRunner::new()
                .status(&pool)
                .await
                .map_err(|_| "migration status unavailable")?
                .is_current();
            Ok((major, private_listener, current))
        }
        .await;
        pool.close().await;
        result
    })
}
pub fn verify_owner(
    owner: &NativeOwners,
    login: String,
    password: String,
    create: bool,
) -> Result<()> {
    let password = zeroize::Zeroizing::new(password);
    if owner.authorization.is_some() {
        return Err("finish the reviewed infrastructure effects before account setup");
    }
    let (observation, _) = owner.observe()?;
    if !observation.api_ready
        || !observation.network_ready
        || !observation.migrations_current
        || !observation.worker_healthy
    {
        return Err("server infrastructure is not healthy enough for account setup");
    }
    let login = LoginIdentifier::new(login.clone(), login.trim().to_lowercase())
        .map_err(|_| "invalid owner login")?;
    bounded(async {
        let pool = connect(owner).await?;
        let result = async {
            let auth = AuthenticationService::with_default_password_config(&pool);
            let state = auth.bootstrap_state().await.map_err(|_| "bootstrap state needs repair")?;
            if create {
                if state != BootstrapState::Open { return Err("an administrator already exists; explicitly sign in to recover this Host"); }
                auth.create_first_admin(login.clone(), PlaintextPassword::new(password.to_string()).map_err(|_| "invalid owner password")?, Timestamp::now()).await.map_err(|_| "first administrator outcome uncertain; inspect and sign in, never replay account creation")?;
            } else if state != BootstrapState::Closed { return Err("create the first owner account before signing in"); }
            let session = auth.verify_first_admin_login(login, PlaintextPassword::new(password.to_string()).map_err(|_| "invalid owner password")?).await.map_err(|_| "owner login was not verified")?;
            let user = session.principal().user_id();
            let result = async {
                let library = AuthRepository::new(&pool).host_owner_library(user).await.map_err(|_| "initial Library verification unavailable")?.ok_or("initial Library root needs repair")?;
                let api = authenticated_api(owner, session.token(), "libraries").await?;
                if !api["data"].as_array().is_some_and(|libraries| libraries.iter().any(|item| item["id"].as_str() == Some(&library.id.to_string()) && item["attributes"]["root_node_id"].as_str() == Some(&library.root_node_id.to_string()))) { return Err("normal authenticated API did not verify the initial Library"); }
                authenticated_api(owner, session.token(), &format!("nodes/{}", library.root_node_id)).await?;
                // Auth owns the single first Library transaction. Only genuine
                // canonical Library/root evidence is persisted for recovery.
                let mut journal = owner.journal()?.ok_or("Host intent missing")?;
                journal.verified_admin = Some(user.to_string()); journal.library_id = Some(library.id.to_string()); owner.persist(&journal)
            }.await;
            let revoked = auth.revoke_current_session(session.token()).await.map_err(|_| "temporary owner session cleanup unavailable");
            result.and(revoked)
        }.await;
        pool.close().await;
        result
    })
}

async fn authenticated_api(
    owner: &NativeOwners,
    token: &synveil_auth::SessionToken,
    path: &str,
) -> Result<serde_json::Value> {
    let journal = owner.journal()?.ok_or("Host intent missing")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "authenticated API client unavailable")?;
    let cookie = zeroize::Zeroizing::new(format!("synveil_session={}", token.to_hex()));
    let mut response = client
        .get(format!("http://127.0.0.1:3000/api/v1/{path}"))
        .header("Cookie", cookie.as_str())
        .header(
            "Origin",
            journal.selection.canonical_origin.trim_end_matches('/'),
        )
        .send()
        .await
        .map_err(|_| "normal authenticated API unavailable")?;
    if !response.status().is_success()
        || response
            .headers()
            .get("x-synveil-server-installation")
            .and_then(|v| v.to_str().ok())
            != Some(journal.server_id.as_str())
        || response
            .headers()
            .get("x-synveil-network-integration")
            .and_then(|v| v.to_str().ok())
            != Some(journal.network_id.as_str())
    {
        return Err("authenticated API identity or ownership verification failed");
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "authenticated API response unavailable")?
    {
        if bytes.len() + chunk.len() > 128 * 1024 {
            return Err("authenticated API response exceeds bound");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid authenticated API resource")
}
pub fn enroll_desktop(owner: &NativeOwners, login: String, password: String) -> Result<String> {
    let password = zeroize::Zeroizing::new(password);
    let (observation, _) = owner.observe()?;
    if observation.derive_state() != synveil_server_bootstrap::HostSetupState::Ready {
        return Err("verify full Host readiness before desktop enrollment");
    }
    bounded(async {
        let pool = connect(owner).await?;
        let result = async {
            let login = LoginIdentifier::new(login.clone(), login.trim().to_lowercase()).map_err(|_| "invalid owner login")?;
            let auth = AuthenticationService::with_default_password_config(&pool);
            let session = auth.verify_first_admin_login(login, PlaintextPassword::new(password.to_string()).map_err(|_| "invalid password")?).await.map_err(|_| "explicit owner authentication required for enrollment")?;
            let result = async {
                let user = session.principal().user_id();
                let mut journal = owner.journal()?.ok_or("Host intent missing")?;
                if journal.verified_admin.as_deref() != Some(&user.to_string()) { return Err("enrollment owner differs from the verified Host owner"); }
                authenticated_api(owner, session.token(), "libraries").await?;
                let device = match &journal.client_device_id { Some(id) => id.parse::<synveil_core::DeviceId>().map_err(|_| "Host desktop device identity invalid")?, None => synveil_core::DeviceId::new() };
                journal.client_device_id = Some(device.to_string()); owner.persist(&journal)?;
                let existing = synveil_metadata::DomainRepository::new(&pool).find_device(device).await.map_err(|_| "desktop device inspection unavailable")?;
                let target = match existing {
                    Some(existing) if existing.owner_user_id() == user => synveil_auth::DeviceEnrollmentTarget::Existing(device),
                    Some(_) => return Err("desktop device ownership conflict"),
                    None => synveil_auth::DeviceEnrollmentTarget::HostNew { device_id: device, display_name: synveil_core::LogicalName::new("Synveil desktop on this Host").map_err(|_| "invalid desktop display name")? },
                };
                // Same canonical device/grant transaction and one-time exchange
                // as Connect. Raw grant never enters the journal or UI status.
                let grant = synveil_auth::DeviceAuthenticationService::new(&pool).create_grant(user, target).await.map_err(|_| "desktop grant outcome uncertain; inspect this owned device before explicit re-enrollment")?;
                Ok(grant.token.expose_secret().to_owned())
            }.await;
            let revoked = auth.revoke_current_session(session.token()).await.map_err(|_| "temporary owner session cleanup failed");
            match (result, revoked) { (Ok(token), Ok(())) => Ok(token), (Err(error), _) | (_, Err(error)) => Err(error) }
        }.await;
        pool.close().await;
        result
    })
}
pub fn observe(owner: &NativeOwners, observation: &mut OwnerObservation) -> Result<()> {
    if !observation.migrations_current {
        return Ok(());
    }
    let journal = owner.journal()?.ok_or("Host intent missing")?;
    bounded(async {
        let pool = match connect(owner).await {
            Ok(pool) => pool,
            Err(_) => return Ok(()),
        };
        let result = async {
            let repo = AuthRepository::new(&pool);
            observation.bootstrap_closed = repo
                .bootstrap_state()
                .await
                .map_err(|_| "bootstrap inspection failed")?
                == BootstrapState::Closed;
            if let Some(user) = journal.verified_admin.as_ref() {
                let user = UserId::parse_str(user)
                    .map_err(|_| "verified administrator receipt invalid")?;
                if let Some(library) = repo
                    .host_owner_library(user)
                    .await
                    .map_err(|_| "Library inspection failed")?
                {
                    observation.explicit_admin_login_verified = true;
                    observation.initial_library_root_coherent =
                        journal.library_id.as_deref() == Some(&library.id.to_string());
                }
            }
            Ok(())
        }
        .await;
        pool.close().await;
        result
    })
}
