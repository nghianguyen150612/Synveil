//! AppImage integration behaviour.
//!
//! Linux-only by construction: it drives the AppImage module, which is
//! cfg-gated to `target_os = "linux"` in the crate root. On other targets this
//! file compiles to no tests rather than asserting a non-Linux substitute
//! behaviour that does not exist.

#![cfg(target_os = "linux")]

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

#[test]
fn unknown_record_and_newer_version_never_authorize_repair_or_removal() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let installed = engine.install(&app, &icon).unwrap();
    let (_, _, record_path, _) = engine.paths();
    let original = fs::read(&record_path).unwrap();
    for field in ["schema_version", "product_version"] {
        let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
        record[field] = if field == "schema_version" {
            serde_json::json!(99)
        } else {
            serde_json::json!("9.0.0")
        };
        fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        let before = fs::read(&record_path).unwrap();
        assert!(engine.repair(&app, &icon).is_err());
        assert!(engine.remove().is_err());
        assert_eq!(fs::read(&record_path).unwrap(), before);
        assert!(installed.desktop_entry_path.exists());
    }
    fs::write(&record_path, b"unknown schema / secret").unwrap();
    assert!(engine.install(&app, &icon).is_err());
    assert!(
        root.path()
            .join("data/applications/synveil-appimage.desktop")
            .exists()
    );
}

#[test]
fn missing_record_cannot_claim_adjacent_file_and_symlink_ancestor_cannot_delete() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let (desktop, _, _, _) = engine.paths();
    fs::create_dir_all(desktop.parent().unwrap()).unwrap();
    fs::write(&desktop, b"unrelated user launcher").unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::Incomplete
    );
    assert!(engine.install(&app, &icon).is_err());
    assert!(engine.remove().is_err());
    assert_eq!(fs::read(&desktop).unwrap(), b"unrelated user launcher");
    fs::remove_file(&desktop).unwrap();
    let record = engine.install(&app, &icon).unwrap();
    let external = root.path().join("outside");
    fs::rename(desktop.parent().unwrap(), &external).unwrap();
    std::os::unix::fs::symlink(&external, desktop.parent().unwrap()).unwrap();
    assert!(engine.remove().is_err());
    assert!(external.join("synveil-appimage.desktop").exists());
    assert!(record.icon_path.exists());
}

#[test]
fn partial_launcher_without_record_is_not_reported_installed_or_replayed() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let (desktop, _, record, _) = engine.paths();
    fs::create_dir_all(desktop.parent().unwrap()).unwrap();
    fs::write(&desktop, b"partial owned launcher fixture").unwrap();
    fs::create_dir_all(root.path().join("config/synveil")).unwrap();
    let durable = root.path().join("config/synveil/keep");
    fs::write(&durable, b"credential and library sentinel").unwrap();

    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::Incomplete
    );
    assert!(engine.repair(&app, &icon).is_err());
    assert!(engine.install(&app, &icon).is_err());
    assert!(!record.exists());
    assert_eq!(
        fs::read(&desktop).unwrap(),
        b"partial owned launcher fixture"
    );
    assert_eq!(
        fs::read(&durable).unwrap(),
        b"credential and library sentinel"
    );
}

#[test]
fn interrupted_owned_integration_removal_is_idempotent_and_scoped() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let record = engine.install(&app, &icon).unwrap();
    fs::create_dir_all(root.path().join("config/synveil")).unwrap();
    let durable = root.path().join("config/synveil/keep");
    fs::write(&durable, b"server and user data sentinel").unwrap();

    fs::remove_file(&record.desktop_entry_path).unwrap();
    fs::remove_file(&record.icon_path).unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::NeedsRepair
    );
    engine.remove().unwrap();
    engine.remove().unwrap();
    assert_eq!(
        engine.inspect().unwrap(),
        AppImageIntegrationStatus::NotIntegrated
    );
    assert!(app.exists());
    assert_eq!(
        fs::read(&durable).unwrap(),
        b"server and user data sentinel"
    );
}

#[test]
fn generated_integration_treats_expansion_characters_as_data() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, _, icon) = fixture();
    let app = artifact(root.path(), "Downloads/$HOME `command` %u.AppImage");
    let record = engine.install(&app, &icon).unwrap();
    let desktop = fs::read_to_string(record.desktop_entry_path).unwrap();
    let unit = fs::read_to_string(record.user_unit_path).unwrap();
    assert!(
        desktop.contains("\\$HOME") && desktop.contains("\\`command\\`") && desktop.contains("%%u")
    );
    assert!(unit.contains("$$HOME") && unit.contains("%%u"));
}

#[test]
fn writable_parent_cannot_authorize_appimage_mutation_or_removal() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let (root, engine, app, icon) = fixture();
    let installed = engine.install(&app, &icon).unwrap();
    let original = fs::read(&installed.desktop_entry_path).unwrap();
    fs::set_permissions(root.path().join("data"), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(engine.repair(&app, &icon).is_err());
    assert!(engine.remove().is_err());
    assert_eq!(fs::read(&installed.desktop_entry_path).unwrap(), original);
    assert!(installed.icon_path.exists());
}
