# Connection setup simplification (Prompt038)

## Product flow and authority

From Prompt037 Welcome, **Connect to Synveil** opens a native page containing
one required **Server address** field, HTTPS helper text, **Connect**, and
**Back**. Connect normalizes, validates, probes, persists, refreshes authoritative
state, then advances to the existing authentication-required stage. P039 owns
authentication; P040 library setup, P041 whole-flow progress, and P042 recovery.

`UserServerAddress` enforces the 2048-byte bound, trims outer whitespace, and
adds `https://` only when a scheme is omitted. Explicit `http://` is rejected.
`CanonicalBaseUrl` remains the final parser and continues rejecting credentials,
paths, query, fragment, malformed ports, controls, backslashes, and non-HTTPS
production origins. The canonical host plus a useful explicit port becomes the
display label; `Synveil` is the safe fallback. This label is not identity.

QML never creates a profile, opens SecretStore, writes SQLite/configuration, or
performs HTTP. The established DesktopUiBridge → DesktopController → local IPC
→ synveil-client single-writer path performs the bounded rustls `GET
/health/ready` probe before durable configuration. Failures therefore leave a
fresh profile unconfigured. Product feedback is typed and does not expose raw
remote, TLS, HTTP, or Rust diagnostics.

## Correction, uncertainty, and restart

Existing edits preserve `ServerProfileId`. Label-only edits preserve origin,
creation time, authentication, and credentials. A real origin change requires
confirmation and ADR-043 fencing: forget enrollment, record cleanup, remove old
origin secrets, then commit the new origin. No credential crosses origins.

An `OutcomeUnknown` is reconciling, not an immediate retry: request an
authoritative refresh and compare the intended profile ID and canonical origin.
Advance only on a match; an unconfigured refresh permits a safe user retry and
a different origin is a conflict. Restart reconstructs successful state from
durable authorities; failed candidates are not persisted. P038 does not infer
private-CA trust from P036's opaque descriptor; that remains an integration
boundary.

This source is stacked on Prompt037 branch `add-unified-first-run-welcome` at
`67e79ff6aad0cca382358a4ab6740e9a19e86418`. After P037 merges, P038 must be
rebased onto and retargeted to main before merge.
