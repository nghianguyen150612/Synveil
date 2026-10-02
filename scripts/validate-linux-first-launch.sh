#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bridge="$root/crates/desktop/src/bridge.rs"
qml="$root/crates/desktop/qml/Main.qml"
native="$root/crates/desktop/src/native/tray.cpp"
appimage="$root/crates/install-engine/src/appimage.rs"
rg -q 'launch_manager.ensure_running\(\)\.await' "$bridge"
rg -q 'Start Synveil when I sign in' "$qml"
rg -q 'confirmStartupChoice' "$qml"
rg -q 'startupChoiceVersion.*1' "$native"
rg -q 'native_desktop_settings_save_startup_choice' "$bridge"
rg -q 'spawn_blocking\(appimage_startup_status\)' "$bridge"
rg -q 'AppImageIntegrationStatus::NotIntegrated' "$bridge"
rg -q 'synveil-appimage-client.service' "$appimage"
! rg -q 'enable --now|/etc/systemd/system/synveil-client|/usr/lib/systemd/system/synveil-client' "$bridge" "$qml" "$native"
printf '%s\n' 'Prompt019 Linux first-launch validation passed.'
