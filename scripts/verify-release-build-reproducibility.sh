#!/usr/bin/env bash
# Rebuild release executables in a fresh target directory and compare exact bytes.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd -P)"
# shellcheck source=deploy/packages/common/reproducible.sh
source "${REPO_ROOT}/deploy/packages/common/reproducible.sh"
# shellcheck source=deploy/packages/common/version.sh
source "${REPO_ROOT}/deploy/packages/common/version.sh"

if [[ $# -gt 0 ]]; then
    printf '[synveil-artifact] ERROR: this verifier takes no arguments\n' >&2
    exit 2
fi

cd "$REPO_ROOT"
host="$(rustc -vV | sed -n 's/^host: //p' | head -n 1)"
case "$host" in
    *-linux-*)
        platform=linux
        artifact_names=(
            synveil-scheduled-maintenance-once
            synveil-client
            synveil-desktop
        )
        ;;
    *-windows-*)
        platform=windows
        artifact_names=(synveil-client synveil-desktop)
        ;;
    *)
        printf '[synveil-artifact] ERROR: unsupported release reproducibility host: %s\n' "$host" >&2
        exit 2
        ;;
esac

snapshot_dir="$(mktemp -d "${TMPDIR:-/tmp}/synveil-repro-snapshot.XXXXXX")"
mkdir -p "${REPO_ROOT}/target"
first_target_dir="$(mktemp -d "${REPO_ROOT}/target/synveil-repro-build-a.XXXXXX")"
second_target_dir="$(mktemp -d "${REPO_ROOT}/target/synveil-repro-build-b.XXXXXX")"
trap 'rm -rf -- "$snapshot_dir" "$first_target_dir" "$second_target_dir"' EXIT

# Each side starts from an empty Cargo target root and independently receives
# the same source-date policy and compiler flags. This avoids treating an old
# binary, possibly built before a policy change, as the canonical baseline.
build_release_set() (
    local target_dir="$1"
    local label="$2"
    local name
    local suffix=""
    if [[ "$platform" == windows ]]; then
        suffix=".exe"
    fi

    export PACKAGES_REPO_ROOT="$REPO_ROOT"
    export SOURCE_DATE_EPOCH="$(synveil_source_date_epoch)"
    export CARGO_TARGET_DIR="$target_dir"
    synveil_prepare_reproducible_rust_build "$REPO_ROOT"

    printf '[synveil-artifact] building %s in a fresh target root\n' "$label"
    if [[ "$platform" == linux ]]; then
        cargo build --locked --release --bin synveil-scheduled-maintenance-once
    fi
    cargo build --locked --release -p synveil-client
    cargo build --locked --release -p synveil-desktop

    for name in "${artifact_names[@]}"; do
        local binary="${target_dir}/release/${name}${suffix}"
        if [[ "$platform" == linux ]]; then
            synveil_assert_linux_release_binary "$binary" "$name $label"
        else
            synveil_assert_portable_release_binary "$binary" "$name $label"
        fi
        if [[ "$label" == build-a ]]; then
            cp -- "$binary" "$snapshot_dir/${name}${suffix}"
        else
            synveil_require_identical_artifacts \
                "$snapshot_dir/${name}${suffix}" "$binary" "$name $label"
        fi
    done
)

build_release_set "$first_target_dir" build-a
build_release_set "$second_target_dir" build-b

printf '[synveil-artifact] independent clean target-root builds are byte-identical on %s\n' "$platform"
