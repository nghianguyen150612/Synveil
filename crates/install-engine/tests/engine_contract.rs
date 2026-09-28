use std::collections::{BTreeMap, BTreeSet, VecDeque};
use synveil_install_engine::*;

#[derive(Clone)]
struct FakeAdapter {
    findings: Vec<PreflightFinding>,
    privilege: bool,
    preconditions: VecDeque<bool>,
    outcomes: VecDeque<ApplyOutcome>,
    verifications: VecDeque<bool>,
    reconciliations: VecDeque<ReconciliationOutcome>,
    final_ok: bool,
    compensation_supported: bool,
    compensation_ok: bool,
    compensation_verified: bool,
    calls: Vec<String>,
}
impl Default for FakeAdapter {
    fn default() -> Self {
        Self {
            findings: vec![],
            privilege: true,
            preconditions: vec![true].into(),
            outcomes: vec![ApplyOutcome::Success].into(),
            verifications: vec![true].into(),
            reconciliations: vec![ReconciliationOutcome::StillUnknown].into(),
            final_ok: true,
            compensation_supported: true,
            compensation_ok: true,
            compensation_verified: true,
            calls: vec![],
        }
    }
}
impl InstallationAdapter for FakeAdapter {
    fn preflight(&mut self) -> Vec<PreflightFinding> {
        self.calls.push("preflight".into());
        self.findings.clone()
    }
    fn privilege_available(&mut self, _: PrivilegeRequirement) -> bool {
        self.calls.push("privilege".into());
        self.privilege
    }
    fn inspect_preconditions(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("precondition:{}", e.effect_id));
        self.preconditions.pop_front().unwrap_or(true)
    }
    fn apply_effect(&mut self, e: &InstallationEffect) -> ApplyOutcome {
        self.calls.push(format!("apply:{}", e.effect_id));
        self.outcomes.pop_front().unwrap_or(ApplyOutcome::Success)
    }
    fn verify_effect(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("verify:{}", e.effect_id));
        self.verifications.pop_front().unwrap_or(true)
    }
    fn reconcile_unknown(&mut self, e: &InstallationEffect) -> ReconciliationOutcome {
        self.calls.push(format!("reconcile:{}", e.effect_id));
        self.reconciliations
            .pop_front()
            .unwrap_or(ReconciliationOutcome::StillUnknown)
    }
    fn compensation_supported(&mut self, e: &InstallationEffect) -> bool {
        self.calls
            .push(format!("compensation_supported:{}", e.effect_id));
        self.compensation_supported
    }
    fn compensate_effect(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("compensate:{}", e.effect_id));
        self.compensation_ok
    }
    fn verify_compensation(&mut self, e: &InstallationEffect) -> bool {
        self.calls
            .push(format!("verify_compensation:{}", e.effect_id));
        self.compensation_verified
    }
    fn verify_installation(&mut self, _: &InstallationPlan) -> bool {
        self.calls.push("final_verify".into());
        self.final_ok
    }
}

fn request(intent: InstallationIntent) -> InstallationRequest {
    InstallationRequest {
        intent,
        target_scope: TargetScope::CurrentUser,
        verified_artifact: None,
        user_choices: BTreeMap::new(),
    }
}
fn effect(id: &str, phase: EffectPhase) -> InstallationEffect {
    InstallationEffect {
        effect_id: id.into(),
        phase,
        owner: ResourceClass::PackageOwned,
        resource_class: ResourceClass::PackageOwned,
        authority: EffectAuthority::InstallerAdapter,
        target_scope: TargetScope::CurrentUser,
        preconditions: vec![Precondition {
            kind: "path-state".into(),
            expected: "absent".into(),
        }],
        privilege: PrivilegeRequirement::CurrentUser,
        mutation_kind: "INSTALL_PAYLOAD".into(),
        verification: Some(VerificationContract {
            verification_id: format!("verify.{id}"),
            expected: "present".into(),
        }),
        retry_policy: RetryPolicy::Never,
        reconciliation_policy: ReconciliationPolicy::InspectByEffectId,
        reversibility: Reversibility::Irreversible,
        safe_inverse: None,
        preservation_constraints: vec![],
        artifact_identity: None,
    }
}
fn plan(intent: InstallationIntent, effects: Vec<InstallationEffect>) -> InstallationPlan {
    InstallationPlan {
        schema_version: 1,
        plan_id: "install.current-user.v1".into(),
        intent,
        target_scope: TargetScope::CurrentUser,
        ordered_effects: effects,
        preservation_set: vec![
            ResourceClass::ApplicationConfig,
            ResourceClass::CredentialState,
            ResourceClass::ClientSyncState,
            ResourceClass::UserLibraryContent,
            ResourceClass::ServerConfig,
            ResourceClass::ServerDatabase,
            ResourceClass::ServerObjectData,
            ResourceClass::ExternalDependency,
        ],
        expected_completion_verification: VerificationContract {
            verification_id: "installation.complete".into(),
            expected: "launch-eligible".into(),
        },
        artifact: None,
    }
}
fn run(p: &InstallationPlan, a: &mut FakeAdapter) -> InstallationResult {
    InstallerEngine.execute(&request(p.intent), p, a)
}
fn apply_count(a: &FakeAdapter) -> usize {
    a.calls.iter().filter(|c| c.starts_with("apply:")).count()
}
fn entered(r: &InstallationResult) -> Vec<Stage> {
    r.events
        .iter()
        .filter_map(|e| match e.kind {
            EngineEventKind::StageEntered { stage } => Some(stage),
            _ => None,
        })
        .collect()
}

