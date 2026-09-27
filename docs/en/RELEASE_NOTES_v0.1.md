# Synveil v0.1 release notes

Synveil v0.1 is the first release-oriented self-hosting and desktop sync
slice. It provides a PostgreSQL-backed server/API foundation and a native
two-process desktop architecture for supported Linux and Windows targets.

## User-visible capabilities

- Self-hosted PostgreSQL-backed API with authenticated profiles, libraries,
  logical file/folder metadata, health/readiness probes, uploads/downloads,
  version history, and bounded sync/change-feed protocols.
- Native `synveil-desktop` Qt UI/control surface paired with the independent
  `synveil-client` synchronization process.
- Server profile onboarding, HTTPS readiness verification, authentication, and
  OS-backed device credential storage.
- Library setup with a user-selected local folder, including safe admission of
  an existing ordinary non-empty folder as initial local content.
- Bidirectional synchronization foundations with durable local state,
  pause/resume controls, bounded recovery, and durable conflict attention.
- Explicit conflict actions where supported: **Accept Remote** and
  **Retry Local** against the current base.
- Safe handling for unavailable roots, pending setup, client/server recovery,
  and ambiguous responses without blind replay or mass deletion.
- Linux DEB/RPM package policy and a Windows portable ZIP policy. Linux uses a
  user systemd client unit; Windows uses current-user Task Scheduler when
  explicitly enabled.

## Compatibility notes

The v0.1 public release has no prior public compatibility contract. Supported
upgrade behavior is the frozen migration and durable-state contract in
`docs/en/UPGRADE_SAFETY.md`; it is not an arbitrary historical client/server
downgrade matrix. Unknown future schema fails closed, migrations are
forward-applied, and automatic downgrade is not guaranteed.

## Important limits

- The desktop package does not bundle PostgreSQL or a server installer.
- macOS and mobile platforms are deferred and unsupported in v0.1.
- Native Windows runtime acceptance is a separate environment gate; cross-build
  results must not be read as native Windows validation.
- Package signing, repository publication, automatic updates, and a full
  package rollback transaction are outside this release contract.
- Filesystem naming differences and unsupported/reserved names can produce a
  safe rejection rather than a sync operation.

Read the [operations guide](RELEASE_OPERATIONS.md) before installing or
upgrading and the [package policy](RELEASE_PACKAGING.md) for artifact details.
