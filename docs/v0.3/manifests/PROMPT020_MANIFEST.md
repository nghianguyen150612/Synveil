# PROMPT020 — Pairing trust and invitation contracts manifest

Status: **PROVISIONAL / ROADMAP RECONCILIATION BLOCKED**. Source contracts and focused tests are provided for review. P020 is not accepted as complete.

| Provenance | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Feature branch | `feat/v03-p020-pairing-trust-contracts` |
| Exact base SHA | `fea3c9307f30318e7ce275fbd406c733cfa145d8` |
| Base state | P019 PR #179 merged; both remote branch refs resolved to the base SHA at task start |
| Authoritative P001–P060 roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` not present in repository/workspace; recorded in P002/P003/P019; reconciliation remains a blocker |
| Draft PR | To be recorded after publication; never auto-merge |

## Audit and changed scope

The audit checked the current versioned `sve1_` enrollment secret, its ten-minute grant TTL and domain-separated verifier; the auth/metadata transaction's active-owner/device/grant locking and digest-only credential insert; the existing owner-authorized API enrollment flow; endpoint HTTPS/TLS validation; Android Keystore-backed vault and enrollment recovery; iOS device-only Keychain and ambiguous-result handling; and P004 D12/D13 and P004 policy contracts. Full findings and paths are in [P020_PAIRING_TRUST_CONTRACTS.md](../security/P020_PAIRING_TRUST_CONTRACTS.md). An independent one-time claim proof/owner-approval ceremony remains unresolved; the invitation identifier is only a locator.

Changed surfaces:

- `crates/pairing-contracts/`: standalone non-production typed QR, invitation states and an in-memory deterministic redemption oracle. No production crate depends on it.
- `.github/workflows/v03-p020-pairing-trust-contracts.yml`: required Ubuntu/Windows focused contract and regression jobs.
- `docs/v0.3/security/P020_PAIRING_TRUST_CONTRACTS.md`: design, audit, assumptions, recovery, compatibility and blockers.
- This manifest.
- `Cargo.toml` and `Cargo.lock`: workspace registration only.

No production route, API/OpenAPI DTO, migration, authorization change, live invitation, mobile client change, automatic enrollment, privileged operation, or P021 work is included. P004-D12/D13 remain unresolved.

## Security review and blockers

The non-production boundaries and negative cases were reviewed locally. No independent security review is claimed. P020 acceptance is blocked on original roadmap reconciliation and formal P004-D12/D13 decisions. Preserve P004, P028, P036, Windows Clippy and real-tailnet qualification obligations. CI status and exact test outcomes will be updated from the final branch head before PR review; hosted CI evidence is not equivalent to those independent gates.

## Validation record

Local environment: Ubuntu Linux; Rust/Cargo `1.99.1`; exact tested source is the current feature branch before the documentation-only result update. Commands and conclusions:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p synveil-pairing-contracts --locked` | PASS: 7 tests, including 16-way concurrent redemption, replay, proof requirement, wrong issuer/installation/server identity/device, expiry, cancellation, forged QR fields, leakage and stale policy; 0 failed |
| `cargo test -p synveil-auth -p synveil-client -p synveil-client-sync -p synveil-api --lib --locked` | PASS: API 123, auth 16, client 132, client-sync 298; 569 passed, 0 failed. The 5,000-entry client-sync matrix test completed; aggregate client-sync suite time was 130.76 seconds. |
| `cargo test -p synveil-server-network --locked --quiet` | PASS: 5 unit tests, 3 doc tests; 0 failed |
| `cargo clippy -p synveil-pairing-contracts -p synveil-auth -p synveil-client -p synveil-client-sync -p synveil-server-network -p synveil-api --all-targets --locked -- -D warnings` | PASS, no warnings |
| `bash scripts/validate-docs.sh` | PASS: all 24 documentation validation groups |
| Workflow YAML parse and `git diff --check` | PASS |

The local container lacked system DBus development metadata. These Linux checks were completed with an unprivileged temporary sysroot for the same `libdbus-1-dev` dependency installed by the Ubuntu CI job. No production database integration test, physical Android/iOS ceremony, real TLS interception, Windows-native acceptance, Windows Clippy qualification, configured real-tailnet qualification, or independent security review was performed. Hosted Linux/Windows P020 workflow results must be recorded after PR creation; queued checks are not a pass. Preserve all named P004/P028/P036 and roadmap blockers regardless of green CI.
