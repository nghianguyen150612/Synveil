#![cfg(target_os = "linux")]

//! Prompt 99 LIVE-LAUNCH1--LIVE-LAUNCH10 acceptance.
//!
//! This ignored Linux-session target launches the repository's real
//! synveil-desktop and synveil-client binaries against one fresh SQLite,
//! profile, and managed-root fixture. It does not need PostgreSQL: the
//! fixture has no enrollment, so the real client reaches a safe
//! authentication-blocked state while its control endpoint, lifecycle, writer
//! lock, and controller-facing status remain live.
//!
//! The supervisor phase links one uniquely named unit fragment through the
//! production fixed user-unit name. The alias is created only after proving
//! that no user-owned Synveil unit or alias exists, and is removed on every
//! exit path. The fragment and all runtime state are disposable.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use sqlx::sqlite::SqlitePoolOptions;
use synveil_client::{
    ControlCapability, ControlClientHello, ControlServerHello, DESKTOP_CONTROL_PROTOCOL_VERSION,
    DesktopControlClient, DesktopControlEndpoint, DesktopProcessStatus, LINUX_USER_SERVICE_NAME,
    read_frame, write_frame,
};
use synveil_client_sync::{
    CanonicalBaseUrl, FilesystemLocalReplica, LocalReplica, LocalStateConfig, LocalStateStore,
    ReplicaScope, ServerProfile,
};
use synveil_core::{DeviceId, LibraryId, UserId};
use tokio::{
    net::UnixListener,
    task::JoinHandle,
    time::{sleep, timeout},
};
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(50);

