#!/usr/bin/env python3
"""Network-free structural validation of the Prompt039 authentication boundary."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(label: str, condition: bool) -> None:
    if not condition:
        raise SystemExit(f"authentication UX validation failed: {label}")
    print(f"ok: {label}")


qml = read("crates/desktop/qml/Main.qml")
bridge = read("crates/desktop/src/bridge.rs")
presentation = read("crates/desktop/src/presentation.rs")
controller = read("crates/client/src/controller.rs")
control = read("crates/client/src/control.rs")
remote = "\n".join(
    path.read_text(encoding="utf-8")
    for path in (ROOT / "crates/client-sync/src/http_remote").glob("*.rs")
)

for name in (
    "authenticationPage", "deviceCodeField", "signInButton",
    "authenticationFeedbackLabel", "authenticationBusyIndicator",
    "signOutButton", "signOutConfirmation", "confirmSignOutButton",
    "cancelSignOutButton",
):
    require(f"stable object name {name}", f'objectName: "{name}"' in qml)

require("existing Authenticate and SignOut retained", "authenticate(" in controller and "sign_out(" in controller)
require("canonical enrollment route retained", "/api/v1/device-enrollment/exchange" in remote)
require("no password or browser auth UI", all(term not in qml for term in ("Account password", "OAuth", "OIDC", "Open browser")))
require("desktop GUI has no HTTP auth client", "reqwest" not in qml + bridge and "XMLHttpRequest" not in qml)
require("desktop GUI has no SecretStore owner", "use synveil_client_sync::SecretStore" not in bridge and "SecretStore" not in qml)
require("transient consumer terminology", 'qsTr("Device setup code")' in qml and 'qsTr("Enrollment token")' not in qml)
require("69-character presentation bound", "maximumLength: 69" in qml)
require("masked sensitive input", "echoMode: TextInput.Password" in qml and "Qt.ImhSensitiveData" in qml)
require("secret cleared before dispatch", qml.index("clear()\n                                        bridge.authenticate(token)") > qml.index("var token = text"))
require("no durable QML secret state", "Qt.labs.settings" not in qml and "property string deviceCode" not in qml)
for result in ("Authenticated", "SignedOut", "InvalidCredentials", "NetworkUnavailable", "ServerUnavailable", "RateLimited", "SecureStoreUnavailable", "Busy", "ProtocolError", "OutcomeUnknown"):
    require(f"typed mapping {result}", f"DesktopControllerCommandResult::{result}" in presentation)
require("unknown outcome refresh and no replay", "if outcome_unknown" in bridge and "controller.refresh_auth_state();" in bridge)
require("single-operation admission retained", "auth_in_flight" in controller and "auth_gate" in bridge)
require("explicit confirmed sign out", "signOutConfirmation.open()" in qml and "bridge.signOut()" in qml)
require("profile reset absent from sign out", "reset_profile" not in bridge[bridge.index("fn request_sign_out"):])
require("library boundary retained", "library_setup_required" in bridge)
require("bounded control input retained", "DEVICE_SECRET_ENCODED_BYTES" in control)
require("no Phase-F marker in implementation", "V0.2 FIRST RUN EXPERIENCE READY" not in qml + bridge + presentation + controller)
print("authentication UX validation passed")
