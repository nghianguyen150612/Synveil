use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use synveil_install_engine::*;
use tempfile::TempDir;

#[derive(Default)]
struct Adapter {
    calls: Vec<String>,
    outcome: Option<ApplyOutcome>,
    outcomes: BTreeMap<String, ApplyOutcome>,
    reconcile: Option<ReconciliationOutcome>,
    effect_ok: bool,
    final_ok: bool,
    compensate_ok: bool,
    compensation_verify_ok: bool,
}
impl InstallationAdapter for Adapter {
    fn preflight(&mut self) -> Vec<PreflightFinding> {
        vec![]
    }
    fn privilege_available(&mut self, _: PrivilegeRequirement) -> bool {
        true
    }
    fn inspect_preconditions(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("inspect:{}", e.effect_id));
        true
    }
    fn apply_effect(&mut self, e: &InstallationEffect) -> ApplyOutcome {
        self.calls.push(format!("apply:{}", e.effect_id));
        self.outcomes
            .get(&e.effect_id)
            .copied()
            .or(self.outcome)
            .unwrap_or(ApplyOutcome::Success)
    }
    fn verify_effect(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("verify:{}", e.effect_id));
        self.effect_ok
    }
    fn reconcile_unknown(&mut self, e: &InstallationEffect) -> ReconciliationOutcome {
        self.calls.push(format!("reconcile:{}", e.effect_id));
        self.reconcile
            .unwrap_or(ReconciliationOutcome::StillUnknown)
    }
    fn compensation_supported(&mut self, _: &InstallationEffect) -> bool {
        true
    }
    fn compensate_effect(&mut self, e: &InstallationEffect) -> bool {
        self.calls.push(format!("compensate:{}", e.effect_id));
        self.compensate_ok
    }
    fn verify_compensation(&mut self, _: &InstallationEffect) -> bool {
        self.calls.push("verify-compensation".into());
        self.compensation_verify_ok
    }
    fn verify_installation(&mut self, _: &InstallationPlan) -> bool {
        self.calls.push("final".into());
        self.final_ok
    }
}

#[derive(Default)]
struct ProcessAdapter {
    marker: PathBuf,
    terminate_after_apply: bool,
    calls: Vec<String>,
}

impl ProcessAdapter {
    fn marker_is_applied(&self) -> bool {
        fs::read(&self.marker).is_ok_and(|bytes| bytes == b"owned-payload-applied")
    }
}

impl InstallationAdapter for ProcessAdapter {
    fn preflight(&mut self) -> Vec<PreflightFinding> {
        vec![]
    }
    fn privilege_available(&mut self, _: PrivilegeRequirement) -> bool {
        true
    }
    fn inspect_preconditions(&mut self, _: &InstallationEffect) -> bool {
        true
    }
    fn apply_effect(&mut self, effect: &InstallationEffect) -> ApplyOutcome {
        self.calls.push(format!("apply:{}", effect.effect_id));
        let mut payload = File::create(&self.marker).unwrap();
        payload.write_all(b"owned-payload-applied").unwrap();
        payload.sync_all().unwrap();
        #[cfg(unix)]
        File::open(self.marker.parent().unwrap())
            .unwrap()
            .sync_all()
            .unwrap();
        if self.terminate_after_apply {
            #[cfg(unix)]
            {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &std::process::id().to_string()])
                    .status();
                panic!("SIGKILL command returned without terminating process");
            }
            #[cfg(not(unix))]
            std::process::exit(86);
        }
        ApplyOutcome::Success
    }
    fn verify_effect(&mut self, effect: &InstallationEffect) -> bool {
        self.calls.push(format!("verify:{}", effect.effect_id));
        self.marker_is_applied()
    }
    fn reconcile_unknown(&mut self, effect: &InstallationEffect) -> ReconciliationOutcome {
        self.calls.push(format!("reconcile:{}", effect.effect_id));
        if self.marker_is_applied() {
            ReconciliationOutcome::VerifiedApplied
        } else {
            ReconciliationOutcome::VerifiedNotApplied
        }
    }
    fn compensation_supported(&mut self, _: &InstallationEffect) -> bool {
        false
    }
    fn compensate_effect(&mut self, _: &InstallationEffect) -> bool {
        false
    }
    fn verify_compensation(&mut self, _: &InstallationEffect) -> bool {
        false
    }
    fn verify_installation(&mut self, _: &InstallationPlan) -> bool {
        self.marker_is_applied()
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
            kind: "path".into(),
            expected: "absent".into(),
        }],
        privilege: PrivilegeRequirement::CurrentUser,
        mutation_kind: "INSTALL".into(),
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
fn plan_with(ids: &[(&str, EffectPhase)]) -> InstallationPlan {
    InstallationPlan {
        schema_version: ENGINE_SCHEMA_VERSION,
        plan_id: "install.current-user.v1".into(),
        intent: InstallationIntent::Install,
        target_scope: TargetScope::CurrentUser,
        ordered_effects: ids.iter().map(|(id, p)| effect(id, *p)).collect(),
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
            verification_id: "complete".into(),
            expected: "ready".into(),
        },
        artifact: None,
    }
}
fn plan() -> InstallationPlan {
    plan_with(&[("package.payload", EffectPhase::Install)])
}
fn reversible_plan(ids: &[&str]) -> InstallationPlan {
    let mut plan = plan_with(
        &ids.iter()
            .map(|id| (*id, EffectPhase::Install))
            .collect::<Vec<_>>(),
    );
    for effect in &mut plan.ordered_effects {
        effect.reversibility = Reversibility::Reversible;
        effect.safe_inverse = Some("REMOVE_OWNED_PAYLOAD".into());
    }
    plan
}
fn request(p: &InstallationPlan) -> InstallationRequest {
    InstallationRequest {
        intent: p.intent,
        target_scope: p.target_scope,
        verified_artifact: p.artifact.clone(),
        user_choices: BTreeMap::new(),
    }
}
fn good_adapter() -> Adapter {
    Adapter {
        effect_ok: true,
        final_ok: true,
        compensate_ok: true,
        compensation_verify_ok: true,
        ..Default::default()
    }
}
fn execute(
    root: &Path,
    p: &InstallationPlan,
    a: &mut Adapter,
    mode: JournalMode,
) -> JournalExecutionResult {
    InstallerEngine.execute_journaled(&request(p), p, a, root, mode)
}

