# Managed edge artifact status

The production managed edge is Caddy, installed as the separately owned
`SERVER_NETWORK_INTEGRATION`. Its exact version, per-platform artifact URL,
SHA-256 digest, license, provenance, closed inventory and compatibility remain
**PENDING QUALIFICATION**. Installation must fail closed until a release
manifest supplies and verifies all fields. CI fixtures are not production
artifacts; no mutable package, `latest` download, or arbitrary installer is
permitted.

The deterministic generated Caddy configuration may only bind the persisted
listener and proxy to `127.0.0.1:3000`. It removes incoming `Forwarded` and
`X-Forwarded-*`/`X-Real-IP` values before setting reviewed metadata. The CA key
is never delivered to the daemon; only the leaf key uses `LoadCredential=`.
