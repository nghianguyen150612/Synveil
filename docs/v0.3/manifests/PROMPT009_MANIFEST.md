# Synveil v0.3 — PROMPT009 Manifest

**Result: P009 source implemented; mandatory native/hosted qualification blocked.** The implementation is a provisional, non-effectful contract increment and is not production installer approval. Queued, skipped, and unexecuted checks are not passes.

## Repository and live preflight

| Item | Evidence |
|---|---|
| Repository | `https://github.com/nghianguyen150612/Synveil` |
| Starting branch and SHA | `origin/develop/v0.3` at `a840af012e8510ae0d8ac454bd60b769029f3aaa` |
| Expected starting SHA | `a840af012e8510ae0d8ac454bd60b769029f3aaa` — exact match |
| Working branch | `feat/v03-p009-plan-confirmation-contract` |
| P009 implementation commits | `7604d49154b7440c29fe7231af8fa55d86591f73`, `09a8352563c672ecb9b03ff777f0c53bc40585fd` (latest source head) |
| Draft PR | [#169](https://github.com/nghianguyen150612/Synveil/pull/169), initial head `7604d49154b7440c29fe7231af8fa55d86591f73` |
| Target branch | `develop/v0.3` |
| P007 integration | PR #166 merged at `84ee556c8619af0a95b92fb031956e3a46660984` |
| P008 integration | PR #168 merged at `a840af012e8510ae0d8ac454bd60b769029f3aaa` |
| Main | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; unchanged by this branch |
| Context roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` absent from repository and GitHub code search; P009 remains provisional |
| P004-D02/D03/D11/D14 | `RECOMMENDED_PENDING_APPROVAL`; no approval inferred |

## Merge qualification and outstanding gaps

- P006 #165 merged with historical failing CI/review checks under an explicit exception. Formatting repair #167 is merged at `f768474a9fea680b6020f2df10df07a2cf8ea6f2`. The repaired head's focused P006 authorization workflow #40320179191, PostgreSQL 17 scheduled-maintenance run #40320179564, strict format/engine-quality gate, Linux Qt UI, Windows Qt UI, and desktop/QML gate completed successfully. This closes the focused P006 checks observed here; it does not erase the earlier merge exception or establish release qualification.
- P006 still documents the fully joined PostgreSQL 17 metadata plus ObjectStore owner-to-byte integration case as not implemented. It also does not establish general RBAC, approval workflow, or broad threat-model acceptance.
- P007 #166 and P008 #168 were integrated with explicit exceptions while some checks were failed, queued, or running. Live PR check snapshots during P009 showed P008 #168 at head `75b57d076346f2866d6244b8987686f149a95376` with `qt-build-and-smoke` successful, `artifact-identity`, `quality`, and `documentation-and-quality` failed, 8 checks in progress, and 65 queued. Native Linux Qt 6 remained queued; Windows Qt 6 and standard-user Windows acceptance were not complete. `artifact-identity` failed at “Materialize approved public publisher keyring”; the other failed job logs were not available while their workflows remained active. Exact jobs: [artifact-identity](https://github.com/nghianguyen150612/Synveil/actions/runs/40437534752/job/121969698878), [documentation-and-quality](https://github.com/nghianguyen150612/Synveil/actions/runs/40437534796/job/121969698957), [quality](https://github.com/nghianguyen150612/Synveil/actions/runs/40437534813/job/121969698558). This is not post-merge qualification.
- P007 #166's merged head `4247c2df713805bcfc4c71e9b301391e630ea386` has a failed Linux Qt 6 job and failed `source-contract`/`qt-build-and-smoke` checks. The Windows Qt 6 jobs and standard-user native acceptance jobs were still in progress/queued at the live snapshot. P007's earlier PR #40319985311 did pass native Windows Qt shell/QML checks, but does not qualify P008 or the later exact merged head. P007 run #40319985300 failed its required unavailable-surface evidence; Linux clean-machine VM acceptance was skipped in #40319985223.
- Earlier P007/P008 manifests report local native desktop blockers: DBus development files and Qt 6 tooling absent; hosted exact-head native success/failure details in those manifests are not interchangeable with P008 post-merge status.

The keyring failure reinforces P004-D14's pending state. No effectful work is included in P009. It does not prevent the non-effectful contract implementation.

## Implementation summary

- `crates/install-engine/src/review_contract.rs` adds a versioned, engine-owned review snapshot, SHA-256 fingerprint, explicit review statuses, ephemeral acknowledgement state machine, invalidation reasons, and typed readiness classification.
- Snapshot creation calls `InstallerEngine::validate`. Constructors remain crate-private while no approved production plan source or provenance gate exists. Synthetic evidence is confined to engine unit tests.
- Fingerprints use domain separation, review schema v1, fixed-order serde JSON, ordered effect vectors, and sorted user choices. Artifact local paths and redacted evidence text are excluded; identity/evidence class, plans, effect/security metadata, material finding codes and policy context are bound.
- The acknowledgement is local, idempotent, generation-bound, expires, and is cleared by invalidation, cancel, or restart. It cannot be serialized into an executable credential.
- The desktop maps typed engine `PlanReviewStatus` values through the Rust view/Qt bridge, displays the real P008 unavailable reason, and keeps the accessible acknowledgement control disabled. A preview DTO marked valid still cannot enable it without a validated engine snapshot. P008 still has no authentic plan, so no fingerprint or effect summary is fabricated. “Preview only — installation is not started.” remains visible.
- There is no call from the wizard/review contract to `InstallerEngine::execute`, an installation adapter, installer journal, OS authorization, privilege helper, network-listener mutation, or credential operation.

## Test and validation record

| Required check | Result | Evidence / limit |
|---|---|---|
| `cargo fmt --all -- --check` | **PASS** | Exact command passed after P009 Rust edits; rerun after documentation-only closeout if source changes |
| `cargo test -p synveil-install-engine --locked` | **PASS** | Full package suite passed; includes 12 new review-contract tests plus existing engine/AppImage/package tests |
| Focused desktop controller harness | **PASS — 30 tests** | Temporary out-of-repository Cargo harness compiled exact checked-in `installer_preflight.rs` and `installer_wizard.rs`; includes valid-looking preview DTO not enabling acknowledgement; not a desktop bridge or native Qt build |
| `cargo test -p synveil-desktop --locked` | **BLOCKED** | `libdbus-sys` cannot find `dbus-1.pc`; desktop Rust bridge and Qt resources were not compiled by this command |
| Engine Clippy | **PASS** | `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | **BLOCKED** | Same missing `dbus-1.pc`; desktop Rust/Qt lint was not reached |
| `./scripts/test-desktop-ui.sh` | **BLOCKED** | After staging intended files and adding Cargo to PATH, script stopped because its `qmake` shim points to missing `/usr/lib/qt5/bin/qmake`; Qt 6 `qmake6` is absent. QML compilation and offscreen smoke did not run |
| `./scripts/validate-docs.sh` | **PASS** | All 24 documentation validation units passed after the final manifest and security document content was written |
| `git diff --check` | **PASS** | Both staged and unstaged diff checks passed after the final documentation content was written |
| Native Linux Qt/QML | **NOT QUALIFIED** | Must record local or exact-head hosted result; no queued job is a pass |
| Native Windows Qt/QML | **NOT QUALIFIED** | Must record exact-head hosted result; no queued job is a pass |
| PostgreSQL 17 inherited P006 gate | **PASS on repaired P006 head; P006 joined ObjectStore case remains absent** | See run #40320179564 and limitation above |

## Remaining approvals and P010 handoff

P004-D02, D03, D11, and D14 remain pending. P010 requires an approved authoritative plan source, publisher/provenance decision, native privilege/effect ownership, recovery qualifications, fresh validation rules, and exact Linux/Windows UI and acceptance evidence. P009 does not implement or authorize any of these.

## Exact source and PR heads

- Source base: `a840af012e8510ae0d8ac454bd60b769029f3aaa`.
- Implementation-only source commits: `7604d49154b7440c29fe7231af8fa55d86591f73`, `09a8352563c672ecb9b03ff777f0c53bc40585fd` (latest source head).
- P009 draft PR: [#169](https://github.com/nghianguyen150612/Synveil/pull/169); initial PR head at creation: `7604d49154b7440c29fe7231af8fa55d86591f73`. The final PR head after this documentation commit is recorded in GitHub and the final engineering report; source history is not rewritten.
- Target: `develop/v0.3`; the PR remains unmerged pending applicable review and CI.
- Final native CI, hosted CI, and required local command statuses: the latest exact-head result is recorded in the final engineering report. Never mark queued, skipped, or unexecuted checks PASS.