#[test]
fn process_restart_child() {
    if let Some(root) = std::env::var_os("SYNVEIL_JOURNAL_INTERRUPT_ROOT") {
        let root = PathBuf::from(root);
        let p = plan();
        let mut adapter = ProcessAdapter {
            marker: root.join("owned-payload"),
            terminate_after_apply: true,
            ..Default::default()
        };
        let _ = InstallerEngine.execute_journaled(
            &request(&p),
            &p,
            &mut adapter,
            &root.join("journal"),
            JournalMode::StartNew,
        );
        panic!("process interruption fixture returned without terminating");
    }
}

#[test]
fn process_restart_recover_child() {
    let Some(root) = std::env::var_os("SYNVEIL_JOURNAL_RECOVER_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let p = plan();
    let mut adapter = ProcessAdapter {
        marker: root.join("owned-payload"),
        ..Default::default()
    };
    let result = InstallerEngine.execute_journaled(
        &request(&p),
        &p,
        &mut adapter,
        &root.join("journal"),
        JournalMode::ResumeExisting,
    );
    assert!(result.completed);
    assert_eq!(result.disposition, RecoveryDisposition::ReconciledApplied);
    assert_eq!(
        adapter
            .calls
            .iter()
            .filter(|call| call.starts_with("apply:"))
            .count(),
        0
    );
    assert!(
        adapter
            .calls
            .iter()
            .any(|call| call.starts_with("reconcile:"))
    );
    assert!(adapter.calls.iter().any(|call| call.starts_with("verify:")));
}

#[test]
fn forced_process_termination_restarts_from_durable_evidence() {
    let root = TempDir::new().unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_restart_child", "--nocapture"])
        .env("SYNVEIL_JOURNAL_INTERRUPT_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        !child.status.success(),
        "child unexpectedly exited successfully"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.status.signal(), Some(9));
    }

    let recovery = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_restart_recover_child", "--nocapture"])
        .env("SYNVEIL_JOURNAL_RECOVER_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        recovery.status.success(),
        "fresh recovery process failed: {recovery:?}"
    );
    assert!(root.path().join("owned-payload").exists());
    let journal = InstallationJournal::open(
        &root.path().join("journal"),
        &plan(),
        JournalMode::ResumeExisting,
    )
    .unwrap();
    assert!(matches!(
        journal.records().last().unwrap().record,
        JournalRecord::TransactionCompleted
    ));
}
fn tx(root: &Path) -> std::path::PathBuf {
    root.join("install.current-user.v1")
}
fn checkpoint(root: &Path, n: u64) -> std::path::PathBuf {
    tx(root).join(format!("checkpoint-{n:016}.json"))
}
fn mutation_started(root: &Path, p: &InstallationPlan) {
    let mut j = InstallationJournal::open(root, p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    j.append(JournalRecord::EffectMutationStarted {
        effect_id: p.ordered_effects[0].effect_id.clone(),
    })
    .unwrap();
}
fn mark_first_verified(j: &mut InstallationJournal, p: &InstallationPlan) {
    j.append(JournalRecord::EffectMutationStarted {
        effect_id: p.ordered_effects[0].effect_id.clone(),
    })
    .unwrap();
    j.append(JournalRecord::EffectVerified {
        effect_id: p.ordered_effects[0].effect_id.clone(),
        result: JournalVerifiedResult::VerifiedSuccess,
    })
    .unwrap();
}
fn applies(a: &Adapter) -> usize {
    a.calls.iter().filter(|c| c.starts_with("apply:")).count()
}

#[test]
fn fresh_journal_is_created() {
    let d = TempDir::new().unwrap();
    let j = InstallationJournal::open(d.path(), &plan(), JournalMode::StartNew).unwrap();
    assert!(j.transaction_directory().is_dir())
}
#[test]
fn nested_journal_root_is_created_for_new_transaction() {
    let d = TempDir::new().unwrap();
    let root = d.path().join("new-parent").join("journal-root");
    let journal = InstallationJournal::open(&root, &plan(), JournalMode::StartNew).unwrap();
    assert!(root.is_dir());
    assert!(journal.transaction_directory().is_dir());
}
#[test]
fn schema_is_one() {
    assert_eq!(JOURNAL_SCHEMA_VERSION, 1)
}
#[test]
fn fingerprint_is_deterministic() {
    let p = plan();
    assert_eq!(plan_fingerprint(&p).unwrap(), plan_fingerprint(&p).unwrap())
}
#[test]
fn identical_plan_fingerprint_matches() {
    assert_eq!(
        plan_fingerprint(&plan()).unwrap(),
        plan_fingerprint(&plan()).unwrap()
    )
}
#[test]
fn changed_plan_changes_fingerprint() {
    let mut p = plan();
    let a = plan_fingerprint(&p).unwrap();
    p.ordered_effects[0].mutation_kind = "OTHER".into();
    assert_ne!(a, plan_fingerprint(&p).unwrap())
}
#[test]
fn transaction_binds_plan_id() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert!(
        matches!(&j.records()[0].record,JournalRecord::TransactionOpened{plan_id,..} if plan_id==&p.plan_id)
    )
}
#[test]
fn generations_are_contiguous() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert_eq!(
        j.records().iter().map(|r| r.generation).collect::<Vec<_>>(),
        vec![0, 1]
    )
}
#[test]
fn hash_chain_links_records() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert_eq!(
        j.records()[1].previous_record_sha256.as_ref(),
        Some(&j.records()[0].record_sha256)
    )
}
#[test]
fn unknown_schema_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let f = checkpoint(d.path(), 0);
    let s = fs::read_to_string(&f)
        .unwrap()
        .replace("\"schema_version\":1", "\"schema_version\":2");
    fs::write(f, s).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalUnsupportedSchema
    )
}
#[test]
fn unknown_fields_fail_closed() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let f = checkpoint(d.path(), 0);
    let s = fs::read_to_string(&f)
        .unwrap()
        .replacen('{', "{\"extra\":1,", 1);
    fs::write(f, s).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn oversized_record_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    OpenOptions::new()
        .write(true)
        .open(checkpoint(d.path(), 0))
        .unwrap()
        .set_len(MAX_CHECKPOINT_BYTES + 1)
        .unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalLimitExceeded
    )
}
#[test]
fn unsafe_transaction_id_rejected() {
    let d = TempDir::new().unwrap();
    let mut p = plan();
    p.plan_id = "../escape".into();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::StartNew)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[cfg(unix)]
