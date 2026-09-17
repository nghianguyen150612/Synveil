use std::process::ExitCode;

use cxx_qt_lib::{QQmlApplicationEngine, QString, QUrl};

mod actions;
mod bridge;
mod presentation;
mod profile;

use bridge::ffi;

fn main() -> ExitCode {
    if std::env::args().any(|argument| argument == "--qml-smoke-test") {
        // The native QApplication intentionally receives a minimal argv.  Pass
        // this test-only switch to the QML bridge without exposing arbitrary
        // process arguments to the UI model.
        std::env::set_var("SYNVEIL_QML_SMOKE_TEST", "1");
    }

    cxx_qt::init_crate!(cxx_qt);
    cxx_qt::init_crate!(cxx_qt_lib);
    cxx_qt::init_qml_module!("com.synveil.desktop");

    let mut application = ffi::native_application_new();
    if application.is_null() {
        return ExitCode::from(70);
    }
    ffi::native_application_set_metadata(
        application.pin_mut(),
        &QString::from("Synveil"),
        &QString::from(env!("CARGO_PKG_VERSION")),
    );

    let mut engine = QQmlApplicationEngine::new();
    if engine.is_null() {
        return ExitCode::from(70);
    }
    let entrypoint = QUrl::from("qrc:/qt/qml/com/synveil/desktop/qml/Main.qml");
    engine.pin_mut().load(&entrypoint);
    let Some(engine_ref) = engine.as_ref() else {
        return ExitCode::from(70);
    };
    if !ffi::native_qml_engine_has_root(engine_ref) {
        return ExitCode::from(70);
    }

    let status = ffi::native_application_exec(application.pin_mut());
    ExitCode::from(u8::try_from(status).unwrap_or(70))
}
