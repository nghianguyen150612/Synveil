#!/usr/bin/env bash
set -euo pipefail

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
ARTIFACT_TYPE="${1:?artifact type required}"
ARTIFACT="${2:?package path required}"
VERSION="${3:?product version required}"
FIXTURE="$(mktemp -d)"
trap 'test -n "${SERVER_PID:-}" && kill "$SERVER_PID" 2>/dev/null || true; rm -rf "$FIXTURE"' EXIT
openssl req -x509 -newkey rsa:2048 -nodes -days 1 \
  -subj '/CN=localhost' -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' \
  -keyout "$FIXTURE/key.pem" -out "$FIXTURE/cert.pem" >/dev/null 2>&1
case "$ARTIFACT_TYPE" in deb|rpm) ;; *) echo 'artifact type must be deb or rpm' >&2; exit 2 ;; esac
python3 "$ROOT/tests/linux_quick_install/https_acceptance.py" \
  --artifact "$ARTIFACT" --artifact-type "$ARTIFACT_TYPE" --version "$VERSION" \
  --source-commit "$(git -C "$ROOT" rev-parse HEAD)" --root "$FIXTURE/release" \
  --certificate "$FIXTURE/cert.pem" --key "$FIXTURE/key.pem" >"$FIXTURE/channel.sha256" &
SERVER_PID=$!
for _ in {1..50}; do test -s "$FIXTURE/channel.sha256" && break; sleep 0.1; done
PIN="$(head -n1 "$FIXTURE/channel.sha256")"
test "${#PIN}" = 64
export SSL_CERT_FILE="$FIXTURE/cert.pem"
ARGS=(--channel-url=https://localhost:4443/SYNVEIL-RELEASE-CHANNEL.json
  --trusted-origin=https://localhost:4443 --trusted-channel-sha256="$PIN"
  --minimum-channel-generation=17 --yes)
report_native_package_state() {
  echo "NATIVE_PACKAGE_DIAGNOSTIC type=$ARTIFACT_TYPE"
  if [[ "$ARTIFACT_TYPE" == deb ]]; then
    dpkg-query -W -f='${db:Status-Abbrev} ${Version}\n' synveil || true
    if output="$(dpkg --verify synveil 2>&1)"; then verify_status=0; else verify_status=$?; fi
    echo "NATIVE_PACKAGE_DIAGNOSTIC verifier=dpkg --verify synveil exit=$verify_status"
  else
    rpm -q --qf '%{NAME} %{VERSION}-%{RELEASE} %{ARCH}\n' synveil || true
    if output="$(rpm -V synveil 2>&1)"; then verify_status=0; else verify_status=$?; fi
    echo "NATIVE_PACKAGE_DIAGNOSTIC verifier=rpm -V synveil exit=$verify_status"
  fi
  if [[ -n "${output:-}" ]]; then
    printf '%s\n' "$output" | sed -n '1,40p'
  else
    echo 'NATIVE_PACKAGE_DIAGNOSTIC verifier_output=(empty)'
  fi
}
set +e
"$ROOT/deploy/install/quick-install.sh" "${ARGS[@]}" 2>&1 | tee "$FIXTURE/first.log"
pipeline_status=("${PIPESTATUS[@]}")
set -e
quick_install_status=${pipeline_status[0]:-1}
tee_status=${pipeline_status[1]:-1}
if (( quick_install_status != 0 )); then
  report_native_package_state
  exit "$quick_install_status"
fi
if (( tee_status != 0 )); then
  echo "NATIVE_ACCEPTANCE_LOG_FAILURE tee_exit=$tee_status" >&2
  exit "$tee_status"
fi
grep -F 'INSTALLED_VERIFIED' "$FIXTURE/first.log"
"$ROOT/deploy/install/quick-install.sh" "${ARGS[@]}" | tee "$FIXTURE/second.log"
grep -F 'ALREADY_INSTALLED_VERIFIED' "$FIXTURE/second.log"
