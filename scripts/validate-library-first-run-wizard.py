#!/usr/bin/env python3
"""Network-free structural checks for the v0.2 P040 first-library wizard."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(label: str, condition: bool) -> None:
    if not condition:
        raise SystemExit(f"library-first-run validation failed: {label}")
    print(f"ok: {label}")


qml = read("crates/desktop/qml/Main.qml")
bridge = read("crates/desktop/src/bridge.rs")
presentation = read("crates/desktop/src/presentation.rs")
controller = read("crates/client/src/controller.rs")
control = read("crates/client/src/control.rs")
host = read("crates/client-sync/src/host.rs")
remote = read("crates/client-sync/src/http_remote.rs")
observation = read("crates/client-sync/src/observation.rs")
adr = read("docs/adr/ADR-045-existing-root-bootstrap-and-initial-upload-admission.md")
roadmap = read("docs/v0.2/ROADMAP.md")

wizard_start = qml.index('id: libraryFirstRunPage')
wizard_end = qml.index('\n    }', wizard_start)
wizard = qml[wizard_start:wizard_end]
dialog_start = qml.index('FolderDialog {')
dialog_end = qml.index('\n    }', dialog_start)
dialog = qml[dialog_start:dialog_end]

for name in (
    "libraryFirstRunPage", "libraryNameField", "chooseLibraryFolderButton",
    "createLibraryButton", "librarySetupBusyIndicator", "librarySetupFeedbackLabel",
):
    require(f"stable QML object name {name}", f'objectName: "{name}"' in wizard)

require("authoritative P040 route", "visible: bridge.library_setup_required" in wizard)
require("normal app waits for first library route", "&& !bridge.library_setup_required" in qml)
require("fresh authenticated zero-library state owns routing", "presentation::library_first_run_required(" in bridge)
require("P038/P039 stay ahead of library setup", "profile_configured" in presentation and "profile_authenticated" in presentation)
require("native directory picker retained", "FolderDialog {" in qml and "selectedFolder.toString()" in dialog)
require("picker cancellation does not submit setup", "onRejected:" in dialog and "setupLibrary" not in dialog)
require("selection alone has no setup request", "setLibraryFolder(selectedFolder.toString())" in dialog and "setupLibrary" not in dialog)
require("one name field and no raw folder path field", wizard.count("TextField {") == 1 and "libraryRootField" not in qml)
require("name and folder must be selected before creation", "libraryNameField.text.trim().length > 0" in wizard and "bridge.library_setup_folder.length > 0" in wizard)
require("busy state disables repeat input", wizard.count("enabled: !bridge.library_setup_busy") >= 3 and "BusyIndicator" in wizard)
require("selected folder has an accessible description", "Accessible.description: text" in wizard)
require("canonical bridge setup command retained", "bridge.setupLibrary(libraryNameField.text)" in qml)
require("controller remains canonical setup owner", "controller.setup_library(name, root_path).await" in bridge and "client.setup_library" in controller)
require("client retains path and root safety checks", "validate_onboarding_root(&root)" in control and "root_overlaps_existing" in control)
require("interrupted setup reuses the pending root identity", "pending_library_for_root" in control and "pending_id.unwrap_or_else(LibraryId::new)" in control)
require("setup admission remains bounded", "gate.try_acquire()" in bridge and "library_setup_in_flight: AtomicBool" in control)
require("success waits for authoritative confirmation", "library_setup_awaiting_confirmation" in bridge and "state.presented.revision" in bridge)
require("confirmation requires a newer fresh snapshot", "library_setup_confirmation_finished(" in bridge and "snapshot_revision > submitted_revision" in presentation)
require("no immediate success-route synthesis", "set_library_setup_required(false)" not in bridge)
require("unknown result uses bounded presentation", 'code: "checking"' in presentation and 'action: "wait"' in presentation)
setup_request = bridge[bridge.index("fn request_library_setup"):bridge.index("fn set_library_setup_feedback_copy")]
require("setup command is dispatched once per submission", setup_request.count("controller.setup_library(") == 1)
require("response uncertainty triggers status refresh", "refresh_controller.refresh_state();" in setup_request)
require("remote metadata excludes local root path", '"name": name.as_str()' in remote and '"root_path"' not in remote[remote.index("pub async fn create_library"):remote.index("pub async fn list_libraries")])
adr_text = " ".join(adr.lower().split())
require("existing ordinary files are admitted without delete intents", "ordinary non-empty content is not itself an error" in adr_text and "existing_tree_scan_creates_content_intents_without_deletes" in observation)
require("incompatible control-tree safety remains client-owned", "validate_existing_marker_for_profile" in host and "control-tree collision" in adr)
require("no mass-deletion interpretation added", "never generates a delete intent for an unknown path" in adr_text)
require("GUI close remains separate from client/runtime ownership", "closing the GUI does not stop the client" in read("docs/en/DESKTOP_CONTROL.md"))
require("QML has no HTTP transport", all(term not in qml for term in ("XMLHttpRequest", "QNetworkAccessManager", "reqwest")))
require("QML has no persistence or credential owner", all(term not in qml for term in ("SQLite", "LocalStateStore", "SecretStore")))

for forbidden in ("replica", "root node", "UUID", "manifest", "journal", "generation", "cursor", "watcher", "object prefix", "LocalStateStore", "SyncRuntime", "IPC", "PostgreSQL"):
    require(f"ordinary wizard copy excludes {forbidden}", forbidden.lower() not in wizard.lower())
require("ordinary wizard copy never advises deleting files", "delete" not in wizard.lower())

implementation = qml + bridge + presentation + controller
require("P041 progress implementation remains absent", "installation-to-sync progress" not in implementation.lower())
require("Phase-F completion marker remains absent from implementation", "V0.2 FIRST RUN EXPERIENCE READY" not in implementation)
require("P040 roadmap scope stays distinct from P041/P042", "### P040 — Library First-Run Wizard" in roadmap and "P041 owns" not in roadmap[roadmap.index("### P040 — Library First-Run Wizard"):roadmap.index("### P041 — Installation-to-Sync Progress Experience")])
print("library first-run wizard validation passed")
