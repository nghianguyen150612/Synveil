# Managed PostgreSQL runtime artifact contract

Production consumption is restricted to a closed, immutable artifact identity
containing the exact PostgreSQL 17 full version, `linux`/`x86_64`, Synveil
packaging revision, SHA-256, file inventory, compatibility range, authenticated
upstream source provenance, and the PostgreSQL License and notices. The runtime
is installed under `/opt/synveil/postgresql/17/<runtime-id>`. The reviewed
systemd unit names that exact digest-addressed directory. No mutable selector or
cluster data is present in the runtime artifact; durable data stays under
`/var/lib/synveil/postgresql/17/data`.

No production artifact or digest is asserted by Prompt033. CI may obtain an
exact PostgreSQL 17 package as a **CI SERVICE FIXTURE**; it is not a released
managed artifact. Publication must use real authenticated upstream bytes and
their computed checksum—never `latest`, a mutable URL, or a placeholder hash.

Managed Host runtime manifests use schema version 2. The signed manifest binds
the exact PostgreSQL 17 minor version, the closed runtime file inventory,
license and notice digests, and upstream provenance. Its exact upstream Git
revision and HTTPS source are repeated in the closed `PROVENANCE` file along
with the component, version, target OS and architecture. `packaging_revision`
binds the wrapper to its Synveil source commit. Host-input verification checks
that identity, the detached signature and file hashes, confirms x86_64 ELF
identity, then runs the verified `postgres --version` from a private copy of
the verified bytes. The runtime is replaceable under `/opt`; the cluster
remains in the separate durable data directory.