#[test]
fn journal_root_symlink_rejected() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    let link = d.path().join("link");
    symlink(d.path().join("real"), &link).unwrap();
    assert_eq!(
        InstallationJournal::open(&link, &plan(), JournalMode::StartNew)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[cfg(unix)]
#[test]
fn transaction_symlink_rejected() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    symlink(d.path().join("elsewhere"), tx(d.path())).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &plan(), JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[cfg(unix)]
#[test]
fn checkpoint_symlink_rejected() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    fs::remove_file(checkpoint(d.path(), 1)).unwrap();
    symlink(checkpoint(d.path(), 0), checkpoint(d.path(), 1)).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[cfg(unix)]
#[test]
fn lock_symlink_rejected() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    fs::create_dir(tx(d.path())).unwrap();
    symlink(d.path().join("outside"), tx(d.path()).join("lock")).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &plan(), JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn second_writer_is_busy() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let _j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalBusy
    )
}
#[test]
fn stale_lock_file_without_live_os_lock_does_not_brick_empty_transaction() {
    let d = TempDir::new().unwrap();
    fs::create_dir(tx(d.path())).unwrap();
    fs::write(tx(d.path()).join("lock"), b"stale lock file").unwrap();
    let journal = InstallationJournal::open(d.path(), &plan(), JournalMode::StartNew).unwrap();
    assert!(journal.transaction_directory().is_dir());
}
#[test]
fn writer_lock_prevents_execution() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let _j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalBusy));
    assert_eq!(applies(&a), 0)
}
#[test]
fn temp_is_contained() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    assert!(
        fs::read_dir(tx(d.path()))
            .unwrap()
            .all(|e| e.unwrap().path().starts_with(tx(d.path())))
    )
}
#[test]
fn adjacent_file_is_preserved() {
    let d = TempDir::new().unwrap();
    fs::write(d.path().join("user-data"), "keep").unwrap();
    let p = plan();
    let mut a = good_adapter();
    execute(d.path(), &p, &mut a, JournalMode::StartNew);
    assert_eq!(
        fs::read_to_string(d.path().join("user-data")).unwrap(),
        "keep"
    )
}
#[test]
fn mutation_checkpoint_precedes_apply() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    execute(d.path(), &p, &mut a, JournalMode::StartNew);
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert!(matches!(
        j.records()[1].record,
        JournalRecord::EffectMutationStarted { .. }
    ))
}
#[test]
fn checkpoint_write_failure_prevents_apply() {
    pre_mutation_fault(JournalFaultPoint::Write)
}
#[test]
fn checkpoint_fsync_failure_prevents_apply() {
    pre_mutation_fault(JournalFaultPoint::FileSync)
}
#[test]
fn checkpoint_commit_failure_prevents_apply() {
    pre_mutation_fault(JournalFaultPoint::Commit)
}
#[test]
fn directory_sync_failure_prevents_apply() {
    pre_mutation_fault(JournalFaultPoint::DirectorySync)
}
fn pre_mutation_fault(point: JournalFaultPoint) {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    let o = JournalOptions {
        fail_at: Some((JournalRecordClass::EffectMutationStart, point)),
    };
    let r = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    assert_eq!(r.error, Some(JournalErrorCode::JournalIoFailed));
    assert_eq!(applies(&a), 0)
}
#[test]
fn post_boundary_faults_reconcile_only_when_a_start_checkpoint_exists() {
    for point in [
        JournalFaultPoint::AfterWrite,
        JournalFaultPoint::AfterFileSync,
        JournalFaultPoint::AfterCommit,
        JournalFaultPoint::AfterCommittedObjectSync,
        JournalFaultPoint::AfterDirectorySync,
    ] {
        let d = TempDir::new().unwrap();
        let p = plan();
        let mut first = good_adapter();
        let result = InstallerEngine.execute_journaled_with_options(
            &request(&p),
            &p,
            &mut first,
            d.path(),
            JournalMode::StartNew,
            JournalOptions {
                fail_at: Some((JournalRecordClass::EffectMutationStart, point)),
            },
        );
        assert_eq!(result.error, Some(JournalErrorCode::JournalIoFailed));
        assert_eq!(applies(&first), 0);

        let mut restarted = good_adapter();
        restarted.reconcile = Some(ReconciliationOutcome::VerifiedNotApplied);
        let result = execute(d.path(), &p, &mut restarted, JournalMode::ResumeExisting);
        match point {
            JournalFaultPoint::AfterWrite | JournalFaultPoint::AfterFileSync => {
                assert!(result.completed);
                assert_eq!(applies(&restarted), 1);
            }
            JournalFaultPoint::AfterCommit
            | JournalFaultPoint::AfterCommittedObjectSync
            | JournalFaultPoint::AfterDirectorySync => {
                assert_eq!(result.disposition, RecoveryDisposition::ReplanRequired);
                assert_eq!(applies(&restarted), 0);
                assert!(
                    restarted
                        .calls
                        .iter()
                        .any(|call| call.starts_with("reconcile:"))
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn disk_full_during_mutation_checkpoint_keeps_apply_at_zero_and_can_resume() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut first,
        d.path(),
        JournalMode::StartNew,
        JournalOptions {
            fail_at: Some((
                JournalRecordClass::EffectMutationStart,
                JournalFaultPoint::DiskFull,
            )),
        },
    );
    assert_eq!(result.error, Some(JournalErrorCode::JournalDiskFull));
    assert_eq!(result.disposition, RecoveryDisposition::ReplanRequired);
    assert_eq!(applies(&first), 0);

    let mut restarted = good_adapter();
    let resumed = execute(d.path(), &p, &mut restarted, JournalMode::ResumeExisting);
    assert!(resumed.completed);
    assert_eq!(applies(&restarted), 1);
}

#[test]
fn disk_full_after_apply_requires_reconciliation_before_later_effects() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[
        ("package.one", EffectPhase::Install),
        ("package.two", EffectPhase::Install),
    ]);
    let mut first = good_adapter();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut first,
        d.path(),
        JournalMode::StartNew,
        JournalOptions {
            fail_at: Some((
                JournalRecordClass::EffectVerification,
                JournalFaultPoint::DiskFull,
            )),
        },
    );
    assert_eq!(result.error, Some(JournalErrorCode::JournalDiskFull));
    assert_eq!(result.disposition, RecoveryDisposition::InspectionRequired);
    assert_eq!(applies(&first), 1);
    assert!(!first.calls.iter().any(|call| call == "apply:package.two"));

    let mut restarted = good_adapter();
    restarted.reconcile = Some(ReconciliationOutcome::VerifiedApplied);
    let resumed = execute(d.path(), &p, &mut restarted, JournalMode::ResumeExisting);
    assert!(resumed.completed);
    assert!(
        restarted
            .calls
            .iter()
            .any(|call| call == "reconcile:package.one")
    );
    assert!(
        !restarted
            .calls
            .iter()
            .any(|call| call == "apply:package.one")
    );
    assert!(
        restarted
            .calls
            .iter()
            .any(|call| call == "apply:package.two")
    );
}

