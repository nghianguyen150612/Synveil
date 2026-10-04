# Managed Server Configuration (Prompt031)

Status: **Implemented configuration foundation; downstream Host work remains open**

This document defines the durable configuration and protected-secret boundary
for the v0.2 managed Host path. It does not provision PostgreSQL, choose object
storage, install services, expose listeners, or create the first administrator.
Those remain P032–P035 responsibilities; P036 owns end-to-end acceptance.

## One authority

`/etc/synveil/server-config.json` is the sole managed, non-secret server
configuration authority. Its JSON document carries `schema_version: 1`, is
strictly typed, rejects unknown fields, and is limited to 32 KiB. Serialization
is deterministic. A SHA-256 fingerprint covers only canonical non-secret
configuration bytes; it is useful for inspection and compare-and-swap updates,
not authentication or signing. Explicit generation starts at zero and advances
for an accepted mutation. Wall-clock values, process IDs, usernames, and secrets
are not serialized.

The schema records a local server-installation UUIDv7, deployment profile,
PostgreSQL major and ownership, non-secret endpoint metadata when known,
logical runtime identity references, credential IDs, dependency identity,
managed PostgreSQL data-root policy, storage state, network state, service
topology version, and setup state. The installation ID is stable across normal
restart and repair. It is local setup evidence, not a public server or TLS
identity. Missing configuration with existing or ambiguous server evidence
requires reconciliation; it does not mint a replacement ID.

Unknown future schema versions fail closed and are left untouched. Linux uses
the fixed production path. `SYNVEIL_SERVER_CONFIG_FILE` is an explicit
absolute override for development, tests, or administration; the selector
does not search the working directory or scan other locations. Other
platforms do not probe the Linux `/etc` path by default and retain explicit
legacy operator mode unless an override is supplied. If the canonical managed
file is absent, current documented legacy operator configuration can still be
used. Once managed configuration is present, conflicting legacy inputs fail
as ambiguous.

## PostgreSQL profiles

| Profile | Owner | Major | P031 state |
|---|---|---:|---|
| `PersonalHomeManaged` | Synveil private runtime | 17 | Managed endpoint may be pending until P033 resolves it |
| `AdvancedExternal` | Operator-owned PostgreSQL | 17 | Protected URL is supplied through the same schema and secret store |

SQLite, Docker-managed Personal/Home, automatic use of system PostgreSQL, and
other PostgreSQL majors are not V1 alternatives. The managed database data
policy is `/var/lib/synveil/postgresql/17`; P033 still owns service identity,
directory creation, cluster initialization, database/role creation, and
service lifecycle. This path is separate from the user-selected object root.

P031 creates a generated 32-byte database role password for managed mode and
stores its 64-character lowercase hexadecimal representation as
`database-password`. Once P033 supplies a validated endpoint, the writer builds
the runtime `database-url` from that endpoint, the protected generated
password, and the configured logical role/database names. The URL is never
written to JSON. Advanced mode accepts a bounded PostgreSQL URL through a typed
secret wrapper and persists it only in the protected credential source.

## Storage, network, and topology state

Storage begins as `NotConfigured`. P032 receives `update_storage`, a typed
compare-and-swap operation that changes only the storage selection. P031 does
not choose a root or create `objects/` or `staging/`. The schema performs
structural path checks; P032 owns path identity, capacity, ownership, and
ObjectStore qualification.

Network begins as `NotConfigured`. P031 exposes only a local-loopback update
boundary for future coordination. It cannot write a wildcard/public address or
set a public origin. P034 owns reachability, TLS, firewall, and remote access.
The service topology identity is version 1; P033 owns units, service identities,
supervision, ordering, and health.

## Linux source layout and runtime delivery