#[test]
fn valid_install_happy_path() {
    let mut a = FakeAdapter::default();
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.install.payload", EffectPhase::Install),
                effect("platform.integration.menu", EffectPhase::Integrate),
            ],
        ),
        &mut a,
    );
    assert!(r.completed);
}
#[test]
fn lifecycle_stage_order() {
    let mut a = FakeAdapter::default();
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.install.payload", EffectPhase::Install),
                effect("platform.integration.menu", EffectPhase::Integrate),
            ],
        ),
        &mut a,
    );
    assert_eq!(
        entered(&r),
        vec![
            Stage::Preflight,
            Stage::Plan,
            Stage::Install,
            Stage::Integrate,
            Stage::Verify,
            Stage::Complete
        ]
    );
}
#[test]
fn preflight_before_plan() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    assert_eq!(entered(&r)[..2], [Stage::Preflight, Stage::Plan]);
}
#[test]
fn plan_before_mutation() {
    let mut a = FakeAdapter::default();
    run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.install", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert!(
        a.calls.iter().position(|c| c == "preflight")
            < a.calls.iter().position(|c| c.starts_with("apply:"))
    );
}
#[test]
fn final_verify_before_complete() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    assert_eq!(*entered(&r).last().unwrap(), Stage::Complete);
    assert_eq!(a.calls.last().unwrap(), "final_verify");
}
#[test]
fn failed_final_never_complete() {
    let mut a = FakeAdapter {
        final_ok: false,
        ..Default::default()
    };
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    assert!(!entered(&r).contains(&Stage::Complete));
}
#[test]
fn blocking_preflight_zero_apply() {
    let mut a = FakeAdapter {
        findings: vec![PreflightFinding {
            finding_id: "disk".into(),
            severity: FindingSeverity::Blocking,
            capability: "space".into(),
            redacted_evidence: "insufficient".into(),
            blocks_planning: true,
        }],
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.install", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(apply_count(&a), 0);
    assert_eq!(r.error, Some(EngineErrorCode::PreflightBlocked));
}
#[test]
fn verify_intent_zero_apply() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Verify, vec![]), &mut a);
    assert!(r.completed);
    assert_eq!(apply_count(&a), 0);
}
#[test]
fn empty_verify_flow_valid() {
    let p = plan(InstallationIntent::Verify, vec![]);
    assert!(
        InstallerEngine
            .validate(&request(InstallationIntent::Verify), &p)
            .is_ok()
    );
}
#[test]
fn deterministic_events() {
    let p = plan(
        InstallationIntent::Install,
        vec![effect("package.install", EffectPhase::Install)],
    );
    let mut a = FakeAdapter::default();
    let x = run(&p, &mut a);
    let mut b = FakeAdapter::default();
    assert_eq!(x.events, run(&p, &mut b).events);
}
#[test]
fn deterministic_effect_order() {
    let mut a = FakeAdapter::default();
    run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.a", EffectPhase::Install),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert!(
        a.calls.iter().position(|x| x == "apply:package.a")
            < a.calls.iter().position(|x| x == "apply:package.b")
    );
}