#[test]
fn disk_full_after_final_verification_requires_read_only_recovery() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut first,
        d.path(),
        JournalMode::StartNew,
        JournalOptions {
            fail_at: Some((
                JournalRecordClass::FinalVerification,
                JournalFaultPoint::DiskFull,
            )),
        },
    );
    assert_eq!(result.error, Some(JournalErrorCode::JournalDiskFull));
    assert_eq!(result.disposition, RecoveryDisposition::InspectionRequired);
    assert_eq!(applies(&first), 1);

    let mut restarted = good_adapter();
    let resumed = execute(d.path(), &p, &mut restarted, JournalMode::ResumeExisting);
    assert!(resumed.completed);
    assert_eq!(applies(&restarted), 0);
    assert!(
        restarted
            .calls
            .iter()
            .any(|call| call.starts_with("verify:"))
    );
    assert!(restarted.calls.iter().any(|call| call == "final"));
}

#[test]
fn committed_completion_after_sync_error_returns_already_completed() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut first,
        d.path(),
        JournalMode::StartNew,
        JournalOptions {
            fail_at: Some((
                JournalRecordClass::TransactionCompletion,
                JournalFaultPoint::AfterDirectorySync,
            )),
        },
    );
    assert_eq!(result.error, Some(JournalErrorCode::JournalIoFailed));
    assert_eq!(result.disposition, RecoveryDisposition::InspectionRequired);
    assert_eq!(applies(&first), 1);

    let mut restarted = good_adapter();
    let resumed = execute(d.path(), &p, &mut restarted, JournalMode::ResumeExisting);
    assert!(resumed.completed);
    assert_eq!(resumed.disposition, RecoveryDisposition::AlreadyCompleted);
    assert_eq!(applies(&restarted), 0);
}
#[test]
fn next_effect_waits_for_verified_checkpoint() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[
        ("package.one", EffectPhase::Install),
        ("package.two", EffectPhase::Install),
    ]);
    let mut a = good_adapter();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::EffectVerification,
            JournalFaultPoint::Write,
        )),
    };
    InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    assert_eq!(applies(&a), 1)
}
#[test]
fn interruption_after_start_reconciles() {
    assert_unknown_resume(
        ReconciliationOutcome::StillUnknown,
        RecoveryDisposition::StillUnknown,
    )
}
#[test]
fn interruption_after_possible_mutation_reconciles() {
    assert_unknown_resume(
        ReconciliationOutcome::StillUnknown,
        RecoveryDisposition::StillUnknown,
    )
}
#[test]
fn unknown_resume_does_not_apply() {
    let (_, a) = unknown_resume(ReconciliationOutcome::StillUnknown);
    assert_eq!(applies(&a), 0)
}
#[test]
fn verified_applied_invokes_verify() {
    let (_, a) = unknown_resume(ReconciliationOutcome::VerifiedApplied);
    assert!(a.calls.iter().any(|c| c.starts_with("verify:")))
}
#[test]
fn reconciled_verified_continues() {
    let (r, _) = unknown_resume(ReconciliationOutcome::VerifiedApplied);
    assert!(r.completed)
}
#[test]
fn verified_not_applied_requires_replan() {
    assert_unknown_resume(
        ReconciliationOutcome::VerifiedNotApplied,
        RecoveryDisposition::ReplanRequired,
    )
}
#[test]
fn still_unknown_stops() {
    assert_unknown_resume(
        ReconciliationOutcome::StillUnknown,
        RecoveryDisposition::StillUnknown,
    )
}
#[test]
fn uncertainty_blocks_later_effect() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[
        ("package.one", EffectPhase::Install),
        ("package.two", EffectPhase::Install),
    ]);
    mutation_started(d.path(), &p);
    let mut a = good_adapter();
    a.reconcile = Some(ReconciliationOutcome::StillUnknown);
    execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(applies(&a), 0)
}
#[test]
fn unresolved_evidence_remains() {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let before = fs::read(checkpoint(d.path(), 1)).unwrap();
    let mut a = good_adapter();
    execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(fs::read(checkpoint(d.path(), 1)).unwrap(), before)
}
fn unknown_resume(x: ReconciliationOutcome) -> (JournalExecutionResult, Adapter) {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    let mut a = good_adapter();
    a.reconcile = Some(x);
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    (r, a)
}
fn assert_unknown_resume(x: ReconciliationOutcome, want: RecoveryDisposition) {
    let (r, _) = unknown_resume(x);
    assert_eq!(r.disposition, want)
}
#[test]
fn verified_effect_is_not_reapplied() {
    let (_, a) = complete_then_resume(true);
    assert_eq!(applies(&a), 0)
}
#[test]
fn completed_effect_is_recognized() {
    let (r, _) = complete_then_resume(true);
    assert_eq!(r.disposition, RecoveryDisposition::AlreadyCompleted)
}
#[test]
fn completed_resume_zero_mutation() {
    let (_, a) = complete_then_resume(true);
    assert_eq!(applies(&a), 0)
}
#[test]
fn completed_resume_no_integration() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[("platform.menu", EffectPhase::Integrate)]);
    let mut first = good_adapter();
    assert!(execute(d.path(), &p, &mut first, JournalMode::StartNew).completed);
    let mut second = good_adapter();
    execute(d.path(), &p, &mut second, JournalMode::ResumeExisting);
    assert_eq!(applies(&second), 0)
}
fn complete_then_resume(ok: bool) -> (JournalExecutionResult, Adapter) {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    execute(d.path(), &p, &mut first, JournalMode::StartNew);
    let mut second = good_adapter();
    second.effect_ok = ok;
    let r = execute(d.path(), &p, &mut second, JournalMode::ResumeExisting);
    (r, second)
}
#[test]
fn result_checkpoint_failure_stops_later_effect() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[
        ("package.one", EffectPhase::Install),
        ("package.two", EffectPhase::Install),
    ]);
    let mut a = good_adapter();
    a.outcome = Some(ApplyOutcome::FailureBeforeMutation);
    let o = JournalOptions {
        fail_at: Some((JournalRecordClass::EffectResult, JournalFaultPoint::Write)),
    };
    InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    assert_eq!(applies(&a), 1)
}
#[test]
fn verification_checkpoint_failure_stops() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::EffectVerification,
            JournalFaultPoint::Write,
        )),
    };
    let r = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    assert!(!r.completed)
}
#[test]
fn resume_last_durable_start_reconciles() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::EffectVerification,
            JournalFaultPoint::Write,
        )),
    };
    InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    let mut b = good_adapter();
    b.reconcile = Some(ReconciliationOutcome::VerifiedApplied);
    execute(d.path(), &p, &mut b, JournalMode::ResumeExisting);
    assert_eq!(applies(&b), 0)
}
#[test]
fn failed_write_not_durable_success() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::EffectVerification,
            JournalFaultPoint::Write,
        )),
    };
    InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert!(
        !j.records()
            .iter()
            .any(|r| matches!(r.record, JournalRecord::EffectVerified { .. }))
    )
}
#[test]
fn compensation_start_can_be_persisted() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::CompensationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    assert!(matches!(
        j.records().last().unwrap().record,
        JournalRecord::CompensationStarted { .. }
    ))
}
#[test]
fn compensation_checkpoint_failure_is_typed() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::CompensationStart,
            JournalFaultPoint::Write,
        )),
    };
    let mut j =
        InstallationJournal::open_with_options(d.path(), &p, JournalMode::StartNew, o).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    mark_first_verified(&mut j, &p);
    assert_eq!(
        j.append(JournalRecord::CompensationStarted {
            effect_id: "package.payload".into()
        })
        .unwrap_err()
        .code,
        JournalErrorCode::JournalIoFailed
    )
}
#[test]
fn verified_compensation_persists() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::CompensationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    j.append(JournalRecord::CompensationVerified {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    assert!(matches!(
        j.records().last().unwrap().record,
        JournalRecord::CompensationVerified { .. }
    ))
}
#[test]
fn interrupted_compensation_not_repeated() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::CompensationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    drop(j);
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.disposition, RecoveryDisposition::InspectionRequired);
    assert!(!a.calls.iter().any(|c| c.starts_with("compensate:")))
}
#[test]
fn missing_resume_zero_mutation() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalMissing));
    assert_eq!(applies(&a), 0)
}
#[test]
fn corrupt_json_zero_mutation() {
    let (d, p) = journal_fixture();
    fs::write(checkpoint(d.path(), 0), "{").unwrap();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalCorrupt));
    assert_eq!(applies(&a), 0)
}
#[test]
fn unexpected_transaction_file_fails_closed_before_mutation() {
    let (d, p) = journal_fixture();
    fs::write(
        tx(d.path()).join("unexpected-state"),
        b"not journal evidence",
    )
    .unwrap();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalCorrupt));
    assert_eq!(applies(&a), 0);
}
#[test]
fn broken_checksum_zero_mutation() {
    let (d, p) = journal_fixture();
    let f = checkpoint(d.path(), 0);
    let s = fs::read_to_string(&f)
        .unwrap()
        .replace("install.current-user.v1", "install.changed.v1");
    fs::write(f, s).unwrap();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalCorrupt));
    assert_eq!(applies(&a), 0)
}
#[test]
fn generation_gap_rejected() {
    let (d, p) = journal_fixture();
    fs::rename(checkpoint(d.path(), 1), checkpoint(d.path(), 2)).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn chain_mismatch_rejected() {
    let (d, p) = journal_fixture();
    let f = checkpoint(d.path(), 1);
    let s = fs::read_to_string(&f).unwrap().replace(
        "previous_record_sha256\":\"",
        "previous_record_sha256\":\"00",
    );
    fs::write(f, s).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn plan_mismatch_zero_mutation() {
    let (d, mut p) = journal_fixture();
    p.expected_completion_verification.expected = "different".into();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalPlanMismatch));
    assert_eq!(applies(&a), 0)
}
#[test]
fn artifact_mismatch_zero_mutation() {
    let (d, mut p) = journal_fixture();
    p.artifact = Some(ArtifactEvidence::NativeManagedArtifact {
        package_identity: "changed".into(),
    });
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.error, Some(JournalErrorCode::JournalPlanMismatch));
    assert_eq!(applies(&a), 0)
}
#[test]
fn start_collision_preserves_evidence() {
    let (d, p) = journal_fixture();
    let before = fs::read(checkpoint(d.path(), 0)).unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::StartNew)
            .unwrap_err()
            .code,
        JournalErrorCode::ActiveTransactionExists
    );
    assert_eq!(fs::read(checkpoint(d.path(), 0)).unwrap(), before)
}
#[test]
fn stale_temp_before_mutation_is_not_committed() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = InstallationJournal::open(d.path(), &p, JournalMode::StartNew).unwrap();
    j.append(JournalRecord::TransactionOpened {
        plan_id: p.plan_id.clone(),
        intent: p.intent,
        target_scope: p.target_scope,
    })
    .unwrap();
    drop(j);
    fs::write(
        tx(d.path()).join("checkpoint-0000000000000001.tmp"),
        "partial",
    )
    .unwrap();
    let mut a = good_adapter();
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert!(r.completed);
    assert_eq!(applies(&a), 1)
}
#[test]
fn stale_temp_after_start_reconciles() {
    let (d, p) = journal_fixture();
    fs::write(
        tx(d.path()).join("checkpoint-0000000000000002.tmp"),
        "partial",
    )
    .unwrap();
    let mut a = good_adapter();
    a.reconcile = Some(ReconciliationOutcome::StillUnknown);
    let r = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(r.disposition, RecoveryDisposition::StillUnknown);
    assert_eq!(applies(&a), 0)
}
#[test]
fn partial_committed_record_fails_closed() {
    let (d, p) = journal_fixture();
    fs::write(checkpoint(d.path(), 1), "{\"schema_version\":1").unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn invalid_non_tail_record_fails_closed() {
    let (d, p) = journal_fixture();
    fs::write(checkpoint(d.path(), 0), "{}").unwrap();
    assert_eq!(
        InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    )
}
#[test]
fn final_verification_failure_can_rerun_without_replay() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    first.final_ok = false;
    execute(d.path(), &p, &mut first, JournalMode::StartNew);
    let mut second = good_adapter();
    let r = execute(d.path(), &p, &mut second, JournalMode::ResumeExisting);
    assert!(r.completed);
    assert_eq!(applies(&second), 0)
}
#[test]
fn completion_write_failure_rechecks_only_final() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut first = good_adapter();
    let o = JournalOptions {
        fail_at: Some((
            JournalRecordClass::TransactionCompletion,
            JournalFaultPoint::Write,
        )),
    };
    InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut first,
        d.path(),
        JournalMode::StartNew,
        o,
    );
    let mut second = good_adapter();
    let r = execute(d.path(), &p, &mut second, JournalMode::ResumeExisting);
    assert!(r.completed);
    assert_eq!(applies(&second), 0);
    assert!(second.calls.contains(&"final".into()))
}
fn journal_fixture() -> (TempDir, InstallationPlan) {
    let d = TempDir::new().unwrap();
    let p = plan();
    mutation_started(d.path(), &p);
    (d, p)
}

