# Prompt038 manifest — connection setup simplification

- Stack parent: PR #65 / `add-unified-first-run-welcome`.
- Starting P037 head: `67e79ff6aad0cca382358a4ab6740e9a19e86418`.
- Main and merged P036 source at start: `85ca700ea1946fc89da4bf4e6257c47a2b74df01`.
- P037 status at start: open. P036 qualification marker remains withheld.

Fresh Connect has one required Server address field and one Connect action.
Rust bounds input, trims outer whitespace, defaults an omitted scheme to HTTPS,
and delegates final acceptance to `CanonicalBaseUrl`. Explicit HTTP is rejected.
A safe canonical host/port label is derived after parse. The readiness probe
precedes the durable single-writer client mutation.

DesktopController/local IPC remain the owner. Existing configuration admission,
stable profile ID, generation fencing, unknown-outcome refresh (without replay),
and origin-change credential fencing remain intact. QML contains stable Connect
automation/accessibility names and no networking, credential, auth, or library
ownership. Focused CI covers parsing, client/desktop tests, generated-module QML
checks, static validation, formatting, and Clippy.

P039–P042 are deferred. Before merge: fetch final P037 in main, rebase/retarget
this PR to main, verify a P038-only diff, and rerun focused CI.
