#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
bridge="$root/crates/desktop/src/bridge.rs"
qml="$root/crates/desktop/qml/Main.qml"
native="$root/crates/desktop/src/native/tray.cpp"
appimage="$root/crates/install-engine/src/appimage.rs"
startup_store="$root/crates/client/src/config.rs"
rg -q 'launch_manager.ensure_running\(\)\.await' "$bridge"
rg -q 'Start Synveil when I sign in' "$qml"
rg -q 'confirmStartupChoice' "$qml"
rg -Fq 'pub const DEFAULT_STARTUP_PREFERENCE_FILE: &str = "startup-preference.conf";' "$startup_store"
rg -Fq '"version=1\nstate=enabled" => Ok(Self::Enabled)' "$startup_store"
rg -Fq 'write!(file, "version=1\nstate={state}\n")' "$startup_store"
rg -Fq 'fn startup_preference_is_explicit_durable_and_malformed_fails_safe()' "$startup_store"
rg -Fq 'fn confirm_startup_choice(self: Pin<&mut Self>, enabled: bool)' "$bridge"
rg -Fq 'store.persist(preference)' "$bridge"
rg -Fq 'apply_startup_choice(&manager, target).await' "$bridge"
rg -q 'spawn_blocking\(appimage_startup_status\)' "$bridge"
rg -q 'AppImageIntegrationStatus::NotIntegrated' "$bridge"
rg -q 'synveil-appimage-client.service' "$appimage"
! rg -q 'enable --now|/etc/systemd/system/synveil-client|/usr/lib/systemd/system/synveil-client' "$bridge" "$qml" "$native"
printf '%s\n' 'Prompt019 Linux first-launch validation passed.'