fn opened_journal(d: &TempDir, p: &InstallationPlan) -> InstallationJournal {
    let mut journal = InstallationJournal::open(d.path(), p, JournalMode::StartNew).unwrap();
    journal
        .append(JournalRecord::TransactionOpened {
            plan_id: p.plan_id.clone(),
            intent: p.intent,
            target_scope: p.target_scope,
        })
        .unwrap();
    journal
}

#[test]
fn journaled_failure_compensates_prior_reversible_effect() {
    let d = TempDir::new().unwrap();
    let p = reversible_plan(&["a", "b"]);
    let mut a = good_adapter();
    a.outcomes
        .insert("b".into(), ApplyOutcome::FailureBeforeMutation);
    let r = execute(d.path(), &p, &mut a, JournalMode::StartNew);
    assert!(!r.completed);
    assert!(a.calls.contains(&"compensate:a".into()));
    let j = InstallationJournal::open(d.path(), &p, JournalMode::ResumeExisting).unwrap();
    assert!(j.records().iter().any(|record| matches!(
        &record.record,
        JournalRecord::CompensationVerified { effect_id } if effect_id == "a"
    )));
}

#[test]
fn journaled_compensation_is_reverse_order() {
    let d = TempDir::new().unwrap();
    let p = reversible_plan(&["a", "b", "c"]);
    let mut a = good_adapter();
    a.outcomes
        .insert("c".into(), ApplyOutcome::FailureBeforeMutation);
    execute(d.path(), &p, &mut a, JournalMode::StartNew);
    let compensated: Vec<_> = a
        .calls
        .iter()
        .filter(|call| call.starts_with("compensate:"))
        .cloned()
        .collect();
    assert_eq!(compensated, ["compensate:b", "compensate:a"]);
}