| Path | Owner/group | Mode | Purpose |
|---|---|---:|---|
| `/etc/synveil` | `root:synveil` | `0750` | Managed configuration root |
| `/etc/synveil/server-config.json` | `root:synveil` | `0640` | Non-secret authority |
| `/etc/synveil/credentials` | `root:root` | `0700` | Administrator-owned source secrets |
| `database-url` | `root:root` | `0600` | Runtime database connection credential |
| `database-password` | `root:root` | `0600` | Generated managed role password for P033 |
| `rebaseline-token-key` | `root:root` | `0600` | Persistent rebaseline HMAC key |

The runtime `synveil` identity may read configuration but cannot freely replace
it or read the root-only source credentials. P033 will deliver per-service
copies using the locked ADR-025 systemd `LoadCredential=` boundary. Runtime
database loading is shared by API and worker: a protected file is authoritative;
explicit `DATABASE_URL` is retained for documented developer/operator use only
when no file source is selected. File plus environment is an error. The
rebaseline loader follows the same file-versus-plaintext-environment ambiguity
rule, using `rebaseline-token-key` and retaining the existing key environment
variable only as explicit compatibility input.

The file loaders require bounded regular single-link files, reject symlinks,
reject relative credential paths, use no-follow opens on Unix, compare opened
file identity and size with inspected metadata, and redact errors. A single trailing LF or CRLF is
normalized for the rebaseline key; key content must otherwise be exactly 64
lowercase hex characters. Rebaseline and database secret wrappers redact
`Debug`/`Display`; managed secret material is zeroized on drop where it enters
Rust-owned buffers. Secret-derived diagnostic fingerprints are not exposed.

## Durable writes and reconciliation

The Linux writer inspects before mutation, creates only the expected missing
directories, checks each expected path for symlink substitution and type,
sets exact modes without recursive chmod, then verifies the result. Secret
creation is exclusive (`create_new`), sets mode before writing, writes all
bytes, syncs the file and containing directory, and rereads for verification.
An existing secret is inspected and preserved; it is never opened with truncate.

Configuration updates validate the current fingerprint, serialize canonical
bytes, create a unique same-directory temporary file exclusively, write and
sync it, atomically rename it over the authority, sync the directory, and
reopen/verify exact bytes. A directory file lock serializes update compare and
swap. Concurrent or unknown edits fail instead of being silently overwritten.

Inspection distinguishes absent, valid, unsupported schema, malformed, wrong
type, redirected/symlinked, permission mismatch, identity conflict, missing or
invalid secret, and partial configuration. If a secret or config write has an
unknown outcome, the next run inspects the authoritative target. It never
generates a replacement secret simply because acknowledgement was lost.
Ordinary repair, update, and reinstall do not rotate keys or database
credentials. Rotation requires an owner-authorized workflow; changing the
rebaseline key invalidates outstanding proof tokens.

The current writer deliberately reports reconciliation states instead of
repairing or deleting unknown ownership. It does not alter PostgreSQL, object
data, storage selection, network reachability, or admin-bootstrap state.

## Current runtime and limits

API and worker share the protected database runtime loader. API loads the
selected configuration and required credentials, validates database settings,
connects/applies migrations and prepares configured storage before opening its
listener. Managed mode is rejected until P033 has materialized `database-url`
and P032 has selected storage. Legacy operator mode continues to support its
documented explicit environment inputs, including the safe loopback API
default.

Filesystem semantics are covered in disposable fixtures, including Linux mode
bits, symlinks, hard links, atomic replacement, CAS, interruption injection,
secret preservation, and redaction. Fixture ownership is not proof of actual
root-owned `/etc` state. P031 did not run PostgreSQL, create a cluster, install a
service, bind a public listener, select storage, create an administrator, or
qualify a clean machine. Ubuntu 24.04/Fedora 42 x86_64 remain managed-Host
implementation targets; Windows native P028 acceptance stays pending.

P032 owns storage selection; P033 owns runtime provisioning/services;
P034 owns reachability; P035 owns first-admin bootstrap; P036 owns complete
Host readiness acceptance. This foundation alone does not complete
`first-run-2.json` or guided self-hosting.
