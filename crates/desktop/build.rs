use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
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
