# Synveil v0.3 — PROMPT011 Manifest

**Result: P011 contracts implemented; production and native qualification remain
blocked.** The source increment is fail-closed and has no installation side
effects. It is not production installer readiness.

## Initial Git state and preflight

| Item | Evidence |
|---|---|
| Repository | `https://github.com/nghianguyen150612/Synveil` |
| Expected starting SHA | `2f07e5b709680ed6d863b05628d9b08d1ac929b5` |
| Actual starting SHA | `2f07e5b709680ed6d863b05628d9b08d1ac929b5` |
| Working branch | `feat/v03-p011-native-authorization-gates` |
| Base | `origin/develop/v0.3` at the expected SHA when P011 started |
| `origin/main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`; unchanged |
| Roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` absent from the verified repository; scope remains provisional |
| P006–P009 integration | PR #165 (P006), #166 (P007), #168 (P008), and #169 (P009) are integrated in `develop/v0.3` |
| P010 integration | PR #170 merged at `2f07e5b709680ed6d863b05628d9b08d1ac929b5` on 2026-10-27 |

### P010 merge exception and post-merge qualification

P010 was merged with recorded CI/review exceptions. The PR's merge-time
comment snapshot recorded 81 checks: 25 successful, 14 in progress, 42 queued,
zero completed failures, and no independent submitted review. That snapshot was
not complete qualification.

An earlier P010 exact-head quality run failed on reserved/deferred dead-code
items. P010 added a narrow repair for those deferred items; that result was not
complete on the exact final PR head at merge time. The exact merged-head
post-merge `Quality (format and clippy)` check later passed in run
[#40483036632](https://github.com/nghianguyen150612/Synveil/actions/runs/40483036632).
Post-merge `Native desktop UI (Linux Qt 6)` and `Native desktop UI (Windows Qt
6)` also passed in that run. P010's Windows `standard-user-native` acceptance
failed in run [#40483036619](https://github.com/nghianguyen150612/Synveil/actions/runs/40483036619),
job `122130484755`, at “Record required unavailable surfaces honestly”. Thus
P010 Qt checks are recorded as passes, while Windows standard-user acceptance
remains failed/withheld. P010 integration is not installer readiness.

### Inherited security and release gates

| Gate | Verified state |
|---|---|
| P006 PostgreSQL/ObjectStore | The repaired P006 focused workflow and PostgreSQL 17 maintenance checks passed on their tested head (run #40320179191 and #40320179564, as recorded by P009). The joined PostgreSQL metadata-to-ObjectStore owner-to-byte end-to-end case remains unimplemented. |
| P028 Windows native acceptance | `standard-user-native` is still failed/withheld; no P011 change qualifies it. |
| P036 production artifacts | Production PostgreSQL 17/edge artifact identity and production keyring are unavailable. Clean-machine and Phase C evidence gates were skipped. Schema or fixture checks are not publisher authentication. |
| P004-D02 | `RECOMMENDED_PENDING_APPROVAL` — native privilege and effect ownership |
| P004-D03 | `RECOMMENDED_PENDING_APPROVAL` — optional Tailscale transport |
| P004-D11 | `RECOMMENDED_PENDING_APPROVAL` — Wizard versus install-engine effect authority |
| P004-D14 | `RECOMMENDED_PENDING_APPROVAL` — publisher signing, revocation, and downgrade policy |

No decision was approved by P011.

## Implementation summary

- Added versioned `ExecutionGateInput`, `ExecutionReadinessReport`, typed
  blockers, native authorization requirements, and an independent ephemeral
  execution-gate state machine in
  `crates/install-engine/src/execution_gate.rs`.
- Readiness reuses `InstallerEngine::validate`, requires the exact current P009
  snapshot and unexpired acknowledgement, and binds channel, scope, actor,
  artifact identity, manifest generation, capability generation, catalog and
  policy versions, installation-state generation, and the full fingerprint.
- Bumped the in-memory P009 review schema to v2 and fingerprinted explicit
  effect-catalog and security-policy versions. Missing versions remain explicit
  and invalidate old context.
- Added typed `EffectId`, per-operation adapter/resource/scope/precondition/
  privilege/verification/reconciliation/retry/reversibility/preservation
  metadata, and pure closed-catalog validation. No production catalog is
  approved or consumed.
- Distinguished native DEB/RPM package-manager authorization, current-user
  Linux AppImage integration, bounded Linux Host-helper requirements, arbitrary
  unsupported system operations, and Windows current-user Setup. Windows
  system scope remains unavailable. No UAC, Polkit, package manager, or helper
  route was invoked.
- Added pure journal-record integrity and replay classification for recovery.
  No path is opened and no production journal record is created or changed.
- Extended the Wizard review presentation with review/execution status,
  authorization category, blocker summary, required policy approvals, recovery
  readiness, and genuine plan-freshness status. Acknowledgement remains
  disabled without an authoritative plan. “Preview only — installation is not
  started.” remains visible.
- No changes were made to API v1, mobile clients, PostgreSQL schema, journal
  schema, package installer behavior, Windows per-user Setup, AppImage
  integration, or existing Host authorization.

## Adversarial coverage

| Required case | Evidence |
|---|---|
| Missing verified plan, missing publisher trust, synthetic artifact, no install eligibility | `execution_gate::tests::missing_plan_and_synthetic_artifact_fail_closed`; P010 `artifact_plan::tests::every_production_channel_fails_closed_without_roots_and_catalogs` |
| Unknown effect, owner/adapter mismatch, unsupported scope | `execution_gate::tests::catalog_rejects_unknown_effect_owner_mismatch_and_scope_mismatch` |
| Expired acknowledgement; changed plan, artifact, role, policy, or capability generation | `execution_gate::tests::acknowledgement_is_plan_bound_expires_and_changed_context_invalidates`; existing `review_contract` fingerprint and invalidation tests |
| Stale asynchronous result, duplicate request, cancellation, restart | `execution_gate::tests::state_machine_ignores_stale_revalidation_and_cancel_discards_state`; existing `review_contract::tests::invalidation_cancel_restart_and_old_completion_cannot_restore_acknowledgement` |
| Root/admin, Synveil login, or review fingerprint cannot replace OS authorization | No UID/admin/login authority input exists; `execution_gate::tests::valid_review_acknowledgement_never_becomes_execution_authority` |
| Denied/cancelled authorization has no weaker-scope fallback | `execution_gate::tests::authorization_refusal_and_cancellation_never_downgrade_scope` |
| Native package-manager, current-user, bounded Host, unsupported Linux system, and Windows system-scope distinction | `execution_gate::tests::package_manager_and_windows_scope_requirements_stay_distinct` |
| Plan mismatch, unknown outcome, missing compensation, missing final verification, damaged journal | `execution_gate::tests::recovery_requires_read_only_evidence_and_never_treats_unknown_as_ready`; `execution_gate::tests::final_verification_record_types_are_not_success_claims` |
| Irreversible operation classification | Recovery fixture with an irreversible effect and no inverse classifies as `CompensationUnavailable`; effect catalog binds reversibility and inverse fields. |
| No arbitrary QML effects/commands or effectful production boundary | `execution_gate::tests::production_gate_boundary_has_no_effectful_api_or_qml_input`; existing Wizard source-boundary and QML smoke tests |
| No package-manager invocation, privilege prompt, journal write, listener change, or artifact launch | Same production source-boundary test checks for those routes; no effect method exists in the gate. Recovery tests use in-memory sealed records. |
| Existing Host and Connect flows remain | Existing `installer_wizard::tests::qml_smoke_startup_and_existing_first_run_routes_remain_present` checks `Main.qml` routes. |

## Validation results

Exact local results after the final source/documentation edit are recorded below;
blocked checks remain blocked rather than being inferred from another workflow.

| Check | Result | Evidence / limit |
|---|---|---|
| `cargo fmt --all -- --check` | **PASS** | Run on the final P011 tree |
| `cargo test -p synveil-install-engine --locked` | **PASS** | Full engine package suite, including P011, journal, recovery, lifecycle, and prior review tests |
| `cargo clippy -p synveil-install-engine --all-targets --locked -- -D warnings` | **PASS** | Strict engine Clippy on the final P011 tree |
| `cargo test -p synveil-desktop --locked` | **BLOCKED** | `libdbus-sys` cannot find `dbus-1.pc` / `libdbus-1-dev`; desktop compilation did not reach project code |
| `cargo clippy -p synveil-desktop --all-targets --locked -- -D warnings` | **BLOCKED** | Same DBus development dependency; desktop lint did not run |
| `./scripts/test-desktop-ui.sh` | **BLOCKED** | Local Qt 6 `qmake`/`qmllint` are unavailable; script uses a shim pointing at missing `/usr/lib/qt5/bin/qmake` |
| `./scripts/validate-installer-engine.sh` | **PASS** | Runs the install-engine locked test suite |
| `./scripts/validate-docs.sh` | **PASS** | Documentation validation completed on the final documentation |
| `git diff --check` | **PASS** | No whitespace errors |
| P011 exact-head native Linux Qt 6 | **NOT RUN** | Requires hosted exact-head workflow; P010's Linux Qt pass is not inherited qualification |
| P011 exact-head native Windows Qt 6 and standard-user acceptance | **NOT RUN** | Requires hosted exact-head workflows; P010 standard-user failure remains separately recorded |
| P006 joined PostgreSQL/ObjectStore, P028, P036 | **BLOCKED / OUTSTANDING** | Inherited gates described above; P011 does not waive them |

The desktop dependency failures do not indicate a P011 desktop source failure,
but they also do not establish desktop compilation, QML lint, or native
acceptance. No CI result is reported as PASS until the exact P011 head has run.

## P012 prerequisites and stop point

P012 needs the authoritative roadmap, explicit applicable P004 decisions, an
approved publisher trust source and exact artifact-byte revalidation, complete
per-channel effect catalogs and owner qualification, approved per-operation
native authorization, read-only journal inspection and recovery qualification,
resolution of relevant P006/P028/P036 gates, and successful exact-head Linux and
Windows desktop/native acceptance. P011 stops here; P012 has not started.

## Git workflow

| Item | Value |
|---|---|
| Branch | `feat/v03-p011-native-authorization-gates` |
| Base/target | `develop/v0.3` |
| Merge | Not performed or requested |
