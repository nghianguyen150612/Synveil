# Managed server network reachability

Status: **NETWORK_READY foundation — Prompt034.** Ordinary choices are **Only
this device**, **Devices on my local network**, and **Use an existing HTTPS
address**. First-admin and the complete Host wizard remain P035 and P036.

## Boundary and modes

The API always remains the private `127.0.0.1:3000` backend and managed
PostgreSQL remains private. Clients reach a mature HTTPS edge; they never reach
either cleartext service. Production `CanonicalBaseUrl` stays HTTPS-only and
secure cookies have no LAN exception.

`LocalOnly` binds the edge only to loopback, requires no ACME or firewall rule,
and never creates a LAN/public socket. `PrivateLan` requires explicit consent
and an explicitly selected, operational RFC1918 Ethernet/Wi-Fi address. It
binds only that address, never a wildcard. `AdvancedExternalHttps` records and
validates an operator-owned origin and edge; Synveil does not rewrite or
restart that proxy, DNS, firewall, or certificate machinery.

The canonical origin is one normalized HTTPS origin root with no credentials,
query, fragment, or subpath. It feeds the API's existing exact Origin,
Sec-Fetch-Site, and CSRF policy. The backend URL is never canonical.

## Inspection and reviewed plan

Inspection returns typed, non-secret candidates with stable interface identity,
address/prefix, operational state, kind, and default-route relevance. Public,
loopback, multicast, unspecified, link-local, Docker/VM bridge, and tunnel
addresses are rejected for ordinary LAN use. One suitable candidate may be
proposed but is not consent; several always require a choice. An address that
disappears yields `AddressChanged`/`NetworkUnavailable`, never reselection.

The plan binds server, service and new UUIDv7 network identities; configuration
generation/fingerprint; mode; fixed backend; exact listener; origin; trust;
edge artifact; TLS identity; firewall effect; and expected effects. Confirmation
re-inspects all evidence. Port 443 is deterministic; a conflict is reported and
the occupant is neither killed nor displaced.

## TLS, trust, and edge

Trust modes are `ManagedPrivateCa`, `PublicWebPki`, and
`OperatorManagedHttps`. Local/private modes use one durable private CA and a
distinct leaf key. Both private keys are protected `SERVER_SECRET` credentials
(`network-ca-key`, `network-tls-key`), never JSON, arguments, logs, environment,
or Caddyfile. The public CA certificate and SHA-256 fingerprint are non-secret.
The edge receives only the leaf key; issuance alone reads the CA key.

Leaf SANs contain only the approved endpoint. Renewal atomically replaces a
leaf under the same CA and then reloads and verifies the edge. Ordinary repair
never rotates the CA. A missing CA key is recovery-required, because generating
a replacement would strand clients. Certificate status is bounded as Healthy,
RenewalDue, Expired, or Invalid.

Later authenticated/local onboarding may receive a descriptor containing only
schema version, server/network IDs, canonical URL, trust mode, CA certificate,
and fingerprint. It adds that CA to one profile's rustls roots while retaining
HTTPS, chain, validity and SAN checks. There is no public `/ca.pem`, automatic
download trust, TOFU, global root import, custom accept-all verifier, or pairing
implementation in P034.

Caddy is selected as the managed Linux edge rather than an ad-hoc proxy. It is
a separate `synveil-edge` service and package lifecycle, proxies only to
`127.0.0.1:3000`, restricts Host, and strips/replaces incoming forwarding
headers. It preserves methods, bodies, streaming and ranges. Its generated
bounded root-owned config contains no key. Least privilege grants only
`CAP_NET_BIND_SERVICE`; systemd supplies only the leaf key. The exact production
Caddy version/artifact/digest/provenance is **pending qualification**, so current
code defines the consumption contract rather than claiming a release artifact.

## Firewall and lifecycle

LocalOnly makes no firewall change. PrivateLan may add exactly one identified,
interface/network-scoped HTTPS rule through an active supported `ufw` or
`firewalld` owner after consent. Existing and equivalent user rules remain
foreign. Ambiguous/raw nftables ownership returns `FirewallNeedsAttention`;
policy is never flushed, reset, or disabled. Advanced mode is operator-owned.
No path opens port 3000, PostgreSQL, worker, migration, or storage. Synveil does
not use UPnP, NAT-PMP, PCP, router credentials, port forwarding, or a mandatory
relay.

The transaction is inspect, plan, confirm, revalidate, CAS `Preparing`,
reconcile CA/leaf/runtime/config/unit/firewall, start edge, verify TLS/name/Host,
request `/health/live` and `/health/ready` through HTTPS, then CAS `Configured`.
Every retry inspects first and preserves the same network ID, CA, origin and
owned rule. Unknown outcomes do not authorize replay. Tampering, firewall
policy changes, vanished addresses, and port hijacks become attention states.
Explicit narrowing removes only the owned listener/rule and preserves server,
database, storage, trust and identities.

## Qualification and handoff

The portable domain/config tests and deterministic interface/firewall fixtures
are not physical-LAN evidence. Linux systemd syntax is checked, while native
namespace firewall and a production Caddy artifact remain limitations for the
P036/P043 acceptance environments. Windows and macOS managed Host networking
are `NOT_YET_QUALIFIED`. A machine-local bootstrap path remains available for
P035; it is never exposed to LAN. P036 composes the final wizard.

`SYNVEIL_SERVER_NETWORK_REACHABILITY_READY`
