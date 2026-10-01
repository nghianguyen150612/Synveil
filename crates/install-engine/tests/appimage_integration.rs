use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Mutex};
use synveil_install_engine::{AppImageIntegration, AppImageIntegrationStatus};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn environment(root: &Path) {
    unsafe {
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("XDG_STATE_HOME", root.join("state"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        std::env::set_var("XDG_RUNTIME_DIR", root.join("runtime"));
    }
}

fn artifact(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"\x7fELFtestAI\x02 artifact").unwrap();
    let mut mode = fs::metadata(&path).unwrap().permissions();
    mode.set_mode(0o755);
    fs::set_permissions(&path, mode).unwrap();
    path
}

fn fixture() -> (
    TempDir,
    AppImageIntegration,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().unwrap();
    environment(root.path());
    let app = artifact(root.path(), "Downloads/Synveil release ☃.AppImage");
    let icon = root.path().join("synveil.svg");
    fs::write(&icon, b"<svg/>\n").unwrap();
    (
        root,
        AppImageIntegration::from_environment().unwrap(),
        app,
        icon,
    )
}

#[test]
fn portable_environment_has_zero_integration_mutations() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, _, _) = fixture();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::NotIntegrated
    );
    assert!(!root.path().join("data").exists());
}

#[test]
fn install_is_safe_idempotent_and_never_enables_startup() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (_root, engine, app, icon) = fixture();
    let first = engine.install(&app, &icon).unwrap();
    let second = engine.install(&app, &icon).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::Healthy
    );
    let desktop = fs::read_to_string(&first.desktop_entry_path).unwrap();
    assert!(desktop.contains("Name=Synveil") && desktop.contains("Synveil release ☃.AppImage\""));
    let unit = fs::read_to_string(&first.user_unit_path).unwrap();
    assert!(
        unit.contains("--synveil-client")
            && !unit.contains("/tmp/.mount_")
            && !unit.contains("token")
    );
    assert!(!first.startup_enabled);
}

#[test]
fn stale_partial_relocation_repair_and_remove_preserve_data() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let first = engine.install(&app, &icon).unwrap();
    fs::remove_file(&app).unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::StaleAppImagePath
    );
    let replacement = artifact(root.path(), "Applications/Synveil-new.AppImage");
    let updated = engine.repair(&replacement, &icon).unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::Healthy
    );
    assert_eq!(updated.appimage_path, replacement.canonicalize().unwrap());
    fs::remove_file(&updated.icon_path).unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::NeedsRepair
    );
    fs::create_dir_all(root.path().join("config/synveil")).unwrap();
    fs::write(
        root.path().join("config/synveil/keep"),
        b"credential sentinel",
    )
    .unwrap();
    engine.repair(&replacement, &icon).unwrap();
    engine.remove().unwrap();
    engine.remove().unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::NotIntegrated
    );
    assert!(replacement.exists() && root.path().join("config/synveil/keep").exists());
    assert!(!first.desktop_entry_path.exists());
}
