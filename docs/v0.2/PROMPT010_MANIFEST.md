# Prompt010 manifest

## Provenance

- Starting accepted main: `6fa83cf01609980feef1304bf5b38cf51bdb2e8d`
  (`feat: add installation lifecycle contract`).
- Task branch: `prompt010-installer-error-model`; one logical implementation
  commit and PR to `main`.
- Error-model schema: 1, independent of engine, journal, and lifecycle schema 1.
- Product version remains `0.1.0`; server/client migration and local schema
  versions are unchanged.

## Contract inventory

- Eight source families: engine, journal, recovery, lifecycle, purge,
  acquisition, preflight, and platform.
- P006 bridge: 19 sorted codes: `AMBIGUOUS_ARTIFACT`,
  `ARTIFACT_DIGEST_MISMATCH`, `ARTIFACT_TOO_LARGE`, `ARTIFACT_TRUNCATED`,
  `DESTINATION_CONFLICT`, `INVALID_MANIFEST`, `MANIFEST_AUTH_FAILED`,
  `MANIFEST_TOO_LARGE`, `NETWORK_ERROR`, `NO_MATCHING_ARTIFACT`,
  `SOURCE_COMMIT_MISMATCH`, `STAGING_ERROR`, `UNSAFE_PATH`, `UNSAFE_REDIRECT`,
  `UNSUPPORTED_ARCHITECTURE`, `UNSUPPORTED_AUTHENTICATION`,
  `UNSUPPORTED_PLATFORM`, `UNTRUSTED_ORIGIN`, and `VERSION_MISMATCH`.
- 19 user categories, 17 actions, 6 retry policies, and 10 presentation stages.
- Diagnostics: 4096 serialized bytes maximum; safe identifiers 96 bytes;
  finite source codes 64 bytes; at most two secondary actions.
- Stable support references contain only finite source family/code data.

## Evidence and safety

The Rust contract exhaustively maps every P007 engine, P008 journal/recovery,
P009 lifecycle/purge, P006 acquisition, and platform/preflight variant. It has
115 dedicated Prompt010 Rust tests and 4 cross-language Python contract tests;
the install-engine total is 381 tests (81 P007 + 78 P008/P008A + 107
P009/P009A + 115 P010). The Python AST drift guard rejects dynamic acquisition
codes. Ordinary English/Vietnamese copy has all 19 categories and is checked
for forbidden implementation terminology.

OutcomeUnknown and ambiguous recovery reconcile first; integrity cannot bypass;
protected/purge failures have no destructive action; unknown acquisition codes
fail closed without echo; successful recovery dispositions produce no error.
Diagnostics accept no arbitrary message, path, URL, command, stderr,
environment, credential, secret, or legacy free-form evidence.

## Validation and boundaries

Validation commands: `cargo fmt --all -- --check`, `cargo check -p
synveil-install-engine --locked`, `cargo test -p synveil-install-engine
--locked`, `cargo clippy -p synveil-install-engine --all-targets -- -D warnings`,
the P007/P008/P009/P010 focused validators, P004 acceptance and P005/P006
validators, documentation validation, and `git diff --check`. CI runs all four
Phase-B engine gates. No Prompt010-specific limitation is known; unrelated
platform/package/database CI concerns remain outside scope.

P011 retains channel/version selection and the Phase-B checkpoint. Later
platform and first-run prompts retain UI rendering/localization. Prompt010 adds
no telemetry, upload, retry executor, native mechanics, Qt, database, or
SecretStore dependency. Publication uses the required one-commit PR/squash
workflow; this manifest does not predict the final squash SHA.
