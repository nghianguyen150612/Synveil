# Upgrade, Repair, Uninstall, and Purge contract

Prompt009 adds one platform-neutral policy layer to `synveil-install-engine`. It validates an immutable installed-state snapshot, a lifecycle request, a P007 plan, and typed lifecycle-effect semantics **before** P007 execution or creation of a P008 transaction. It does not coordinate execution or duplicate the journal.

## Installed state and preservation

`InstalledStateSnapshot` uses finite payload, native-package, integration, runtime, journal, and startup classifications. Product identity is an exact version plus an explicitly optional source commit. Snapshots contain only logical identities/presence, never secret values. Credential presence is complete evidence only when Present carries a non-empty stable non-secret identity, Absent carries no identity, and Unknown remains unverified. Unknown/newer/interrupted package, native-package, or integration state and unknown runtime ownership fail closed for operations that depend on them; an incomplete/ambiguous journal also blocks new mutation.

Every ordinary Upgrade, Repair, and Uninstall plan must preserve application configuration, credential state, client sync state, user library content, server configuration, server database, server object data, and external dependencies. `PreservationSnapshot` compares stable logical evidence before and after; missing evidence cannot produce verified success. An explicit `Disabled` startup preference remains disabled.

## Upgrade

Upgrade requires explicit source-to-target `UpgradeCompatibility` and exact P006 artifact binding (artifact id/type, version, commit, artifact and manifest hashes). Supported is the only mutating disposition; unknown, unsupported, intermediate-required, and downgrade dispositions fail closed. Already-installed healthy targets are no-ops. P011, not this policy, selects releases.

Package replacement does not authorize client/server migrations, resets, credential rewriting, first-run, or sync. Canonical application owners retain ADR-021 locking, ledger, checksum, and future-schema safeguards. Binary rollback is not promised when newer application state may exist; compatibility must first be proven and databases are never reset to enable rollback.

## Repair

Same-version reinstall is Repair. Repair restores only proven package payload, Synveil-owned integration, native-package state through native authority, and scoped owned runtime state. Lifecycle operations are bound to one canonical owner/resource/authority tuple: payload operations use PACKAGE_OWNED/InstallerAdapter, integration operations use PLATFORM_INTEGRATION_OWNED/PlatformIntegrationAdapter, native package operations use NATIVE_PACKAGE_STATE/NativePackageManager, and runtime stop/cleanup uses EPHEMERAL_RUNTIME_STATE/InstallerAdapter. It preserves unknown adjacent files and all durable state. A different artifact version is Upgrade, not Repair. Runtime stop is typed and ownership-scoped; arbitrary process-name killing and shell commands are absent.

## Ordinary Uninstall

Uninstall removes only proven package payload, proven owned integration, native state through its package manager, and proven ephemeral runtime state. It structurally preserves every durable class, including credentials, sync databases, libraries, and Host data, so compatible preserved state remains available to a reinstall. An already absent payload/integration is a verified no-op.

## Separate Purge boundary

Purge is not an `InstallationIntent` and cannot be hidden in Uninstall. `PurgeRequest` separately requires destructive intent, confirmation evidence, independently authorized classes and target identities, canonical ownership, proven containment, resolved traversal, and symlink rejection. Scope cannot expand. The generic installer Purge contract exposes only APPLICATION_CONFIG. Credential removal belongs to a separately scoped OS SecretStore-owner action and client-state removal belongs to a separately scoped synveil-client action; user libraries, server configuration/databases/objects, native package databases, and external dependencies are not representable generic installer purge targets. An unavailable root remains protected.

## Journal and cancellation

Mutable lifecycle plans remain P007 `InstallationPlan`s executed through `InstallerEngine::execute_journaled`. Active incomplete or outcome-unknown P008 transactions require reconciliation before any new lifecycle mutation. Changed plans retain P008 deterministic fingerprints and create a replan rather than changing a transaction in place. Cancellation before mutation is a zero-mutation stop; cancellation after the boundary follows journal reconciliation, never assumed rollback or destructive cleanup.

## Acceptance bridges and deferrals

Fixture coverage maps damaged owned payload plus populated state to **INSTALL-JOURNEY-6**, and ordinary removal plus populated state to **INSTALL-JOURNEY-7**. The historical upgrade fixture binds v0.1.0 to `fa23232ff0154f627ebdd221ec5435134f177af0` for **UPGRADE-TEMPLATE-1** without claiming that a v0.1.0-to-v0.2.0 release path is supported. These are policy fixtures; native acceptance remains pending.

P010 owns user-facing error wording. P011 owns channels and release selection. Later Linux, AppImage, and Windows prompts own platform mechanics; Host prompts own server provisioning/data operations.
