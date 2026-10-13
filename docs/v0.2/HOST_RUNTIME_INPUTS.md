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
