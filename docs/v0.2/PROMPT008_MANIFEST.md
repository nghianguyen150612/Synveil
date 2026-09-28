# Prompt008 implementation manifest

- Starting accepted main: `07d51d035edaed698f8f9d93dd6e067098e3f2c6`
- Starting subject: `feat: add installer common engine`
- Journal schema: `1` (separate from engine schema `1`)
- Implementation: `crates/install-engine/src/journal.rs`, exported by `lib.rs`
- Tests: `crates/install-engine/tests/journal_contract.rs`
- Storage: caller-rooted, one `plan_id` directory, exclusive `lock`, immutable
  append-only generation files
- Plan binding: SHA-256 of deterministic compact serde JSON bytes
- Integrity: contiguous generations plus previous-record and record SHA-256
- Bounds: 1 MiB/checkpoint, 16,384 checkpoints, 4 MiB serialized plan
- Locking: `fs2` exclusive OS lock held through recovery and execution
- Commit: create-new temp, write/flush, file sync, no-replace hard link, remove
  temp, sync committed file, sync transaction directory
- Recovery: explicit start/resume; verified effects skip apply; unmatched
  mutation-start reconciles; unknown/partial/ambiguous compensation stops;
  read-only final verification may rerun; durable completion is idempotent
- Dedicated Prompt008 tests: 61
- Total install-engine tests: 142 (81 Prompt007 plus 61 Prompt008)

Validation commands are `cargo fmt --all -- --check`, `cargo check -p
synveil-install-engine --locked`, `cargo test -p synveil-install-engine
--locked`, `cargo clippy -p synveil-install-engine --all-targets -- -D
warnings`, the P007 and P008 focused scripts, P004/P005/P006 validation scripts,
documentation validation, and `git diff --check`.

Directory synchronization uses safe standard APIs where supported. Actual
filesystem and device persistence behavior requires later native physical/VM
power-loss evidence. Current evidence is static, fixture, and deterministic
process-interruption simulation; it does not mark INSTALL-JOURNEY-8 native PASS.

P009 retains detailed lifecycle policy. P010 retains user-facing error copy and
diagnostic UX. P011 retains channel/version policy. Publication follows one
task-branch commit and a squash PR to `main`; this manifest does not predict the
final squash SHA.
