# P019 — Installer network preflight

Status: **provisional, read-only presentation integration; production execution disabled**.
Target: `develop/v0.3`. Branch: `feat/v03-p019-synveilnet-installer-preflight`.
Exact base and P018 #178 merge: `001c1e6f1104acc679777b060035c9dc09991810`.
P018 source head: `a310c194359056dc731816f99bc855b6cf975a8e`.
Implementation and validation SHAs are recorded in the [manifest](../manifests/PROMPT019_MANIFEST.md).

## Scope reconciliation

`Synveil-v0.3-Context-Roadmap-P001-P060.md` was absent from the supplied workspace,
repository at the exact base, and scoped GitHub code search. The checked-in
[P002 architecture](../architecture/P002_SYSTEM_ARCHITECTURE.md) tentatively groups
P019–P024 under pairing, while this explicit task requests installer network
preflight. P002 is not the missing authoritative roadmap. This implementation
follows only the supplied provisional task; it neither implements pairing nor
claims authoritative P019 completion. Obtain and reconcile the authoritative
roadmap before accepting the milestone; stop and reconcile if its P019 scope
conflicts. Its absence remains a release/review restriction, not evidence of approval.

## Audit before implementation

| Owner examined | Integration decision |
| --- | --- |
| `crates/desktop/src/installer_wizard.rs` | Reuse DeviceChoice → ConnectionChoice → Preflight/Review, generation tokens, request binding, cancellation and disabled confirmation/execution gates. No second wizard. |
| `crates/desktop/src/installer_preflight.rs` | Preserve bounded package/platform inspection, channel/scope binding and artifact-plan blockers. Replace the duplicate Host-only LAN query with the shared P017 snapshot. |
| `crates/desktop/src/bridge.rs`, `qml/InstallerWizard.qml` | Keep existing Qt dispatch and background blocking worker. Run async network inspection alongside it, publish sanitized summaries, and abort network work on navigation/cancellation. |
| `crates/client/src/transport.rs`, `transport/native.rs` | Reuse `LocalDiscovery`, `NativeDiscovery`, P017 time bounds, trusted executable checks, schema parsing and kill-on-drop child ownership. |
| `crates/server-network/src/transport.rs` | Consume `LocalCapabilities::summary`; discard addresses, routes, node IDs and names. Preserve unknown installation/configuration as unknown. |
| `crates/client/src/transport/qualification.rs`, P018 native example | Preserve separate authenticated qualification and read-only GET boundaries. The wizard has no approved-endpoint/credential acquisition path, so it performs no handshake. |
| `crates/install-engine/src/review_contract.rs`, `execution_gate.rs`, `artifact_plan.rs` | Preserve engine validation, fingerprint/confirmation binding, current evidence requirements and permanent production-disable gate. Discovery cannot construct an authoritative plan or acknowledgement. |

See the [P017 contract](P017_SYNVEILNET_TRANSPORT_DISCOVERY.md),
[P018 qualification](P018_AUTHENTICATED_PATH_QUALIFICATION.md), and
[P009 confirmation contract](../installer/P009_PLAN_CONFIRMATION_SECURITY.md).

## Presentation and async lifecycle

`transport::preflight::NetworkPreflight` is a typed, ephemeral presentation model.
It exposes independent LAN and Tailscale statuses plus tri-state Tailscale
installation/configuration facts. It contains no origin, address, peer identifier,
credential, authorization token or serialized command output.

| Observation | Presentation/action |
| --- | --- |
| Usable private physical LAN candidate | Local capability observed; exact HTTPS endpoint approval, server identity verification and Synveil authentication still required. |
| Tailscale running with accepted local metadata | Local overlay capability observed; no authenticated-connectivity claim, regardless of discovered peers. |
| Installed, not configured | Separate operator-approved setup, then fresh preflight. |
| Configured, disconnected | Restore tailnet separately, then fresh preflight. |
| Missing CLI | Required CLI absent; operator-approved installation is separate. Tailscale remains optional for LAN-only use. |
| No LAN connectivity | Restore selected connection, then fresh preflight. |
| Restricted privileges | Operator permission review; no elevation request. |
| Unsupported native platform/schema or contradictory evidence | Explain limitation; use existing manually approved HTTPS flow or qualified Linux environment. No success inference. |
| Timeout/cancellation | Actionable unavailable result; fresh preflight required. |

