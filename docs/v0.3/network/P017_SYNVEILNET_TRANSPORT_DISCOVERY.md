# P017 — SynveilNet transport discovery and selection

Status: provisional, opt-in read-only foundation. This is not SynveilNet onboarding
or production network qualification. Baseline is
`8c48483b7e0d29ec956a940a97cb5d16bcbdefe2` on `develop/v0.3` after P016A PR #176.
The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` was absent from the
workspace, repository and scoped GitHub code search. Reconcile it before claiming
authoritative P017 completion; stop if newly supplied scope conflicts.

## Repository audit and ownership

| Existing owner | Audit and reuse |
| --- | --- |
| `synveil-server-network` | Existing `NetworkCandidate`, `InterfaceKind`, private-LAN planning, canonical origin, firewall plan and reconciliation contracts. Added UI-neutral transport evidence. Shared Linux interface classification extracted without changing the existing Host classification order. |
| `synveil-server-host` | Native `network::candidates` uses bounded `ip` queries and physical sysfs evidence; excludes tunnels and bridges. Reuses shared classification. Privileged plan, TLS certificate, service and firewall effects remain behind existing confirmation; discovery calls none of them. |
| `synveil-client` | Existing `NetworkHintSource` is only a change hint. New optional `transport` module owns bounded local inspection and selection, without changing hint/session/process behavior. |
| `synveil-client-sync` | `ServerProfile`, `HostTrust`, profile-bound `LoadedDeviceCredential`, `HttpSyncRemote::probe_health`, protected credential store and canonical TLS transport are reused. No new identity, credential store or enrollment protocol. |
| `synveil-auth` / API | Existing one-time grant expiry, device credential verification/revocation and owner/device scope remain authoritative. No authentication or API production code changes. |
| `synveil-platform` | Existing runtime, platform paths, lifecycle and protected secret-store abstraction remain unchanged. Unsupported native discovery reports a typed reason rather than elevating or installing a provider. |
| Desktop Installer / install engine | Existing three-step Wizard preflight and disabled Tailscale finding remain unchanged. New sanitized `CapabilitySummary` is a future consumption boundary; it cannot authorize an installation plan or establish onboarding. |

See the pending policies in
[P004 security contracts](../security/P004_SECURITY_POLICY_CONTRACTS.md) and
[P004 implementation gates](../security/P004_IMPLEMENTATION_GATES.md).

## Evidence and trust model

Transport kinds are `Lan`, `Tailscale`, `Unavailable` (the LAN, TAILSCALE and
UNAVAILABLE conceptual states). Tailscale evidence separately represents absent or
failed inspection, installed but not configured, configured but disconnected, and
connected local metadata. Unknown installation/configuration is `None` in the
summary, not an assertion of absence. Typed reasons include permission denial,
unsupported state, expiry, inconsistent evidence, untrusted endpoint, identity
conflict, authentication required, access revoked, unreachable, route change, timeout and cancellation.

Connected metadata is not reachability. Discovery summaries always report
`authenticated: false`. Only `Selection::Selected` records a successful current
Synveil handshake; it is ephemeral advisory evidence, never permission or a session.
Normal authenticated operations still enforce revocation and scope independently.
No capability result is persisted or treated as a reusable authorization cache.

An `Endpoint` is derived from a profile with explicitly approved `HostTrust`, using
its exact canonical HTTPS origin and server installation identity. Callers must
obtain profiles and credentials through the existing review/store lifecycle. The
production `SynveilVerifier` owns immutable profile/credential/scope bindings.
Endpoint equality includes profile identity and the approved network/trust generation;
a verifier rejects bindings for different server installations or owner/device/library scopes. It
uses the existing anonymous readiness GET, validated TLS and server/network identity
headers, followed by the authenticated owner/device checkpoint GET. Readiness must
succeed before sending the profile-bound bearer. A readiness identity mismatch,
redirect, TLS failure or malformed protocol stops before the authenticated request.
No discovered IP, peer hostname, interface name or tailnet membership grants trust.
No credential is copied to a changed origin or manufactured from a discovery hint.

`LocalDiscovery` and `EndpointVerifier` are trusted in-process adapter ports, not
untrusted IPC inputs. Fake implementations establish deterministic tests; they are
not a production authorization path. Never attach a verifier that reports success
from ping, installation status, anonymous readiness alone or peer naming.

## Read-only local inspection

The Linux adapter has a closed command vocabulary, no shell and no caller-supplied
program/flags:

- `/usr/bin/ip -j -d address show`
- `/usr/bin/ip -4|-6 -j route get <approved numeric address>`
- `/usr/bin/tailscale status --json --peers=true`

Executable paths and their ancestors must be root-owned, not group/world writable
and not symlinks. Commands run as the current user with cleared environment, null
stdin/stderr, bounded stdout, and kill-on-drop. The adapter reads sysfs interface
classification evidence. It does not bind a listener, scan peers, send ping, join a
tailnet, call `tailscale up`, alter routes/firewall/services, request elevation,
install software, access installer journals or exchange enrollment grants.

Limits: eight endpoints, 64 interfaces, 128 addresses, 256 peer records, eight
addresses per peer, 128 KiB per command, two seconds per command/local selection
inspection, two seconds per authenticated handshake and fifteen seconds for the
complete selection. A stalled handshake is an unreachable read-only probe and
may retry/fall back only under the explicit policy, within the total budget. Native inspection alone
is bounded by its finite query count; callers should use the selection wrapper or
a two-second outer timeout for preflight. A dropped/expired future kills and reaps
the owned CLI child. There are at most two read-only handshakes per eligible
endpoint (at most two GETs each); only `Unreachable` is retried. There is no
background retry loop or enrollment retry.

Tailscale `BackendState` is allowlisted. `NeedsLogin`/`NoState` mean installed,
`Stopped` means disconnected, `NeedsMachineAuth` requires action. `Running` requires
kernel TUN mode, an online unexpired local node and consistent node/IP evidence.
Unknown states, malformed records, duplicate node IDs/IPs, public peer addresses
and unknown expiry encoding fail closed. The upstream `ipnstate.PeerStatus`
format omits false `Expired` and omits `KeyExpiry` for nodes without a key expiry;
that documented omission is accepted. An explicit expiry is checked against the
clock. The CLI format reference is
[Tailscale ipnstate](https://github.com/tailscale/tailscale/blob/main/ipn/ipnstate/ipnstate.go).
No real daemon/version interoperability claim follows from these schema fixtures.

A LAN route must be direct, on an operational observed physical Ethernet/Wi-Fi
adapter, with a source on the endpoint's private IPv4 subnet or IPv6 ULA prefix.
Tailscale routes require a source from local overlay metadata, a Tailscale adapter,
and the exact explicitly approved stable node ID for the destination IP. Duplicate
routes, conflicting peer ownership and disagreement between adapters fail closed.
Tailscale CGNAT and `fd7a:115c:a1e0::/48` addresses cannot become LAN candidates.

## Deterministic selection policy

| Explicit policy | Ordered candidates | Allowed outage fallback |
| --- | --- | --- |
| `LanOnly` (default) | LAN only | None |
| `PreferLan` | LAN, then overlay | To an independently approved overlay endpoint |
| `PreferTailscale` | Overlay, then LAN | To an independently approved LAN endpoint |
| `TailscaleOnly` | Overlay only | None |

Canonical origins break same-transport ties lexically, independent of input order.
All supplied bindings must name the same approved server installation. Duplicate
origins are rejected. Missing CLI, not configured, disconnected and unreachable are
availability failures; identity/auth/expiry/permission/unsupported/inconsistent
failures stop selection. No policy changes an origin's approved transport,
credential scope or TLS requirements. An overlay-only outage never selects LAN.

After the authenticated handshake, local evidence is inspected again. Any observed
change invalidates the result as `RouteChanged`; it is not reused on another path.
This fences observed changes during the probe, not every kernel routing race or
future connection. Full before/after equality is deliberately conservative and may
reject when unrelated peer/interface evidence changes. Future consumers must run
fresh checks and normal authentication at use time.

## Tests, compatibility and qualification boundary

Deterministic fake adapters cover dual availability, policies, missing/partial
installation, disconnected tailnet, retry bounds, cancellation, route changes,
spoofed/conflicting peer identity, expiry, permission denial, unsupported hosts,
IPv4/IPv6 and endpoint binding. Native tests cover read-only command vocabulary,
malformed metadata, output/time bounds, child cleanup and bounded local inspection.
A loopback TLS test proves the existing authenticated handshake checks both approved
identity headers before bearer transmission; existing redirect/TLS/revocation and
credential-binding regression tests also run. These are not real LAN/tailnet tests.

Native inspection is Linux-only and deliberately conservative. Windows/macOS and
mobile native inspection, DNS/MagicDNS endpoint resolution, userspace Tailscale,
subnet-router/gateway routes, nonstandard CLI paths, symlinked CLI installations,
large status outputs and unrecognized daemon schemas require future qualification
or explicit operator action. Hosted Linux CI requires a successful trusted iproute2 address query; Windows CI
checks the typed unsupported boundary. Local Linux evidence in this environment includes
missing `ip`/Tailscale CLIs, not demonstrated endpoint reachability. No genuine
qualified tailnet was available, so real-world Tailscale reachability is unverified.

The v0.2 wire API, Android/iOS code, sessions, profile storage, sync/storage
operations, approval workflows, host permissions and Wizard production paths are
unchanged. Tailscale is optional; LAN-only selection works without it. The only
shared production refactor is interface classification with existing behavior
preserved. No dependency on Tailscale software or a Tailscale SDK is added.

P004-D02/D03/D11/D14 remain unresolved. P028 Windows native acceptance, P036
publisher/signing and independent security review remain open obligations. This
increment authorizes neither network enrollment/listener expansion nor production
SynveilNet onboarding. No automatic merge or P018 work is included.
