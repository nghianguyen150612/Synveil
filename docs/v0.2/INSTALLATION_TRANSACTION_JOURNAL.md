# Installation transaction journal

Prompt008 extends the Prompt007 common engine with a durable, caller-rooted
transaction journal. It does not replace the engine or define upgrade, repair,
uninstall, purge, channel selection, or user-facing error policy.

## Layout and identity

`JOURNAL_SCHEMA_VERSION` is `1`, independent of engine schema 1. A caller
supplies the journal root; the common crate never chooses a platform state
directory. A validated `plan_id` names one transaction directory:

```text
<root>/<plan_id>/
  lock
  checkpoint-0000000000000000.json
  checkpoint-0000000000000001.json
  ...
```

The plan fingerprint is lowercase SHA-256 over `serde_json::to_vec` of the
closed `InstallationPlan`. Its vectors, `BTreeMap`, and closed structures give
deterministic bytes. The hash binds local recovery evidence to the exact plan,
including artifact evidence; it is not publisher authentication. P006 remains
the release-trust authority.

Each strict JSON envelope contains schema, generation, plan fingerprint,
previous-record SHA-256, typed record, and a SHA-256 over all preceding envelope
fields. Recovery requires generation zero through the last generation without
gaps and validates every link before writing. Limits are 1 MiB per checkpoint,
16,384 records, and 4 MiB of serialized plan data.

## Writer and path safety

The transaction lock uses an OS advisory exclusive lock and stays held across
load, inspection, reconciliation, checkpoints, and mutation. A surviving lock
file is not evidence of a live process; a second live writer receives
`JOURNAL_BUSY`. `StartNew` and `ResumeExisting` are explicit. Start refuses an
existing transaction, while resume refuses a missing one.

Stable IDs accept only lowercase ASCII letters, digits, dot, and hyphen, and
reject empty/traversal forms. The implementation rejects symlink roots,
transaction directories, locks, checkpoints, and temporary checkpoints before
use. Journal cleanup is limited to recognized regular `.tmp` files inside the
transaction directory; adjacent application or user state is never removed.

## Durable checkpoint protocol

A checkpoint is serialized completely into a create-new temporary file, flushed
and `sync_all`ed, hard-linked to an unused immutable final generation (providing
no-replace behavior), the temporary name removed, the committed file synced,
and the parent directory synced. Only then does append succeed. Known regular
temporary files are never replay evidence and may be removed during recovery.
Write, file-sync, commit, and directory-sync failures are typed stops.

Rust's safe standard APIs expose directory `sync_all` on supported targets, but
filesystem, mount, storage-controller, and OS guarantees vary. These tests show
fixture and process-interruption behavior, not physical/VM power-loss safety or
native clean-machine acceptance. Native qualification remains with P020, P028,
P045, and P047; no unsafe platform-specific overclaim is introduced.

## Mutation and recovery state machine

The engine validates the exact request/plan and loads the entire chain before
resume writes. Before every `apply_effect`, it durably commits
`EffectMutationStarted`. If that append fails, apply is not called. Successful
or no-op application is verified and then recorded as `EffectVerified`; no next
effect starts until that checkpoint is durable. A post-mutation journal failure
stops later effects and leaves the mutation-start evidence authoritative.

On resume:

* a completed transaction returns `AlreadyCompleted` with no install mutation;
* every previously verified effect is skipped and read-only reverified (unless
  the whole transaction is already durably complete); drift stops for replan or
  inspection rather than reapplying;
* an unmatched mutation-start is `OutcomeUnknown`: reconciliation happens
  before any apply;
* `VerifiedApplied` is verified, checkpointed, and may continue;
* `VerifiedNotApplied` stops with `ReplanRequired`; the old effect is not run;
* `StillUnknown` preserves evidence and stops;
* known partial mutation stops for inspection; failure-before-mutation can
  report replan without an automatic retry loop;
* an unmatched `CompensationStarted` is inspection-required because Prompt007
  has no compensation reconciliation primitive; compensation success is not
  truthful until `CompensationVerified` is durable;
* final installation verification is read-only and can rerun. Completion is
  durable only after `FinalVerificationSucceeded` and `TransactionCompleted`.

Missing, corrupt, oversized, unsupported-schema, mixed-plan, gapped, symlinked,
or semantically impossible journals fail closed before adapter mutation.
Internal finite codes and dispositions are intentionally not P010 UI copy.

## INSTALL-JOURNEY-8 evidence and handoff

The dedicated fixtures simulate interruption around package-payload mutation,
platform integration, and final verification. They establish the common-engine
portion of `INSTALL-JOURNEY-8` at static, fixture, and process-interruption
simulation levels only. Acquisition interruption remains P006/P017/platform
work; Host/bootstrap interruption remains Phase E. INSTALL-JOURNEY-8 is not
marked native PASS.

P009 owns lifecycle and supported-source policy, including repair/uninstall/
purge decisions. P010 owns localized presentation and diagnostics. P011 owns
channel/version selection.

## Prompt008A recovery hardening

The in-memory and durable paths now use the same common-engine compensation candidate policy: only an explicitly reversible effect with a safe inverse and adapter support is eligible, and eligible effects are visited in reverse successful-application order. The durable path surrounds that shared lifecycle policy with fallible checkpoints. It commits `CompensationStarted` before calling `compensate_effect`, calls `verify_compensation` only after compensation succeeds, and commits `CompensationVerified` only after both operations succeed. Failure to persist the start prevents compensation; interruption after the start requires inspection and never replays compensation. A verified compensation is represented as `Compensated`, causes a stopped/replan-required recovery, and is neither treated as installed nor automatically reapplied.

Schema version 1 recovery now applies a strict semantic state machine in addition to the existing hash chain. `TransactionOpened` is required exactly once at generation zero; effect starts follow plan order; effect results require a prior mutation start; duplicate transitions are rejected; and no record may follow completion. `FinalVerificationSucceeded` requires every planned effect to be durably verified, while `TransactionCompleted` requires durable final verification. These rules run both before public `append` creates a checkpoint and again while loading all persisted records. A valid SHA-256 chain proves local integrity and ordering, not that an impossible record sequence is semantically valid.
