//! Opt-in read-only Linux qualification, using existing approved profiles/keyring.
use serde::Deserialize;
use synveil_client::transport::{
    Endpoint, LocalDiscovery, Selection, SelectionPolicy, SynveilVerifier, discover_and_select,
    native::NativeDiscovery,
};
use synveil_client_sync::{LocalStateConfig, LocalStateStore, ReplicaScope};

use synveil_server_network::transport::Transport;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    state_db: std::path::PathBuf,
    policy: String,
    bindings: Vec<Binding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    profile_id: String,
    transport: String,
    peer_id: Option<String>,
    owner_user_id: String,
    device_id: String,
    library_id: String,
}
#[tokio::main]
async fn main() {
    if !cfg!(target_os = "linux") {
        eprintln!("UNSUPPORTED_NATIVE_PLATFORM");
        std::process::exit(2);
    }
    if run().await.is_err() {
        eprintln!("P018_NATIVE_QUALIFICATION_FAILED_OR_BLOCKED");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), ()> {
    // Input contains identifiers/configuration only. Credentials are never accepted
    // as command arguments, environment variables or files by this procedure.
    let path = std::env::args_os().nth(1).ok_or(())?;
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| ())?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 32768 {
        return Err(());
    }
    let config: Config = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let policy = match config.policy.as_str() {
        "LanOnly" => SelectionPolicy::LanOnly,
        "PreferLan" => SelectionPolicy::PreferLan,
        "PreferTailscale" => SelectionPolicy::PreferTailscale,
        "TailscaleOnly" => SelectionPolicy::TailscaleOnly,
        _ => return Err(()),
    };
    if config.bindings.is_empty() || config.bindings.len() > 8 || !config.state_db.is_file() {
        return Err(());
    }
    let platform = synveil_platform::current();
    let state = LocalStateStore::open(&LocalStateConfig::new(config.state_db))
        .await
        .map_err(|_| ())?;
    let mut verifier = SynveilVerifier::new();
    let mut endpoints: Vec<Endpoint> = Vec::new();
    let mut identities = Vec::new();
    for b in config.bindings {
        let transport = match b.transport.as_str() {
            "Lan" => Transport::Lan,
            "Tailscale" => Transport::Tailscale,
            _ => return Err(()),
        };
        let profile = state
            .server_profile(b.profile_id.parse().map_err(|_| ())?)
            .await
            .map_err(|_| ())?
            .ok_or(())?;
        identities.push(
            profile
                .managed_host_trust()
                .ok_or(())?
                .server_id()
                .to_owned(),
        );
        let credential = state
            .load_device_credential(
                b.profile_id.parse().map_err(|_| ())?,
                platform.secret_store(),
            )
            .await
            .map_err(|_| ())?
            .ok_or(())?;
        let e = verifier
            .add(
                profile,
                transport,
                b.peer_id,
                credential,
                ReplicaScope::new(
                    b.owner_user_id.parse().map_err(|_| ())?,
                    b.device_id.parse().map_err(|_| ())?,
                    b.library_id.parse().map_err(|_| ())?,
                ),
            )
            .map_err(|_| ())?;
        endpoints.push(e);
    }
    let before = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        NativeDiscovery.inspect(&endpoints),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let (tx, rx) = tokio::sync::watch::channel(false);
    let selection = discover_and_select(&NativeDiscovery, &verifier, &endpoints, policy, rx);
    tokio::pin!(selection);
    let result = tokio::select! {
        result = &mut selection => result,
        _ = tokio::signal::ctrl_c() => { tx.send_replace(true); selection.await }
    };
    match &result {
        Selection::Selected { transport, origin } => {
            let e = endpoints.iter().find(|e| e.origin() == origin).ok_or(())?;
            let evidence = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                NativeDiscovery.inspect(&endpoints),
            )
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
            if evidence != before {
                println!("P018_NATIVE_UNAVAILABLE reason=RouteChanged");
                return Err(());
            }
            let identity = &identities[endpoints
                .iter()
                .position(|candidate| candidate == e)
                .ok_or(())?];
            let route = evidence
                .routes
                .iter()
                .find(|r| r.destination == e.address())
                .ok_or(())?;
            // A further snapshot is observation only, never an authorization cache.
            println!(
                "P018_NATIVE_AUTHENTICATED transport={transport:?} origin={origin} server_installation={identity} interface={} source={} ",
                route.interface_id, route.source
            );
            Ok(())
        }
        Selection::Unavailable(reason) => {
            println!("P018_NATIVE_UNAVAILABLE reason={reason:?}");
            Err(())
        }
    }
}
