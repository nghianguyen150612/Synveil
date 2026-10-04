# Managed PostgreSQL runtime artifact contract

Production consumption is restricted to a closed, immutable artifact identity
containing the exact PostgreSQL 17 full version, `linux`/`x86_64`, Synveil
packaging revision, SHA-256, file inventory, compatibility range, authenticated
upstream source provenance, and the PostgreSQL License and notices. The runtime
is staged under `/opt/synveil/postgresql/17/<runtime-id>` and a root-owned atomic
`current` symlink selects it. A cluster is never present in this artifact.

No production artifact or digest is asserted by Prompt033. CI may obtain an
exact PostgreSQL 17 package as a **CI SERVICE FIXTURE**; it is not a released
managed artifact. Publication must use real authenticated upstream bytes and
their computed checksum—never `latest`, a mutable URL, or a placeholder hash.
