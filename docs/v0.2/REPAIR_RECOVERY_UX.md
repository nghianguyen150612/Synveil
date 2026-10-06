# Repair and recovery UX — Prompt042

P042 closes Phase F source scope. It adds a presentation and bounded action
composition to ADR-048; it does not create a durable recovery authority.
Native acceptance, release qualification, and P043–P048 remain separate.

## Ownership audit

Audited the roadmap, installation product and upgrade/repair/uninstall
contracts, P041 progress contract, ADR-048/054/071, Desktop Control, Release
Operations and Upgrade Safety; Main.qml, bridge/presentation, controller,
control, process, launch, client-sync host/runtime, install-engine lifecycle
and AppImage integration, Synveil.iss, and AppRun.

`DesktopControllerSnapshot::recovery_summary()` remains the canonical bounded
projection (128 items, exact aggregate counts, connection generation).
`UiRecoveryItem` adds finite `RecoveryState` and `RecoveryCapability`; QML
renders fixed safe values. Process availability remains launch-manager-owned.
The UI does not parse errors or persist recovery truth. Stale generation,
revision, or freshness cannot admit a root or restart mutation. Reopening the
GUI inspects current owners; it never replays a previous action.

## Platform and action matrix

| Product action | Canonical owner | Support and platform scope | Mutation and unknown result | Success evidence | Preserved state and unsupported behavior |
|---|---|---|---|---|---|
| Repair Synveil | install-engine lifecycle; Windows SynveilSetup; AppImage integration; native DEB/RPM manager | Guidance only on Windows/Linux; unavailable elsewhere | Desktop performs no repair mutation. A user uses the same-version official Windows installer repair mode, existing AppImage integration repair, or native package manager. Their existing lifecycle reconciliation governs uncertain results. | Desktop makes no completion claim; installer/integration verification remains authoritative. | Profiles, credentials, bindings, setup/first-sync evidence, pause, conflicts, pending work, startup preference, server configuration/data and unknown adjacent files are preserved. No download, upgrade, downgrade, arbitrary command or reset. |
| Reconnect server | P038 connection form → controller → profile owner | Supported where profile configuration is needed; an existing configured connection can be explicitly reviewed via Edit connection | Opens the existing form, keeps current origin/label, uses canonical HTTPS validation and identity/credential fencing. OutcomeUnknown retains admission until a fresh newer profile snapshot; inspect current configuration before retry. | Authoritative profile validation/configuration and fresh controller snapshot, not form submission alone | Origin never changes silently; TLS and identity checks remain; credentials are not cleared as a shortcut. Transient outages stay waiting with bounded runtime retry. |
| Restore missing folder | controller retry_recovery → SyncNow → host/runtime root gate | Supported for an unavailable configured root; recovering roots remain waiting | Reconnect drive/restore access to the SAME original folder; button only requests a bounded recheck. OutcomeUnknown retains admission until a fresh newer snapshot. | Available root after canonical path, marker, profile/library identity and redirect validation | ROOT UNAVAILABLE != EMPTY TREE; ROOT UNAVAILABLE != DELETE EVERYTHING. No mkdir, relocation, new binding, marker edit, library recreation or remote deletion. Wrong roots fail closed. |
| Restart background service | BackgroundClientManager → existing supervised stop → explicit stop proof → existing ensure_inner launch owner → control readiness | Supported only for a running Linux user service whose kernel endpoint peer matches its MainPID. Windows task, direct process, unavailable supervisor and other platforms receive guidance | Shares launch in-flight/cooldown gate. Native graceful stop stays with existing service authority. Requires loaded/inactive/dead, MainPID=0, ControlPID=0 and no reachable endpoint before launch. Pending Stopping/Starting is transient manager admission state. OutcomeUnknown inspects before any continuation; never repeats uncertain stop/start. Check service status is an inspection/continuation action, not a second restart. | Running from canonical endpoint inspection after restart; start acknowledgement never proves success | One process and writer lock, canonical executable resolution, all durable user/client/server state and startup preferences remain. No process-name killing, forced task termination or QML supervisor. |

Linux stop-state parsing deliberately rejects deactivating, missing properties,
nonzero process/control IDs, missing units and failed status commands. Native stop/start/probe commands run asynchronously under explicit timeouts; cancellation is uncertain and
retains the manager's reconciliation fence. Existing launch cooldown remains. Only the service utility child is canceled on timeout; the Synveil client is never forcibly killed.

P042 also repairs the pending-setup root-seeding query inherited from P041: it
now selects the first-sync evidence required by the canonical replica decoder.
The existing idempotence/descendant test additionally proves that repeated setup
preserves both incomplete and completed first-sync evidence. No schema, root
identity or setup semantics change.

## Presentation and first-run integration

The recovery card asks what needs action and what is already waiting. Missing
folder guidance names the original folder without an absolute path or marker
instructions. Repair is explicitly **How to repair Synveil**, a guidance dialog,
not a mutation button. Unsupported restart receives ordinary product guidance.
Stable accessibility names identify the summary, four product actions, busy
indicator and feedback. Keyboard focus returns after repair guidance.

P041 still owns Synveil ready / Server ready / Signed in / Library ready / First
sync. Durable first-sync evidence is untouched. Current root/auth/runtime health
still fences first-sync presentation. Recovery never resumes PausedByUser,
clears conflicts or substitutes a new pending setup identity. Existing Resume,
Sign in, attention decisions and Resume setup keep their original owners.

## Validation boundaries

Focused tests cover typed waiting/action states, guidance capabilities, stale
admission, preserved root identity/first-sync evidence, restart coalescing,
full-stop proof, uncertain stop/start, timeout and GUI reopen. Existing host/root,
redirect, marker, overlap, no-deletion, writer-lock, process/control, installer
and AppImage suites remain intact. The P042 workflow separates projection,
client lifecycle, root/data safety, installer contract, desktop/QML, and quality.

See [the evidence manifest](PROMPT042_MANIFEST.md) for commands actually run,
results and unavailable/native evidence. No native Windows/Linux graphical
acceptance or clean-machine qualification follows from source/offscreen tests.
P043 is explicitly deferred.
