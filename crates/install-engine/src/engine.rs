use crate::*;
use std::collections::BTreeSet;

#[derive(Default)]
pub struct InstallerEngine;

impl InstallerEngine {
    pub fn validate(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
    ) -> Result<(), EngineErrorCode> {
        if plan.schema_version != ENGINE_SCHEMA_VERSION {
            return Err(EngineErrorCode::UnsupportedSchema);
        }
        if request.intent != plan.intent || plan.plan_id.is_empty() {
            return Err(EngineErrorCode::InvalidPlan);
        }
        if request.target_scope != plan.target_scope {
            return Err(EngineErrorCode::InvalidPlan);
        }
        if request.verified_artifact != plan.artifact {
            return Err(EngineErrorCode::InvalidPlan);
        }
        if plan
            .expected_completion_verification
            .verification_id
            .is_empty()
            || plan.expected_completion_verification.expected.is_empty()
            || REQUIRED_PRESERVATION
                .iter()
                .any(|class| !plan.preservation_set.contains(class))
        {
            return Err(EngineErrorCode::InvalidPlan);
        }
        let mut ids = BTreeSet::new();
        let mut integration_seen = false;
        for effect in &plan.ordered_effects {
            if !valid_id(&effect.effect_id) || !ids.insert(&effect.effect_id) {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if effect.target_scope != plan.target_scope
                || effect.preconditions.is_empty()
                || effect.verification.is_none()
                || effect.mutation_kind.trim().is_empty()
            {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if effect.phase == EffectPhase::Integrate {
                integration_seen = true;
            } else if integration_seen {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if plan.intent == InstallationIntent::Verify {
                return Err(EngineErrorCode::InvalidPlan);
            }
            validate_authority(effect)?;
            if forbidden(effect.resource_class)
                || forbidden(effect.owner)
                || effect.owner == ResourceClass::ApplicationConfig
            {
                return Err(EngineErrorCode::UnauthorizedOwnership);
            }
            if plan.preservation_set.contains(&effect.resource_class)
                || effect
                    .preservation_constraints
                    .contains(&effect.resource_class)
            {
                return Err(EngineErrorCode::UnauthorizedOwnership);
            }
            if effect.reversibility == Reversibility::Reversible && effect.safe_inverse.is_none() {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if effect.owner == ResourceClass::NativePackageState
                && effect.reversibility == Reversibility::Reversible
            {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if effect.reconciliation_policy == ReconciliationPolicy::NotApplicable
                && effect.retry_policy != RetryPolicy::Never
            {
                return Err(EngineErrorCode::InvalidPlan);
            }
            if let Some(expected) = &effect.artifact_identity
                && plan.artifact.as_ref().map(ArtifactEvidence::identity) != Some(expected.as_str())
            {
                return Err(EngineErrorCode::InvalidPlan);
            }
        }
        Ok(())
    }

    pub fn execute<A: InstallationAdapter>(
        &self,
        request: &InstallationRequest,
        plan: &InstallationPlan,
        adapter: &mut A,
    ) -> InstallationResult {
        let mut run = Run::new(&plan.plan_id);
        run.stage_enter(Stage::Preflight);
        if adapter.preflight().iter().any(|f| f.blocks_planning) {
            return run.stop(EngineErrorCode::PreflightBlocked, false);
        }
        run.stage_complete(Stage::Preflight);
        run.stage_enter(Stage::Plan);
        if let Err(code) = self.validate(request, plan) {
            return run.stop(code, false);
        }
        run.stage_complete(Stage::Plan);
        if plan.intent == InstallationIntent::Verify {
            return final_verify(plan, adapter, run);
        }
        let mut applied: Vec<&InstallationEffect> = Vec::new();
        let mut current_phase = None;
        for effect in &plan.ordered_effects {
            let stage = match effect.phase {
                EffectPhase::Install => Stage::Install,
                EffectPhase::Integrate => Stage::Integrate,
            };
            if current_phase != Some(stage) {
                if let Some(previous) = current_phase {
                    run.stage_complete(previous);
                }
                run.stage_enter(stage);
                current_phase = Some(stage);
            }
            if !adapter.privilege_available(effect.privilege) {
                return fail_with_compensation(
                    run,
                    EngineErrorCode::PrivilegeUnavailable,
                    &applied,
                    adapter,
                );
            }
            if !adapter.inspect_preconditions(effect) {
                return fail_with_compensation(run, EngineErrorCode::PlanStale, &applied, adapter);
            }
            run.event(EngineEventKind::EffectStarted {
                effect_id: effect.effect_id.clone(),
            });
            match adapter.apply_effect(effect) {
                outcome @ (ApplyOutcome::Success | ApplyOutcome::Noop) => {
                    let state = if verify_effect_with_event(effect, adapter, &mut run) {
                        if outcome == ApplyOutcome::Noop {
                            EffectResultState::VerifiedNoop
                        } else {
                            EffectResultState::VerifiedSuccess
                        }
                    } else {
                        run.record(effect, EffectResultState::KnownPartialMutation);
                        return fail_with_compensation(
                            run,
                            EngineErrorCode::VerificationFailed,
                            &applied,
                            adapter,
                        );
                    };
                    run.record(effect, state);
                    applied.push(effect);
                }
                ApplyOutcome::FailureBeforeMutation => {
                    run.record(effect, EffectResultState::FailureBeforeMutation);
                    return fail_with_compensation(
                        run,
                        EngineErrorCode::EffectFailed,
                        &applied,
                        adapter,
                    );
                }
                ApplyOutcome::KnownPartialMutation => {
                    run.record(effect, EffectResultState::KnownPartialMutation);
                    return fail_with_compensation(
                        run,
                        EngineErrorCode::EffectFailed,
                        &applied,
                        adapter,
                    );
                }
                ApplyOutcome::OutcomeUnknown => {
                    run.record(effect, EffectResultState::OutcomeUnknown);
                    match adapter.reconcile_unknown(effect) {
                        ReconciliationOutcome::VerifiedApplied
                            if verify_effect_with_event(effect, adapter, &mut run) =>
                        {
                            run.record(effect, EffectResultState::VerifiedSuccess);
                            applied.push(effect);
                        }
                        ReconciliationOutcome::VerifiedApplied => {
                            return fail_with_compensation(
                                run,
                                EngineErrorCode::VerificationFailed,
                                &applied,
                                adapter,
                            );
                        }
                        ReconciliationOutcome::VerifiedNotApplied => {
                            return fail_with_compensation_replan(
                                run,
                                EngineErrorCode::ReconciliationRequired,
                                &applied,
                                adapter,
                                true,
                            );
                        }
                        ReconciliationOutcome::StillUnknown => {
                            return fail_with_compensation(
                                run,
                                EngineErrorCode::OutcomeUnknown,
                                &applied,
                                adapter,
                            );
                        }
                    }
                }
            }
        }
        if let Some(stage) = current_phase {
            run.stage_complete(stage);
        }
        final_verify(plan, adapter, run)
    }
}

const REQUIRED_PRESERVATION: [ResourceClass; 8] = [
    ResourceClass::ApplicationConfig,
    ResourceClass::CredentialState,
    ResourceClass::ClientSyncState,
    ResourceClass::UserLibraryContent,
    ResourceClass::ServerConfig,
    ResourceClass::ServerDatabase,
    ResourceClass::ServerObjectData,
    ResourceClass::ExternalDependency,
];

fn verify_effect_with_event<A: InstallationAdapter>(
    effect: &InstallationEffect,
    adapter: &mut A,
    run: &mut Run,
) -> bool {
    let verification_id = effect
        .verification
        .as_ref()
        .expect("validated verification")
        .verification_id
        .clone();
    run.event(EngineEventKind::VerificationStarted {
        verification_id: verification_id.clone(),
    });
    let succeeded = adapter.verify_effect(effect);
    run.event(EngineEventKind::VerificationRecorded {
        verification_id,
        succeeded,
    });
    succeeded
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
}
fn forbidden(c: ResourceClass) -> bool {
    matches!(
        c,
        ResourceClass::CredentialState
            | ResourceClass::ClientSyncState
            | ResourceClass::UserLibraryContent
            | ResourceClass::ServerDatabase
            | ResourceClass::ServerObjectData
            | ResourceClass::ExternalDependency
    )
}
fn validate_authority(e: &InstallationEffect) -> Result<(), EngineErrorCode> {
    if e.owner == ResourceClass::NativePackageState
        && e.authority != EffectAuthority::NativePackageManager
    {
        return Err(EngineErrorCode::UnauthorizedOwnership);
    }
    if e.owner == ResourceClass::PlatformIntegrationOwned
        && e.authority != EffectAuthority::PlatformIntegrationAdapter
    {
        return Err(EngineErrorCode::UnauthorizedOwnership);
    }
    Ok(())
}

fn final_verify<A: InstallationAdapter>(
    plan: &InstallationPlan,
    adapter: &mut A,
    mut run: Run,
) -> InstallationResult {
    run.stage_enter(Stage::Verify);
    run.event(EngineEventKind::VerificationStarted {
        verification_id: plan
            .expected_completion_verification
            .verification_id
            .clone(),
    });
    let ok = adapter.verify_installation(plan);
    run.event(EngineEventKind::VerificationRecorded {
        verification_id: plan
            .expected_completion_verification
            .verification_id
            .clone(),
        succeeded: ok,
    });
    if !ok {
        return run.stop(EngineErrorCode::FinalVerificationFailed, false);
    }
    run.stage_complete(Stage::Verify);
    run.stage_enter(Stage::Complete);
    run.stage_complete(Stage::Complete);
    run.event(EngineEventKind::InstallationCompleted);
    run.result(true, true, None, false)
}

fn fail_with_compensation<A: InstallationAdapter>(
    run: Run,
    code: EngineErrorCode,
    applied: &[&InstallationEffect],
    adapter: &mut A,
) -> InstallationResult {
    fail_with_compensation_replan(run, code, applied, adapter, false)
}
fn fail_with_compensation_replan<A: InstallationAdapter>(
    mut run: Run,
    mut code: EngineErrorCode,
    applied: &[&InstallationEffect],
    adapter: &mut A,
    replan: bool,
) -> InstallationResult {
    for effect in compensation_candidates(applied, adapter) {
        let succeeded = adapter.compensate_effect(effect);
        let verified = succeeded && adapter.verify_compensation(effect);
        run.compensations.push(CompensationRecord {
            effect_id: effect.effect_id.clone(),
            succeeded,
            verified,
        });
        if !verified {
            code = EngineErrorCode::CompensationFailed;
        }
    }
    run.stop(code, replan)
}

pub(crate) fn compensation_candidates<'a, A: InstallationAdapter>(
    applied: &[&'a InstallationEffect],
    adapter: &mut A,
) -> Vec<&'a InstallationEffect> {
    applied
        .iter()
        .rev()
        .copied()
        .filter(|effect| {
            effect.reversibility == Reversibility::Reversible
                && effect.safe_inverse.is_some()
                && adapter.compensation_supported(effect)
        })
        .collect()
}

struct Run {
    plan_id: String,
    next: u64,
    effects: Vec<EffectRecord>,
    compensations: Vec<CompensationRecord>,
    events: Vec<EngineEvent>,
}
impl Run {
    fn new(id: &str) -> Self {
        Self {
            plan_id: id.into(),
            next: 0,
            effects: vec![],
            compensations: vec![],
            events: vec![],
        }
    }
    fn event(&mut self, kind: EngineEventKind) {
        self.events.push(EngineEvent {
            schema_version: ENGINE_SCHEMA_VERSION,
            plan_id: self.plan_id.clone(),
            sequence: self.next,
            kind,
        });
        self.next += 1;
    }
    fn stage_enter(&mut self, stage: Stage) {
        self.event(EngineEventKind::StageEntered { stage });
    }
    fn stage_complete(&mut self, stage: Stage) {
        self.event(EngineEventKind::StageCompleted { stage });
    }
    fn record(&mut self, effect: &InstallationEffect, result: EffectResultState) {
        self.effects.push(EffectRecord {
            effect_id: effect.effect_id.clone(),
            result,
            redacted_evidence: "adapter-classification".into(),
        });
        self.event(EngineEventKind::EffectResultRecorded {
            effect_id: effect.effect_id.clone(),
            result,
        });
    }
    fn stop(mut self, code: EngineErrorCode, safe: bool) -> InstallationResult {
        self.event(EngineEventKind::InstallationStopped { code });
        self.result(false, false, Some(code), safe)
    }
    fn result(
        self,
        completed: bool,
        verified: bool,
        error: Option<EngineErrorCode>,
        safe: bool,
    ) -> InstallationResult {
        InstallationResult {
            schema_version: ENGINE_SCHEMA_VERSION,
            plan_id: self.plan_id,
            completed,
            installation_verified: verified,
            launch_eligible: verified,
            first_run_complete: false,
            sync_ready: false,
            server_ready: false,
            error,
            safe_to_replan: safe,
            effects: self.effects,
            compensations: self.compensations,
            events: self.events,
        }
    }
}
