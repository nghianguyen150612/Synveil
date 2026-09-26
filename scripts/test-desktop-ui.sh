#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
# Use the same path-remapped, non-incremental release environment as package
# production builds so the release smoke binary is an auditable artifact.
# shellcheck source=deploy/packages/common/reproducible.sh
source "$repo_root/deploy/packages/common/reproducible.sh"
synveil_prepare_reproducible_rust_build "$repo_root"

cargo fmt --manifest-path crates/desktop/Cargo.toml -- --check

if [[ -n "${SYNVEIL_QMAKE:-}" && -x "${SYNVEIL_QMAKE}" ]]; then
    qt_query_bin="$SYNVEIL_QMAKE"
elif command -v qmake6 >/dev/null 2>&1; then
    qt_query_bin="$(command -v qmake6)"
elif [[ -x /usr/lib/qt6/bin/qmake6 ]]; then
    qt_query_bin=/usr/lib/qt6/bin/qmake6
elif [[ -x /usr/lib/qt6/bin/qmake ]]; then
    qt_query_bin=/usr/lib/qt6/bin/qmake
elif command -v qmake >/dev/null 2>&1; then
    qt_query_bin="$(command -v qmake)"
else
    echo "A Qt 6 qmake executable is required for the desktop gate." >&2
    exit 1
fi
qt_bin_dir="$(cd -- "$(dirname -- "$qt_query_bin")" && pwd)"
case ":$PATH:" in
    *":$qt_bin_dir:"*) ;;
    *) export PATH="$qt_bin_dir:$PATH" ;;
esac
qt_version="$($qt_query_bin -query QT_VERSION)"
qt_major="${qt_version%%.*}"
qt_minor="${qt_version#*.}"
qt_minor="${qt_minor%%.*}"
if [[ "$qt_major" != 6 || "$qt_minor" -lt 4 ]]; then
    echo "Qt 6.4 or newer is required; found $qt_version." >&2
    exit 1
fi

cargo test -p synveil-desktop --locked
cargo clippy -p synveil-desktop --all-targets --all-features --locked -- -D warnings
cargo build -p synveil-desktop --locked
cargo build -p synveil-desktop --release --locked

if [[ -x /usr/lib/qt6/bin/qmllint ]]; then
    qmllint_bin=/usr/lib/qt6/bin/qmllint
elif [[ -x "$qt_bin_dir/qmllint" ]]; then
    qmllint_bin="$qt_bin_dir/qmllint"
elif command -v qmllint >/dev/null 2>&1; then
    qmllint_bin="$(command -v qmllint)"
else
    echo "A Qt 6 qmllint executable is required for the desktop gate." >&2
    exit 1
fi

module_qmldir="$(find target/debug/build \
    -type f \
    -path '*/out/qt-build-utils/qml_modules/com/synveil/desktop/qmldir' \
    -printf '%T@ %p\n' | sort -n | tail -n 1 | cut -d' ' -f2-)"
if [[ -z "$module_qmldir" ]]; then
    echo "CXX-Qt did not generate the desktop QML module metadata." >&2
    exit 1
fi
module_dir="$(dirname "$module_qmldir")"

lint_root="$(mktemp -d)"
lint_module="$lint_root/com/synveil/desktop"
mkdir -p "$lint_module/qml"
cp crates/desktop/qml/Main.qml "$lint_module/qml/Main.qml"
cp "$module_dir/qmldir" "$lint_module/qmldir"
cp "$module_dir/plugin.qmltypes" "$lint_module/plugin.qmltypes"

qt_qml_dir="$($qt_query_bin -query QT_INSTALL_QML)"
"$qmllint_bin" \
    -I "$qt_qml_dir" \
    -I "$lint_root" \
    "$lint_module/qml/Main.qml"

smoke_root="$(mktemp -d)"
trap 'rm -rf -- "$lint_root" "$smoke_root"' EXIT
mkdir -p "$smoke_root/config" "$smoke_root/data" "$smoke_root/cache" "$smoke_root/runtime"
chmod 700 "$smoke_root/runtime"

run_offscreen_smoke() {
    local binary="$1"
    (
        cd -- "$smoke_root"
        SYNVEIL_CONFIG_DIR="$smoke_root/config" \
        SYNVEIL_DATA_DIR="$smoke_root/data" \
        SYNVEIL_CACHE_DIR="$smoke_root/cache" \
        SYNVEIL_RUNTIME_DIR="$smoke_root/runtime" \
        SYNVEIL_CLIENT_CONFIG="$smoke_root/config/client.conf" \
        QT_QPA_PLATFORM=offscreen \
        QML_DISABLE_DISK_CACHE=1 \
            timeout 15s "$binary" --qml-smoke-test
    )
}

run_offscreen_smoke "$repo_root/target/debug/synveil-desktop"
run_offscreen_smoke "$repo_root/target/release/synveil-desktop"
