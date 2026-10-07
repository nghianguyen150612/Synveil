#!/usr/bin/env python3
"""Network-free structural contract checks for Prompt037."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


qml = read("crates/desktop/qml/Main.qml")
bridge = read("crates/desktop/src/bridge.rs")
presentation = read("crates/desktop/src/presentation.rs")
contract = read("docs/v0.2/UNIFIED_WELCOME_EXPERIENCE.md")
adr = read("docs/adr/ADR-067-v0.2-unified-first-run-welcome.md")


def require(label: str, condition: bool) -> None:
    if not condition:
        raise SystemExit(f"unified-welcome validation failed: {label}")
    print(f"ok: {label}")


for value in (
    'qsTr("Host Synveil',
    'qsTr("Connect to Synveil',
    'objectName: "welcomePage"',
    'objectName: "hostSynveilButton"',
    'objectName: "connectToSynveilButton"',
    'Accessible.name: qsTr("Host Synveil")',
    'Accessible.name: qsTr("Connect to Synveil")',
):
    require(value, value in qml)

startup_choice = qml[qml.index('id: startupChoicePopup') : qml.index('id: signOutConfirmation')]
startup_choice_popup, startup_choice_layout = startup_choice.split("ColumnLayout {", 1)
startup_choice_layout = startup_choice_layout.split("\n        }", 1)[0]
require(
    "startup choice accessibility label is attached to an Item",
    'Accessible.name: qsTr("Choose whether Synveil starts when you sign in")' in startup_choice_layout
    and "Accessible.name:" not in startup_choice_popup,
)

welcome = qml[qml.index("id: welcomePage") : qml.index("id: hostSetupPage")]
for forbidden in ("serverUrlField", "Password", "PostgreSQL", "DATABASE_URL", "systemd", "Caddy"):
    require(f"Welcome excludes {forbidden}", forbidden not in welcome)

require("typed presentation enum", "enum WelcomeDestination" in presentation)
require("configured profile bypass", "if profile_configured" in presentation)
require("partial Host resume", "HostSetupState::StorageReady" in presentation and "HostResume" in presentation)
require("explicit P036 Host intent", ".begin_host();" in bridge)
connect_body = bridge[bridge.index("fn choose_connect") : bridge.index("fn show_welcome")]
require("Connect invokes no Host owner", "host_coordinator" not in connect_body and "begin_host" not in connect_body)
require("Connect retains controller authority", "DesktopController" in bridge and "configure_profile" in bridge)
require("no GUI HTTP client", "reqwest" not in bridge and "XMLHttpRequest" not in qml)
combined = qml + bridge + presentation
for forbidden in ("welcome_completed", "welcome_seen", "first_run_done"):
    require(f"no durable {forbidden}", forbidden not in combined)

require("server/client storage distinction", "Server data" in contract or "server data" in contract)
for prompt in ("P038", "P039", "P040", "P041", "P042"):
    require(f"{prompt} boundary", prompt in contract and prompt in adr)
require("P036 blocked status retained", "BLOCKED_BY_ENVIRONMENT_AND_PRODUCTION_ARTIFACTS" in contract)
require("P036 readiness marker not emitted", "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" not in combined)
print("unified-welcome validation passed")
