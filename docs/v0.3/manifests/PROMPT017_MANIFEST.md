# Prompt 017 manifest — SynveilNet transport discovery

## Scope and baseline

Provisional P017 as explicitly requested; not an authoritative roadmap closeout.
Repository: `nghianguyen150612/Synveil`. Base `develop/v0.3` and existing working
branch `feat/v03-p017-synveilnet-transport-discovery` both verified at
`8c48483b7e0d29ec956a940a97cb5d16bcbdefe2`. P016A merged via PR #176.
No authoritative P001–P060 roadmap was found by repository/workspace filename
search and scoped GitHub code search. Newly supplied conflicting scope must be
reconciled before further implementation.

## Delivered files and boundaries

- `crates/server-network/src/transport.rs`: typed local capabilities, safe summary,
  route/peer evidence, separate IPv4/IPv6 LAN/overlay validation.
- `crates/server-network/src/lib.rs` and `crates/server-host/src/native/network.rs`:
  shared existing Linux interface classification, preserving Host behavior.
- `crates/client/src/transport.rs`: explicit policies, approved profile-bound
  endpoints, existing Synveil HTTPS/auth verifier, bounded deterministic selection.
- `crates/client/src/transport/native.rs`: bounded Linux local metadata inspector
  with fixed read-only commands and no elevation or enrollment.
- `crates/client/src/controller.rs`: initializer-only cleanup in the existing
  P016A generation-fence test for strict current-toolchain Clippy.
- `crates/client/src/lib.rs`, `crates/client/Cargo.toml`, `Cargo.lock`: additive
  module and existing workspace dependency wiring. Production dependencies reuse
  workspace packages; the TLS test enables the existing rcgen PEM encoder.
- `crates/client/src/transport/tests.rs`,
  `crates/client/src/transport/native/tests.rs`,
  `crates/server-network/tests/transport.rs`: deterministic adapter, policy,
  spoofing, identity, route, bounds, cancellation, IPv4/IPv6 and native tests.
- `crates/client-sync/Cargo.toml` and `crates/client-sync/src/http_remote/tests.rs`: loopback TLS test of the reused
  handshake, requiring approved server/network identity before sending a bearer.
- `.github/workflows/v03-p017-transport-discovery.yml`: focused Linux/Windows
  contracts, authentication regressions, Clippy, formatting and documentation.
- [Transport contract and repository audit](../network/P017_SYNVEILNET_TRANSPORT_DISCOVERY.md).
- This manifest.

No production Wizard call site, auth/enrollment implementation, API, migration,
credential/session store, mobile client, listener/firewall/service effect or
approval policy is changed. No custom VPN, public host exposure or mandatory
Tailscale installation is introduced. `CapabilitySummary` is future preflight
metadata and always unauthenticated. Selection is advisory, not a permission.

## Validation evidence

Local validation on 2026-10-29, Linux x86_64 / Debian 13, Rust and Cargo 1.99.1:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy -p synveil-client -p synveil-server-network -p synveil-server-host --all-targets --locked -- -D warnings` | PASS |
| `cargo test -p synveil-server-config -p synveil-server-network -p synveil-client -p synveil-server-host --lib --tests --locked` | PASS: 170 tests, zero failed/ignored; includes 120 client unit tests, 11 controller/IPC integration tests, 29 configuration tests, 2 Host tests, 8 network tests |
| `cargo test -p synveil-client-sync --lib http_remote::tests:: --locked` | PASS: 32 tests, zero failed/ignored, including the new P017 loopback TLS identity handshake |
| `bash scripts/validate-docs.sh` | PASS: 24 documentation validation groups |
| `git diff --check` | PASS |
| Python `yaml.safe_load` of the focused workflow | PASS |

Local Rust commands used the existing host D-Bus runtime `libdbus-1.so.3`
(version 1.16.2) through a temporary pkg-config descriptor and linker symlink:
`PKG_CONFIG_PATH=/tmp/p017-native/lib/pkgconfig`. This was development-tooling
setup outside discovery, with no repository or system library change. Hosted
Linux installs the standard `libdbus-1-dev` build package. The local environment
has neither `ip` nor Tailscale CLI, so its native test establishes bounded missing
capability behavior; hosted Linux explicitly requires successful local iproute2
address inspection. No tailnet reachability was tested.

During development, the cancellation test exposed a test sender with no remaining
receiver; the fixture now uses `send_replace` before resubscribing. The first
strict Clippy run identified P016A's existing field-reassignment fixture lint;
only that test initializer was cleaned up. The TLS fixture additionally required
enabling rcgen's test-only PEM feature. Final commands above passed after these
corrections; no test was disabled or warning suppressed.

The initial hosted workflow was rejected before job execution because unquoted
Rust namespace filters contain YAML colon-space syntax. Both commands now use
block scalars; the corrected workflow parses locally. Initial failed run:
[40883699341](https://github.com/nghianguyen150612/Synveil/actions/runs/40883699341)
on `98733c5450ede716846d9091d362d2603aeef65c`. This was not a test failure.
Final endpoint binding also fences profile identity and rejects differing
server/owner/device/library scopes in the production verifier; the complete
170-test regression and strict Clippy commands were rerun successfully.

Hosted workflow: `.github/workflows/v03-p017-transport-discovery.yml`. At commit
time hosted results are pending, not claimed as passing. Exact PR source/merge
SHAs, job conclusions and run URLs are recorded in the PR validation body after
execution. Source presence alone is not CI evidence. Windows contract compilation
does not satisfy P028 native application acceptance.

## Remaining gates and merge boundary

Native discovery is Linux-only; Windows/macOS/mobile native inspection and
DNS/MagicDNS, userspace mode, subnet routers and unrecognized daemon schemas remain
unsupported. Local CLI absence is tested; real-world LAN/Tailscale reachability
has not been demonstrated. Fake metadata and loopback TLS do not qualify a tailnet.

P004-D02/D03/D11/D14, P028 Windows native acceptance, P036 publisher/signing and
independent security review remain unresolved obligations. Production SynveilNet
onboarding remains unavailable. Merge only as this bounded provisional increment
after review and applicable checks; no auto-merge and no P018.