struct EnvironmentGuard {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl EnvironmentGuard {
    fn set(values: &[(&'static str, &Path)]) -> Self {
        let previous = values
            .iter()
            .map(|(key, value)| {
                let previous = std::env::var_os(key);
                // This target is ignored and always run serially. The variables
                // are scoped to the disposable fixture and restored in Drop.
                unsafe {
                    std::env::set_var(key, value);
                }
                (*key, previous)
            })
            .collect();
        Self { previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.iter().rev() {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

struct ProcessFixture {
    root: PathBuf,
    data_dir: PathBuf,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    runtime_dir: PathBuf,
    config_file: PathBuf,
    state_file: PathBuf,
    profile_id: synveil_client_sync::ServerProfileId,
    endpoint: Option<DesktopControlEndpoint>,
    client_binary: PathBuf,
    desktop_binary: PathBuf,
}

impl ProcessFixture {
    async fn create() -> Self {
        let root = std::env::temp_dir().join(format!("sv99-{}", Uuid::now_v7().simple()));
        let data_dir = root.join("data");
        let config_dir = root.join("config");
        let cache_dir = root.join("cache");
        let runtime_dir = root.join("runtime");
        let config_file = config_dir.join("client.conf");
        let state_file = data_dir.join("client-sync/state.sqlite3");
        let library_root = root.join("library");
        fs::create_dir_all(&data_dir).expect("live data directory must be created");
        fs::create_dir_all(&config_dir).expect("live config directory must be created");
        fs::create_dir_all(&cache_dir).expect("live cache directory must be created");
        fs::create_dir(&runtime_dir).expect("live runtime directory must be created");
        fs::create_dir(&library_root).expect("live managed root must be created");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("live fixture root must be private");
        fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
            .expect("live runtime directory must be private");

        let profile = ServerProfile::new(
            CanonicalBaseUrl::parse_for_loopback_test("http://127.0.0.1:1/")
                .expect("loopback test origin"),
            "Prompt 99 live launch fixture",
        )
        .expect("live profile must build");
        let profile_id = profile.profile_id();
        let library_id = LibraryId::new();
        let scope = ReplicaScope::new(UserId::new(), DeviceId::new(), library_id);
        let replica =
            FilesystemLocalReplica::initialize_for_profile(&library_root, scope, profile_id)
                .expect("live managed root must initialize");
        let binding_id = replica.binding_id();
        drop(replica);

        let state = LocalStateStore::open(&LocalStateConfig::new(&state_file))
            .await
            .expect("live SQLite state must open");
        state
            .save_server_profile(&profile)
            .await
            .expect("live non-secret profile must persist");
        state.close_pool().await;
        drop(state);

        // The normal public binding method requires an enrollment. This live
        // target proves the process/control boundary with a profile-bound,
        // credential-less fixture, so seed only the existing non-secret
        // replica registry row through the same SQLite schema used by the
        // production process.
        let sqlite = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}", state_file.display()))
            .await
            .expect("live SQLite inspection connection must open");
        sqlx::query(
            "INSERT INTO replicas (
                 library_id, owner_user_id, device_id, root_binding_id,
                 created_at_ms, updated_at_ms, server_profile_id
             ) VALUES (?, ?, ?, ?, 0, 0, ?)",
        )
        .bind(library_id.to_string())
        .bind(scope.owner_user_id().to_string())
        .bind(scope.device_id().to_string())
        .bind(binding_id.to_string())
        .bind(profile_id.to_string())
        .execute(&sqlite)
        .await
        .expect("live replica registry row must persist");
        sqlx::query(
            "INSERT INTO observation_state (library_id, updated_at_ms)
             VALUES (?, 0)",
        )
        .bind(library_id.to_string())
        .execute(&sqlite)
        .await
        .expect("live observation state row must persist");
        sqlite.close().await;

        fs::write(
            &config_file,
            format!(
                "profile_id={profile_id}\nlibrary.{library_id}={}\n",
                library_root.display()
            ),
        )
        .expect("live client manifest must persist");
        fs::set_permissions(&config_file, fs::Permissions::from_mode(0o600))
            .expect("live client manifest must be private");

        let client_binary = binary_path("synveil-client", "SYNVEIL_CLIENT_BIN");
        let desktop_binary = binary_path("synveil-desktop", "SYNVEIL_DESKTOP_BIN");
        assert!(
            client_binary.is_file(),
            "real synveil-client binary is required: {}",
            client_binary.display()
        );
        assert!(
            desktop_binary.is_file(),
            "real synveil-desktop binary is required: {}",
            desktop_binary.display()
        );

        Self {
            root,
            data_dir,
            config_dir,
            cache_dir,
            runtime_dir,
            config_file,
            state_file,
            profile_id,
            endpoint: None,
            client_binary,
            desktop_binary,
        }
    }

    fn with_parent_environment(&mut self) -> EnvironmentGuard {
        let environment = EnvironmentGuard::set(&[
            ("SYNVEIL_CLIENT_CONFIG", &self.config_file),
            ("SYNVEIL_DATA_DIR", &self.data_dir),
            ("SYNVEIL_CONFIG_DIR", &self.config_dir),
            ("SYNVEIL_CACHE_DIR", &self.cache_dir),
            ("SYNVEIL_RUNTIME_DIR", &self.runtime_dir),
        ]);
        self.endpoint = Some(
            DesktopControlEndpoint::for_profile(
                synveil_platform::current().as_ref(),
                self.profile_id,
            )
            .expect("live control endpoint must resolve"),
        );
        environment
    }

    fn endpoint(&self) -> &DesktopControlEndpoint {
        self.endpoint
            .as_ref()
            .expect("parent environment must resolve endpoint")
    }

    fn endpoint_path(&self) -> &Path {
        self.endpoint()
            .unix_path()
            .expect("Linux live endpoint must be a Unix socket")
    }

    fn child_command_environment(&self, command: &mut Command, working_dir: &Path) {
        command
            .current_dir(working_dir)
            .env("SYNVEIL_CLIENT_CONFIG", &self.config_file)
            .env("SYNVEIL_DATA_DIR", &self.data_dir)
            .env("SYNVEIL_CONFIG_DIR", &self.config_dir)
            .env("SYNVEIL_CACHE_DIR", &self.cache_dir)
            .env("SYNVEIL_RUNTIME_DIR", &self.runtime_dir)
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("QT_FORCE_STDERR_LOGGING", "1")
            .env("QML_DISABLE_DISK_CACHE", "1")
            .env_remove("SYNVEIL_QML_SMOKE_TEST")
            .env_remove("SYNVEIL_QML_LIVE_TEST")
            .env_remove("SYNVEIL_QML_LIVE_TEST_RAPID_CLICKS")
            .env_remove("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_SYNC")
            .env_remove("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_READY")
            .env_remove("SYNVEIL_QML_LIVE_TEST_EXIT_ON_TERMINAL");
    }

    fn spawn_shell(
        &self,
        binary: &Path,
        log: PathBuf,
        exit_after_ready: bool,
        exit_on_terminal: bool,
    ) -> ChildCapture {
        self.spawn_shell_in(
            binary,
            log,
            exit_after_ready,
            exit_on_terminal,
            &self.root,
            None,
        )
    }

    fn spawn_shell_in(
        &self,
        binary: &Path,
        log: PathBuf,
        exit_after_ready: bool,
        exit_on_terminal: bool,
        working_dir: &Path,
        path: Option<&str>,
    ) -> ChildCapture {
        let stdout = File::create(&log).expect("live desktop stdout log must open");
        let stderr = stdout
            .try_clone()
            .expect("live desktop stderr log must clone");
        let mut command = Command::new(binary);
        self.child_command_environment(&mut command, working_dir);
        command
            .env("SYNVEIL_QML_LIVE_TEST", "1")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        if let Some(path) = path {
            command.env("PATH", path);
        }
        if exit_after_ready {
            command.env("SYNVEIL_QML_LIVE_TEST_EXIT_AFTER_READY", "1");
        }
        if exit_on_terminal {
            command.env("SYNVEIL_QML_LIVE_TEST_EXIT_ON_TERMINAL", "1");
        }
        ChildCapture {
            child: command
                .spawn()
                .unwrap_or_else(|error| panic!("real desktop shell must spawn: {error}")),
            log,
        }
    }

    fn client_pids(&self, binary: &Path) -> Vec<u32> {
        let Ok(expected_executable) = fs::canonicalize(binary) else {
            return Vec::new();
        };
        let expected_config = format!("SYNVEIL_CLIENT_CONFIG={}", self.config_file.display());
        let mut pids = fs::read_dir("/proc")
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
            .filter(|pid| {
                let exe = fs::read_link(format!("/proc/{pid}/exe"))
                    .ok()
                    .and_then(|path| fs::canonicalize(path).ok());
                if exe.as_ref() != Some(&expected_executable) {
                    return false;
                }
                let Ok(environment) = fs::read(format!("/proc/{pid}/environ")) else {
                    return false;
                };
                environment
                    .split(|byte| *byte == 0)
                    .any(|entry| entry == expected_config.as_bytes())
            })
            .collect::<Vec<_>>();
        pids.sort_unstable();
        pids
    }

    async fn wait_for_client_pids(&self, binary: &Path, expected: usize) -> Vec<u32> {
        let deadline = Instant::now() + WAIT;
        loop {
            let pids = self.client_pids(binary);
            if pids.len() == expected {
                return pids;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected} exact client process(es), observed {:?}",
                pids
            );
            sleep(POLL).await;
        }
    }

    async fn wait_for_control(&self) -> DesktopControlClient {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok(mut client) = DesktopControlClient::connect(self.endpoint().clone()).await
                && client.ping().await == Ok(DesktopProcessStatus::Running)
            {
                return client;
            }
            assert!(
                Instant::now() < deadline,
                "real client control endpoint did not become ready"
            );
            sleep(POLL).await;
        }
    }

    async fn wait_for_endpoint_absent(&self) {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if !self.endpoint_path().exists() {
                return;
            }
            sleep(POLL).await;
        }
        panic!(
            "client control endpoint remained after process stop: {}",
            self.endpoint_path().display()
        );
    }

    fn remove_endpoint_after_exact_client_stop(&self) {
        assert!(
            self.client_pids(&self.client_binary).is_empty(),
            "fixture endpoint cleanup requires the exact client process to be absent"
        );
        if self.endpoint_path().exists() {
            fs::remove_file(self.endpoint_path())
                .expect("stale fixture endpoint must be removable after client crash");
        }
    }

    async fn sqlite_integrity_check(&self) {
        let sqlite = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}", self.state_file.display()))
            .await
            .expect("live SQLite integrity connection must open");
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&sqlite)
            .await
            .expect("live SQLite integrity check must run");
        assert_eq!(
            integrity, "ok",
            "live state must remain SQLite-integrity clean"
        );
        let schema: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&sqlite)
        .await
        .expect("live SQLite schema version must be readable");
        assert_eq!(
            schema, 6,
            "live fixture must retain client schema version 6"
        );
        sqlite.close().await;
    }
}