#[test]
fn irreversible_prior_effect_is_not_journal_compensated() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[("a", EffectPhase::Install), ("b", EffectPhase::Install)]);
    let mut a = good_adapter();
    a.outcomes
        .insert("b".into(), ApplyOutcome::FailureBeforeMutation);
    execute(d.path(), &p, &mut a, JournalMode::StartNew);
    assert!(!a.calls.iter().any(|call| call.starts_with("compensate:")));
}

#[test]
fn compensation_start_failure_prevents_compensation_call() {
    let d = TempDir::new().unwrap();
    let p = reversible_plan(&["a", "b"]);
    let mut a = good_adapter();
    a.outcomes
        .insert("b".into(), ApplyOutcome::FailureBeforeMutation);
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut a,
        d.path(),
        JournalMode::StartNew,
        JournalOptions {
            fail_at: Some((
                JournalRecordClass::CompensationStart,
                JournalFaultPoint::Write,
            )),
        },
    );
    assert_eq!(result.error, Some(JournalErrorCode::JournalIoFailed));
    assert!(!a.calls.iter().any(|call| call.starts_with("compensate:")));
}

#[test]
fn compensation_verification_failure_stops_without_earlier_compensation() {
    let d = TempDir::new().unwrap();
    let p = reversible_plan(&["a", "b", "c"]);
    let mut a = good_adapter();
    a.outcomes
        .insert("c".into(), ApplyOutcome::FailureBeforeMutation);
    a.compensation_verify_ok = false;
    let result = execute(d.path(), &p, &mut a, JournalMode::StartNew);
    assert_eq!(result.disposition, RecoveryDisposition::InspectionRequired);
    assert!(a.calls.contains(&"compensate:b".into()));
    assert!(!a.calls.contains(&"compensate:a".into()));
}

