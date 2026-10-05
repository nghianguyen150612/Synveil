# Prompt039 source manifest

- Starting main: `7627bfa4da04c8d8ca9a713719eacd644ee621c9`.
- Merged sources: P037 `63e5ece71e3f3415c9a3ecf749d682ff1f18fab7`;
  P038 `7627bfa4da04c8d8ca9a713719eacd644ee621c9`.
- P036 native Phase-E marker remains withheld. P037/P038 had no final corrected
  hosted focused-workflow PASS claim at this prompt's start.
- Product term: **one-time device code** (field label: **Device setup code**).
- Changed surfaces: Rust authentication presentation and tests, native QML,
  static validator/docs integration, focused workflow, roadmap, ADR, and these
  Prompt039 documents.
- States cover required, signing in, authenticated, status checking,
  invalid/network/server/rate-limit/secure-storage/protocol failures,
  reconciliation, signing out, and sign-out attention.
- Protocol, local IPC, HTTP owner, `SecretStore` owner, and one-time semantics
  are unchanged. The masked sensitive 69-byte-bound field is cleared before
  dispatch and never enters durable QML state.
- Success remains durable-first. Unknown outcomes refresh canonical state and
  never replay authentication or sign-out automatically.
- Confirmed sign-out preserves profile configuration and libraries. Existing
  generation/origin fencing prevents cross-origin credential use.
- Stable QML object names and accessible names cover the page, field, actions,
  feedback, busy state, and confirmation.
- `.github/workflows/authentication-ux-polish.yml` provides focused domain,
  desktop/QML, security, and quality jobs with native dependencies.
- P040 library onboarding, P041 end-to-end progress, and P042 broad recovery
  remain deferred. No Phase-F checkpoint is emitted.
