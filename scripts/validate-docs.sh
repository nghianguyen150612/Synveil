#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
failures=0

echo "DOC-UNIT-1: checking primary relative links"
while IFS= read -r document; do
    while IFS= read -r target; do
        [[ -z "$target" || "$target" == \#* || "$target" == http://* || "$target" == https://* || "$target" == mailto:* ]] && continue
        target="${target%%#*}"
        candidate="$(dirname "$document")/$target"
        if [[ ! -e "$candidate" ]]; then
            echo "broken link: $document -> $target" >&2
            failures=$((failures + 1))
        fi
    done < <(grep -oE '\]\([^)]+' "$document" | sed 's/^](//' || true)
done < <(find docs README.md -type f \( -name '*.md' -o -name 'README.md' \) -print | sort)

echo "DOC-UNIT-2: checking bilingual core navigation"
for topic in RELEASE_OPERATIONS RELEASE_NOTES_v0.1 RELEASE_PACKAGING UPGRADE_SAFETY SECURITY; do
    test -f "docs/en/${topic}.md" || failures=$((failures + 1))
    test -f "docs/vi/${topic}.md" || failures=$((failures + 1))
    rg -q "${topic}\.md" docs/README.md || failures=$((failures + 1))
done

echo "DOC-UNIT-3: checking documented package/script paths"
for path in \
    deploy/packages/build.sh \
    deploy/packages/build-windows.sh \
    deploy/install/install.sh \
    deploy/install/uninstall.sh \
    deploy/install/MANIFEST \
    deploy/systemd-user/synveil-client.service \
    crates/api/src/bin/synveil-api.rs \
    crates/api/src/bin/synveil-worker.rs; do
    test -e "$path" || failures=$((failures + 1))
done

echo "DOC-UNIT-4: checking unsupported platforms are not advertised"
if rg -n -i 'macOS[^\n]*(supported|support)|supported[^\n]*macOS|iOS[^\n]*(supported|support)|Android[^\n]*(supported|support)' \
    docs/en/RELEASE_OPERATIONS.md docs/vi/RELEASE_OPERATIONS.md \
    docs/en/RELEASE_NOTES_v0.1.md docs/vi/RELEASE_NOTES_v0.1.md \
    | rg -v 'not supported|unsupported|không hỗ trợ|không được hỗ trợ|hoãn'; then
    echo "unsupported platform appears advertised in release-facing docs" >&2
    failures=$((failures + 1))
fi

echo "DOC-UNIT-5: checking Windows validation wording"
for document in docs/en/RELEASE_OPERATIONS.md docs/vi/RELEASE_OPERATIONS.md docs/en/RELEASE_NOTES_v0.1.md docs/vi/RELEASE_NOTES_v0.1.md; do
    rg -q -i 'native Windows|Windows runtime|Windows.*cross|cross.*Windows|Windows.*native|Windows.*compile|Windows.*acceptance' "$document" || failures=$((failures + 1))
done

echo "DOC-UNIT-6: checking release-facing path/secret hygiene"
if rg -n '/mnt/|/home/[^` ]+|/Users/|BEGIN (RSA|OPENSSH|EC) PRIVATE KEY|gh[pousr]_[A-Za-z0-9_]+|sk-[A-Za-z0-9]' \
    docs/en/RELEASE_OPERATIONS.md docs/vi/RELEASE_OPERATIONS.md \
    docs/en/RELEASE_NOTES_v0.1.md docs/vi/RELEASE_NOTES_v0.1.md; then
    echo "developer path or credential-like material found" >&2
    failures=$((failures + 1))
fi

echo "DOC-UNIT-7: checking the Prompt119 release-freeze record"
freeze_record="docs/PROMPT119_RELEASE_FREEZE.md"
test -f "$freeze_record" || failures=$((failures + 1))
for fact in \
    'Starting authoritative HEAD: `5ac915bdbd625a57e586c94c61f53ffb76a90004`' \
    'Product version: `0.1.0`' \
    'Server migrations: `36`' \
    'Client migrations: `7`' \
    'LOCAL_SCHEMA_VERSION: `7`' \
    'PostgreSQL 17 live acceptance: **BLOCKED_BY_ENVIRONMENT**' \
    'RELEASE_BLOCKER: none' \
    '`./deploy/packages/build.sh --format=all --output-dir=target/packages`' \
    '`./deploy/packages/build-windows.sh --output-dir=target/windows-packages`' \
    '`scripts/validate-release-artifacts.sh --manifest=target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt`'; do
    rg -Fq "$fact" "$freeze_record" 2>/dev/null || failures=$((failures + 1))
done
for required_path in \
    Cargo.toml Cargo.lock api/openapi.yaml migrations crates/client-sync/migrations \
    crates/desktop/qml/Main.qml deploy/packages/build.sh \
    deploy/packages/build-windows.sh deploy/install/install.sh \
    deploy/install/uninstall.sh deploy/systemd-user/synveil-client.service \
    LICENSE deploy/NOTICE; do
    rg -Fq "\`$required_path\`" "$freeze_record" 2>/dev/null || failures=$((failures + 1))
done

if [[ "$failures" -ne 0 ]]; then
    echo "documentation validation failed: $failures issue(s)" >&2
    exit 1
fi
echo "documentation validation passed"