#[test]
fn duplicate_id_rejected() {
    let e = effect("package.same", EffectPhase::Install);
    assert_eq!(
        InstallerEngine.validate(
            &request(InstallationIntent::Install),
            &plan(InstallationIntent::Install, vec![e.clone(), e])
        ),
        Err(EngineErrorCode::InvalidPlan)
    );
}
#[test]
fn invalid_id_rejected() {
    for id in ["", "UPPER", "bad id", "bad/path"] {
        assert!(
            InstallerEngine
                .validate(
                    &request(InstallationIntent::Install),
                    &plan(
                        InstallationIntent::Install,
                        vec![effect(id, EffectPhase::Install)]
                    )
                )
                .is_err()
        );
    }
}
#[test]
fn unsupported_schema_rejected() {
    let mut p = plan(InstallationIntent::Install, vec![]);
    p.schema_version = 2;
    assert_eq!(
        InstallerEngine.validate(&request(InstallationIntent::Install), &p),
        Err(EngineErrorCode::UnsupportedSchema)
    );
}
#[test]
fn phase_order_rejected() {
    let p = plan(
        InstallationIntent::Install,
        vec![
            effect("integration.x", EffectPhase::Integrate),
            effect("package.x", EffectPhase::Install),
        ],
    );
    assert!(
        InstallerEngine
            .validate(&request(InstallationIntent::Install), &p)
            .is_err()
    );
}
#[test]
fn missing_precondition_rejected() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.preconditions.clear();
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn missing_verification_rejected() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.verification = None;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn intent_mismatch_rejected() {
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Repair),
                &plan(InstallationIntent::Install, vec![])
            )
            .is_err()
    );
}
#[test]
fn target_mismatch_rejected() {
    let mut r = request(InstallationIntent::Install);
    r.target_scope = TargetScope::System;
    assert!(
        InstallerEngine
            .validate(&r, &plan(InstallationIntent::Install, vec![]))
            .is_err()
    );
}
#[test]
fn preservation_conflict_rejected() {
    let mut p = plan(
        InstallationIntent::Install,
        vec![effect("package.x", EffectPhase::Install)],
    );
    p.preservation_set.push(ResourceClass::PackageOwned);
    assert_eq!(
        InstallerEngine.validate(&request(InstallationIntent::Install), &p),
        Err(EngineErrorCode::UnauthorizedOwnership)
    );
}
#[test]
fn effect_preservation_conflict_rejected() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.preservation_constraints.push(ResourceClass::PackageOwned);
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn artifact_mismatch_rejected() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.artifact_identity = Some("missing".into());
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn request_artifact_mismatch_rejected() {
    let mut r = request(InstallationIntent::Install);
    r.verified_artifact = Some(ArtifactEvidence::NativeManagedArtifact {
        package_identity: "x".into(),
    });
    assert!(
        InstallerEngine
            .validate(&r, &plan(InstallationIntent::Install, vec![]))
            .is_err()
    );
}
#[test]
fn invalid_plan_zero_apply() {
    let mut a = FakeAdapter::default();
    let mut p = plan(
        InstallationIntent::Install,
        vec![effect("BAD", EffectPhase::Install)],
    );
    let r = run(&p, &mut a);
    assert!(!r.completed);
    assert_eq!(apply_count(&a), 0);
    p.ordered_effects.clear();
}
#[test]
fn verify_with_effect_rejected() {
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Verify),
                &plan(
                    InstallationIntent::Verify,
                    vec![effect("package.x", EffectPhase::Install)]
                )
            )
            .is_err()
    );
}
#[test]
fn reversible_requires_inverse() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.reversibility = Reversibility::Reversible;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn invalid_reconciliation_rejected() {
    let mut e = effect("package.x", EffectPhase::Install);
    e.reconciliation_policy = ReconciliationPolicy::NotApplicable;
    e.retry_policy = RetryPolicy::AfterReplan;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}

