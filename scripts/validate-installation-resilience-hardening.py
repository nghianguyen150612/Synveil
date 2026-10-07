#!/usr/bin/env python3
"""P044 structural regression checks for recovery, preservation and evidence."""
from __future__ import annotations

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    journal = read("crates/install-engine/src/journal.rs")
    execute = journal[journal.index("pub fn execute_journaled_with_options") :]
    start = execute.index("journal.append(JournalRecord::EffectMutationStarted")
    apply = execute.index("adapter.apply_effect(effect)")
    require(start < apply, "durable mutation-start must precede adapter apply")
    require("adapter.reconcile_unknown(effect)" in execute[:apply],
            "unmatched mutation must reconcile before any new apply")
    require("JournalFaultPoint::AfterWrite" in journal
            and "JournalFaultPoint::AfterFileSync" in journal
            and "JournalFaultPoint::AfterCommit" in journal
            and "JournalFaultPoint::AfterCommittedObjectSync" in journal
            and "JournalFaultPoint::AfterDirectorySync" in journal,
            "journal durability fault boundaries missing")
    require("JournalErrorCode::JournalDiskFull" in journal
            and "std::io::ErrorKind::StorageFull" in journal,
            "journal disk-full classification missing")
    require("create_directory_durable" in journal and "sync_directory(parent)" in journal,
            "new journal directory parent durability missing")
    require("if !name.starts_with(\"checkpoint-\") || !name.ends_with(\".json\")"
            in journal and "JournalErrorCode::JournalCorrupt" in journal,
            "unexpected transaction-directory objects must fail closed")
    require("journal_recovery_stop(e.code)" in execute,
            "post-mutation journal failure must require inspection")
    require(not re.search(r"remove_dir_all|\.clear\(\)", journal),
            "journal recovery must not reset or recursively delete durable state")

    journal_tests = read("crates/install-engine/tests/journal_contract.rs")
    for token in (
        "post_boundary_faults_reconcile_only_when_a_start_checkpoint_exists",
        "disk_full_during_mutation_checkpoint_keeps_apply_at_zero_and_can_resume",
        "disk_full_after_apply_requires_reconciliation_before_later_effects",
        "disk_full_after_final_verification_requires_read_only_recovery",
        "committed_completion_after_sync_error_returns_already_completed",
        "forced_process_termination_restarts_from_durable_evidence",
        "changed_plan_changes_fingerprint",
        "plan_mismatch_zero_mutation",
        "completed_resume_zero_mutation",
        "interrupted_compensation_not_repeated",
        "nested_journal_root_is_created_for_new_transaction",
        "unknown_resume_does_not_apply",
        "still_unknown_stops",
        "verification_checkpoint_failure_stops",
        "completion_without_final_verification_is_rejected",
        "generation_gap_rejected",
        "journal_root_symlink_rejected",
        "compensation",
    ):
        require(token in journal_tests, "journal resilience test absent: " + token)
    require("process_restart_recover_child" in journal_tests
            and "JournalMode::ResumeExisting" in journal_tests,
            "fresh process recovery harness missing")

    acquisition = read("scripts/release_download.py")
    stage = acquisition[acquisition.index("def stage_artifact") :]
    parent_sync = acquisition[acquisition.index("def _sync_staging_parent") :
                               acquisition.index("class BoundedArgumentParser")]
    require("INSUFFICIENT_DISK_SPACE" in acquisition
            and "os.link" in stage
            and "_raise_staging_error(error)" in stage,
            "bounded no-clobber acquisition and disk-full handling missing")
    require("_sync_staging_parent(root, root_identity, directory_fd)" in stage
            and "_sync_windows_directory" in parent_sync
            and "os.fsync(directory_fd)" in parent_sync,
            "platform parent-directory durability path missing")
    require("return acquisition_result(\"VERIFIED\"" in stage
            and stage.index("_raise_staging_error(error)") < stage.index("return acquisition_result(\"VERIFIED\""),
            "staging failures must not return verified evidence")
    require(not re.search(r"headers\s*=\s*\{[^}]*\bRange\b", acquisition, re.I | re.S),
            "unauthenticated HTTP range-resume path added")

    quick = read("scripts/linux_quick_install.py")
    require("EXIT_DISK_SPACE" in quick and "INSUFFICIENT_DISK_SPACE" in quick,
            "quick-install disk-full classification missing")
    require("OutcomeUnknown" in quick and "Inspect native package state" in quick,
            "ambiguous native package result must require inspection")
    require("if installed is None and package_payload_present()" in quick
            and "test_partial_payload_with_absent_package_metadata_stops_before_retry"
            in read("tests/linux_quick_install/test_linux_quick_install.py"),
            "partial native payload must stop before package-manager retry")
    production = [
        "scripts/linux_quick_install.py",
        "deploy/install/quick-install.sh",
        "deploy/packages/debian/preinst",
        "deploy/packages/debian/postinst",
        "deploy/packages/debian/prerm",
        "deploy/packages/debian/postrm",
        "deploy/packages/rpm/synveil.spec.tmpl",
    ]
    lock_delete = re.compile(
        r"(?:\brm\b|\bunlink\b|remove_file)\s+[^\n]*"
        r"(?:dpkg|apt|rpm|dnf)[^\n]*lock|"
        r"(?:dpkg|apt|rpm|dnf)[^\n]*lock[^\n]*(?:\brm\b|\bunlink\b|remove_file)",
        re.I,
    )
    for path in production:
        require(not lock_delete.search(read(path)), "package-manager lock deletion in " + path)

    recovery_sources = "\n".join(read(path) for path in (
        "crates/install-engine/src/journal.rs",
        "crates/install-engine/src/lifecycle.rs",
        "crates/install-engine/src/appimage.rs",
        "scripts/linux_quick_install.py",
        "deploy/windows/installer/Synveil.iss",
    ))
    require(not re.search(r"\bDROP\s+(?:DATABASE|TABLE)\b|"
                          r"\b(?:reset|recreate)\s+(?:the\s+)?(?:database|db)\b|"
                          r"(?:database|client\.db)\s*\.\s*(?:unlink|remove)\b",
                          recovery_sources, re.I),
            "recovery must never reset or delete user/server database state")
    require("shutil.rmtree(staging" in quick
            and not re.search(r"\b(?:rm\s+-rf\s+/(?:\s|$)|remove_dir_all\([^)]*(?:install|package|root))",
                              journal + quick, re.I),
            "cleanup must remain scoped to owned staging state")

    error_model = read("crates/install-engine/src/error_model.rs")
    require("AcquisitionFailureCode::InsufficientDiskSpace" in error_model
            and "JournalErrorCode::JournalDiskFull" in error_model
            and "JOURNAL_DISK_FULL_RECONCILE_FIRST" in error_model,
            "finite disk-full and reconciliation error mapping missing")

    release_signature = read("scripts/release_signature.py")
    release_channel = read("scripts/release_channel.py")
    lifecycle = read("crates/install-engine/src/lifecycle.rs")
    require("Ed25519PublicKey.from_public_bytes" in release_signature
            and ".verify(signature, raw)" in release_signature,
            "P043 Ed25519 release verification regressed")
    require("evaluate_high_water" in release_channel
            and "CHANNEL_ROLLBACK" in release_channel
            and "CHANNEL_EQUIVOCATION" in release_channel,
            "P043 channel freshness/high-water controls regressed")
    require("if target < source" in lifecycle and "NewerThanSupported" in lifecycle,
            "P043 downgrade rejection regressed")
    require("src_dir_fd=directory_fd" in acquisition and "O_NOFOLLOW" in acquisition
            and "st_file_attributes" in acquisition,
            "P043 acquisition path protections regressed")
    require("last-use" in quick.lower() or "require_staged_evidence" in quick,
            "P043 native last-use artifact verification regressed")

    appimage = read("crates/install-engine/src/appimage.rs")
    appimage_tests = read("crates/install-engine/tests/appimage_integration.rs")
    require("let previous = self.existing_record()?" in appimage
            and "AppImageIntegrationStatus::Incomplete" in appimage,
            "AppImage ambiguous ownership must remain fail-closed")
    require("partial_launcher_without_record_is_not_reported_installed_or_replayed" in appimage_tests,
            "AppImage partial-state preservation fixture missing")

    windows_build = read("scripts/build-windows-installer.ps1")
    windows_files = windows_build[windows_build.index("function Read-Payload") :
                                  windows_build.index("$repo =")]
    require(windows_files.index("$lines = foreach") < windows_files.index("$lines += 'Source: \"' + $manifestSource"),
            "Windows target ownership manifest must follow payload files")
    windows_iss = read("deploy/windows/installer/Synveil.iss")
    require("LoadTrustedPreviousManifest" in windows_iss
            and "RemoveProvenObsoleteFiles" in windows_iss
            and "CurrentManifestOwns" in windows_iss
            and "DelTree(" not in windows_iss,
            "Windows recovery cleanup must use trusted old-minus-new ownership")
    windows_lifecycle = read("scripts/test-windows-installer-lifecycle.ps1")
    windows_workflow = read(".github/workflows/windows-installer.yml")
    require("Interrupt-UpgradeAfterOwnedPayloadCopy" in windows_lifecycle
            and "prior_manifest_and_registration_matched" in windows_lifecycle
            and "P044_NEWER_LICENSE_HASH" in windows_workflow,
            "Windows native partial-upgrade process-interruption harness missing")

    resilience = read("docs/v0.2/INSTALLATION_RESILIENCE_HARDENING.md")
    manifest = read("docs/v0.2/PROMPT044_MANIFEST.md")
    for evidence_class in ("source/unit", "fixture", "process-interruption",
                           "Native CI", "VM reboot", "VM power-cycle"):
        require(evidence_class in resilience, "resilience evidence class missing: " + evidence_class)
    for preserved in ("credentials", "client state", "libraries", "server",
                      "unknown neighboring files", "startup"):
        require(preserved in resilience.lower(), "preservation invariant missing: " + preserved)
    require("BLOCKED / unavailable native power-cycle evidence" in resilience
            and "BLOCKED / unavailable native power-cycle evidence" in manifest,
            "unavailable power-cycle evidence must remain explicitly blocked")
    require("P045 remains the owner" in resilience and "P045 is explicitly deferred" in manifest,
            "P045 scope boundary missing")
    require("interruption-power-cycle" in read("tests/install-acceptance/scenarios/install-journey-8.json"),
            "P004 INSTALL-JOURNEY-8 power-cycle minimum was changed")

    roadmap = read("docs/v0.2/ROADMAP.md")
    p044 = roadmap.split("### P044 —", 1)[1].split("### P045 —", 1)[0]
    p045 = roadmap.split("### P045 —", 1)[1].split("### P046 —", 1)[0]
    require("**Implemented" in p044, "P044 roadmap evidence status missing")
    require("**Implemented" not in p045, "P045 must remain deferred")
    p048 = roadmap.split("### P048 —", 1)[1].split("## Detailed ownership", 1)[0]
    require("**Implemented" not in p048
            and "No v0.2.0 tag or release is created." in manifest,
            "P044 must not claim or create the v0.2.0 release")

    print("installation resilience hardening: PASS")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, ValueError) as error:
        print("installation resilience hardening: FAIL: " + str(error), file=sys.stderr)
        sys.exit(1)
