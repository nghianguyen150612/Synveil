#!/usr/bin/env bash
# Compare two independently built desktop release trees and report exactly
# where they differ.
#
# The reproduction verifier answers only *whether* two builds match. When they
# do not, this script is what tells us *what* differs: the generated QML C++,
# the other generated C++/resource inputs, and the linked ELF sections.
#
# It is a diagnostic, not a gate. It always exits 0 so that a hosted run still
# uploads its report when the binaries differ; the reproducibility gate itself
# is `verify-release-build-reproducibility.sh`.
#
# Usage:
#   diagnose-desktop-reproducibility.sh <build-a-target-dir> <build-b-target-dir> [report-dir]
# SPDX-License-Identifier: MIT
set -euo pipefail

if [[ $# -lt 2 ]]; then
    printf 'usage: %s <build-a-target-dir> <build-b-target-dir> [report-dir]\n' "$0" >&2
    exit 2
fi

BUILD_A="$1"
BUILD_B="$2"
REPORT_DIR="${3:-${TMPDIR:-/tmp}/synveil-desktop-repro-diagnosis}"

for dir in "$BUILD_A" "$BUILD_B"; do
    if [[ ! -d "$dir" ]]; then
        printf '[synveil-diagnosis] ERROR: build tree does not exist: %s\n' "$dir" >&2
        exit 2
    fi
done

mkdir -p "$REPORT_DIR"

# Cargo stores generated Qt/CXX inputs below release/build/<crate>-<hash>/out,
# not at the target root. Inspect those real inputs and retain raw generator
# provenance alongside the canonicalized C++ consumed by the compiler.
python3 "$(dirname "$0")/diagnose-generated-qt-inputs.py" \
    "$BUILD_A" "$BUILD_B" "$REPORT_DIR"

printf '[synveil-diagnosis] comparing linked binaries\n'
: > "${REPORT_DIR}/binaries.txt"

for binary_name in synveil-desktop synveil-client; do
    a_bin="${BUILD_A}/release/${binary_name}"
    b_bin="${BUILD_B}/release/${binary_name}"
    [[ -f "$a_bin" && -f "$b_bin" ]] || continue

    if cmp -s "$a_bin" "$b_bin"; then
        printf '[synveil-diagnosis]   %s: byte-identical\n' "$binary_name" |
            tee -a "${REPORT_DIR}/binaries.txt"
        continue
    fi

    printf '[synveil-diagnosis]   %s: DIFFERS\n' "$binary_name" |
        tee -a "${REPORT_DIR}/binaries.txt"

    if command -v readelf >/dev/null 2>&1; then
        # Compare section headers: identical headers with differing bytes point
        # at content, while differing headers point at layout/link order.
        readelf -S -W "$a_bin" | sed 's/\[ *[0-9]*\]//' | awk '{$1=$1};1' |
            LC_ALL=C sort > "${REPORT_DIR}/sections-a.txt"
        readelf -S -W "$b_bin" | sed 's/\[ *[0-9]*\]//' | awk '{$1=$1};1' |
            LC_ALL=C sort > "${REPORT_DIR}/sections-b.txt"
        if diff -q "${REPORT_DIR}/sections-a.txt" "${REPORT_DIR}/sections-b.txt" >/dev/null; then
            printf '[synveil-diagnosis]     section layout identical; bytes differ inside sections\n' |
                tee -a "${REPORT_DIR}/binaries.txt"
        else
            printf '[synveil-diagnosis]     SECTION LAYOUT DIFFERS:\n' |
                tee -a "${REPORT_DIR}/binaries.txt"
            diff -u "${REPORT_DIR}/sections-a.txt" "${REPORT_DIR}/sections-b.txt" |
                grep -E '^[+-][^+-]' | head -n 20 |
                sed 's/^/[synveil-diagnosis]       /' | tee -a "${REPORT_DIR}/binaries.txt" || true
        fi
    fi

    # Show the first differing byte's surrounding printable text. This is where
    # a leaked absolute path shows up verbatim.
    #
    # `cmp` exits non-zero precisely because the files differ, so the
    # difference is expected here and must not trip `set -e`/`pipefail`.
    first_offset="$(cmp -l "$a_bin" "$b_bin" 2>/dev/null | head -n 1 | awk '{print $1}' || true)"
    if [[ -n "$first_offset" ]]; then
        printf '[synveil-diagnosis]     first differing byte offset: %s\n' "$first_offset" |
            tee -a "${REPORT_DIR}/binaries.txt"
        start=$(( first_offset > 96 ? first_offset - 96 : 1 ))
        printf '[synveil-diagnosis]     build-a context: ' | tee -a "${REPORT_DIR}/binaries.txt"
        dd if="$a_bin" bs=1 skip=$(( start - 1 )) count=160 status=none 2>/dev/null |
            LC_ALL=C tr -c '[:print:]' '.' | tee -a "${REPORT_DIR}/binaries.txt"
        printf '\n[synveil-diagnosis]     build-b context: ' | tee -a "${REPORT_DIR}/binaries.txt"
        dd if="$b_bin" bs=1 skip=$(( start - 1 )) count=160 status=none 2>/dev/null |
            LC_ALL=C tr -c '[:print:]' '.' | tee -a "${REPORT_DIR}/binaries.txt"
        printf '\n' | tee -a "${REPORT_DIR}/binaries.txt"
    fi

    # Report any private path that survived, naming it, so the next run points
    # straight at the offending generated input.
    for candidate in "$a_bin" "$b_bin"; do
        LC_ALL=C grep -aoE '(/home/[!-~]{0,160}|/Users/[!-~]{0,160}|/tmp/synveil-[!-~]{0,160}|/usr/src/synveil/target/[!-~]{0,160})' "$candidate" 2>/dev/null |
            LC_ALL=C sort -u | head -n 10 |
            sed "s|^|[synveil-diagnosis]     private path marker in $(basename "$candidate"): |" |
            tee -a "${REPORT_DIR}/binaries.txt" || true
    done
done

printf '[synveil-diagnosis] report written under %s\n' "$REPORT_DIR"
printf '[synveil-diagnosis] generated inputs: %s\n' "${REPORT_DIR}/generated-inputs.txt"
printf '[synveil-diagnosis] binaries:         %s\n' "${REPORT_DIR}/binaries.txt"

# A diagnostic never fails the build; the gate decides pass or fail.
exit 0
