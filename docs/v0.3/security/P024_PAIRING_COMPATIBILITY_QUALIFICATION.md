# P024 pairing compatibility qualification

**Status:** provisional reference qualification only; production pairing remains blocked.

## Provenance

| Item | Evidence |
|---|---|
| Repository / target | `nghianguyen150612/Synveil`, `develop/v0.3` |
| Assigned branch | `feat/v03-p024-pairing-compatibility-closeout` |
| Exact starting SHA | `40801060ceb5c5243c879496eb726136ac81e26b` |
| Implementation SHA | Recorded in the P024 manifest and final delivery report after the implementation commit. |
| Final PR head SHA | Recorded after final hosted Linux and Windows runs. |
| P019 repair / P023 ancestry | Merged commits `ccca06177e5ae7c39087a433b3a48ff2f1679753` and `40801060ceb5c5243c879496eb726136ac81e26b` are ancestors of the starting SHA. |
| Roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` is not present in the checkout. Scope stays provisional pending reconciliation. |

The local workspace was clean on an older `work` branch at `604499f`; that checkout was preserved. P024 uses a separate worktree created from the assigned feature ref, which resolved to the exact base SHA. The assigned remote feature ref existed at the base and had no P024 commits or open PR.

## Scope and architecture

P024 adds isolated reference tests and a focused Ubuntu/Windows workflow. The end-to-end fixture composes existing contracts and helpers; it does not add another authorization implementation:

1. P020 validates a bounded synthetic invitation.
2. P021's existing SQLite helper issues the invitation under its writer fence.
3. P022 parses the scanned QR as untrusted claim data and authenticates separately supplied identity facts before challenge creation.
4. The fixture records owner approval and client consent as separate challenge-bound evidence, then calls the P022 ceremony confirmation rule.
5. The fixture inserts the existing P021 approval adapter evidence and calls P021's existing atomic consume helper while one SQLite write transaction is held.
6. P023's typed recovery record transitions to enrolled in the same transaction. The database stores only existing synthetic markers.
7. A simulated lost response can read terminal metadata; replay is denied and returns no credential.

The added source is `crates/pairing-contracts/tests/p024_qualification.rs`. It only uses synthetic identifiers, deterministic timestamps, temporary SQLite files, independent identity fixture state and `NON_PRODUCTION_REFERENCE_ONLY` / `NON_PRODUCTION_SYNTHETIC_V1` markers. A separate denial test proves that advancing directly to P021 redemption cannot bypass a rejected/missing P022 approval.

No production crate depends on `synveil-pairing-contracts`. It has no production API, migration, endpoint, token issuer, discovery handler, enrollment authority, cryptographic signer, or mobile code. P021/P022/P023 schema files are test-only fixtures.

### Audited compatibility surfaces

| Surface | Source audit and P024 boundary |
|---|---|
| Existing API and grants | `api/openapi.yaml`, `crates/api/src/device_auth.rs`, `crates/auth/src/device_credentials.rs`, and `crates/metadata/src/device_credentials.rs` retain the existing owner-scoped `sve1_` grant and `svd1_` credential path. P024 changes no route, schema, authorization, or lifecycle behavior. |
| Android | `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt` preflights and scopes Keystore-backed vault records, and treats ambiguous enrollment/storage outcomes as recovery-required. No client change or pairing prompt is added. |
| iOS | `clients/ios/Infrastructure/Security/KeychainCredentialStore.swift` remains the device-only secure store. Existing enrollment and ambiguous-response recovery are unchanged. |
| Offline work | `crates/client-sync` is unchanged. Existing queues and checkpoints are not read, migrated, or reset by pairing qualification. |
| Host identity / network trust | P017/P018/P019 endpoint and transport boundaries are unchanged. A QR claim or matching fingerprint string is not independent server identity evidence. |

These are source and regression audits, not physical-device observations.

## Compatibility matrix

| Server / client state | Pairing result | Existing v0.2 authentication and data |
|---|---|---|
| v0.2 server with v0.2 Android/iOS client | No pairing protocol negotiated | Existing enrollment and credentials unchanged |
| v0.2 server with a newer reference client | Pairing unsupported; retain authenticated endpoint verification | Legacy enrollment remains available |
| Supported reference protocol version 1 | Synthetic reference path only | No effect on deployed sessions |
| Unknown, future, or malformed protocol version | Fail closed; no downgrade | Existing auth remains unchanged |
| Optional pairing capability absent | Continue legacy enrollment | No forced capability upgrade |
| Stale invitation or policy revision | Reject; require a fresh owner-issued invitation | Existing credentials remain valid under existing rules |
| Existing `sve1_` grant or `svd1_` credential | Do not rotate, invalidate, or reconstruct | Existing credential lifecycle remains authoritative |
| Previously enrolled Android/iOS device | No forced pairing | Profile, secure-store, owner/device and endpoint bindings remain intact |
| Fresh enrollment / explicit re-enrollment | New invitation and ceremony required | Existing re-enrollment authorization remains unchanged |
| Repeated or interrupted attempt | Terminal outcomes remain terminal; metadata-only recovery | No secret replay; queues/checkpoints preserved |
| Wrong owner, device, installation, endpoint, TLS identity, or policy | Reject closed | No weaker endpoint or authentication fallback |

The matrix is an explicit compatibility policy fixture, not evidence that a deployed mobile client negotiated P024. Native behavior still requires Android/iOS device evidence.

## Qualification test inventory

P024's isolated suite is `cargo test -p synveil-pairing-contracts --locked`; it includes the new `p024_qualification` integration tests plus all P020–P023 regressions.

| Threat / behavior | Existing reference evidence |
|---|---|
| Forged/substituted QR, malformed/unknown fields | `tests/ceremony.rs`: strict DTOs, all invitation fields bound, substituted QR rejected |
| Independent server identity, HTTPS endpoint and TLS mismatch | `tests/ceremony.rs`: missing identity, wrong identity/endpoint, invalid TLS, redirect and claim proof cases |
| Cross-owner/device/install and replay binding | `tests/ceremony.rs`, `tests/durable_ledger.rs`, `tests/recovery.rs`: exact bindings, target and authority fences |
| Separate owner approval and client consent | `tests/ceremony.rs`, and P024 end-to-end composition |
| Missing/revoked approval or consent; withdrawal | `tests/ceremony.rs`, `tests/recovery.rs` |
| Expired invitation/challenge and stale policy | `tests/durable_ledger.rs`, `tests/ceremony.rs`, `tests/recovery.rs` |
| Durable consume, duplicate/replayed confirmation, concurrent redemption | `tests/durable_ledger.rs`, `tests/recovery.rs` |
| Revocation versus confirmation, cancelled ceremony resurrection | `tests/recovery.rs` cancellation and concurrency cases |
| Lost response before/after commit, credential loss, no bearer reconstruction | `tests/recovery.rs` crash, restart, ambiguous outcome and explicit re-enrollment cases |
| Duplicate re-enrollment and cross-binding | `tests/recovery.rs` reservation and exact-bound owner action cases |
| Later phase cannot bypass earlier denial | `p024_qualification.rs::later_reference_state_cannot_bypass_a_denied_ceremony_phase` |
| Version and compatibility cases | `p024_qualification.rs::compatibility_fixture_keeps_legacy_enrollment_separate_from_pairing_versions`; strict P020/P022/P023 DTO version checks |

The fixtures use trusted in-process facts, not request booleans, QR fingerprints, or caller-supplied snapshots as authenticated evidence. SQLite fixture results are deterministic and do not authorize production transactions.

## Qualification boundaries and assumptions

**Reference qualification:** Rust contract behavior, SQLite transactions, deterministic identity facts, synthetic owner/client decisions, simulated interruption, recovery and concurrency covered by the named tests.

**Production qualification not performed:** approved TLS server identity and proof protocol; live authenticated owner/device possession; PostgreSQL persistence/API transactions; Android Keystore and iOS Keychain on devices; physical Android/iOS pairing and recovery; live LAN/Tailscale communication; cryptographic security review. No production credential is minted.

Security assumptions are limited to trusted test fixture construction, fixture-owned SQLite state, the test process, and deterministic injected time. A fixture's claim of authenticated TLS/owner/client evidence is only an oracle. It does not approve a real evidence mechanism.

## Validation results

Local and hosted results, exact tested SHAs, commands, and any blocked gates are recorded in [PROMPT024_MANIFEST.md](../manifests/PROMPT024_MANIFEST.md). CI is a focused Linux/Windows Rust desktop contract gate and is not P028 native Windows acceptance.

## Release readiness

**Reference pairing qualification:** complete only when the focused tests and final-head Linux/Windows workflow pass; see the manifest for the observed result. **Production pairing:** blocked. Do not enable pairing or merge this Draft PR based solely on this reference evidence.

Unresolved gates include P004-D12 first-contact identity/proof approval, P004-D13 credential lifecycle approval, remaining P004 decisions, P028 real Windows native acceptance, P036 signing/publisher qualification, independent security review, physical Android/iOS pairing and recovery, real LAN/Tailscale qualification, roadmap reconciliation, and the Windows installer interruption reporting `INSTALLER_VERIFY_FAILURE: secret-like marker in output`.

The P019 Windows Clippy repair has a passing focused hosted run: [run 41437684199](https://github.com/nghianguyen150612/Synveil/actions/runs/41437684199), Windows job `P019 preflight contracts (windows-latest)`, including its strict Clippy step, on repaired P019 source `c095efa092135575b3c941c9fd888222c4557a88`. The separate installer blocker is [run 41437684142, Windows ownership lifecycle and Inno process interruption](https://github.com/nghianguyen150612/Synveil/actions/runs/41437684142/job/125251483708), which is failed. The observed failure marker is recorded as `INSTALLER_VERIFY_FAILURE: secret-like marker in output`; this P024 work does not investigate or suppress it.