#[test]
fn matching_precondition_applies() {
    let mut a = FakeAdapter::default();
    run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(apply_count(&a), 1);
}
#[test]
fn stale_precondition_no_apply() {
    let mut a = FakeAdapter {
        preconditions: vec![false].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.error, Some(EngineErrorCode::PlanStale));
    assert_eq!(apply_count(&a), 0);
}
#[test]
fn changed_second_precondition_detected() {
    let mut a = FakeAdapter {
        preconditions: vec![true, false].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.a", EffectPhase::Install),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert_eq!(r.error, Some(EngineErrorCode::PlanStale));
    assert_eq!(apply_count(&a), 1);
}
#[test]
fn stale_never_replans() {
    let mut a = FakeAdapter {
        preconditions: vec![false, true].into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(
        a.calls
            .iter()
            .filter(|x| x.starts_with("precondition:"))
            .count(),
        1
    );
}
#[test]
fn privilege_unavailable_no_apply() {
    let mut a = FakeAdapter {
        privilege: false,
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.error, Some(EngineErrorCode::PrivilegeUnavailable));
    assert_eq!(apply_count(&a), 0);
}

#[test]
fn verified_success_state() {
    let mut a = FakeAdapter::default();
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.effects[0].result, EffectResultState::VerifiedSuccess);
}
#[test]
fn verified_noop_state() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Noop].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.effects[0].result, EffectResultState::VerifiedNoop);
}
#[test]
fn failure_before_mutation_state() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::FailureBeforeMutation].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(
        r.effects[0].result,
        EffectResultState::FailureBeforeMutation
    );
}
#[test]
fn verification_failure_partial() {
    let mut a = FakeAdapter {
        verifications: vec![false].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.effects[0].result, EffectResultState::KnownPartialMutation);
}
#[test]
fn known_partial_distinct() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::KnownPartialMutation].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.effects[0].result, EffectResultState::KnownPartialMutation);
}
#[test]
fn unknown_distinct() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.effects[0].result, EffectResultState::OutcomeUnknown);
}
#[test]
fn result_states_serialize_distinctly() {
    let values = [
        EffectResultState::VerifiedSuccess,
        EffectResultState::VerifiedNoop,
        EffectResultState::FailureBeforeMutation,
        EffectResultState::KnownPartialMutation,
        EffectResultState::OutcomeUnknown,
    ];
    let json: Vec<_> = values
        .iter()
        .map(|v| serde_json::to_string(v).unwrap())
        .collect();
    assert_eq!(json.iter().collect::<BTreeSet<_>>().len(), 5);
}

#[test]
fn unknown_invokes_reconciliation() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert!(a.calls.contains(&"reconcile:package.x".into()));
}
#[test]
fn unknown_never_replays() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown, ApplyOutcome::Success].into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(apply_count(&a), 1);
}
#[test]
fn reconcile_applied_verifies_and_continues() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        reconciliations: vec![ReconciliationOutcome::VerifiedApplied].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert!(r.completed);
    assert!(a.calls.contains(&"verify:package.x".into()));
}
#[test]
fn reconcile_not_applied_requests_replan() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        reconciliations: vec![ReconciliationOutcome::VerifiedNotApplied].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert!(r.safe_to_replan);
    assert_eq!(apply_count(&a), 1);
}
#[test]
fn unresolved_stops() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![effect("package.x", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert_eq!(r.error, Some(EngineErrorCode::OutcomeUnknown));
}
#[test]
fn unresolved_blocks_later_effects() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::OutcomeUnknown].into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.a", EffectPhase::Install),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert_eq!(apply_count(&a), 1);
}

fn reversible(id: &str) -> InstallationEffect {
    let mut e = effect(id, EffectPhase::Install);
    e.reversibility = Reversibility::Reversible;
    e.safe_inverse = Some("REMOVE_OWNED_PAYLOAD".into());
    e
}
#[test]
fn reversible_prior_compensated() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Success, ApplyOutcome::FailureBeforeMutation].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                reversible("package.a"),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert_eq!(r.compensations.len(), 1);
}
#[test]
fn compensation_reverse_order() {
    let mut a = FakeAdapter {
        outcomes: vec![
            ApplyOutcome::Success,
            ApplyOutcome::Success,
            ApplyOutcome::FailureBeforeMutation,
        ]
        .into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![
                reversible("package.a"),
                reversible("package.b"),
                effect("package.c", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    let c: Vec<_> = a
        .calls
        .iter()
        .filter(|x| x.starts_with("compensate:"))
        .cloned()
        .collect();
    assert_eq!(c, vec!["compensate:package.b", "compensate:package.a"]);
}
#[test]
fn irreversible_never_compensated() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Success, ApplyOutcome::FailureBeforeMutation].into(),
        ..Default::default()
    };
    run(
        &plan(
            InstallationIntent::Install,
            vec![
                effect("package.a", EffectPhase::Install),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert!(!a.calls.iter().any(|x| x.starts_with("compensate:")));
}
#[test]
fn partially_reversible_not_compensated() {
    let mut e = effect("package.a", EffectPhase::Install);
    e.reversibility = Reversibility::PartiallyReversible;
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Success, ApplyOutcome::FailureBeforeMutation].into(),
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![e, effect("package.b", EffectPhase::Install)],
        ),
        &mut a,
    );
    assert!(r.compensations.is_empty());
}
#[test]
fn compensation_failure_reported() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Success, ApplyOutcome::FailureBeforeMutation].into(),
        compensation_ok: false,
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                reversible("package.a"),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert_eq!(r.error, Some(EngineErrorCode::CompensationFailed));
}
#[test]
fn compensation_verification_required() {
    let mut a = FakeAdapter {
        outcomes: vec![ApplyOutcome::Success, ApplyOutcome::FailureBeforeMutation].into(),
        compensation_verified: false,
        ..Default::default()
    };
    let r = run(
        &plan(
            InstallationIntent::Install,
            vec![
                reversible("package.a"),
                effect("package.b", EffectPhase::Install),
            ],
        ),
        &mut a,
    );
    assert!(!r.compensations[0].verified);
}

