# Synveil patch provenance

This directory is the published `sqlx-sqlite` 0.8.6 crate from
<https://crates.io/crates/sqlx-sqlite/0.8.6>.

- Upstream SQLx source commit: `bab1b022bd56a64f9a08b46b36b97c5cff19d77e`
  (`sqlx-sqlite` in the SQLx 0.8.6 release).
- Published crate archive SHA-256:
  `c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`.
- Local source changes: only the `libsqlite3-sys` version requirement in
  `Cargo.toml`, from `0.30.1` to `>=0.39.0, <0.40.0`. No Rust or C source is
  modified.

The upstream SQLx maintainers merged
<https://github.com/transact-rs/sqlx/pull/4502> at
`10462fa4414c6c7f1ed1d39bb31678e3b726878c`, widening the SQLx 0.9 SQLite
adapter's `libsqlite3-sys` range to `<0.40.0` to permit the same 0.39.x line.
Synveil applies the narrowly-scoped range update to its existing 0.8.6 driver
and independently compiles it against `libsqlite3-sys` 0.39.1. This vendor
patch should be removed when an upstream SQLx release compatible with the
project is published and resolves to the fixed binding without a patch.

This is a reviewable dependency compatibility patch, not an advisory ignore.
The independent security review remains required before accepting it.
