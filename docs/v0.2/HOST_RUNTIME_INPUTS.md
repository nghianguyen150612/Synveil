# Managed Host runtime inputs

Managed Host accepts only explicitly supplied, signed runtime inputs. The
desktop and privileged helper never download dependencies or choose their
checksums. This path supports Ubuntu 24.04 and Fedora 42 on x86_64.

The build/operator prepares one input registration JSON with exactly three
entries: `server`, `postgresql`, and `caddy`. Each entry names absolute paths
for `manifest`, `signature`, `trusted_keyring`, and `input`. The keyring is an
independent trust decision supplied by the operator; Synveil does not fetch a
publisher key or claim an upstream identity. The server manifest comes from
`scripts/build-host-server-input.sh` and must be signed by the approved build
process. PostgreSQL and Caddy manifests must identify the exact supplied
version, provenance, licenses/notices, platform and closed file inventory.
Schema-v2 runtime manifests bind a 40-character upstream Git revision and
HTTPS source URL to the exact `PROVENANCE` fields: component, version, source
revision, source URL, qualified OS and architecture. `packaging_revision`
binds the runtime package to the Synveil source commit. Registration rejects
provenance bytes that disagree with either identity and verifies the runtime's
reported version from a private copy of its already verified executable.

On the supported build platform, create a new output directory and assemble
the bundle:

```sh
python3 deploy/server/assemble-host-inputs.py \
  --inputs /absolute/path/host-runtime-registration.json \
  --output /absolute/path/host-runtime-bundle
```

The assembler verifies each signature and every payload file before publishing
the bundle. It preserves signed manifests and payloads in a closed inventory.
Registration is a separate explicit root administration action on the target
Host:

```sh
sudo /usr/libexec/synveil-host/register-host-inputs.py \
  --bundle /absolute/path/host-runtime-bundle \
  --server-keyring /absolute/path/trusted-server-builders.gpg \
  --postgresql-keyring /absolute/path/trusted-postgresql-publisher.gpg \
  --caddy-keyring /absolute/path/trusted-caddy-publisher.gpg
```

The registrar checks the target OS and architecture, verifies all three
signatures again using the explicitly supplied trusted keyrings, verifies the
closed payloads again, and publishes immutable digest-addressed cache objects
under `/var/cache/synveil/host-runtime`. It then atomically updates the
root-owned `/etc/synveil/host-runtime-catalog.json` consumed by the privileged
helper. Existing cache objects are preserved. Registration does not start a
service, initialize a database, create a Host, or alter user data; the desktop
performs a separate full review and asks for consent before Host effects.

The source build script emits an **unsigned**, unqualified server input. It
does not sign or publish it. No PostgreSQL or Caddy production bytes, trusted
publisher keyrings, signatures, or production digests are checked into this
repository. Until those authentic inputs are supplied and approved, the Host
helper fails closed at dependency preflight. That is an input blocker, not a
substitute for source implementation or native acceptance.

The package installs the bounded producer and registrar at
`/usr/libexec/synveil-host/`. Package installation only supplies these tools;
it does not register runtime inputs or provision Managed Host state.

## P036 production inventory and installation binding

`acceptance/p036/production-artifacts.schema.json` defines inventory schema v1.
The required `production-artifacts.json` is deliberately absent until the real
artifacts and trusted signer policy exist. A populated metadata file without
the referenced artifact, detached signatures, provenance, license and notice
files does not satisfy the gate.

The inventory has eight entries: `desktop`, `server`, `postgresql17`, and
`edge`, each for Ubuntu 24.04 x86_64 and Fedora 42 x86_64. Every entry binds an
exact version, source revision, architecture, path, size and independently
calculated SHA-256. Desktop and server source revisions equal the inventory's
Synveil `source_commit`; PostgreSQL and edge revisions equal their authenticated
upstream source commits. Their signed runtime manifests separately bind
`packaging_revision` to that Synveil commit. The inventory product version
must equal the checked-out Cargo workspace version; the current source remains
`0.1.0`, so this gate does not assert a v0.2.0 package. The desktop entry names
the exact DEB or RPM. Server, PostgreSQL and Caddy entries name a signed
runtime-manifest-v2 file and its closed payload root. Provenance is separately
signed and bound to component, version, platform, architecture and source
revision. License IDs are fixed to MIT for Synveil desktop/server, PostgreSQL
for PostgreSQL, and Apache-2.0 for Caddy; the actual license and notice bytes
are hashed and checked.

