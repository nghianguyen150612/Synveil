# PROMPT023 — Pairing revocation and re-enrollment recovery manifest

**Status:** provisional / non-production / roadmap reconciliation blocked.

**Target:** `develop/v0.3`.

**Draft PR:** pending push and hosted qualification.

## Provenance

| Field | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Working branch | `feat/v03-p023-pairing-revocation-recovery` |
| Exact base | `288222beed0c8e56a3c60dbbeb8e2835a6551750` |
| Base evidence | Target and assigned remote feature branch both resolved to the exact base. P022 PR #182 was merged at this SHA. |
| Implementation source SHA | `74890238cfe12b9c9a11c51bdb6cab16205117a1` (contracts, tests, workflow, and security specification). |
| Final delivery SHA | Pending the evidence-only manifest commit; the exact final PR head will be recorded in the PR description and delivery report. |
| Draft PR URL | Pending creation. |
| Roadmap source | `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not present at the exact base/workspace; scope remains provisional. |

The original workspace checkout on branch `work` contained 139 staged paths. That index and worktree were preserved. P023 was built in a separate linked worktree from the assigned remote feature branch at the exact base.

## Deliverables and implementation boundary

- `crates/pairing-contracts/src/recovery.rs`: version-1 recovery states, exact binding checks, untrusted request DTO, non-serializable trusted evidence, monotonic transitions, and bounded recovery decisions.
- `crates/pairing-contracts/tests/recovery.rs`: independent-connection SQLite tests for P021/P022 composition, crash/commit recovery, cancellation and revocation, secure-storage loss, explicit re-enrollment, revocation races, stale identity/policy, and concurrent reservations.
- `crates/pairing-contracts/tests/support/schema_p023.sql`: P023 test-only credential markers, recovery rows, owner-action evidence, and unique reenrollment reservations.
- `crates/pairing-contracts/tests/support/mod.rs`: P021 issue checks callable inside a P023-owned transaction; existing P021 issue behavior continues through that helper.
- `.github/workflows/v03-p023-pairing-revocation-recovery.yml`: focused Ubuntu/Windows contract, auth/API/client/client-sync, installer, server-network, formatting, strict Clippy, and documentation matrix.
- `docs/v0.3/security/P023_PAIRING_REVOCATION_RECOVERY.md`: audit, reference states, transaction behavior, recovery cases, compatibility, and gates.
- `docs/v0.3/manifests/PROMPT023_MANIFEST.md`: provenance and validation evidence.

No production crate depends on the pairing-contracts crate. P023 adds no production migration, route, enrollment endpoint, credential material, authentication mechanism, mobile behavior, or automatic re-enrollment. P023 markers are synthetic and never represent a bearer or digest.

## Validation matrix

| Check | Local result | Hosted final-head result |
|---|---|---|
| `cargo test -p synveil-pairing-contracts --locked` | Pass: 13 unit, 16 P022, 12 P021, and 11 P023 tests; no failures. | Pending Ubuntu and Windows run. |
| `cargo test -p synveil-auth -p synveil-api -p synveil-client -p synveil-client-sync -p synveil-server-network --lib --locked` | Pass. The container lacked DBus development metadata; this completed using a temporary Debian sysroot outside the repository. | Pending Ubuntu and Windows run. |
| `cargo test -p synveil-install-engine --locked` | Pass. | Pending Ubuntu and Windows run. |
| `cargo fmt --all -- --check` | Pass. | Pending Ubuntu and Windows run. |
| Strict Clippy for pairing, auth/API/client/client-sync, install-engine, and server-network (`--all-targets --locked -- -D warnings`) | Pass. | Pending Ubuntu and Windows run. |
| `bash scripts/validate-docs.sh` | Pass (24 validation groups). | Pending Ubuntu run. |
| `git diff --check` | Pass. | Pending Ubuntu and Windows run. |

The P023 matrix does not include the unrelated Desktop package in its Clippy target set. The inspected P019 run [41162909951](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909951), on P022 checkpoint `c160bf9d7f41d66fca6351f5966fb0cd8ef9923d`, completed with Ubuntu success and Windows failure. The Windows P019 preflight, UI contracts, authentication/network regressions, and formatting passed; strict Clippy failed only in unchanged `crates/desktop/src/host_ui.rs`: unused `PreviewPartiallyAvailable`, `NativePackageManagerManaged`, `MissingPackageManager`, and `MissingNativeDependency` variants, plus `default()` calls for the `NativeHostOwners` unit struct. These remain separate from P023. No warning suppression or P019 code change is included.

## Hosted workflow evidence

Pending push. This section will identify workflow run IDs, job results, and the exact PR head SHA. Only completed successful jobs for the final source SHA count as passes. P023 runs remain focused; they do not replace full workspace, native mobile, installer-host, or real-network qualification.

The implementation source SHA above identifies the final tested P023 code, tests, workflow, and security specification. This manifest-only evidence commit follows it; Git cannot embed a commit's own SHA in its contents. The final delivery SHA will therefore be stated in the Draft PR description and final delivery report.

## Security and compatibility limits

P004-D12/D13 remain `RECOMMENDED_PENDING_APPROVAL`. The authoritative P001–P060 roadmap remains unavailable. Keep P004, P028, P036, independent security review, physical Android/iOS pairing and recovery, and real LAN/tailnet gates unresolved. This reference implementation is not production-ready security.

P023 preserves existing v0.2 server and Android/iOS enrollment, secure storage, profile bindings, device identities, offline queues, and sync checkpoints. It does not invalidate deployed credentials or reset device identity. Unknown committed credential outcomes remain ambiguous and require fresh authorized enrollment; no lost bearer is reconstructed or replayed.

Do not merge this Draft PR without explicit authorization. Do not start P024.