#[test]
fn package_owned_accepted() {
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(
                    InstallationIntent::Install,
                    vec![effect("package.x", EffectPhase::Install)]
                )
            )
            .is_ok()
    );
}
#[test]
fn native_state_native_authority_accepted() {
    let mut e = effect("package.native", EffectPhase::Install);
    e.owner = ResourceClass::NativePackageState;
    e.resource_class = ResourceClass::NativePackageState;
    e.authority = EffectAuthority::NativePackageManager;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_ok()
    );
}
#[test]
fn native_state_wrong_authority_rejected() {
    let mut e = effect("package.native", EffectPhase::Install);
    e.owner = ResourceClass::NativePackageState;
    e.resource_class = ResourceClass::NativePackageState;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_err()
    );
}
#[test]
fn platform_integration_accepted() {
    let mut e = effect("platform.menu", EffectPhase::Integrate);
    e.owner = ResourceClass::PlatformIntegrationOwned;
    e.resource_class = ResourceClass::PlatformIntegrationOwned;
    e.authority = EffectAuthority::PlatformIntegrationAdapter;
    assert!(
        InstallerEngine
            .validate(
                &request(InstallationIntent::Install),
                &plan(InstallationIntent::Install, vec![e])
            )
            .is_ok()
    );
}
macro_rules! forbidden_test {
    ($name:ident,$class:expr) => {
        #[test]
        fn $name() {
            let mut e = effect("forbidden.x", EffectPhase::Install);
            e.owner = $class;
            e.resource_class = $class;
            assert_eq!(
                InstallerEngine.validate(
                    &request(InstallationIntent::Install),
                    &plan(InstallationIntent::Install, vec![e])
                ),
                Err(EngineErrorCode::UnauthorizedOwnership)
            );
        }
    };
}
forbidden_test!(credential_rejected, ResourceClass::CredentialState);
forbidden_test!(client_sync_rejected, ResourceClass::ClientSyncState);
forbidden_test!(library_rejected, ResourceClass::UserLibraryContent);
forbidden_test!(server_database_rejected, ResourceClass::ServerDatabase);
forbidden_test!(server_objects_rejected, ResourceClass::ServerObjectData);
forbidden_test!(
    external_dependency_rejected,
    ResourceClass::ExternalDependency
);
forbidden_test!(
    application_config_rejected,
    ResourceClass::ApplicationConfig
);

#[test]
fn launch_eligible_after_success() {
    let mut a = FakeAdapter::default();
    assert!(run(&plan(InstallationIntent::Install, vec![]), &mut a).launch_eligible);
}
#[test]
fn launch_not_first_run() {
    let mut a = FakeAdapter::default();
    assert!(!run(&plan(InstallationIntent::Install, vec![]), &mut a).first_run_complete);
}
#[test]
fn launch_not_server_ready() {
    let mut a = FakeAdapter::default();
    assert!(!run(&plan(InstallationIntent::Install, vec![]), &mut a).server_ready);
}
#[test]
fn launch_not_sync_ready() {
    let mut a = FakeAdapter::default();
    assert!(!run(&plan(InstallationIntent::Install, vec![]), &mut a).sync_ready);
}
#[test]
fn final_failure_launch_ineligible() {
    let mut a = FakeAdapter {
        final_ok: false,
        ..Default::default()
    };
    assert!(!run(&plan(InstallationIntent::Install, vec![]), &mut a).launch_eligible);
}
#[test]
fn complete_implies_verified() {
    for ok in [true, false] {
        let mut a = FakeAdapter {
            final_ok: ok,
            ..Default::default()
        };
        let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
        assert!(!r.completed || r.installation_verified);
    }
}

