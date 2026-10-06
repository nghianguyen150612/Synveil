#!/usr/bin/env python3
"""Network-free structural checks for the v0.2 P041 progress projection."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(label: str, condition: bool) -> None:
    if not condition:
        raise SystemExit(f"installation-to-sync validation failed: {label}")
    print(f"ok: {label}")


qml = read("crates/desktop/qml/Main.qml")
bridge = read("crates/desktop/src/bridge.rs")
presentation = read("crates/desktop/src/presentation.rs")
controller = read("crates/client/src/controller.rs")
control = read("crates/client/src/control.rs")
runtime = read("crates/client-sync/src/runtime.rs")
sync_cycle = read("crates/client-sync/src/sync_cycle.rs")
host = read("crates/client-sync/src/host.rs")
state = read("crates/client-sync/src/state.rs")
roadmap = read("docs/v0.2/ROADMAP.md")
progress_doc = read("docs/v0.2/INSTALLATION_TO_SYNC_PROGRESS.md")
adr = read("docs/adr/ADR-071-v0.2-installation-to-sync-progress.md")

progress_start = qml.index('id: firstRunProgressPage')
progress_end = qml.index('id: libraryFirstRunPage', progress_start)
progress_qml = qml[progress_start:progress_end]

for stage_id in ("app_ready", "server_ready", "signed_in", "library_ready", "first_sync"):
    require(f"canonical stage identifier {stage_id}", f'"{stage_id}"' in presentation)
for state_code in ("pending", "active", "waiting", "complete", "action_required"):
    require(f"finite stage state {state_code}", f'"{state_code}"' in presentation)

require("Rust owns the progress model", "pub struct FirstRunProgress" in presentation)
require("QML consumes the typed stage list", "first_run_progress_stages" in bridge and "model: bridge.first_run_progress_stages" in progress_qml)
require("installation telemetry is not fabricated", "no native installer telemetry" in presentation.lower() and "Synveil ready" in presentation)
require("no installer replacement was added", "installer" not in progress_qml.lower() and "Inno Setup" not in qml)

for source, label in ((state, "state evidence"), (runtime, "runtime evidence"), (control, "control evidence"), (controller, "controller evidence"), (presentation, "presentation evidence")):
    require(label, "first_sync_completed" in source)
require("quiescent hook exists", "async fn record_quiescent" in runtime and "record_quiescent" in host)
require("true idle proof is used", "pub const fn is_idle" in sync_cycle and "result.is_idle()" in runtime and "first_sync_completion_proven" in presentation)
require("pending wake blocks durable completion", "pending.is_none()" in runtime and "pending_wake.is_some()" in runtime)
require("progress remains active", "DesktopControllerSyncOutcome::Progress" in presentation and "FirstRunProgressStageState::Active" in presentation)
require("progress is not completion", "last_outcome == Some(DesktopControllerSyncOutcome::Progress)" in presentation)
require("more-work scheduling remains bounded", "more_work_likely" in sync_cycle and "FAIR_FOLLOW_UP_DELAY" in runtime)
require("freshness is required for completion", "DesktopControllerFreshness::Fresh" in presentation and "snapshot.freshness" in presentation)
require("revision and generation ordering is present", "snapshot_is_acceptable" in presentation and "connection_generation" in bridge)
require("controller latest-value guard is retained", "publish_fresh" in controller)

require(
    "no numeric progress percentage",
    not re.search(r"\b\d+\s*%", progress_qml + progress_doc),
)
require("no progress timer", "Timer" not in progress_qml and "setInterval" not in progress_qml)
require("no QML sync loop", "while (" not in progress_qml and "for (" not in progress_qml)
require("no QML HTTP or database owner", all(term not in qml for term in ("XMLHttpRequest", "QNetworkAccessManager", "SQLite", "LocalStateStore", "SecretStore")))
require("no P042 repair action surface", all(term not in progress_qml for term in ("Repair Synveil", "Reconnect server", "Restore missing folder", "Restart background service")))
require("no destructive reset or relocation action", all(term not in (qml + presentation).lower() for term in ("reset synveil", "relocate root", "delete library")))

for name in (
    "firstRunProgressPage",
    "firstRunProgressList",
    "firstRunProgressStatus",
    "firstRunProgressAction",
    "appReadyProgressStage",
    "serverReadyProgressStage",
    "signedInProgressStage",
    "libraryReadyProgressStage",
    "firstSyncProgressStage",
):
    require(f"stable accessibility object name {name}", f'"{name}"' in qml)
require("stage accessibility names are semantic", "Accessible.name" in progress_qml and "stateLabel" in progress_qml)
require("progress page has bounded product copy", "Getting Synveil ready" in progress_qml and "Syncing your files…" in presentation)
require("engineering terms stay out of stage copy", all(term not in progress_qml.lower() for term in ("rebaseline", "outbound intent", "wake_pending", "checkpoint", "sqlite", "ipc")))

require("new bindings start first sync unproven", "first_sync_completed\n             ) VALUES (?, ?, ?, ?, ?, ?, ?, 0)" in state)
require("legacy bindings receive migration default", "DEFAULT 1" in read("crates/client-sync/migrations/0008_first_sync_completion.sql"))
require("P040 remains the library owner", "library_setup_required" in qml and "bind_replica" in host)
require("existing libraries are not forced into the page", "if !snapshot" in presentation and "first_sync_completed" in presentation)
require("P042 remains deferred", "P042" in progress_doc and "P042" in roadmap)
require("Phase-F checkpoint is not claimed", "V0.2 FIRST RUN EXPERIENCE READY" not in (qml + bridge + presentation + controller + control + runtime + host + progress_doc + adr + roadmap))

print("installation-to-sync progress validation passed")
