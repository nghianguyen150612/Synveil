#!/usr/bin/env python3
"""Network-free structural checks for Prompt038's ownership and UX contract."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(label: str, condition: bool) -> None:
    if not condition:
        raise SystemExit(f"connection-setup validation failed: {label}")
    print(f"ok: {label}")


qml = read("crates/desktop/qml/Main.qml")
bridge = read("crates/desktop/src/bridge.rs")
controller = read("crates/client/src/controller.rs")
profiles = read("crates/client-sync/src/profiles.rs")
host = read("crates/client-sync/src/host.rs")

for token in (
    '"connectSetupPage"', 'objectName: "serverAddressField"',
    'objectName: "connectServerButton"', 'objectName: "connectBackButton"',
    'objectName: "connectionFeedbackLabel"', 'qsTr("Synveil connects securely over HTTPS.")',
):
    require(token, token in qml)

label_index = qml.index("id: profileLabelField")
fresh = qml[qml.index("id: serverAddressField") : label_index]
require("one fresh required field", fresh.count("TextField {") == 1)
require("fresh label field absent", "profileLabelField" not in fresh)
require("client-owned convenience type", "struct UserServerAddress" in profiles)
require("CanonicalBaseUrl final authority", "CanonicalBaseUrl::parse(&candidate)" in profiles)
require("explicit HTTP not upgraded", 'value.contains(\"://\")' in profiles)
require("controller and local IPC retained", "connect_to_server" in controller and "dispatch_command" in controller)
require("unknown outcome refresh retained", "controller.refresh_state();" in bridge)
require("profile identity preserved", "profile_id" in controller)
require("origin credential fencing retained", "cleanup" in host and "credential" in host)
require("QML has no HTTP client", "XMLHttpRequest" not in qml and "reqwest" not in qml)
require("QML has no profile storage", "ServerProfile" not in qml and "SQLite" not in qml)
require("no authentication input on fresh page", "enrollmentTokenField" not in fresh)
require("no library setup on fresh page", "libraryNameField" not in fresh)
require("no Phase-F marker", "V0.2 FIRST RUN EXPERIENCE READY" not in qml + bridge + controller + profiles)
print("connection-setup validation passed")
