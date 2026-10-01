use std::{env, path::PathBuf, process::ExitCode};
use synveil_install_engine::{AppImageIntegration, AppImageIntegrationError};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let command = args
        .next()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_default();
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

fn fail(error: impl std::fmt::Debug) -> ExitCode {
    eprintln!("synveil-appimage-integration: {error:?}");
    ExitCode::FAILURE
}
