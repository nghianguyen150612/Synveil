#!/usr/bin/env bash
set -euo pipefail

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
PROFILE="${1:?explicit profile required}"
ARTIFACT="${2:?package path required}"
VERSION="${3:?product version required}"
FIXTURE="$(mktemp -d)"
trap 'test -n "${SERVER_PID:-}" && kill "$SERVER_PID" 2>/dev/null || true; rm -rf "$FIXTURE"' EXIT
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -subj '/CN=localhost' -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' \
  -keyout "$FIXTURE/key.pem" -out "$FIXTURE/cert.pem" >/dev/null 2>&1
TYPE=deb
test "$PROFILE" = fedora-x86_64 && TYPE=rpm
python3 "$ROOT/tests/linux_quick_install/https_acceptance.py" \
  --artifact "$ARTIFACT" --artifact-type "$TYPE" --version "$VERSION" \
  --source-commit "$(git -C "$ROOT" rev-parse HEAD)" --root "$FIXTURE/release" \
  --certificate "$FIXTURE/cert.pem" --key "$FIXTURE/key.pem" >"$FIXTURE/channel.sha256" &
SERVER_PID=$!
for _ in {1..50}; do test -s "$FIXTURE/channel.sha256" && break; sleep 0.1; done
PIN="$(head -n1 "$FIXTURE/channel.sha256")"
test "${#PIN}" = 64
export SSL_CERT_FILE="$FIXTURE/cert.pem"
ARGS=(--platform-profile="$PROFILE" --channel-url=https://localhost:4443/SYNVEIL-RELEASE-CHANNEL.json
  --trusted-origin=https://localhost:4443 --trusted-channel-sha256="$PIN"
  --minimum-channel-generation=17 --yes)
"$ROOT/deploy/install/quick-install.sh" "${ARGS[@]}" | tee "$FIXTURE/first.log"
grep -F 'INSTALLED_VERIFIED' "$FIXTURE/first.log"
"$ROOT/deploy/install/quick-install.sh" "${ARGS[@]}" | tee "$FIXTURE/second.log"
grep -F 'ALREADY_INSTALLED_VERIFIED' "$FIXTURE/second.log"