impl Drop for ProcessFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct ChildCapture {
    child: Child,
    log: PathBuf,
}

impl ChildCapture {
    fn log_offset(&self) -> usize {
        fs::metadata(&self.log)
            .map(|metadata| metadata.len() as usize)
            .unwrap_or_default()
    }

    fn output(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn stop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("desktop child status must be readable")
            .is_none()
        {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl Drop for ChildCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn wait_for_child_exit(child: &mut ChildCapture) -> ExitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = child
            .child
            .try_wait()
            .expect("desktop child status must be readable")
        {
            return status;
        }
        assert!(Instant::now() < deadline, "desktop shell did not exit");
        sleep(POLL).await;
    }
}

fn log_delta(path: &Path, offset: usize) -> String {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(bytes.get(offset..).unwrap_or_default()).into_owned()
}

async fn wait_for_log(path: &Path, offset: usize, needle: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        let output = log_delta(path, offset);
        if output.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "desktop log did not contain safe evidence {needle}; tail={}",
            output
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .join(" | ")
        );
        sleep(POLL).await;
    }
}

async fn wait_for_log_generation(path: &Path, offset: usize, minimum: u64) -> u64 {
    let deadline = Instant::now() + WAIT;
    loop {
        let output = log_delta(path, offset);
        if let Some(generation) = output
            .lines()
            .filter_map(generation_from_line)
            .find(|value| *value >= minimum)
        {
            return generation;
        }
        assert!(
            Instant::now() < deadline,
            "desktop log did not publish controller generation >= {minimum}; tail={}",
            output
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .join(" | ")
        );
        sleep(POLL).await;
    }
}

fn generation_from_line(line: &str) -> Option<u64> {
    if !line.contains("connection=Connected") || !line.contains("freshness=Current") {
        return None;
    }
    line.split_whitespace()
        .find_map(|field| field.strip_prefix("generation="))
        .and_then(|value| value.parse().ok())
}

fn binary_path(name: &str, override_name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(override_name) {
        return PathBuf::from(path);
    }
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository.join("target"));
    target.join("debug").join(name)
}

