#!/usr/bin/env bash
# Source/mechanics regression only; never native product acceptance.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
export SYNVEIL_REAL_QMAKE=unused SYNVEIL_REAL_RCC=unused
export SYNVEIL_REAL_QMLCACHEGEN=unused SYNVEIL_QT_WRAPPER_DIR=unused
export SYNVEIL_SOURCE_DATE_EPOCH=0 SYNVEIL_QML_SOURCE_ROOT=unused
export SYNVEIL_QML_CANONICAL_ROOT=unused
rustc --edition=2021 --test -D warnings "$repo_root/scripts/reproducible-qt-wrapper.rs" -o "$work/tests"
"$work/tests"
