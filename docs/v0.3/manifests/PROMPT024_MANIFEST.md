# PROMPT024 — Pairing compatibility and qualification closeout

**Status:** provisional reference qualification; production pairing blocked.

## Provenance

| Field | Value |
|---|---|
| Repository / target | `nghianguyen150612/Synveil` / `develop/v0.3` |
| Assigned branch | `feat/v03-p024-pairing-compatibility-closeout` |
| Exact starting base | `40801060ceb5c5243c879496eb726136ac81e26b` |
| Starting remote feature ref | Same exact SHA as the base; no P024 commits or open PR existed. |
| P019 repair ancestor | `ccca06177e5ae7c39087a433b3a48ff2f1679753` (merged PR #184). |
| P023 ancestor | `40801060ceb5c5243c879496eb726136ac81e26b` (merged PR #183). |
| Implementation source SHA | `84561b26dffb941295d17c1f01944021b5707148` (P024 tests and contracts; identity, decision, and recovery facts are reloaded from trusted fixture state). |
| Final verified source/test SHA | `34299610a897c322ad30b43fb1bd179620b378d2` (implementation sources at `84561b26dffb941295d17c1f01944021b5707148`, plus evidence documentation). |
| Draft PR | [#185](https://github.com/nghianguyen150612/Synveil/pull/185), targeting `develop/v0.3`, open as Draft. |
| Roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` is unavailable in the checkout. Scope is provisional. |

The original local checkout at `/workspace/Synveil` was clean on branch `work`, HEAD `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`. It was preserved. GitHub confirmed the assigned feature ref and `develop/v0.3` both resolved to the starting base. Work was isolated in `/workspace/Synveil-P024` on the assigned branch. No unrelated staged or unstaged work was overwritten.

## Source audit and implementation

The architecture, source audit, explicit compatibility matrix and evidence boundary are in [P024 qualification](../security/P024_PAIRING_COMPATIBILITY_QUALIFICATION.md).

- `crates/pairing-contracts/tests/p024_qualification.rs` composes the existing P020 invitation DTO, P021 issue/consume transaction helpers, P022 first-contact ceremony, and P023 recovery record using deterministic fixture evidence and temporary SQLite storage.
- `.github/workflows/v03-p024-pairing-compatibility.yml` runs the focused contract and regression gate on Ubuntu 24.04 and Windows latest.
- This manifest and the P024 security qualification describe limits and evidence.

No production crate, API, migration, issuer, networking/discovery behavior, Android or iOS source is changed. Existing `sve1_` and `svd1_` authentication and device authorization, Android Keystore vault and iOS Keychain, host identity checks, offline queues and sync checkpoints retain their current behavior.

## Test inventory

The new P024 cases cover:

1. P020 bounded invitation validation and P021 durable issue.
2. Strict QR claim parsing as untrusted input.
3. Independently loaded server installation, HTTPS endpoint, identity fingerprint and authenticated-TLS fixture evidence.
4. Challenge reservation and separate owner approval/client consent bound to that challenge.
5. P022 confirmation before P021's atomic consume; P023 recovery state updated in that same transaction.
6. Lost confirmation response, metadata-only recovery and denied replay with exactly one synthetic marker.
7. Direct later-phase redemption after missing owner approval is denied and leaves the invitation issued.
8. An 11-case compatibility fixture covering v0.2 peers, new/unknown versions, capabilities, stale metadata, existing credentials and devices, fresh/repeated attempts, and wrong bindings.

P020–P023 security-specific tests remain in `tests/ceremony.rs`, `tests/durable_ledger.rs`, and `tests/recovery.rs`; P024 does not copy those rules. The inventory mapped to threat cases is in the security qualification document.

## Local validation

| Check | Result |
|---|---|
| Focused P024 integration test | Pass: 3 tests. |
| Full P020–P024 contracts | Pass: 55 tests (13 unit, 16 ceremony, 12 durable ledger, 3 P024, 11 recovery). |
| Auth/API/client/client-sync/server-network regressions | Pass: 574 tests (auth 16, API 123, client 132, client-sync 298, server-network 5). Executed under Rust 1.99.1 in an isolated Debian container with `libdbus-1-dev`; no source or lockfile changes. |
| Installer-engine tests | Pass: 581 tests (library, appimage integration, and contract suites). Re-run under the workspace UID after a root-container attempt hit the suite's expected non-root environment guard. |
| Workspace formatting | Pass: `cargo fmt --all -- --check`. |
| Strict all-target Clippy (`-D warnings`) | Pass for pairing-contracts, auth, API, client, client-sync, install-engine and server-network. |
| Documentation validation | Pass: `bash scripts/validate-docs.sh` (24 groups). |
| `git diff --check` | Pass. |

The initial direct host regression invocation stopped during dependency compilation because the host has no `dbus-1.pc`. The completed regression run used the exact Cargo commands from the hosted workflow in a disposable Rust 1.99.1 Debian container with `libdbus-1-dev`. Its first installer-engine run used container root and hit the existing appimage test non-root guard; the same command was then run successfully as the workspace UID. No product test or guard was changed to obtain a pass.
| Focused hosted P024 CI | Pass on final verified source/test SHA `34299610a897c322ad30b43fb1bd179620b378d2`: [run 41562668922](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668922), Ubuntu 24.04 and Windows latest. |

## Hosted validation

The P024 implementation source is `84561b26dffb941295d17c1f01944021b5707148`; the focused P024 workflow passed on final verified source/test head `34299610a897c322ad30b43fb1bd179620b378d2` in [run 41562668922](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668922), with both Ubuntu 24.04 and Windows jobs successful. P020–P023 also passed both OS jobs on the same head: [P020 run 41562668991](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668991), [P021 run 41562669011](https://github.com/nghianguyen150612/Synveil/actions/runs/41562669011), [P022 run 41562669079](https://github.com/nghianguyen150612/Synveil/actions/runs/41562669079), and [P023 run 41562668944](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668944). P019 preflight passed both OS jobs, including repaired Windows strict Clippy, in [run 41562668987](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668987).

The PR-triggered global Rust CI passed all jobs on final verified source/test head `34299610a897c322ad30b43fb1bd179620b378d2`: [run 41562668930](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668930). This includes Linux, Windows and macOS checks/tests, strict formatting/Clippy, and Linux/Windows Qt desktop build jobs. A green Windows desktop build is not P028 native acceptance. The separate Windows native run [41562668963](https://github.com/nghianguyen150612/Synveil/actions/runs/41562668963) built a candidate and passed disposable standard-user install, then failed its evidence-recording gate with `BLOCKED_BY_ENVIRONMENT: required P028 GUI/logon evidence unavailable and named-pipe gate pending`; actual P028 acceptance remains unresolved.

The installer-resilience workflow passed on the final verified source/test head in [run 41562669012](https://github.com/nghianguyen150612/Synveil/actions/runs/41562669012), including Windows ownership lifecycle and Inno interruption. The prior Windows Inno interruption job [41437684142](https://github.com/nghianguyen150612/Synveil/actions/runs/41437684142/job/125251483708) failed with `INSTALLER_VERIFY_FAILURE: secret-like marker in output`. Its investigation remains an unresolved independent gate and the historical failure is not relabeled as passing.

## Known blockers and release conclusion

Required unresolved gates are P004-D12 first-contact identity/proof, P004-D13 credential lifecycle/rotation/recovery, other P004 decisions, P028 native Windows acceptance, P036 signing/publisher qualification, independent security review, physical Android/iOS pairing/recovery, real LAN/Tailscale qualification, original roadmap reconciliation, and investigation of the Windows installer interruption reporting `INSTALLER_VERIFY_FAILURE: secret-like marker in output`.

**Release readiness:** P024 is a reference-only closeout. Local contract/regression evidence, focused hosted Linux/Windows P024 jobs, P019–P023 compatibility workflows, installer-resilience regressions, and global Rust CI pass on final verified source/test head `34299610a897c322ad30b43fb1bd179620b378d2`. Production pairing remains blocked. The Draft PR must stay unmerged. No P025 work is included.