fn systemctl(args: &[&str]) -> std::process::Output {
    let executable = if Path::new("/usr/bin/systemctl").is_file() {
        "/usr/bin/systemctl"
    } else {
        "/bin/systemctl"
    };
    Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|error| panic!("systemctl must be callable: {error}"))
}

fn systemctl_value(unit: &str, property: &str) -> Option<String> {
    let property = format!("--property={property}");
    let output = systemctl(&["--user", "show", unit, &property, "--value"]);
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn systemd_user_runtime_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .expect("the live systemd user session needs XDG_RUNTIME_DIR"),
    )
}

fn kill_pid(pid: u32, signal: &str) {
    let status = Command::new("/bin/kill")
        .args([format!("-{signal}"), pid.to_string()])
        .status()
        .unwrap_or_else(|error| panic!("kill must be callable: {error}"));
    assert!(
        status.success() || signal == "KILL",
        "kill -{signal} {pid} failed with {status}"
    );
}

struct DisposableUserUnit {
    fragment: PathBuf,
    alias: PathBuf,
    alias_parent: PathBuf,
    alias_parent_created: bool,
    installed: bool,
}

impl DisposableUserUnit {
    fn create(fixture: &ProcessFixture) -> Self {
        let fixed_load_state = systemctl_value(LINUX_USER_SERVICE_NAME, "LoadState")
            .expect("systemd must report the fixed Synveil unit state");
        assert_eq!(
            fixed_load_state, "not-found",
            "the live gate will not touch an existing user-owned Synveil unit"
        );

        let alias = systemd_user_runtime_dir()
            .join("systemd/user")
            .join(LINUX_USER_SERVICE_NAME);
        let alias_parent = alias
            .parent()
            .expect("systemd user unit alias must have a parent")
            .to_path_buf();
        let alias_parent_created = !alias_parent.is_dir();
        fs::create_dir_all(&alias_parent)
            .expect("systemd user unit search directory must be available");
        if alias_parent_created {
            fs::set_permissions(&alias_parent, fs::Permissions::from_mode(0o700))
                .expect("disposable systemd user unit directory must be private");
        }
        assert!(
            fs::symlink_metadata(&alias).is_err(),
            "the fixed disposable alias must not already exist"
        );
        let unique_fragment_name =
            format!("synveil-client-p99-{}.service", Uuid::now_v7().simple());
        let fragment = fixture.root.join(&unique_fragment_name);
        let unit = format!(
            "[Unit]\nDescription=Disposable Synveil Prompt 99 client\nStartLimitIntervalSec=30s\nStartLimitBurst=20\n\n[Service]\nType=simple\nExecStart={}\nEnvironment=SYNVEIL_CLIENT_CONFIG={}\nEnvironment=SYNVEIL_DATA_DIR={}\nEnvironment=SYNVEIL_CONFIG_DIR={}\nEnvironment=SYNVEIL_CACHE_DIR={}\nEnvironment=SYNVEIL_RUNTIME_DIR={}\nWorkingDirectory={}\nRestart=on-failure\nRestartSec=200ms\nRestartPreventExitStatus=78\n\n[Install]\nWantedBy=default.target\n",
            fixture.client_binary.display(),
            fixture.config_file.display(),
            fixture.data_dir.display(),
            fixture.config_dir.display(),
            fixture.cache_dir.display(),
            fixture.runtime_dir.display(),
            fixture.root.display(),
        );
        fs::write(&fragment, unit).expect("disposable user unit must be written");
        let verification = Command::new("systemd-analyze")
            .args(["verify", fragment.to_str().expect("unit path is UTF-8")])
            .output()
            .expect("systemd-analyze must be callable");
        assert!(
            verification.status.success(),
            "disposable user unit must pass systemd-analyze verify: {}",
            String::from_utf8_lossy(&verification.stderr)
        );
        std::os::unix::fs::symlink(&fragment, &alias)
            .expect("fixed disposable alias must link to fresh fragment");
        let reload = systemctl(&["--user", "daemon-reload"]);
        assert!(
            reload.status.success(),
            "systemd user daemon-reload must accept disposable unit: {}",
            String::from_utf8_lossy(&reload.stderr)
        );
        assert_eq!(
            systemctl_value(LINUX_USER_SERVICE_NAME, "LoadState").as_deref(),
            Some("loaded"),
            "systemd must load the fresh disposable fragment through the production name"
        );
        Self {
            fragment,
            alias,
            alias_parent,
            alias_parent_created,
            installed: true,
        }
    }

