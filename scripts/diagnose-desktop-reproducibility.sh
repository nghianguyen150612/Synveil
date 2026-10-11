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

# Locations of interest, relative to a Cargo target root.
#   qmlcachegen  - qmlcachegen output; embeds the QML source path
#   qt-build-utils/qml_modules - generated qmldir and plugin C++
#   rcc - generated resource C++ from the QML resource collection
generated_roots=(
    qmlcachegen
    qt-build-utils/qml_modules
    rcc
)

printf '[synveil-diagnosis] comparing generated inputs\n'
: > "${REPORT_DIR}/generated-inputs.txt"

for sub in "${generated_roots[@]}"; do
    a_root="${BUILD_A}/${sub}"
    b_root="${BUILD_B}/${sub}"
    if [[ ! -d "$a_root" && ! -d "$b_root" ]]; then
        continue
    fi
    printf '[synveil-diagnosis]   scanning %s\n' "$sub"
    # Compare by path relative to the generated root so the two build trees
    # (which differ by construction) are compared on equal terms.
    for label in a b; do
        root_var="BUILD_${label^^}"
        root="${!root_var}/${sub}"
        [[ -d "$root" ]] || continue
        ( cd "$root" && find . -type f | LC_ALL=C sort ) > "${REPORT_DIR}/files-${label}.txt"
    done

    if ! diff -q "${REPORT_DIR}/files-a.txt" "${REPORT_DIR}/files-b.txt" >/dev/null 2>&1; then
        printf '[synveil-diagnosis]   FILE SET DIFFERS under %s\n' "$sub" | tee -a "${REPORT_DIR}/generated-inputs.txt"
        diff -u "${REPORT_DIR}/files-a.txt" "${REPORT_DIR}/files-b.txt" |
            sed 's/^/[synveil-diagnosis]     /' | tee -a "${REPORT_DIR}/generated-inputs.txt" || true
    fi

    while IFS= read -r relative; do
        a_file="${BUILD_A}/${sub}/${relative}"
        b_file="${BUILD_B}/${sub}/${relative}"
        [[ -f "$a_file" && -f "$b_file" ]] || continue
        if cmp -s "$a_file" "$b_file"; then
            continue
        fi
        printf '[synveil-diagnosis]   DIFFERS: %s/%s\n' "$sub" "${relative}" |
            tee -a "${REPORT_DIR}/generated-inputs.txt"
        # The first differing line almost always names the leaked or
        # nondeterministic token, which is the whole point of this report.
        diff -u "$a_file" "$b_file" 2>/dev/null |
            grep -E '^[+-][^+-]' | head -n 20 |
            sed 's/^/[synveil-diagnosis]     /' | tee -a "${REPORT_DIR}/generated-inputs.txt" || true
    done < "${REPORT_DIR}/files-a.txt"
done

printf '[synveil-diagnosis] comparing linked binaries\n'
: > "${REPORT_DIR}/binaries.txt"

