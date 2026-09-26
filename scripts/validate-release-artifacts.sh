#!/usr/bin/env bash
# Validate a generated release-artifact manifest against the exact current
# source tree and the files it names. This intentionally rejects stale
# manifests instead of comparing a dirty-tree build with a clean-checkpoint
# hash or accepting an arbitrary expected value.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck source=deploy/packages/common/reproducible.sh
source "${REPO_ROOT}/deploy/packages/common/reproducible.sh"

MANIFEST="${REPO_ROOT}/target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt"

usage() {
    cat >&2 <<'EOF'
Usage: scripts/validate-release-artifacts.sh [--manifest=PATH]

Validate the generated Linux release-artifact manifest. The manifest must be
created by deploy/packages/build.sh and must describe the same current source
tree. A source-fingerprint or byte mismatch is a hard failure with expected,
actual, size, build-ID, and first-differing-byte diagnostics.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest=*)
            MANIFEST="${1#--manifest=}"
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            printf '[synveil-artifact] ERROR: unexpected argument: %s\n' "$1" >&2
            usage
            exit 2
            ;;
    esac
done

if [[ "$MANIFEST" != /* ]]; then
    MANIFEST="${REPO_ROOT}/${MANIFEST}"
fi
[[ -f "$MANIFEST" ]] || {
    printf '[synveil-artifact] ERROR: generated artifact manifest is missing: %s\n' "$MANIFEST" >&2
    exit 1
}

grep -Fxq 'Synveil Linux release artifact manifest' "$MANIFEST" || {
    printf '[synveil-artifact] ERROR: invalid artifact manifest header: %s\n' "$MANIFEST" >&2
    exit 1
}
grep -Fxq 'format=1' "$MANIFEST" || {
    printf '[synveil-artifact] ERROR: unsupported artifact manifest format: %s\n' "$MANIFEST" >&2
    exit 1
}

manifest_source_fingerprint="$(sed -n 's/^source_fingerprint=//p' "$MANIFEST" | head -n 1)"
current_source_fingerprint="$(synveil_source_fingerprint "$REPO_ROOT")"
if [[ -z "$manifest_source_fingerprint" || "$manifest_source_fingerprint" != "$current_source_fingerprint" ]]; then
    printf '[synveil-artifact] ERROR: artifact manifest belongs to a different source tree\n' >&2
    printf '[synveil-artifact]   manifest source_fingerprint=%s\n' "${manifest_source_fingerprint:-<missing>}" >&2
    printf '[synveil-artifact]   current  source_fingerprint=%s\n' "$current_source_fingerprint" >&2
    printf '[synveil-artifact]   rebuild release artifacts from the current worktree before validating them\n' >&2
    exit 1
fi

manifest_source_date_epoch="$(sed -n 's/^source_date_epoch=//p' "$MANIFEST" | head -n 1)"
[[ "$manifest_source_date_epoch" =~ ^[0-9]+$ ]] || {
    printf '[synveil-artifact] ERROR: invalid source_date_epoch in manifest: %s\n' "$manifest_source_date_epoch" >&2
    exit 1
}

in_artifacts=0
entries=0
while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == 'artifacts=sha256 size build_id path' ]]; then
        in_artifacts=1
        continue
    fi
    [[ "$in_artifacts" -eq 1 ]] || continue
    read -r expected_sha expected_size expected_build_id relative_path extra <<< "$line"
    if [[ -n "$extra" || ! "$expected_sha" =~ ^[0-9a-fA-F]{64}$ || ! "$expected_size" =~ ^[0-9]+$ || ! "$expected_build_id" =~ ^[0-9a-fA-F]+$ || -z "$relative_path" ]]; then
        printf '[synveil-artifact] ERROR: malformed artifact manifest entry: %s\n' "$line" >&2
        exit 1
    fi
    case "$relative_path" in
        /*|../*|*/../*|*/..)
            printf '[synveil-artifact] ERROR: unsafe artifact manifest path: %s\n' "$relative_path" >&2
            exit 1
            ;;
    esac
    artifact="${REPO_ROOT}/${relative_path}"
    [[ -f "$artifact" ]] || {
        printf '[synveil-artifact] ERROR: manifest references missing artifact: %s\n' "$relative_path" >&2
        exit 1
    }
    synveil_assert_linux_release_binary "$artifact" "$relative_path"

    actual_sha="$(synveil_artifact_sha256 "$artifact")"
    actual_size="$(synveil_artifact_size "$artifact")"
    actual_build_id="$(synveil_artifact_build_id "$artifact")"
    if [[ "$actual_sha" != "${expected_sha,,}" || "$actual_size" != "$expected_size" || "$actual_build_id" != "$expected_build_id" ]]; then
        printf '[synveil-artifact] ERROR: release artifact manifest mismatch: %s\n' "$relative_path" >&2
        printf '[synveil-artifact]   expected: size=%s sha256=%s build_id=%s\n' "$expected_size" "$expected_sha" "$expected_build_id" >&2
        printf '[synveil-artifact]   actual:   size=%s sha256=%s build_id=%s\n' "$actual_size" "$actual_sha" "$actual_build_id" >&2
        exit 1
    fi
    entries=$((entries + 1))
done < "$MANIFEST"

[[ "$entries" -eq 3 ]] || {
    printf '[synveil-artifact] ERROR: expected three Linux release artifacts, found %s\n' "$entries" >&2
    exit 1
}

printf '[synveil-artifact] release artifact manifest verified: %s\n' "$MANIFEST"
printf '[synveil-artifact] source_fingerprint=%s source_date_epoch=%s entries=%s\n' \
    "$current_source_fingerprint" "$manifest_source_date_epoch" "$entries"