    fn start(&self) {
        let output = systemctl(&["--user", "start", LINUX_USER_SERVICE_NAME]);
        assert!(
            output.status.success(),
            "disposable user unit must start: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn stop(&self) {
        let output = systemctl(&["--user", "stop", LINUX_USER_SERVICE_NAME]);
        assert!(
            output.status.success(),
            "disposable user unit must stop: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn main_pid(&self) -> Option<u32> {
        systemctl_value(LINUX_USER_SERVICE_NAME, "MainPID")
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|pid| *pid != 0)
    }

    async fn wait_active(&self, fixture: &ProcessFixture) -> u32 {
        let deadline = Instant::now() + WAIT;
        loop {
            if systemctl_value(LINUX_USER_SERVICE_NAME, "ActiveState").as_deref() == Some("active")
                && let Some(pid) = self.main_pid()
                && fixture.client_pids(&fixture.client_binary) == [pid]
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "disposable user unit did not become active"
            );
            sleep(POLL).await;
        }
    }

    async fn wait_restarted(&self, fixture: &ProcessFixture, old_pid: u32) -> u32 {
        let deadline = Instant::now() + WAIT;
        loop {
            if systemctl_value(LINUX_USER_SERVICE_NAME, "ActiveState").as_deref() == Some("active")
                && let Some(pid) = self.main_pid()
                && pid != old_pid
                && fixture.client_pids(&fixture.client_binary) == [pid]
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "disposable user unit did not restart with a new exact client PID"
            );
            sleep(POLL).await;
        }
    }

    fn remove(&mut self) {
        if !self.installed {
            return;
        }
        let _ = systemctl(&["--user", "stop", LINUX_USER_SERVICE_NAME]);
        let _ = systemctl(&["--user", "disable", LINUX_USER_SERVICE_NAME]);
        if let Ok(target) = fs::read_link(&self.alias)
            && fs::canonicalize(&target).ok() == fs::canonicalize(&self.fragment).ok()
        {
            let _ = fs::remove_file(&self.alias);
        }
        let _ = systemctl(&["--user", "daemon-reload"]);
        let _ = fs::remove_file(&self.fragment);
        if self.alias_parent_created {
            let _ = fs::remove_dir(&self.alias_parent);
        }
        self.installed = false;
    }
}

impl Drop for DisposableUserUnit {
    fn drop(&mut self) {
        self.remove();
    }
}

struct FakeProtocolServer {
    path: PathBuf,
    task: JoinHandle<()>,
}

impl FakeProtocolServer {
    async fn bind(path: &Path) -> Self {
        let listener = UnixListener::bind(path).expect("fake protocol endpoint must bind");
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("fake protocol endpoint must be private");
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let Ok(Ok(payload)) =
                        timeout(Duration::from_secs(5), read_frame(&mut stream)).await
                    else {
                        return;
                    };
                    let Ok(hello) = serde_json::from_slice::<ControlClientHello>(&payload) else {
                        return;
                    };
                    if hello.protocol_version != DESKTOP_CONTROL_PROTOCOL_VERSION {
                        return;
                    }
                    let _ = write_frame(
                        &mut stream,
                        &ControlServerHello {
                            protocol_version: DESKTOP_CONTROL_PROTOCOL_VERSION + 1,
                            capabilities: Vec::<ControlCapability>::new(),
                            error: None,
                        },
                    )
                    .await;
                });
            }
        });
        Self {
            path: path.to_path_buf(),
            task,
        }
    }

    async fn stop(self) {
        self.task.abort();
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for FakeProtocolServer {
    fn drop(&mut self) {
        self.task.abort();
        let _ = fs::remove_file(&self.path);
    }
}

fn stage_package_layout(fixture: &ProcessFixture) -> PathBuf {
    let package = fixture.root.join("package");
    let bin = package.join("usr/bin");
    let unit = package.join("usr/lib/systemd/user");
    let desktop = package.join("usr/share/applications");
    let icon = package.join("usr/share/icons/hicolor/scalable/apps");
    let docs = package.join("usr/share/doc/synveil");
    for directory in [&bin, &unit, &desktop, &icon, &docs] {
        fs::create_dir_all(directory).expect("package-equivalent directory must be created");
    }
    let packaged_client = bin.join("synveil-client");
    let packaged_desktop = bin.join("synveil-desktop");
    fs::copy(&fixture.client_binary, &packaged_client)
        .expect("package-equivalent client must copy");
    fs::copy(&fixture.desktop_binary, &packaged_desktop)
        .expect("package-equivalent desktop must copy");
    for path in [&packaged_client, &packaged_desktop] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("packaged executable must be runnable");
    }
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (source, destination) in [
        (
            repository.join("deploy/systemd-user/synveil-client.service"),
            unit.join("synveil-client.service"),
        ),
        (
            repository.join("deploy/applications/synveil.desktop"),
            desktop.join("synveil.desktop"),
        ),
        (
            repository.join("deploy/icons/hicolor/scalable/apps/synveil.svg"),
            icon.join("synveil.svg"),
        ),
        (repository.join("LICENSE"), docs.join("LICENSE")),
        (repository.join("deploy/NOTICE"), docs.join("NOTICE")),
    ] {
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!(
                "package-equivalent payload must copy {}: {error}",
                source.display()
            )
        });
    }
    assert!(!package.starts_with(&repository));
    assert!(packaged_client.is_file());
    assert!(packaged_desktop.is_file());
    assert!(unit.join("synveil-client.service").is_file());
    package
}

