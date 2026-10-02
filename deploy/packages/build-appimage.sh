#!/usr/bin/env bash
# Deterministic x86_64 AppImage builder for the two user-level Synveil processes.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd -P)"
# shellcheck source=deploy/packages/common/version.sh
source "${SCRIPT_DIR}/common/version.sh"
# shellcheck source=deploy/packages/common/reproducible.sh
source "${SCRIPT_DIR}/common/reproducible.sh"

LINUXDEPLOY_VERSION=1-alpha-20251107-1
LINUXDEPLOY_SHA256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
QT_PLUGIN_VERSION=1-alpha-20250213-1
QT_PLUGIN_SHA256=15106be885c1c48a021198e7e1e9a48ce9d02a86dd0a1848f00bdbf3c1c92724
OUTPUT_DIR="${REPO_ROOT}/target/packages"
TOOL_DIR="${REPO_ROOT}/target/appimage-tools"
DESKTOP_BINARY=""
CLIENT_BINARY=""
INTEGRATION_BINARY=""
BUILD_ONLY=0

usage() { cat <<'EOF'
Usage: build-appimage.sh [--output-dir=DIR] [--tool-dir=DIR]
       [--desktop-binary=PATH] [--client-binary=PATH] [--build-only]

Build Synveil-<workspace-version>-x86_64.AppImage and its exact release
manifest. Tools are version-pinned and SHA-256 verified before execution.
--build-only creates one artifact; the default independently rebuilds and
requires byte identity. Binary overrides must name real release ELF files.
EOF
}
while (($#)); do case "$1" in
  --output-dir=*) OUTPUT_DIR=${1#*=};; --tool-dir=*) TOOL_DIR=${1#*=};;
  --desktop-binary=*) DESKTOP_BINARY=${1#*=};; --client-binary=*) CLIENT_BINARY=${1#*=};;
  --build-only) BUILD_ONLY=1;; --help|-h) usage; exit 0;;
  *) printf '[synveil-appimage] ERROR: unknown argument: %s\n' "$1" >&2; exit 2;;
esac; shift; done

[[ $(uname -m) == x86_64 ]] || { echo '[synveil-appimage] ERROR: only Linux x86_64 is qualified' >&2; exit 1; }
[[ $OUTPUT_DIR == /* ]] || OUTPUT_DIR="${REPO_ROOT}/${OUTPUT_DIR}"
[[ $TOOL_DIR == /* ]] || TOOL_DIR="${REPO_ROOT}/${TOOL_DIR}"
[[ $OUTPUT_DIR != / && $OUTPUT_DIR != *'/../'* && $OUTPUT_DIR != */.. ]] || { echo '[synveil-appimage] ERROR: unsafe output directory' >&2; exit 1; }
VERSION=$(synveil_cargo_version)
SOURCE_DATE_EPOCH=$(synveil_source_date_epoch); export SOURCE_DATE_EPOCH
export ARCH=x86_64 VERSION="$VERSION" APPIMAGE_EXTRACT_AND_RUN=1
export CARGO_TARGET_DIR="${REPO_ROOT}/target"
ARTIFACT="Synveil-${VERSION}-x86_64.AppImage"
mkdir -p "$OUTPUT_DIR" "$TOOL_DIR"

fetch_tool() {
  local name=$1 url=$2 expected=$3 path="${TOOL_DIR}/$1"
  if [[ ! -f $path ]]; then curl --fail --location --proto '=https' --tlsv1.2 --output "${path}.part" "$url"; mv "${path}.part" "$path"; fi
  printf '%s  %s\n' "$expected" "$path" | sha256sum --check --status || { rm -f "$path"; echo "[synveil-appimage] ERROR: checksum mismatch: $name" >&2; exit 1; }
  chmod 0755 "$path"; printf '%s' "$path"
}
LINUXDEPLOY=$(fetch_tool linuxdeploy-x86_64.AppImage "https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_VERSION}/linuxdeploy-x86_64.AppImage" "$LINUXDEPLOY_SHA256")
QT_PLUGIN=$(fetch_tool linuxdeploy-plugin-qt-x86_64.AppImage "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/${QT_PLUGIN_VERSION}/linuxdeploy-plugin-qt-x86_64.AppImage" "$QT_PLUGIN_SHA256")
export LINUXDEPLOY_PLUGIN_QT="$QT_PLUGIN"

if [[ -z $DESKTOP_BINARY || -z $CLIENT_BINARY || -z $INTEGRATION_BINARY ]]; then
  synveil_prepare_reproducible_rust_build "$REPO_ROOT"
