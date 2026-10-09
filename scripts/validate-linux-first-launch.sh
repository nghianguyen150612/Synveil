#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bridge="$root/crates/desktop/src/bridge.rs"
qml="$root/crates/desktop/qml/Main.qml"
native="$root/crates/desktop/src/native/tray.cpp"
appimage="$root/crates/install-engine/src/appimage.rs"
preferences="$root/crates/client/src/config.rs"
rg -q 'launch_manager.ensure_running\(\)\.await' "$bridge"
rg -q 'Start Synveil when I sign in' "$qml"
rg -q 'confirmStartupChoice' "$qml"
# The current canonical startup choice owner is the client preference store,
# not the historical Qt settings key removed by later first-run work.
rg -Fq '"version=1\nstate=enabled"' "$preferences"
rg -Fq '"version=1\nstate=disabled"' "$preferences"
rg -Fq 'StartupPreferenceStore::current()' "$bridge"
rg -Fq 'store.persist(preference)' "$bridge"
rg -Fq 'DesktopClientConfigError::ConfigurationMalformed' "$preferences"
rg -q 'spawn_blocking\(appimage_startup_status\)' "$bridge"
rg -q 'AppImageIntegrationStatus::NotIntegrated' "$bridge"
rg -q 'synveil-appimage-client.service' "$appimage"
! rg -q 'enable --now|/etc/systemd/system/synveil-client|/usr/lib/systemd/system/synveil-client' "$bridge" "$qml" "$native"
printf '%s\n' 'Prompt019 Linux first-launch validation passed.'