#[test]
fn verified_compensation_resume_requires_replan_without_replay() {
    let d = TempDir::new().unwrap();
    let p = reversible_plan(&["a"]);
    let mut j = opened_journal(&d, &p);
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::CompensationStarted {
        effect_id: "a".into(),
    })
    .unwrap();
    j.append(JournalRecord::CompensationVerified {
        effect_id: "a".into(),
    })
    .unwrap();
    drop(j);
    let mut a = good_adapter();
    let result = execute(d.path(), &p, &mut a, JournalMode::ResumeExisting);
    assert_eq!(result.disposition, RecoveryDisposition::ReplanRequired);
    assert_eq!(applies(&a), 0);
    assert!(!a.calls.iter().any(|call| call.starts_with("compensate:")));
}

#[test]
fn completion_directly_after_open_is_rejected_without_checkpoint() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    assert_eq!(
        j.append(JournalRecord::TransactionCompleted)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    );
    assert_eq!(j.records().len(), 1);
}

#[test]
fn completion_without_final_verification_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    mark_first_verified(&mut j, &p);
    assert_eq!(
        j.append(JournalRecord::TransactionCompleted)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    );
}

#[test]
fn final_verification_with_unverified_effect_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    assert_eq!(
        j.append(JournalRecord::FinalVerificationSucceeded)
            .unwrap_err()
            .code,
        JournalErrorCode::JournalCorrupt
    );
}