fi
if [[ -z $DESKTOP_BINARY ]]; then (cd "$REPO_ROOT" && cargo build --release --locked -p synveil-desktop); DESKTOP_BINARY="$REPO_ROOT/target/release/synveil-desktop"; fi
if [[ -z $CLIENT_BINARY ]]; then (cd "$REPO_ROOT" && cargo build --release --locked -p synveil-client); CLIENT_BINARY="$REPO_ROOT/target/release/synveil-client"; fi
if [[ -z $INTEGRATION_BINARY ]]; then (cd "$REPO_ROOT" && cargo build --release --locked -p synveil-install-engine --bin synveil-appimage-integration); INTEGRATION_BINARY="$REPO_ROOT/target/release/synveil-appimage-integration"; fi
for binary in "$DESKTOP_BINARY" "$CLIENT_BINARY" "$INTEGRATION_BINARY"; do
  [[ -f $binary && -x $binary ]] || { echo "[synveil-appimage] ERROR: production binary missing: $binary" >&2; exit 1; }
  synveil_assert_linux_release_binary "$binary" "$(basename "$binary")"
done

WORK=$(mktemp -d /tmp/synveil-appimage.XXXXXX)
trap 'rm -rf "$WORK"; rm -f "$OUTPUT_DIR/$ARTIFACT.part"' EXIT
build_one() {
  local label=$1 appdir="$WORK/$1/Synveil.AppDir" destination="$WORK/$1/$ARTIFACT"
  mkdir -p "$appdir/usr/bin"
  install -m0755 "$DESKTOP_BINARY" "$appdir/usr/bin/synveil-desktop"
  install -m0755 "$CLIENT_BINARY" "$appdir/usr/bin/synveil-client"
  install -m0755 "$INTEGRATION_BINARY" "$appdir/usr/bin/synveil-appimage-integration"
  install -m0644 "$SCRIPT_DIR/appimage/synveil.desktop" "$appdir/synveil.desktop"
  install -m0644 "$REPO_ROOT/deploy/icons/hicolor/scalable/apps/synveil.svg" "$appdir/synveil.svg"
  install -m0755 "$SCRIPT_DIR/appimage/AppRun" "$appdir/AppRun"
  QML_SOURCES_PATHS="$REPO_ROOT/crates/desktop/qml" "$LINUXDEPLOY" --appdir "$appdir" \
    --executable "$appdir/usr/bin/synveil-desktop" --executable "$appdir/usr/bin/synveil-client" --executable "$appdir/usr/bin/synveil-appimage-integration" \
    --desktop-file "$appdir/synveil.desktop" --icon-file "$appdir/synveil.svg" --plugin qt
  # linuxdeploy may regenerate AppRun while deploying the desktop entry. The
  # reviewed entry point is part of the product contract, so restore it after
  # deployment and validate the final AppDir before filesystem creation.
  rm -f "$appdir/AppRun"
  install -m0755 "$SCRIPT_DIR/appimage/AppRun" "$appdir/AppRun"
  # Normalize all payload timestamps before filesystem creation.
  find "$appdir" -print0 | xargs -0 touch --no-dereference --date="@${SOURCE_DATE_EPOCH}"
  (cd "$WORK/$label" && OUTPUT="$destination" "$LINUXDEPLOY" --appdir "$appdir" --output appimage)
  [[ -s $destination ]] || { echo '[synveil-appimage] ERROR: AppImage tool produced no artifact' >&2; exit 1; }
  python3 "$REPO_ROOT/scripts/validate-appimage-build.py" --appdir "$appdir"
}
build_one primary
if ((BUILD_ONLY == 0)); then
  build_one rebuilt
  cmp -s "$WORK/primary/$ARTIFACT" "$WORK/rebuilt/$ARTIFACT" || { sha256sum "$WORK"/*/"$ARTIFACT" >&2; echo '[synveil-appimage] ERROR: independent AppImages differ' >&2; exit 1; }
fi
cp "$WORK/primary/$ARTIFACT" "$OUTPUT_DIR/$ARTIFACT.part"
chmod 0755 "$OUTPUT_DIR/$ARTIFACT.part"; mv "$OUTPUT_DIR/$ARTIFACT.part" "$OUTPUT_DIR/$ARTIFACT"
COMMIT=$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || printf '%040d' 0)
python3 "$REPO_ROOT/scripts/release_manifest.py" create --artifact-root "$OUTPUT_DIR" --product-version "$VERSION" --source-commit "$COMMIT" \
  --output "$OUTPUT_DIR/SYNVEIL-RELEASE-MANIFEST.json" \
  --artifact "{\"id\":\"appimage-linux-x86_64\",\"artifact_type\":\"appimage\",\"filename\":\"$ARTIFACT\",\"platform\":\"linux\",\"architecture\":\"x86_64\",\"role\":\"portable\",\"components\":[\"synveil-desktop\",\"synveil-client\"]}"
python3 "$REPO_ROOT/scripts/validate-appimage-build.py" --artifact "$OUTPUT_DIR/$ARTIFACT" --manifest "$OUTPUT_DIR/SYNVEIL-RELEASE-MANIFEST.json"
sha256sum "$OUTPUT_DIR/$ARTIFACT"
