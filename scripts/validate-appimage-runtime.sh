#!/usr/bin/env bash
# Real-artifact P016 lifecycle acceptance in an isolated user environment.
set -euo pipefail
[[ $# == 1 && -x $1 ]] || { echo 'usage: validate-appimage-runtime.sh ARTIFACT' >&2; exit 2; }
source_artifact=$(realpath -- "$1")
work=$(mktemp -d /tmp/synveil-appimage-runtime.XXXXXX)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work"/{home,config,data,state,cache,runtime}
artifact="$work/Synveil initial.AppImage"
cp "$source_artifact" "$artifact"; chmod 0755 "$artifact"
export HOME="$work/home" XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export XDG_STATE_HOME="$work/state" XDG_CACHE_HOME="$work/cache" XDG_RUNTIME_DIR="$work/runtime"
export APPIMAGE_EXTRACT_AND_RUN=1

# Portable status inspection has no mutation boundary.
[[ $("$artifact" --synveil-integration-status) == NotIntegrated ]]
[[ ! -e "$XDG_DATA_HOME/applications/synveil-appimage.desktop" ]]
printf 'durable user state\n' >"$XDG_CONFIG_HOME/preserve"
"$artifact" --synveil-integrate
"$artifact" --synveil-integrate
[[ $("$artifact" --synveil-integration-status) == Healthy ]]
desktop="$XDG_DATA_HOME/applications/synveil-appimage.desktop"
icon="$XDG_DATA_HOME/icons/hicolor/scalable/apps/synveil.svg"
record="$XDG_STATE_HOME/synveil/appimage-integration.json"
unit="$XDG_CONFIG_HOME/systemd/user/synveil-appimage-client.service"
for path in "$desktop" "$icon" "$record" "$unit"; do [[ -f $path ]]; done
grep -Fq "Exec=\"$artifact\"" "$desktop"
grep -Fq "ExecStart=\"$artifact\" --synveil-client" "$unit"
! grep -Eq '(/tmp/\.mount_|password|token|secret)' "$desktop" "$unit" "$record"
[[ ! -e "$XDG_CONFIG_HOME/systemd/user/default.target.wants/synveil-appimage-client.service" ]]

replacement="$work/Synveil replacement 雪.AppImage"
cp "$artifact" "$replacement"; chmod 0755 "$replacement"
rm "$artifact"
[[ $("$replacement" --synveil-integration-status) == StaleAppImagePath ]]
"$replacement" --synveil-integration-repair
[[ $("$replacement" --synveil-integration-status) == Healthy ]]
grep -Fq "Exec=\"$replacement\"" "$desktop"
rm "$icon"
[[ $("$replacement" --synveil-integration-status) == NeedsRepair ]]
"$replacement" --synveil-integration-repair
"$replacement" --synveil-integration-remove
[[ $("$replacement" --synveil-integration-status) == NotIntegrated ]]
[[ -f "$XDG_CONFIG_HOME/preserve" && -f "$replacement" ]]
printf 'APPIMAGE-RUNTIME-1..24 real lifecycle: PASS\n'
