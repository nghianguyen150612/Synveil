# Prompt034 manifest

- Starting main: `a6df03604d01ee6d4fca2dda72869e6d783c5898`
- Branch: `codex/p034-server-network-reachability`
- Inherited: P029 product contract, P030 dependency strategy, P031 managed
  configuration, P032 storage, and P033 services are locked. P028 Windows
  native acceptance remains independently withheld.
- Modes: `LocalOnly`, `PrivateLan`, `AdvancedExternalHttps`.
- Backend: fixed `127.0.0.1:3000`; PostgreSQL exposure is unchanged/private.
- Identity/state: one UUIDv7 network integration ID, persisted in `Preparing`
  and retained in `Configured` and repair.
- Edge: separate least-privileged `synveil-edge` Caddy service. Production
  version, artifact, digest, provenance and closed inventory are pending; CI
  fixtures cannot qualify them.
- Trust: managed private CA, public WebPKI, operator-managed HTTPS. CA/leaf keys
  are distinct `network-ca-key`/`network-tls-key` credentials; canonical HTTPS
  origin is the existing API origin authority.
- Selection: typed private Ethernet/Wi-Fi candidates; no guessing, wildcard,
  tunnel/container adoption, or automatic reselection.
- Firewall: bounded owned `ufw`/`firewalld` rule contract; foreign rules remain
  foreign; nftables ambiguity needs attention; no router mutation.
- Port: exact persisted 443; conflicts fail closed. Mode broadening is explicit;
  narrowing removes only owned exposure.
- Repair/recovery: inspection precedes reconciliation, IDs/CA/origin are stable,
  leaf renewal may use the same CA, and unknown effects are never blindly
  replayed.
- Evidence: portable unit/static/systemd checks are distinct from native
  firewall, physical LAN, and production artifact evidence.
- Deferred: P035 first-admin and P036 full clean-machine Host acceptance.

No secret values are part of this manifest.