#[test]
fn plan_json_round_trip() {
    let p = plan(
        InstallationIntent::Install,
        vec![effect("package.x", EffectPhase::Install)],
    );
    assert_eq!(
        InstallationPlan::from_json(&serde_json::to_string(&p).unwrap()).unwrap(),
        p
    );
}
#[test]
fn result_json_round_trip() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    assert_eq!(
        serde_json::from_str::<InstallationResult>(&serde_json::to_string(&r).unwrap()).unwrap(),
        r
    );
}
#[test]
fn event_json_round_trip() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    let e = &r.events[0];
    assert_eq!(
        serde_json::from_str::<EngineEvent>(&serde_json::to_string(e).unwrap()).unwrap(),
        *e
    );
}
#[test]
fn unknown_enum_fails_closed() {
    assert!(serde_json::from_str::<ResourceClass>("\"FUTURE_OWNER\"").is_err());
}
#[test]
fn unknown_plan_field_fails_closed() {
    let mut v = serde_json::to_value(plan(InstallationIntent::Install, vec![])).unwrap();
    v.as_object_mut()
        .unwrap()
        .insert("future".into(), true.into());
    assert!(InstallationPlan::from_json(&v.to_string()).is_err());
}
#[test]
fn wrong_schema_from_json_fails_closed() {
    let mut p = plan(InstallationIntent::Install, vec![]);
    p.schema_version = 9;
    assert_eq!(
        InstallationPlan::from_json(&serde_json::to_string(&p).unwrap()),
        Err(EngineErrorCode::UnsupportedSchema)
    );
}
#[test]
fn wrong_result_schema_fails_closed() {
    let mut a = FakeAdapter::default();
    let mut value =
        serde_json::to_value(run(&plan(InstallationIntent::Install, vec![]), &mut a)).unwrap();
    value["schema_version"] = 2.into();
    assert_eq!(
        InstallationResult::from_json(&value.to_string()),
        Err(EngineErrorCode::UnsupportedSchema)
    );
}
#[test]
fn wrong_event_schema_fails_closed() {
    let mut a = FakeAdapter::default();
    let result = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    let mut value = serde_json::to_value(&result.events[0]).unwrap();
    value["schema_version"] = 2.into();
    assert_eq!(
        EngineEvent::from_json(&value.to_string()),
        Err(EngineErrorCode::UnsupportedSchema)
    );
}

#[test]
fn upgrade_intent_transported() {
    let p = plan(InstallationIntent::Upgrade, vec![]);
    assert!(
        InstallerEngine
            .validate(&request(InstallationIntent::Upgrade), &p)
            .is_ok()
    );
}
#[test]
fn repair_intent_transported() {
    let p = plan(InstallationIntent::Repair, vec![]);
    assert!(
        InstallerEngine
            .validate(&request(InstallationIntent::Repair), &p)
            .is_ok()
    );
}
#[test]
fn uninstall_intent_transported() {
    let p = plan(InstallationIntent::Uninstall, vec![]);
    assert!(
        InstallerEngine
            .validate(&request(InstallationIntent::Uninstall), &p)
            .is_ok()
    );
}
#[test]
fn sequence_numbers_contiguous() {
    let mut a = FakeAdapter::default();
    let r = run(&plan(InstallationIntent::Install, vec![]), &mut a);
    assert!(
        r.events
            .iter()
            .enumerate()
            .all(|(i, e)| e.sequence == i as u64)
    );
}
#[test]
fn local_artifact_not_authenticated_label() {
    let a = ArtifactEvidence::LocallySuppliedArtifact {
        artifact_id: "local".into(),
        local_path: "/tmp/a".into(),
    };
    assert_eq!(a.identity(), "local");
    assert!(
        !serde_json::to_string(&a)
            .unwrap()
            .contains("VERIFIED_RELEASE")
    );
}