fn assert_success(status: ExitStatus, label: &str) {
    assert!(status.success(), "{label} must exit successfully: {status}");
}

#[tokio::test]
#[ignore = "explicit Prompt 99 Linux live launch gate; requires Qt and a systemd user session"]
async fn live_production_desktop_launch_acceptance() {
    let mut fixture = ProcessFixture::create().await;
    let _environment = fixture.with_parent_environment();
    let mut shell_one = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch1-shell.log"),
        false,
        false,
    );

    // LIVE-LAUNCH1: a real GUI process starts with no client, asks the real
    // manager to launch one packaged sibling, and reaches one coherent
    // controller Fresh snapshot over the real Prompt 96 endpoint.
    wait_for_log(
        &shell_one.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(&shell_one.log, 0, "root=Folder available").await;
    wait_for_log(&shell_one.log, 0, "launch=Background service started.").await;
    let initial_pids = fixture
        .wait_for_client_pids(&fixture.client_binary, 1)
        .await;
    let mut control = fixture.wait_for_control().await;
    assert_eq!(
        control.ping().await.expect("live client must answer Ping"),
        DesktopProcessStatus::Running
    );
    assert_eq!(
        control
            .list_libraries()
            .await
            .expect("live client library list must answer")
            .libraries
            .len(),
        1
    );
    eprintln!(
        "LIVE-LAUNCH1 PASS real GUI start -> one exact client PID -> IPC Running/Fresh; active_client_pids={} launch_result=direct",
        initial_pids.len()
    );

    // LIVE-LAUNCH2: another real GUI reuses the already-running process and
    // produces no second exact executable/config pair.
    let mut shell_two = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch2-shell.log"),
        false,
        false,
    );
    wait_for_log(
        &shell_two.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(
        &shell_two.log,
        0,
        "launch=Background service already running.",
    )
    .await;
    assert_eq!(
        fixture
            .wait_for_client_pids(&fixture.client_binary, 1)
            .await,
        initial_pids
    );
    eprintln!(
        "LIVE-LAUNCH2 PASS second real GUI reused the exact existing client; duplicate_client_pids=0"
    );

    // LIVE-LAUNCH3: route a real QML/tray-facing quit through the desktop
    // controller stop path. The client endpoint and PID must survive, and no
    // Prompt 96 Shutdown is sent by GUI teardown.
    let mut shell_quit = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch3-shell.log"),
        true,
        false,
    );
    wait_for_log(
        &shell_quit.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    let quit_status = wait_for_child_exit(&mut shell_quit).await;
    assert_success(quit_status, "LIVE-LAUNCH3 GUI quit");
    assert_eq!(
        fixture
            .wait_for_client_pids(&fixture.client_binary, 1)
            .await,
        initial_pids
    );
    assert_eq!(
        control.ping().await.expect("client must survive GUI quit"),
        DesktopProcessStatus::Running
    );
    assert!(!shell_quit.output().contains("Shutdown"));
    eprintln!(
        "LIVE-LAUNCH3 PASS GUI/tray quit exited desktop while client remained Running; Prompt96_shutdown_requests=0"
    );

    // LIVE-LAUNCH4: reopening the GUI obtains Fresh again from that same
    // client generation and does not spawn a replacement.
    let mut shell_reopen = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch4-shell.log"),
        true,
        false,
    );
    wait_for_log(
        &shell_reopen.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(
        &shell_reopen.log,
        0,
        "launch=Background service already running.",
    )
    .await;
    assert_success(
        wait_for_child_exit(&mut shell_reopen).await,
        "LIVE-LAUNCH4 GUI reopen",
    );
    assert_eq!(
        fixture
            .wait_for_client_pids(&fixture.client_binary, 1)
            .await,
        initial_pids
    );
    eprintln!(
        "LIVE-LAUNCH4 PASS reopened GUI reused client and reached Fresh; replacement_client_pids=0"
    );

    // The next phase intentionally changes the supervisor. Close the two
    // open shells and kill only the exact fixture client.
    shell_one.stop();
    shell_two.stop();
    drop(control);
    for pid in fixture.client_pids(&fixture.client_binary) {
        kill_pid(pid, "KILL");
    }
    fixture
        .wait_for_client_pids(&fixture.client_binary, 0)
        .await;
    fixture.remove_endpoint_after_exact_client_stop();
    fixture.wait_for_endpoint_absent().await;

    let mut unit = DisposableUserUnit::create(&fixture);
    unit.start();
    let first_supervised_pid = unit.wait_active(&fixture).await;

    // LIVE-LAUNCH5: with the real GUI open, kill the supervised process as an
    // unexpected crash. systemd supplies a new PID and the existing
    // controller moves through stale/reconnecting to a new Fresh generation.
    let mut shell_supervised = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch5-shell.log"),
        false,
        false,
    );
    wait_for_log(
        &shell_supervised.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    let crash_offset = shell_supervised.log_offset();
    let old_generation = log_delta(&shell_supervised.log, 0)
        .lines()
        .filter_map(generation_from_line)
        .max()
        .expect("LIVE-LAUNCH5 initial Fresh generation must be logged");
    kill_pid(first_supervised_pid, "KILL");
    wait_for_log(
        &shell_supervised.log,
        crash_offset,
        "SYNVEIL-QML-LIVE STATE connection=Reconnecting freshness=Last known status",
    )
    .await;
    let restarted_pid = unit.wait_restarted(&fixture, first_supervised_pid).await;
    wait_for_log_generation(&shell_supervised.log, crash_offset, old_generation + 1).await;
    let mut restarted_control = fixture.wait_for_control().await;
    assert_eq!(
        restarted_control
            .ping()
            .await
            .expect("restarted supervised client must answer Ping"),
        DesktopProcessStatus::Running
    );
    assert_ne!(restarted_pid, first_supervised_pid);
    eprintln!(
        "LIVE-LAUNCH5 PASS supervised unexpected crash -> stale/reconnecting -> new PID/Fresh controller generation; gui_start_requests_after_crash=0"
    );
    drop(restarted_control);
    shell_supervised.stop();

    // LIVE-LAUNCH6: crash again while the GUI is closed. systemd restarts
    // without manual repair; a later real GUI reuses that recovered process.
    let second_old_pid = unit
        .main_pid()
        .expect("supervised client must remain active");
    kill_pid(second_old_pid, "KILL");
    let second_restarted_pid = unit.wait_restarted(&fixture, second_old_pid).await;
    assert_ne!(second_restarted_pid, second_old_pid);
    assert!(fixture.client_pids(&fixture.client_binary) == [second_restarted_pid]);
    let mut shell_after_closed_crash = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch6-shell.log"),
        true,
        false,
    );
    wait_for_log(
        &shell_after_closed_crash.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(
        &shell_after_closed_crash.log,
        0,
        "launch=Background service already running.",
    )
    .await;
    assert_success(
        wait_for_child_exit(&mut shell_after_closed_crash).await,
        "LIVE-LAUNCH6 GUI reopen",
    );
    assert_eq!(
        fixture
            .wait_for_client_pids(&fixture.client_binary, 1)
            .await,
        vec![second_restarted_pid]
    );
    eprintln!(
        "LIVE-LAUNCH6 PASS closed-GUI supervisor crash recovered before reopen; manual_client_spawn=0"
    );

    // LIVE-LAUNCH7: stop only for a clean race setup, keep the fresh unit
    // linked, and start two real GUI requesters concurrently. systemd and
    // the launch manager must leave one active client and a readable SQLite
    // state rather than duplicate writers/processes.
    unit.stop();
    fixture
        .wait_for_client_pids(&fixture.client_binary, 0)
        .await;
    fixture.wait_for_endpoint_absent().await;
    let mut shell_race_one = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch7-shell-one.log"),
        false,
        false,
    );
    let mut shell_race_two = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch7-shell-two.log"),
        false,
        false,
    );
    wait_for_log(
        &shell_race_one.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(
        &shell_race_two.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    let race_pids = fixture
        .wait_for_client_pids(&fixture.client_binary, 1)
        .await;
    assert_eq!(race_pids.len(), 1);
    assert_eq!(unit.main_pid(), Some(race_pids[0]));
    assert!(
        shell_race_one.output().contains("launch=") && shell_race_two.output().contains("launch=")
    );
    eprintln!(
        "LIVE-LAUNCH7 PASS concurrent real GUI ensure/start requests left one supervised client/writer; active_client_pids=1"
    );
    shell_race_one.stop();
    shell_race_two.stop();
    unit.stop();
    fixture
        .wait_for_client_pids(&fixture.client_binary, 0)
        .await;
    fixture.wait_for_endpoint_absent().await;

    // LIVE-LAUNCH8: a regular-file endpoint is an endpoint-security terminal
    // state. The real controller and launch manager must not unlink, repair,
    // spawn, or storm against it.
    fs::write(fixture.endpoint_path(), b"not-a-socket")
        .expect("unsafe endpoint regular file must be created");
    fs::set_permissions(fixture.endpoint_path(), fs::Permissions::from_mode(0o600))
        .expect("unsafe endpoint regular file must be private");
    let mut shell_unsafe = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch8-shell.log"),
        false,
        true,
    );
    wait_for_log(
        &shell_unsafe.log,
        0,
        "SYNVEIL-QML-LIVE ACTION quit_terminal",
    )
    .await;
    assert_success(
        wait_for_child_exit(&mut shell_unsafe).await,
        "LIVE-LAUNCH8 shell",
    );
    assert!(
        fs::metadata(fixture.endpoint_path())
            .expect("unsafe endpoint must remain")
            .file_type()
            .is_file()
    );
    assert!(fixture.client_pids(&fixture.client_binary).is_empty());
    let unsafe_output = shell_unsafe.output();
    assert!(
        unsafe_output.contains("Connection unavailable")
            || unsafe_output.contains("Background service unavailable"),
        "unsafe endpoint must produce a generic unavailable label: {unsafe_output}"
    );
    assert!(
        unsafe_output.contains("Background service launch blocked by an unsafe state."),
        "unsafe endpoint must suppress launch: {unsafe_output}"
    );
    fs::remove_file(fixture.endpoint_path()).expect("unsafe endpoint must be fixture-removed");
    eprintln!(
        "LIVE-LAUNCH8 PASS regular-file endpoint -> EndpointSecurity/launch suppression; spawn_requests=0 endpoint_preserved=true"
    );

    // LIVE-LAUNCH9: a real Unix endpoint that completes the handshake with a
    // higher protocol version is classified by controller/orchestration, not
    // by an enum-only unit test, and remains untouched.
    let fake_protocol = FakeProtocolServer::bind(fixture.endpoint_path()).await;
    let mut shell_protocol = fixture.spawn_shell(
        &fixture.desktop_binary,
        fixture.root.join("live-launch9-shell.log"),
        false,
        true,
    );
    wait_for_log(
        &shell_protocol.log,
        0,
        "SYNVEIL-QML-LIVE ACTION quit_terminal",
    )
    .await;
    assert_success(
        wait_for_child_exit(&mut shell_protocol).await,
        "LIVE-LAUNCH9 shell",
    );
    let protocol_metadata =
        fs::symlink_metadata(fixture.endpoint_path()).expect("protocol endpoint must remain");
    assert!(protocol_metadata.file_type().is_socket());
    assert_eq!(
        protocol_metadata.uid(),
        fs::metadata("/proc/self")
            .expect("current process metadata")
            .uid()
    );
    assert_eq!(protocol_metadata.permissions().mode() & 0o777, 0o600);
    assert!(fixture.client_pids(&fixture.client_binary).is_empty());
    let protocol_output = shell_protocol.output();
    assert!(protocol_output.contains("Incompatible background service"));
    assert!(
        protocol_output.contains("Background service launch blocked by an unsafe state.")
            || protocol_output.contains("Background service launch was denied.")
            || protocol_output.contains("Background service is incompatible."),
        "protocol endpoint must suppress launch: {protocol_output}"
    );
    fake_protocol.stop().await;
    eprintln!(
        "LIVE-LAUNCH9 PASS real incompatible Prompt96 handshake -> ProtocolIncompatible/launch suppression; spawn_requests=0 endpoint_preserved=true"
    );

    // LIVE-LAUNCH10: copy the actual binaries and launch metadata into a
    // package-shaped tree outside the repository. The packaged desktop must
    // find its sibling client from current_exe, start that sibling directly,
    // reach the real endpoint/Fresh state, and not depend on PATH or source
    // tree layout.
    unit.remove();
    let package = stage_package_layout(&fixture);
    let packaged_client = package.join("usr/bin/synveil-client");
    let packaged_desktop = package.join("usr/bin/synveil-desktop");
    let mut packaged_shell = fixture.spawn_shell_in(
        &packaged_desktop,
        fixture.root.join("live-launch10-package-shell.log"),
        true,
        false,
        &package,
        Some("/usr/bin"),
    );
    wait_for_log(
        &packaged_shell.log,
        0,
        "SYNVEIL-QML-LIVE STATE connection=Connected freshness=Current",
    )
    .await;
    wait_for_log(&packaged_shell.log, 0, "launch=Background service started.").await;
    assert_success(
        wait_for_child_exit(&mut packaged_shell).await,
        "LIVE-LAUNCH10 packaged GUI",
    );
    let package_pids = fixture.wait_for_client_pids(&packaged_client, 1).await;
    assert_eq!(package_pids.len(), 1);
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(
        !fs::canonicalize(&packaged_client)
            .expect("packaged client must canonicalize")
            .starts_with(&repository)
    );
    kill_pid(package_pids[0], "KILL");
    fixture.wait_for_client_pids(&packaged_client, 0).await;
    fixture.remove_endpoint_after_exact_client_stop();
    fixture.wait_for_endpoint_absent().await;
    fixture.sqlite_integrity_check().await;
    eprintln!(
        "LIVE-LAUNCH10 PASS package-shaped outside-source tree used actual desktop/client sibling path and reached IPC/Fresh; PATH_source_dependency=0"
    );

    assert_eq!(
        systemctl_value(LINUX_USER_SERVICE_NAME, "LoadState").as_deref(),
        Some("not-found"),
        "live gate must leave no fixed Synveil user unit loaded"
    );
    eprintln!("LIVE-LAUNCH MATRIX PASS LIVE-LAUNCH1..LIVE-LAUNCH10");
}