# Build-script output outside the QML-specific roots can also feed linked
# objects (for example CXX-Qt's generated source). Compare generated source
# inputs across the entire Cargo release build tree before classifying a
# machine-code-only difference as compiler nondeterminism.
printf '[synveil-diagnosis] comparing Cargo-generated build sources\n'
: > "${REPORT_DIR}/cargo-generated-sources.txt"
if [[ -d "${BUILD_A}/release/build" || -d "${BUILD_B}/release/build" ]]; then
    for label in a b; do
        root_var="BUILD_${label^^}"
        root="${!root_var}/release/build"
        if [[ -d "$root" ]]; then
            ( cd "$root" && find . -type f \
                \( -name '*.rs' -o -name '*.cpp' -o -name '*.cc' -o -name '*.c' \
                   -o -name '*.h' -o -name '*.hpp' -o -name '*.inc' \) | LC_ALL=C sort ) \
                > "${REPORT_DIR}/cargo-generated-files-${label}.txt"
        else
            : > "${REPORT_DIR}/cargo-generated-files-${label}.txt"
        fi
    done
    if ! diff -q "${REPORT_DIR}/cargo-generated-files-a.txt" \
        "${REPORT_DIR}/cargo-generated-files-b.txt" >/dev/null 2>&1; then
        printf '[synveil-diagnosis] generated source file sets differ\n' |
            tee -a "${REPORT_DIR}/cargo-generated-sources.txt"
        diff -u "${REPORT_DIR}/cargo-generated-files-a.txt" \
            "${REPORT_DIR}/cargo-generated-files-b.txt" |
            head -n 80 | sed 's/^/[synveil-diagnosis]   /' |
            tee -a "${REPORT_DIR}/cargo-generated-sources.txt" || true
    fi
    while IFS= read -r relative; do
        a_file="${BUILD_A}/release/build/${relative}"
        b_file="${BUILD_B}/release/build/${relative}"
        [[ -f "$a_file" && -f "$b_file" ]] || continue
        if cmp -s "$a_file" "$b_file"; then
            continue
        fi
        printf '[synveil-diagnosis] DIFFERS: release/build/%s\n' "$relative" |
            tee -a "${REPORT_DIR}/cargo-generated-sources.txt"
        diff -u "$a_file" "$b_file" 2>/dev/null |
            head -n 80 | sed 's/^/[synveil-diagnosis]   /' |
            tee -a "${REPORT_DIR}/cargo-generated-sources.txt" || true
    done < "${REPORT_DIR}/cargo-generated-files-a.txt"
fi

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
        zero_offset=$(( first_offset - 1 ))
        for label in a b; do
            root_var="BUILD_${label^^}"
            candidate="${!root_var}/release/${binary_name}"
            printf '[synveil-diagnosis]     build-%s differing bytes:' "$label" |
                tee -a "${REPORT_DIR}/binaries.txt"
            od -An -tx1 -N 32 -j "$zero_offset" "$candidate" 2>/dev/null |
                tr -s ' ' | tee -a "${REPORT_DIR}/binaries.txt"
            if command -v readelf >/dev/null 2>&1 && command -v addr2line >/dev/null 2>&1; then
                while read -r section address section_offset section_size; do
                    [[ -n "$section" ]] || continue
                    section_start=$((16#$section_offset))
                    section_length=$((16#$section_size))
                    if (( zero_offset >= section_start && zero_offset < section_start + section_length )); then
                        virtual_address=$((16#$address + zero_offset - section_start))
                        printf '[synveil-diagnosis]     differing section: %s file_offset=0x%s virtual_address=0x%x\n' \
                            "$section" "$section_offset" "$virtual_address" |
                            tee -a "${REPORT_DIR}/binaries.txt"
                        addr2line -f -C -e "$candidate" "0x$(printf '%x' "$virtual_address")" 2>/dev/null |
                            sed 's/^/[synveil-diagnosis]       symbol: /' |
                            tee -a "${REPORT_DIR}/binaries.txt" || true
                        if command -v objdump >/dev/null 2>&1; then
                            start_address=$((virtual_address > 32 ? virtual_address - 32 : 0))
                            stop_address=$((virtual_address + 96))
                            objdump -d --start-address="$start_address" --stop-address="$stop_address" "$candidate" 2>/dev/null |
                                tail -n 16 | sed 's/^/[synveil-diagnosis]       /' |
                                tee -a "${REPORT_DIR}/binaries.txt" || true
                        fi
                        break
                    fi
                done < <(
                    readelf -S -W "$candidate" 2>/dev/null |
                        sed -E 's/^[[:space:]]*\[[[:space:]]*[0-9]+\][[:space:]]*//' |
                        awk '$1 ~ /^\./ && NF >= 5 { print $1, $3, $4, $5 }'
                )
            fi
        done
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
printf '[synveil-diagnosis] Cargo sources:    %s\n' "${REPORT_DIR}/cargo-generated-sources.txt"
printf '[synveil-diagnosis] binaries:         %s\n' "${REPORT_DIR}/binaries.txt"

# A diagnostic never fails the build; the gate decides pass or fail.
exit 0