#[test]
fn final_verification_with_partial_effect_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    j.append(JournalRecord::EffectMutationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    j.append(JournalRecord::EffectOutcome {
        effect_id: "package.payload".into(),
        outcome: EffectResultState::KnownPartialMutation,
    })
    .unwrap();
    assert!(j.append(JournalRecord::FinalVerificationSucceeded).is_err());
}

#[test]
fn final_verification_with_compensated_effect_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::CompensationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    j.append(JournalRecord::CompensationVerified {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    assert!(j.append(JournalRecord::FinalVerificationSucceeded).is_err());
}

#[test]
fn duplicate_transaction_open_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    assert!(
        j.append(JournalRecord::TransactionOpened {
            plan_id: p.plan_id.clone(),
            intent: p.intent,
            target_scope: p.target_scope
        })
        .is_err()
    );
}

#[test]
fn effect_verified_before_mutation_started_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    assert!(
        j.append(JournalRecord::EffectVerified {
            effect_id: "package.payload".into(),
            result: JournalVerifiedResult::VerifiedSuccess
        })
        .is_err()
    );
}

#[test]
fn duplicate_mutation_started_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    j.append(JournalRecord::EffectMutationStarted {
        effect_id: "package.payload".into(),
    })
    .unwrap();
    assert!(
        j.append(JournalRecord::EffectMutationStarted {
            effect_id: "package.payload".into()
        })
        .is_err()
    );
}

#[test]
fn duplicate_effect_verified_is_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    mark_first_verified(&mut j, &p);
    assert!(
        j.append(JournalRecord::EffectVerified {
            effect_id: "package.payload".into(),
            result: JournalVerifiedResult::VerifiedSuccess
        })
        .is_err()
    );
}

#[test]
fn later_effect_cannot_start_before_prior_verification() {
    let d = TempDir::new().unwrap();
    let p = plan_with(&[("a", EffectPhase::Install), ("b", EffectPhase::Integrate)]);
    let mut j = opened_journal(&d, &p);
    assert!(
        j.append(JournalRecord::EffectMutationStarted {
            effect_id: "b".into()
        })
        .is_err()
    );
}

#[test]
fn duplicate_completion_and_records_after_completion_are_rejected() {
    let d = TempDir::new().unwrap();
    let p = plan();
    let mut j = opened_journal(&d, &p);
    mark_first_verified(&mut j, &p);
    j.append(JournalRecord::FinalVerificationSucceeded).unwrap();
    j.append(JournalRecord::TransactionCompleted).unwrap();
    assert!(j.append(JournalRecord::TransactionCompleted).is_err());
    assert!(
        j.append(JournalRecord::StageEntered {
            stage: Stage::Complete
        })
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn journal_ancestor_link_refuses_execution_before_effects() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    let p = plan();
    let mut adapter = Adapter::default();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut adapter,
        &alias.join("journal"),
        JournalMode::StartNew,
        JournalOptions::default(),
    );
    assert!(!result.completed);
    assert!(!adapter.calls.iter().any(|call| call.starts_with("apply:")));
    assert!(!outside.join("journal").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn journal_writable_ancestor_cannot_relocate_private_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let shared = temp.path().join("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
    let p = plan();
    let mut adapter = Adapter::default();
    let result = InstallerEngine.execute_journaled_with_options(
        &request(&p),
        &p,
        &mut adapter,
        &shared.join("private"),
        JournalMode::StartNew,
        JournalOptions::default(),
    );
    assert!(!result.completed);
    assert!(!adapter.calls.iter().any(|call| call.starts_with("apply:")));
    assert!(!shared.join("private").exists());
}