Both connection methods are explicit inspection choices. LAN-only never selects
Tailscale; Tailscale-only never selects LAN. Local-only and manual choices remain.
All choices show both capability summaries at Review. These are preferences only;
no endpoint or selection policy is persisted or applied to the normal client.

Each requested preflight takes exactly one P017 local snapshot with an empty
endpoint list, under a two-second outer deadline. It invokes no endpoint route
queries, endpoint packet probes, verifier, retry loop or authenticated selection.
Native inspection retains its fixed address/status command vocabulary and output
bounds. Package inspection stays on the existing `spawn_blocking` worker; network
inspection uses Tokio, never the Qt event thread. The old Host LAN query is removed
from this flow to avoid duplicate network inspection; the Host setup flow itself
is unchanged.

Back, close, reopen and invalid selection transitions abort the owned async task;
dropping its network future kills its owned CLI child. Generation/request binding
also rejects queued stale callbacks. Already-started package inspection on the
existing blocking worker can finish its bounded information queries after abort;
it cannot mutate installer/network state or publish a cancelled result.

A monotonic timestamp bounds the whole host snapshot to 30 seconds, and network
observations have the same lifetime. Expired completion is rejected. Review filters
expired host evidence out of every report/plan input. A one-second presentation
refresh updates expiry without probing; it stops after the report expires. The
user can explicitly run fresh preflight from Review. No stale snapshot, preview,
network installation fact or discovered peer enables acknowledgement/execution.
Future effectful work must re-inspect and validate at use time even within this
presentation lifetime; 30 seconds is not an authorization cache.

## Trust and compatibility boundaries

There is no automatic pairing, enrollment, `tailscale up`, CLI installation,
firewall/route/service mutation, listener binding, public exposure, journal access,
credential loading/writing or production installation action in this integration.
The existing P009/P010/P011/P012 review and execution gates remain disabled.

The wizard never infers an approved endpoint from discovery. Actual connections
continue through existing exact-origin HostTrust approval, server/network identity
verification, server/owner/device/library-scoped credentials, TLS validation and
fail-closed P017/P018 selection. Authentication/identity failures remain terminal;
there is no fallback operation in this preview. P018 mismatch, 401/403, TLS,
credential-isolation, cancellation and unchanged-state tests remain regression
requirements. v0.2 wire APIs, storage, enrollment, credentials and platform
acceptance contracts are unchanged.

## Unsupported capabilities and release restrictions

Native discovery remains Linux-only. Windows/macOS native network inspection,
mobile inspection, userspace Tailscale, DNS/MagicDNS endpoint discovery, subnet
routers, unknown daemon schemas, untrusted/nonstandard CLI paths and oversized
metadata remain unsupported or require operator action under P017. A supported
manual connection flow does not imply supported native inspection.

Focused P019 CI requires Linux and Windows contract jobs, including native platform
boundary tests, wizard/install-engine regressions, P017/P018 HTTPS tests,
authentication, strict Clippy and formatting. Existing P015 CI retains PostgreSQL 17
owner-to-byte and P005/P006 regressions. Windows Qt compilation/tests or simulations
are **not P028 native acceptance**. Deterministic HTTPS fixtures use simulated
paths and cannot qualify real LAN or Tailscale connectivity.

**REAL_TAILNET_QUALIFICATION_BLOCKED** remains. Qualified native LAN evidence,
P004-D02/D03/D11/D14, P028, P036 and independent security review remain open.
First-contact/pairing trust decisions are not resolved here. No production
SynveilNet readiness, milestone closeout, automatic merge or P020 work is authorized.
