# P018 — authenticated path qualification

Status: provisional deterministic qualification; native LAN and tailnet acceptance
remain blocked. Base: `dc00aed815bfd06a0090338f267b19ad7ec8d164`, P017 merged via
PR #177, target `develop/v0.3`. Branch:
`test/v03-p018-synveilnet-authenticated-path-qualification`.
The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` remains unavailable
in this checkout and supplied workspace. No authoritative P018 completion is
claimed. Reconcile that document before accepting this provisional scope; stop
if it conflicts. No P019 work is authorized by this increment.

## Architecture audit

P018 reuses [P017](P017_SYNVEILNET_TRANSPORT_DISCOVERY.md): `Endpoint::from_profile`
requires explicitly approved HostTrust and a numeric private LAN or overlay origin.
Linux `NativeDiscovery` supplies bounded, read-only interface/route/peer evidence.
`SynveilVerifier::add` binds immutable profile, origin, server installation,
owner/device/library scope and credentials loaded by `LocalStateStore` from the
existing protected SecretStore. Independent endpoints require independent approval;
no discovered name, IP, node ID or tailnet membership establishes authentication.

`HttpSyncRemote::probe_health` first validates TLS and the anonymous readiness
response with approved server/network identity headers, then requests the
owner/device/library checkpoint with its profile-bound bearer. Redirects, bad TLS,
identity conflicts, unsupported or contradictory evidence and authorization errors
stop selection. An expired credential rejected as unauthenticated is not an outage;
grant expiry/revocation remain owned by existing auth/API contracts. There is no
new authentication stack, network identity database, authorization cache or remote
mutation in qualification. Every new selection authenticates again.

P017 selection keeps its explicit policies: LanOnly and TailscaleOnly never cross
transports; PreferLan and PreferTailscale permit an independently approved binding
only after an availability failure. Only unreachable read-only handshakes retry
(at most twice per endpoint, two seconds each, fifteen seconds total selection).
The before/after route fence is conservative observed equality, not kernel route
pinning or future authorization. Normal operations must authenticate at use time.

## Deterministic evidence and boundaries

`crates/client/src/transport/qualification.rs` runs actual Rustls HTTPS servers
on ephemeral **loopback only** listeners, with independent generated CAs,
installation/network IDs, profiles and device secrets. Enrollment receipts are
obtained during fixture setup and persisted/loaded through the existing credential
lifecycle using a test memory SecretStore and temporary SQLite database. The
selection stage performs only readiness and checkpoint GETs.

The private test module maps those HTTPS origins onto simulated P017 route evidence.
It constructs internal bindings solely because production explicitly rejects
loopback as a qualified LAN/overlay path. No production endpoint rule is relaxed.
These are HTTPS integration tests with simulated network paths, not physical LAN
or Tailscale tests. Separate installations/scopes are rejected as alternatives;
a second fixture pair models one installation/scope approved on two paths.

Assertions cover all four policies, independent TLS trust, spoofed installation
identity rejected before bearer transmission, profile credential non-transfer,
401/403 stopping fallback, fresh authentication after success/revocation/recovery,
route change fencing, unavailable routes without HTTP, two-attempt outage bounds,
network outage followed by mandatory fallback authentication, IPv4 and IPv6 HTTPS
when the host supports IPv6 loopback, cancellation closing a stalled request, and
unchanged SQLite/WAL, trust descriptor, enrollment and secret state after failed selection, plus a Linux IPv4 route-table snapshot comparison. Existing P017 tests
retain simulated IPv6 LAN/overlay routes, unsupported/expired/contradictory evidence,
full selection deadlines and native child cleanup. Existing client-sync contracts
retain TLS, redirects, protocol, scope, expiry, missing/forgotten credentials and
credential lifecycle regression coverage.

Fixtures assert loopback bind addresses and GET-only qualification traffic. Native
P017 command vocabulary tests retain read-only inspection. No production listener,
route/firewall operation, installer/Wizard integration, custom VPN, public exposure
or automatic tailnet enrollment is introduced. The local disposable PostgreSQL
regression service is intentionally bound to `127.0.0.1:55432`; Docker creates its
own test-service networking. This is not SynveilNet exposure. Privileged host
firewall invariance is not independently measured by these nonprivileged tests.

## Opt-in Linux native procedure

Run only on a qualified Linux host with an **already enrolled** owner/device,
existing approved numeric HTTPS profiles, supported protected OS credential store,
and operational trusted `/usr/bin/ip` and (for overlay) `/usr/bin/tailscale`.
Use an existing current-schema state database with no pending credential cleanup;
opening the existing store can perform its normal schema/cleanup lifecycle. Back
up non-secret profile/state metadata beforehand. This runner never creates a
profile, approves trust or exchanges an enrollment grant. It accepts no bearer.

Create an operator-private JSON file with the following configuration; substitute
existing identifiers, never secrets:

```json
{
  "state_db": "/operator/existing/client-sync/state.sqlite3",
  "policy": "LanOnly",
  "bindings": [{
    "profile_id": "existing-profile-uuid",
    "transport": "Lan",
    "peer_id": null,
    "owner_user_id": "existing-owner-uuidv7",
    "device_id": "existing-device-uuidv7",
    "library_id": "existing-library-uuidv7"
  }]
}
```

```sh
cargo run -p synveil-client --example p018_native_qualification --locked -- /operator/qualification.json
```

For overlay use `Tailscale`, `TailscaleOnly` and the independently approved stable
peer ID; for preferred policies supply both approved profiles for the same
installation and owner/device/library scope. Do not invent trust descriptors or
copy credentials between profiles. Record exact Git SHA, Rust/OS/kernel and
iproute2/Tailscale versions, UTC time, configuration family (IPv4/IPv6), approved
installation identity and the runner's sanitized result. Protect endpoint/interface
metadata as operational information. Never capture raw status JSON, keyring values,
enrollment tokens, bearer headers or TLS private keys.

The runner records selected origin, installation, transport, interface and source;
an additional before/after native snapshot rejects observed changes. Output is
point-in-time evidence only. Repeat all four policies, then have the operator
manually disconnect/reconnect each qualified path and revoke/restore test access.
Run again for every outage, recovery, certificate/identity rejection and credential
state. Record unavailable reasons and bounds. The runner itself must not alter
routes, firewall, Tailscale state or access. External manual changes belong in the
operator's evidence log, with before/after observations; do not automate enrollment.
Cancellation is covered deterministically; a killed runner grants no session.

Supported provisional native configuration remains direct physical Ethernet/Wi-Fi
private IPv4 or IPv6 ULA, and kernel-TUN Tailscale with exact approved numeric peer
address/stable ID and consistent online/unexpired local/peer evidence. DNS/MagicDNS,
userspace mode, subnet routers/gateways, symlinked/nonstandard CLI paths, unknown
schemas, Windows/macOS native inspection and unqualified host configurations remain
outside scope. No claim of real daemon compatibility follows from schema fixtures.

## Environment, CI and remaining gates

Local environment: Debian 13 x86_64 managed cloud container, Linux 6.18.55,
iproute2 6.15.0, no Tailscale CLI, Rust 1.99.1
(2026-10-13), existing system D-Bus runtime plus locally extracted Debian 1.16.2-2
development headers/pkg-config metadata. No system package, route or firewall
configuration is changed by the Rust qualification. PostgreSQL 17 Docker is used
only for retained P005/P015 authorization regressions.

`REAL_TAILNET_QUALIFICATION_BLOCKED`: runtime policy reports `vpn_configured: false`
and `automatic_tailscale_access: false`; no qualified tailnet endpoint or operator
credentials were supplied. Actual authenticated LAN connectivity is also unqualified.
There is no fabricated real-network pass. See the
[P018 manifest](../manifests/PROMPT018_MANIFEST.md) for observed validation and CI.

Focused P018 CI runs formatting, strict Clippy, P017 transport/network contracts,
P018 HTTPS integration tests, client-sync/auth/client/Host lifecycle regressions,
native runner compilation, documentation checks and whitespace checks. Existing
P015 CI is triggered by these crate/docs/lockfile changes and explicitly retains
P005 PostgreSQL, P006 content and P015 joined owner-to-byte regressions. Existing
P016A Host tests remain in the full Host suite. Simulated CI cannot satisfy native
acceptance.

P004-D02/D03/D11/D14, P028 native acceptance, P036 publisher/signing, authoritative
roadmap reconciliation and independent security review remain open. Do not merge
automatically, enable production SynveilNet onboarding or begin P019.
