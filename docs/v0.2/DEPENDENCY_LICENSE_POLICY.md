# Dependency license policy decision

`sqlx-core 0.8.6` previously pulled `webpki-roots 0.26.11` through SQLx's
`tls-rustls-ring-webpki` feature. That crate declares `CDLA-Permissive-2.0`,
which is outside the repository's general license allowlist. The dependency is
not needed for SQLx's TLS implementation: SQLx provides the existing
`tls-rustls-ring-native-roots` feature.

The workspace now selects that feature. SQLx loads the operating system's
trusted certificates through `rustls-native-certs`; rustls still performs
certificate-chain and hostname verification, and a missing or untrusted root
does not become a successful TLS connection. This keeps external PostgreSQL
`verify-full` verification enabled while using the target platform's trust
store. The decision removes only SQLx's older bundled-root dependency; reqwest's
separately reviewed `webpki-roots 1.0.9` exception remains exact-version scoped
in `deny.toml`.

The policy does not add `CDLA-Permissive-2.0` to the global allowlist and does
not suppress cargo-deny. Revisit this choice if SQLx changes its TLS backend or
the supported platform trust-store contract changes.
