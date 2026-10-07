#!/usr/bin/env python3
"""Structural P042 guardrails; behavioral and native evidence stay separate."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
def read(path):
    return (ROOT / path).read_text()
def require(label, condition):
    if not condition:
        raise SystemExit(f"repair/recovery invariant failed: {label}")
def section(source, start, end):
    return source.split(start, 1)[1].split(end, 1)[0]

presentation = read("crates/desktop/src/presentation.rs")
bridge = read("crates/desktop/src/bridge.rs")
qml = read("crates/desktop/qml/Main.qml")
launch = read("crates/client/src/launch.rs")
controller = read("crates/client/src/controller.rs")
host = read("crates/client-sync/src/host.rs")
runtime = read("crates/client-sync/src/runtime.rs")
root = read("crates/client-sync/src/replica.rs")
contract = read("docs/v0.2/REPAIR_RECOVERY_UX.md")

require("one canonical recovery projection", "snapshot.recovery_summary()" in presentation and "pub fn recovery_summary" in controller)
require("Rust owns semantic state/capability", all(s in presentation for s in ("pub enum RecoveryState", "pub enum RecoveryCapability", "GuidanceOnly", "Unavailable", "Supported")))
repair = section(presentation, "pub fn repair_presentation", "/// An action rendered")
require("unresolved repair cannot claim direct support", "RecoveryCapability::Supported" not in repair)
require("same-version repair remains native-owned guidance", "same installed version" in repair and "package manager" in repair and "AppImage" in repair and "installer" in repair)
require("QML gates repair guidance", 'bridge.repair_capability === "guidance_only"' in qml)
require("QML gates supported folder check", 'modelData.capability === "supported"' in qml)
for name in ("recoveryPage", "recoverySummary", "repairSynveilButton", "reconnectServerButton", "restoreMissingFolderButton", "restartBackgroundServiceButton", "recoveryBusyIndicator", "recoveryFeedbackLabel"):
    require(f"stable accessible object {name}", f'"{name}"' in qml)
require("root recheck stays with canonical runtime", "controller.retry_recovery(library_id).await" in bridge and "self.sync_now(library_id).await" in controller)
require("root loss remains fenced", "RootUnavailable" in runtime and "WrongRootBinding" in runtime and "RootAvailability::Unavailable" in host and "marker" in root)
require("missing root is never an empty tree", "ROOT UNAVAILABLE != EMPTY TREE" in contract and "ROOT UNAVAILABLE != DELETE EVERYTHING" in contract)
require("same-root product guidance", "original folder" in presentation)
root_action = section(bridge, "fn request_recovery_retry(", "fn request_background_restart(")
require("root recovery has no filesystem/binding mutation", all(term not in root_action for term in ("create_dir", "remove_file", "remove_dir", "write_marker", "bind_replica", "setup_library", "open_folder")))
require("freshness/revision/generation action fences", all(s in presentation for s in ("recovery_mutation_allowed", "current.revision == presented.revision", "current.connection_generation == presented.connection_generation", "DesktopControllerFreshness::Fresh")))
require("unknown profile/check admission survives result callback", bridge.count("if !outcome_unknown") >= 2 and "configuration_reconcile" in bridge and "recovery_reconcile" in bridge and "ReconciliationFence" in bridge)
require("fresh authoritative refresh releases unknown admission", "configuration_gate.release()" in bridge and "recovery_gate.release()" in bridge and "f.satisfied(snapshot.connection_generation, snapshot.revision, fresh)" in bridge)
require("restart belongs to launch manager", "pub async fn restart(&self)" in launch and "manager.restart().await" in bridge)
restart = section(launch, "pub async fn restart(&self)", "pub async fn autostart_status")
require("restart shares bounded launch gate", "gate.in_flight" in restart and "retry_after" in restart and "time::timeout" in restart)
require("uncertain stop/start inspect before replay", "RestartPhase::Stopping" in restart and "RestartPhase::Starting" in restart and "stopped_for_restart().await" in restart and "self.backend.inspect().await" in restart)
require("resolved status checks cannot start a new restart", "status_check && pending.is_none()" in restart and "manager.check_restart_status().await" in bridge)
require("late uncertain callback retains authoritative readiness", "actions::restart_reconciliation_pending" in bridge and "p042_status_check_after_readiness_never_restarts" in launch)
require("stop proof precedes launch", restart.index("stopped_for_restart().await") < restart.index("self.ensure_inner().await"))
require("supervised restart matches the kernel control peer", "client.peer_process_id() == Some(service_pid)" in launch and "linux_running_service_pid" in launch)
require("native full-stop proof is explicit", all(s in launch for s in ("ActiveState=inactive", "SubState=dead", "MainPID=0", "ControlPID=0", "linux_restart_stop_proven")))
require("canonical executable resolver remains", "packaged_client_path_from(desktop)?" in launch)
require("QML owns no transport/database/process supervisor", all(s not in qml for s in ("XMLHttpRequest", "QNetworkAccessManager", "SQLite", "SecretStore", "LocalStateStore", "QProcess", "Qt.createQmlObject")))
require("no unsafe kill/shell reset implementation", not re.search(r'\b(killall|pkill|taskkill)\b|Command::new\("(?:sh|bash|cmd|powershell)"\)', bridge + restart))
require("no destructive product reset/relocation", all(s not in (qml + presentation).lower() for s in ("reset synveil", "factory reset", "delete library", "relocate root", "delete all state")))
require("first-sync evidence untouched", "first_sync_completion_proven" in presentation and "record_quiescent" in runtime)
require("pause/attention/setup keep existing owners", all(s in bridge for s in ("controller.pause_sync()", "controller.resolve_conflict", "resume_pending_setup")))
require("P043 explicitly deferred", "P043 is explicitly deferred" in contract)
require("P042 retains its bounded ownership after P043", "Desktop performs no repair mutation." in contract)
require("focused behavioral coverage", "p042_unknown_stop" in launch and "p042_unknown_start" in launch and "p042_same_root_restore" in presentation)
print("repair and recovery UX validation passed")
