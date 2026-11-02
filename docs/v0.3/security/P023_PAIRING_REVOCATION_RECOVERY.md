# Synveil v0.3 — P023 pairing revocation and re-enrollment recovery

**Status:** provisional, isolated qualification only; not production-ready.

**Base:** `develop/v0.3` at `288222beed0c8e56a3c60dbbeb8e2835a6551750` (P022 PR #182 merged).

**Scope:** typed revocation and recovery reference states, deterministic SQLite qualification, and focused Ubuntu/Windows CI.
**Roadmap:** the authoritative P001–P060 roadmap was not present at the exact base; P023 remains provisional until its assignment and acceptance language are reconciled.

P004-D12 and P004-D13 remain `RECOMMENDED_PENDING_APPROVAL`. Nothing in this qualification approves a pairing protocol, identity ceremony, credential lifetime, rotation policy, or production enrollment behavior.

## 1. Baseline audit

| Surface at the exact base | Existing behavior and P023 reuse |
|---|---|
| P020 typed invitations | `synveil-pairing-contracts` validates a versioned, strictly decoded QR claim bound to installation, owner, HTTPS endpoint, fingerprint, target device, time, audience, and policy revision. It contains no bearer value. QR possession alone is not proof or authority. |
| P021 invitation ledger | Test-only, file-backed SQLite keeps immutable invitation bindings and terminal states. `BEGIN IMMEDIATE`, current authority checks, exact approval projection, and atomic consume plus a synthetic marker allow one redemption. P023 reuses the P021 tables and consume checks. |
| P022 first-contact ceremony | `ReferenceCeremony` binds independent identity observations, claim proof, separate owner approval and client consent, and a full immutable challenge. Its serialized snapshot is trusted fixture state. P023 reuses its confirm/cancel/revoke and recovery behavior. |
| Existing `sve1_` grant and device credential path | `crates/core/src/device_secrets.rs` keeps enrollment grants and device credentials purpose-separated, high-entropy, digest-derived, constant-time compared, redacted, and zeroized. `crates/auth/src/device_credentials.rs` uses a 600-second enrollment-grant lifetime. `crates/metadata/src/device_credentials.rs` rechecks owner, device, grant, expiry, consumption, and revocation inside the transaction that inserts the credential verifier and consumes the grant. A lost response cannot retrieve the bearer. P023 does not call or change this issuer. |
| Owner/device authorization and revocation | Existing API issuance/revocation remains owner-authorized in `crates/api/src/device_auth.rs`. Metadata writes serialize against current owner/device/grant state. P023 uses separate test authority rows to model the same fail-closed recheck; it adds no API route or production authorization cache. |
| Android secure storage | Android's Keystore-backed credential vault binds its encrypted envelope to profile, canonical origin, transport policy, owner, device, and credential IDs. Enrollment preflights and reads back the vault. Ambiguous exchange or storage failure requires recovery; ordinary profile/cache data does not become the credential store. |
| iOS secure storage | `KeychainCredentialStore` uses a device-only, non-synchronizable Keychain item, validates the stored binding and payload on read, and verifies writes. Enrollment exchange is single-shot; ambiguous responses and Keychain failures require recovery. P023 changes no native client or persisted profile data. |
| P004-D12 / D13 | D12 still asks for approval of first-contact identity and recovery. D13 still asks for credential lifetime, overlap, replacement, and identity-preservation policy. Both remain pending in `P004_MAINTAINER_DECISIONS.md`. |

The current Android/iOS behavior and exact source references are also summarized in P020 and P022. P023 does not claim physical Android Keystore, iOS Keychain, camera, TLS interception, or device recovery qualification.

## 2. Versioned reference state and trust boundary

`crates/pairing-contracts/src/recovery.rs` defines `RecoveryStateV1`, `RecoveryRecordV1`, `RecoveryBindingV1`, `RecoveryRequestV1`, and `CredentialReferenceStateV1`. The serialized request accepts only locators, claimed bindings, a version, and an action. Unknown fields such as bearer tokens, owner authorization, TLS validity, or secure-store assertions are rejected.

The reference lifecycle names these conditions explicitly:

- `PairingPending` and the transaction-local `PairingCancellationPending`;
- terminal `PairingCancelled`, `InvitationRevoked`, `OwnerRevoked`, and `DeviceRevoked`;
- `CredentialExpired`, `SecureStorageLost`, `SecureStorageInaccessible`, and `AmbiguousCredentialIssuance`;
- `ExplicitReenrollmentRequired` and `ReenrollmentStarted` for a separately authorized fresh invitation and ceremony;
- `Enrolled`, which records only the synthetic test outcome.

Terminal invitation or credential records cannot transition back to pending or active. A fresh enrollment gets a different invitation, ceremony, and recovery record. A pending pairing cancellation's intermediate state is written and finalized while the same SQLite write transaction is held, so another connection sees either the old pending record or its terminal result.

`RecoveryRequestV1` is untrusted client input. `TrustedRecoveryEvidence` and `OwnerReenrollmentApproval` have no serde implementation; the SQLite harness builds them from independently stored authority, policy, identity, device, credential, and owner-action rows while it holds the writer fence. A new invitation locator from a request grants nothing. It must exactly match the independently recorded owner action, which is bound to the original recovery, source invitation, owner, device, and new invitation. This evidence models a test observation and is not an approved owner-authentication mechanism.

`qualify_recovery` always returns metadata-only, fresh-enrollment-required, fresh-pairing-may-start, or a bounded denial. It never returns, reconstructs, logs, or serializes a bearer credential. `FreshPairingMayStart` only permits the trusted test harness to make a separate P020/P021/P022 record; it is not an enrollment grant.

## 3. Durable revocation and recovery reference

P023's `tests/support/schema_p023.sql` is applied only to fresh temporary test databases after the P021 qualification schema. The database contains versioned recovery rows, fixed `NON_PRODUCTION_REFERENCE_ONLY` credential markers, exact owner-action evidence, and a unique reenrollment reservation per source invitation. It has no token, digest, credential, QR body, or private-key column. P021 remains schema version 1; there is no production migration or runtime schema change.

Every P023 state-changing operation starts `BEGIN IMMEDIATE` before reading current authority and state. Confirmation calls P022's current-evidence check, then P021's existing `redeem_locked` operation, then writes P023's synthetic reference row in the same transaction. Cancellation or revocation that wins first moves the P021 invitation to its existing `CANCELLED` terminal state and the P022 ceremony to `Cancelled` or `Revoked`. If confirmation wins first, the invitation remains P021 `CONSUMED` and the P022 ceremony remains `Confirmed`; later owner/device revocation marks the P023 reference credential revoked and current authority inactive. Neither ordering resurrects an invitation or ceremony.

P023's retry and reenrollment checks reread authority, device state, policy revision, server fingerprint, credential state, and owner-action evidence under the same write fence as the reservation. A recovery decision cached before revocation cannot create a new invitation after revocation. Duplicate or concurrent requests can reserve only one fresh invitation per prior invitation. SQL, JSON, or lock failures reduce to bounded test errors; raw driver detail is not returned as a protocol error.

The P023 integration database is a standalone temporary file using P021's rollback journal, full synchronous mode, foreign keys, and bounded busy timeout. It is deleted with the fixture. The existing P021 child-process tests remain in the same focused suite.

## 4. Deterministic recovery qualification

| Scenario | Reference outcome and evidence |
|---|---|
| Lost response before transaction commit | Child-process exit after P021/P022/P023 writes but before commit recovers to the pending invitation and `ReadyToConfirm` snapshot; no synthetic credential marker is visible. Retrying the still-pending ceremony can commit once. |
| Lost response after commit | The committed invitation remains consumed. The synthetic outcome is classified ambiguous and requires fresh authorized enrollment. Replaying P021 redemption is denied; no bearer exists to recover. |
| Device credential lost after enrollment | `SecureStorageLost` requires fresh enrollment. Existing profile, device, queue, and checkpoint bindings are not modified. |
| Interrupted pairing or client crash | Reopening the standalone DB returns only the exact-bound P022 metadata snapshot. No new invitation or credential is produced by inspection. |
| Duplicate re-enrollment attempts | A unique source-invitation reservation denies a second attempt and leaves only one fresh invitation. |
| Revoked device requests replacement | Current device authority denies the request even with previously observed owner-action evidence. No same-device replacement is silently issued. |
| Incorrect installation, owner, or device binding | Any claimed-binding mismatch returns `BindingMismatch` before fresh-pairing qualification. |
| Server identity rotation | A changed independently configured fingerprint returns `ServerIdentityRotated`; the old invitation/claim does not adopt the new identity. |
| Stale policy revision | A changed current revision returns `StalePolicy`; P022 confirmation and P021 consume do not commit. |
| Revocation races pairing confirmation | Separate SQLite connections serialize at the writer fence. If confirmation wins, the subsequent revocation marks the synthetic credential revoked; if revocation wins, the invitation and ceremony become terminal before confirmation. |
| Concurrent recovery attempts | Independent connections using different request IDs for one source invitation reserve at most one replacement invitation. |

The executable evidence is in `crates/pairing-contracts/src/recovery.rs` unit tests and `crates/pairing-contracts/tests/recovery.rs`. P020, P021, and P022 focused tests continue to run unchanged, except P021 test support exposes its existing issue checks under a caller-owned transaction for atomic P023 composition.

## 5. Compatibility, scope limits, and gates

P023 adds no production dependency, route, enrollment endpoint, credential issuer, authentication mechanism, runtime migration, QR scanner, mobile flow, identity reset, automatic re-enrollment, or network exposure. The existing v0.2 server and Android/iOS secure storage, profile bindings, offline queues, and sync checkpoints are untouched. No existing credential is invalidated or device identity replaced.

This is an isolated reference qualification, not production-ready security. It does not prove SQLite behavior under physical power loss or hostile storage, a live API authorization boundary, real TLS identity, cryptographic proof, secure hardware, real LAN/tailnet behavior, or native client recovery. D12/D13 and the authoritative roadmap remain unresolved. Keep the outstanding P004, P028, P036, independent security review, physical Android/iOS, and real-tailnet gates open.

The existing P019 Windows Desktop Clippy failures are a separate baseline issue in unchanged desktop host UI code. P023 CI runs strict Clippy for the pairing, authorization/client/sync, installer engine, and server-network boundaries; it does not suppress or claim to repair the unrelated Windows Desktop warnings. Their exact hosted conclusion is recorded in the P023 manifest after the final-head checks.

## 6. Validation evidence

Exact base, implementation and final source SHAs, local checks, hosted Ubuntu/Windows results, and remaining blockers are recorded in [PROMPT023_MANIFEST.md](../manifests/PROMPT023_MANIFEST.md). A queued, skipped, or cancelled hosted check is not a pass, and no result from an earlier commit is attributed to a later head.
