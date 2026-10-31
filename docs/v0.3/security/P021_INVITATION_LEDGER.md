# P021 — Durable invitation ledger qualification

Status: provisional, non-production, pending authoritative roadmap reconciliation.
Base: `511de37361941bb55ffe119f63ce94182b4c7230` (P020 #180 merged).

## Audit and boundary

P020's `crates/pairing-contracts/src/lib.rs` remains unchanged. Its mutex ledger
and boolean evidence are a state-machine oracle, not durable authorization.
The new adapter lives exclusively in `crates/pairing-contracts/tests/support/`;
SQLx, Tokio and UUID are dev dependencies. No production crate can import this
adapter. Each fixture uses a unique temporary file, never a production database.
`schema_v1.sql` is explicitly initialized by tests, uses `user_version=1`, and
refuses repeated initialization; it is outside runtime migration discovery.

The audited v0.2 `sve1_` path uses purpose-separated digest types in
`crates/core/src/device_secrets.rs`, a ten-minute grant in
`crates/auth/src/device_credentials.rs`, and
`crates/metadata/src/device_credentials.rs::consume_grant_with_clock`. The latter
locks owner, device and grant, then evaluates the server clock, rechecks active
owner/device, digest, consumption, revocation and expiry, inserts the credential
verifier and consumes the grant in one PostgreSQL transaction. Failed insertion
rolls back; lost responses do not recover the bearer. API owner authorization in
`crates/api/src/device_auth.rs` remains untouched. SQLite's single-writer fence
is a reference serialization convention, not a replacement for those row locks.

P004-D12/D13 remain `RECOMMENDED_PENDING_APPROVAL`. P004 contracts and gates
require an approved identity ceremony, current authorization, one-time atomic
redemption and secure mobile storage. This work selects no ceremony, device
credential lifetime, production proof verifier, or enrollment wire format.

## Reference storage and transaction

An invitation stores the validated P020 payload as immutable JSON. This binds its
owner, installation, HTTPS endpoint, approved identity fingerprint, device,
audience, issue/expiry times and policy revision. Audience is fixed by P020
validation; policy must fit SQLite's positive signed integer domain. An ID is a
locator only. Bindings cannot be edited or deleted through the reference schema.

The trusted harness provisions `authority` rows for current installation owner,
endpoint, fingerprint, revision and activity, and an `approvals` row containing
an exact invitation binding. Only this harness controls these records. The
approval stands in for independently authenticated claim proof, valid TLS,
identity comparison and explicit confirmation; it does not perform those checks
or approve their mechanism. Redemption accepts no positive authorization boolean
and reads both tables inside its write transaction. A copied invitation without
that exact approval fails. The installation argument models the trusted server's
configured installation context, never a client assertion. The device argument
must match the approved target, but this fixture does not authenticate a device.

Every issue, cancel and redeem operation starts `BEGIN IMMEDIATE` before reading
state or authority. Authority changes on independent connections use SQLite's
same writer fence. A revocation committed before redemption acquires that fence
is observed; a revocation serialized afterward does not retroactively undo an
already committed outcome. Clock injection is trusted harness evidence and is
evaluated after locking. The reference uses rollback-journal DELETE mode,
foreign keys ON, synchronous FULL and a bounded 15-second busy timeout. Lock or
storage failure yields a bounded error, never success or an automatic retry.

`CREATED` can become `ISSUED`, `CANCELLED` or `EXPIRED`; `ISSUED` can become
`CONSUMED`, `CANCELLED` or `EXPIRED`. SQL triggers reject all terminal updates,
binding changes, deletion and invalid initial states. Expiry is inclusive at
`now >= expires_at`; time before issuance denies. Invalid authority/device,
missing approval or stale policy denies without consuming. Terminal outcomes
are immutable. Expiry is recorded lazily on eligible issue/redemption attempts;
there is no scheduler. Revocation denies rather than automatically cancelling.

Redemption compares `ISSUED`, changes it to `CONSUMED`, and inserts one
`synthetic_verifiers` row in the same transaction. Its primary key is the
invitation ID; its only other field is the literal
`NON_PRODUCTION_SYNTHETIC_V1`. It is not a digest, proof verifier or credential.
A deferred foreign key and state/verifier consistency checks prevent committing
consumption without that row. A trigger prevents inserting a verifier for an
unconsumed invitation. Verifier deletion/update and duplicate insertion fail.
Any insert failure or abandoned transaction rolls back both mutations.

The public reference outcomes are bounded and return no credential. Driver/JSON
errors are reduced to `LedgerError`; no SQL parameters, raw error messages,
tokens, credentials, trace events or serialized diagnostics are emitted. No
secret input or cryptographic verifier type is introduced. Fixture identifiers
are synthetic public metadata. This is structural exclusion of secrets, not a
claim that arbitrary strings supplied by a future caller are safe to log.

## Recovery and verification

The deterministic Linux/Windows matrix runs P020 and durable tests, existing
auth/API/client/client-sync regressions, installer regressions, formatting and
strict all-target Clippy. Independent file-backed connections contend for the
same invitation; exactly one succeeds and one verifier persists. Independent
readers see neither uncommitted half-consumption nor an uncommitted verifier. Cancellation
can win before consumption or lose to a committed consume, with one terminal
outcome. Tests also cover stale policy, committed issuer revocation, different
installation/device, absent or altered approval, invalid issue time, inclusive
expiry, duplicate creation, immutable terminal data, explicit rollback, failed
verifier insertion and a deferred-constraint failure at commit.

A fresh child test process exits with live uncommitted transactions after the
first mutation and after both mutations, bypassing Rust destructors. Dirty-page pressure spills writes to an on-disk
rollback journal before exit. Reopening
checks state, verifier count, SQLite integrity and foreign keys before retry.
Another child commits and exits without returning a redemption response; replay
must deny and retain exactly one verifier. This models interrupted processes and
lost responses; it does not qualify filesystem controllers, torn writes,
physical power loss, media corruption, backup restoration or malicious DB edits.
Trusted database writers and the OS/filesystem remain inside the reference trust
boundary. A production design needs separately reviewed durable storage and
anti-rollback/recovery policy.

Unknown committed credential outcomes in a future production implementation
still require authorized revocation/re-enrollment; replay must never reconstruct
or replace the original bearer. No actual device credential is created here.

## Remaining gates

`Synveil-v0.3-Context-Roadmap-P001-P060.md` remains absent from the checkout and
workspace; P021 scope is not roadmap acceptance. Keep P004-D12/D13 and all
outstanding P004/P028/P036 gates, independent security review, native Android/iOS
pairing qualification and real-tailnet qualification pending. This qualification
proves neither production enrollment security nor resistance to physical power
loss. No automatic pairing, production QR issuance, server API endpoint, runtime
migration, privilege elevation or Tailscale mutation is included. Existing v0.2
API, enrollment, Android/iOS and credential storage behavior is unchanged. Do not
merge automatically or begin P022.