P036 verification also requires an independently provisioned publisher
keyring and expected fingerprints for the inventory signer and each component
signer. GitHub repository variable `SYNVEIL_P036_TRUSTED_KEYRING_B64` contains
the base64 bytes of that public keyring; the workflow writes it to a private
runner-temporary file and passes its path as `SYNVEIL_P036_TRUSTED_KEYRING`.
`SYNVEIL_P036_INVENTORY_FINGERPRINT`, `SYNVEIL_P036_DESKTOP_FINGERPRINT`,
`SYNVEIL_P036_SERVER_FINGERPRINT`,
`SYNVEIL_P036_POSTGRESQL17_FINGERPRINT`, and
`SYNVEIL_P036_EDGE_FINGERPRINT` inputs are public trust metadata, not artifact
credentials. The inventory signature is `production-artifacts.json.sig`.
Those values must come from the approved publisher/release trust decision; this
repository does not invent them.

`scripts/validate-p036-production-artifacts.py` verifies these signatures,
paths, sizes, hashes, package/runtime versions, source identity, architecture,
provenance and license bytes. On native runners it invokes the same
`prepare-host-runtime.py` verifier used for production registration. The native
acceptance harness then assembles the three verified server runtimes and
registers them through `register-host-inputs.py`; the privileged Host helper
re-verifies the closed signed runtime catalog before installation. Inventory
verification is therefore a precondition of the actual acceptance installation
and the existing root catalog remains the runtime authority.

Until the inventory, detached signatures, independently trusted public
keyring/fingerprints and real artifact files are supplied, the P036 artifact
gate reports `BLOCKED_BY_EXTERNAL_INPUT`.

The validator applies the versioned JSON Schema and rejects duplicate JSON
keys, unknown fields, redirected paths, incomplete platform/component matrices,
placeholder identities, size or digest mismatches, and signatures from any key
other than the independently pinned fingerprint. It runs each verified runtime
version probe from a private copy. Server version probes cover the API, worker
and migration binaries. The signed server provenance records source, version,
platform and architecture without asserting that release qualification has
already passed.

## P036 native runner contract

The workflow intentionally keeps the required labels
`synveil-ubuntu-24.04-p036`, `synveil-fedora-42-p036`, and
`synveil-p036-vm-control`. The Ubuntu/Fedora runners must be disposable,
x86_64, booted with systemd, have Rust stable/Cargo, sudo, Python venv support,
package-repository access, sufficient build disk, and an active desktop session
with a registered polkit agent for the normal `pkexec` Host authorization. A
root-owned `/etc/synveil-p036-disposable` marker must be mode 0400 and contain
`synveil-p036-disposable-v1`. The journey runs as an unprivileged desktop user;
it does not replace polkit with direct root execution. The runner must start
without existing Synveil Host configuration, database, storage, runtime or
service state.

The VM-control runner also needs libvirt `virsh`, OpenSSH, and a pre-provisioned
disposable Ubuntu 24.04 or Fedora 42 guest with a completed native Host journey.
Its domain name and UUID, SSH target, private identity path and pinned
`known_hosts` file are supplied as repository variables. The domain description
must be `synveil-p036-disposable-v1` and all disks must resolve inside
`/var/lib/libvirt/images` or `/var/lib/synveil-p036-vm`. The workflow stops and
starts only that UUID-verified disposable domain to test recovery after a hard
power interruption. Missing runner labels, markers, inputs or capabilities
remain `BLOCKED_ENVIRONMENT`; they do not skip or replace the required jobs.
