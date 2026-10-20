#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fresh".to_owned());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => fail("native acceptance runtime could not start"),
    };
    if let Err(error) = runtime.block_on(run(&mode)) {
        fail(&error);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    fail("P036 native acceptance requires Linux");
}

#[cfg(target_os = "linux")]
fn fail(message: &str) -> ! {
    eprintln!("P036 native acceptance failed: {message}");
    std::process::exit(1)
}

#[cfg(target_os = "linux")]
async fn run(mode: &str) -> Result<(), String> {
    use std::{fs::File, io::Read, os::unix::fs::MetadataExt, path::PathBuf, time::Duration};
    use synveil_client::{
        BackgroundClientManager, BackgroundLaunchResult, DesktopController,
        DesktopControllerCommandResult, DesktopControllerConfig, ServerProfileId,
    };
    use synveil_server_bootstrap::{CanonicalOwners, HostSetupState, ServerBootstrapCoordinator};
    use synveil_server_host::{
        client::NativeHostOwners,
        protocol::{HostSelection, Request, Response},
        supported_platform,
    };
    use zeroize::Zeroizing;

    if std::env::var_os("SYNVEIL_P036_NATIVE_ACCEPTANCE")
        .as_deref()
        .is_none_or(|value| value != "1")
    {
        return Err("runner did not explicitly enable disposable native acceptance".to_owned());
    }
    let platform = supported_platform()
        .ok_or_else(|| "only Ubuntu 24.04 or Fedora 42 x86_64 is qualified".to_owned())?;
    let owner_uid = nix::unistd::geteuid();
    if owner_uid.is_root() {
        return Err("run as the disposable runner's unprivileged desktop account".to_owned());
    }

    let mut coordinator = ServerBootstrapCoordinator::new(NativeHostOwners::default());
    coordinator.begin_host();
    coordinator
        .owners_mut()
        .authorize()
        .map_err(|_| "Host helper authorization was unavailable".to_owned())?;

    match mode {
        "fresh" => {
            let selection = HostSelection::default();
            let context = coordinator
                .owners_mut()
                .prepare_review(selection, None)
                .map_err(|error| format!("Host review failed: {error}"))?;
            let binding = coordinator
                .review(&context.plan)
                .map_err(|error| format!("Host review became stale: {error:?}"))?;
            coordinator
                .owners_mut()
                .confirm_context(context.clone(), None)
                .map_err(|_| "Host review confirmation was rejected".to_owned())?;
            coordinator
                .confirm_with_progress(&context.plan, &binding, |_, _| {})
                .map_err(|error| {
                    format!("Host provisioning stopped for reconciliation: {error:?}")
                })?;

            let login = format!("p036-{}", uuid::Uuid::now_v7().simple());
            let mut random = Zeroizing::new(vec![0_u8; 32]);
            File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(&mut random))
                .map_err(|_| "ephemeral owner credential generation failed".to_owned())?;
            let password = Zeroizing::new(
                random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            );
            let account = coordinator.owners_mut().request(Request::Account {
                login: login.clone(),
                password: password.to_string(),
                create: true,
            });
            match account {
                Ok(Response::Complete) => {}
                Ok(Response::Error { .. }) | Err(_) => {
                    // Account creation can commit while its response is lost.
                    // Reconcile bootstrap state first; only an authoritative
                    // closed state permits sign-in with the same credential.
                    coordinator.owners_mut().authorize().map_err(|_| {
                        "Host helper could not be reopened for reconciliation".to_owned()
                    })?;
                    let state = inspect_bootstrap(coordinator.owners_mut())?;
                    if !state {
                        return Err(
                            "administrator creation outcome is unresolved; inspect before retry"
                                .to_owned(),
                        );
                    }
                    match coordinator.owners_mut().request(Request::Account {
                        login: login.clone(),
                        password: password.to_string(),
                        create: false,
                    }) {
                        Ok(Response::Complete) => {}
                        _ => return Err("existing administrator sign-in did not verify".to_owned()),
                    }
                }
                _ => return Err("unexpected administrator creation response".to_owned()),
            }

            let (descriptor, library_id) = match coordinator
                .owners_mut()
                .request(Request::Verify)
                .map_err(|_| {
                "Host readiness response was unavailable".to_owned()
            })? {
                Response::Ready {
                    descriptor,
                    library_id,
                } => (descriptor, library_id),
                Response::Error { .. } => {
                    return Err("Host did not report authoritative Ready".to_owned());
                }
                _ => return Err("unexpected Host readiness response".to_owned()),
            };
            if coordinator
                .status()
                .map_err(|_| "Host readiness inspection failed".to_owned())?
                != HostSetupState::Ready
            {
                return Err("Host owner observation is not Ready".to_owned());
            }

            let profile_id = ServerProfileId::new();
            let controller =
                DesktopController::new(DesktopControllerConfig::for_profile(profile_id));
            controller
                .start()
                .await
                .map_err(|_| "desktop controller failed to start".to_owned())?;
            let manager = BackgroundClientManager::for_profile(profile_id);
            let launch = tokio::time::timeout(Duration::from_secs(90), manager.ensure_running())
                .await
                .map_err(|_| "background client startup exceeded 90 seconds".to_owned())?;
            if !matches!(
                launch,
                BackgroundLaunchResult::StartedDirect
                    | BackgroundLaunchResult::StartedSupervised
                    | BackgroundLaunchResult::StartRequested
                    | BackgroundLaunchResult::AlreadyStarting
                    | BackgroundLaunchResult::AlreadyRunning
            ) {
                return Err("installed desktop background client did not start".to_owned());
            }
            wait_for_connection(&controller, Duration::from_secs(60)).await?;
            let configured = controller
                .configure_managed_host(profile_id, descriptor)
                .await
                .map_err(|_| "verified Host profile configuration failed".to_owned())?;
            if !matches!(
                configured,
                DesktopControllerCommandResult::ProfileConfigured
                    | DesktopControllerCommandResult::ProfileAlreadyConfigured
            ) {
                return Err("verified Host profile was not configured".to_owned());
            }
            let token = match coordinator
                .owners_mut()
                .request(Request::EnrollDesktop {
                    login: login.clone(),
                    password: password.to_string(),
                })
                .map_err(|_| {
                    "normal owner authentication for desktop enrollment failed".to_owned()
                })? {
                Response::Enrollment { token } => Zeroizing::new(token),
                _ => return Err("desktop enrollment was not issued".to_owned()),
            };
            if controller.authenticate(token.as_str()).await.map_err(|_| {
                "desktop enrollment delivery is unknown; reconcile its status".to_owned()
            })? != DesktopControllerCommandResult::Authenticated
            {
                return Err("desktop did not authenticate with the one-time enrollment".to_owned());
            }

            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .ok_or_else(|| "runner home directory is unavailable".to_owned())?;
            let client_folder = home.join(format!("Synveil P036 {}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&client_folder)
                .map_err(|_| "fresh desktop sync folder could not be created".to_owned())?;
            std::fs::write(
                client_folder.join("p036-preservation-fixture.txt"),
                b"Synveil P036 native first-sync preservation fixture.\n",
            )
            .map_err(|_| "native first-sync fixture could not be written".to_owned())?;
            let attached = controller
                .attach_host_library(
                    library_id.clone(),
                    client_folder.to_string_lossy().into_owned(),
                )
                .await
                .map_err(|_| "initial Host Library binding outcome is unknown".to_owned())?;
            if !matches!(
                attached,
                DesktopControllerCommandResult::LibraryConfigured
                    | DesktopControllerCommandResult::LibraryAlreadyConfigured
            ) {
                return Err("initial Host Library was not attached to the desktop".to_owned());
            }
            let library = library_id
                .parse::<synveil_client::LibraryId>()
                .map_err(|_| "Host returned an invalid Library identity".to_owned())?;
            match controller.sync_now(library).await {
                Ok(
                    DesktopControllerCommandResult::Accepted
                    | DesktopControllerCommandResult::Coalesced
                    | DesktopControllerCommandResult::AlreadyRunningFollowupRecorded
                    | DesktopControllerCommandResult::OutcomeUnknown,
                ) => {}
                _ => return Err(
                    "desktop did not accept or authoritatively reconcile the initial sync request"
                        .to_owned(),
                ),
            }
            wait_for_first_sync(&controller, &library_id, Duration::from_secs(300)).await?;
            let server_id = coordinator
                .owners_mut()
                .inspect()
                .map_err(|_| "Host identity inspection failed".to_owned())?
                .server_installation_id
                .ok_or_else(|| "Host server identity is unavailable".to_owned())?;
            let service_user = std::fs::metadata("/etc/synveil/host-setup.json")
                .map(|metadata| metadata.uid())
                .map_err(|_| "Host journal is not installed".to_owned())?;
            if service_user != 0 {
                return Err("Host journal is not root-owned".to_owned());
            }
            controller
                .stop()
                .await
                .map_err(|_| "desktop controller did not stop cleanly".to_owned())?;
            println!(
                "{{\"evidence\":\"PASS_NATIVE\",\"scenario\":\"fresh_host_first_sync\",\"journey\":\"production_host_coordinator_and_desktop_controller\",\"qml_surface\":\"NOT_EXECUTED\",\"first_sync_fixture\":\"p036-preservation-fixture.txt\",\"platform\":{},\"server_installation_id\":{},\"library_id\":{},\"first_sync_completed\":true}}",
                serde_json::to_string(&platform).unwrap_or_else(|_| "null".to_owned()),
                serde_json::to_string(&server_id).unwrap_or_else(|_| "null".to_owned()),
                serde_json::to_string(&library_id).unwrap_or_else(|_| "null".to_owned()),
            );
            Ok(())
        }
        "repair-existing" => {
            repair_existing(&mut coordinator)?;
            println!(
                "{{\"evidence\":\"PASS_NATIVE\",\"scenario\":\"preserving_host_repair\",\"platform\":{}}}",
                serde_json::to_string(&platform).unwrap_or_else(|_| "null".to_owned())
            );
            Ok(())
        }
        "remove-software-preserving" => {
            remove_software_preserving(&mut coordinator)?;
            println!(
                "{{\"evidence\":\"PASS_NATIVE\",\"scenario\":\"remove_hosted_software_preserving_data\",\"platform\":{}}}",
                serde_json::to_string(&platform).unwrap_or_else(|_| "null".to_owned())
            );
            Ok(())
        }
        "verify-existing" => {
            verify_existing(&mut coordinator)?;
            println!(
                "{{\"evidence\":\"PASS_NATIVE\",\"scenario\":\"host_readiness_reconciliation\",\"platform\":{}}}",
                serde_json::to_string(&platform).unwrap_or_else(|_| "null".to_owned())
            );
            Ok(())
        }
        _ => Err("mode must be fresh, repair-existing, or verify-existing".to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn inspect_bootstrap(
    owners: &mut synveil_server_host::client::NativeHostOwners,
) -> Result<bool, String> {
    use synveil_server_host::protocol::{Request, Response};
    match owners
        .request(Request::Inspect)
        .map_err(|_| "Host state could not be reconciled".to_owned())?
    {
        Response::Inspection { observation, .. } => Ok(observation.bootstrap_closed),
        _ => Err("Host inspection response was invalid".to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn verify_existing(
    coordinator: &mut synveil_server_bootstrap::ServerBootstrapCoordinator<
        synveil_server_host::client::NativeHostOwners,
    >,
) -> Result<(), String> {
    use synveil_server_bootstrap::{CanonicalOwners, HostSetupState};
    use synveil_server_host::protocol::{Request, Response};
    let observation = coordinator
        .owners_mut()
        .inspect()
        .map_err(|_| "Host owner state is unavailable".to_owned())?;
    if observation.derive_state() != HostSetupState::Ready {
        return Err("Host is not authoritatively Ready after service recovery".to_owned());
    }
    match coordinator
        .owners_mut()
        .request(Request::Verify)
        .map_err(|_| "Host verification response was unavailable".to_owned())?
    {
        Response::Ready { .. } => Ok(()),
        _ => Err("Host verify operation failed".to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn repair_existing(
    coordinator: &mut synveil_server_bootstrap::ServerBootstrapCoordinator<
        synveil_server_host::client::NativeHostOwners,
    >,
) -> Result<(), String> {
    use synveil_server_bootstrap::HostSetupState;
    use synveil_server_host::protocol::{HostPurpose, Request, Response};
    coordinator.begin_host();
    let context = match coordinator
        .owners_mut()
        .request(Request::ReviewLifecycle {
            purpose: HostPurpose::RepairHost,
        })
        .map_err(|_| "preserving Host repair review failed".to_owned())?
    {
        Response::Review { context } => context,
        _ => return Err("preserving Host repair was not reviewed".to_owned()),
    };
    let binding = coordinator
        .review(&context.plan)
        .map_err(|_| "preserving Host repair plan became stale".to_owned())?;
    coordinator
        .owners_mut()
        .confirm_context(context.clone(), None)
        .map_err(|_| "preserving Host repair confirmation was rejected".to_owned())?;
    coordinator
        .confirm_with_progress(&context.plan, &binding, |_, _| {})
        .map_err(|error| format!("preserving Host repair needs reconciliation: {error:?}"))?;
    if coordinator
        .status()
        .map_err(|_| "Host repair state could not be observed".to_owned())?
        != HostSetupState::Ready
    {
        return Err("Host is not Ready after preserving repair".to_owned());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_software_preserving(
    coordinator: &mut synveil_server_bootstrap::ServerBootstrapCoordinator<
        synveil_server_host::client::NativeHostOwners,
    >,
) -> Result<(), String> {
    use synveil_server_bootstrap::CanonicalOwners;
    use synveil_server_host::protocol::{HostPurpose, Request, Response};
    coordinator.begin_host();
    let context = match coordinator
        .owners_mut()
        .request(Request::ReviewLifecycle {
            purpose: HostPurpose::RemoveSoftware,
        })
        .map_err(|_| "data-preserving server removal review failed".to_owned())?
    {
        Response::Review { context } => context,
        _ => return Err("data-preserving server removal was not reviewed".to_owned()),
    };
    let binding = coordinator
        .review(&context.plan)
        .map_err(|_| "data-preserving removal plan became stale".to_owned())?;
    coordinator
        .owners_mut()
        .confirm_context(context.clone(), None)
        .map_err(|_| "data-preserving removal confirmation was rejected".to_owned())?;
    coordinator
        .confirm_with_progress(&context.plan, &binding, |_, _| {})
        .map_err(|error| format!("data-preserving removal needs reconciliation: {error:?}"))?;
    let observation = coordinator
        .owners_mut()
        .inspect()
        .map_err(|_| "Host could not be inspected after software removal".to_owned())?;
    if observation.api_ready || observation.network_ready || observation.worker_healthy {
        return Err("Host services remained ready after hosted software removal".to_owned());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn wait_for_connection(
    controller: &synveil_client::DesktopController,
    timeout: std::time::Duration,
) -> Result<(), String> {
    use synveil_client::DesktopControllerConnectionState;
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if controller.snapshot().connection_state == DesktopControllerConnectionState::Connected {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Err("desktop background client did not reach Connected".to_owned())
}

#[cfg(target_os = "linux")]
async fn wait_for_first_sync(
    controller: &synveil_client::DesktopController,
    library_id: &str,
    timeout: std::time::Duration,
) -> Result<(), String> {
    use synveil_client::{DesktopControllerAuthState, DesktopControllerRootState};
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        controller.refresh_state();
        let snapshot = controller.snapshot();
        if snapshot.profile_authenticated {
            if let Some(library) = snapshot
                .libraries
                .iter()
                .find(|library| library.library_id == library_id)
            {
                if library.first_sync_completed
                    && library.root_state == DesktopControllerRootState::Available
                    && library.auth_state == DesktopControllerAuthState::Ready
                {
                    return Ok(());
                }
                if library.auth_state != DesktopControllerAuthState::Ready
                    || library.root_state == DesktopControllerRootState::Unavailable
                {
                    return Err(
                        "first sync is blocked by authoritative client or root state".to_owned(),
                    );
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err("client did not report first_sync_completed within 300 seconds".to_owned())
}
