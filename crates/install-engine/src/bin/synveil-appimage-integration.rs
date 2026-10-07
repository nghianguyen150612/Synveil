//! AppImage integration helper, invoked by `deploy/packages/appimage/AppRun`.
//!
//! The command is Linux-only: the module it drives owns XDG surfaces, a systemd
//! user unit and POSIX ownership checks. Cargo cannot express a
//! target-conditional `[[bin]]`, and `build-appimage.sh` builds this binary by
//! name on Linux, so the declaration remains unconditional and the body is
//! gated instead.
//!
//! On a non-Linux target this helper performs no integration and exits
//! non-zero. It deliberately does not report success: a portable
//! non-Linux install has nothing to integrate, and a silent success would let
//! packaging claim an AppImage registration that does not exist.

use std::process::ExitCode;

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!(
        "synveil-appimage-integration: AppImage integration is Linux-only; \
         this binary performs no integration on this platform"
    );
    ExitCode::from(2)
}

#[cfg(target_os = "linux")]
use std::{env, path::PathBuf};
#[cfg(target_os = "linux")]
use synveil_install_engine::{AppImageIntegration, AppImageIntegrationError};

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let command = args
        .next()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_default();
    if args.next().is_some() {
        eprintln!("synveil-appimage-integration: unexpected argument");
        return ExitCode::from(2);
    }
    let engine = match AppImageIntegration::from_environment() {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let artifact = || {
        env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or(AppImageIntegrationError::InvalidEnvironment)
    };
    let icon = || {
        env::var_os("SYNVEIL_APPIMAGE_ICON")
            .map(PathBuf::from)
            .ok_or(AppImageIntegrationError::InvalidEnvironment)
    };
    let result = match command.as_str() {
        "status" => engine.inspect().map(|s| println!("{s:?}")),
        "install" | "repair" => artifact()
            .and_then(|a| icon().map(|i| (a, i)))
            .and_then(|(a, i)| engine.install(&a, &i).map(|_| ())),
        "remove" => engine.remove(),
        "enable" => engine.set_startup(true),
        "disable" => engine.set_startup(false),
        _ => {
            eprintln!(
                "usage: synveil-appimage-integration status|install|repair|remove|enable|disable"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

#[cfg(target_os = "linux")]
fn fail(error: AppImageIntegrationError) -> ExitCode {
    let category = match error {
        AppImageIntegrationError::InvalidEnvironment => "invalid-environment",
        AppImageIntegrationError::InvalidArtifact => "invalid-artifact",
        AppImageIntegrationError::UnsafePath => "unsafe-path",
        AppImageIntegrationError::UnownedSurface => "unknown-ownership",
        AppImageIntegrationError::Io(_) => "filesystem-error",
        AppImageIntegrationError::InvalidRecord => "unknown-state",
        AppImageIntegrationError::UserSystemdUnavailable => "user-supervisor-unavailable",
        AppImageIntegrationError::SystemctlFailed => "user-supervisor-failed",
    };
    eprintln!("synveil-appimage-integration: {category}");
    ExitCode::FAILURE
}
