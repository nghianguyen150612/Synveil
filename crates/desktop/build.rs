use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // Release builds set this to stabilize Qt QML AOT output. Track it so
    // Cargo reruns the generator when the build policy changes.
    println!("cargo:rerun-if-env-changed=QT_HASH_SEED");
    println!("cargo:rerun-if-env-changed=QMAKE");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    CxxQtBuilder::new_qml_module(
        QmlModule::new("com.synveil.desktop")
            .version(1, 0)
            .qml_files(["qml/Main.qml"]),
    )
    .crate_include_root(Some("src".to_owned()))
    .files(["src/bridge.rs"])
    .cpp_file("src/native/tray.cpp")
    .qt_module("Widgets")
    .build();
}
