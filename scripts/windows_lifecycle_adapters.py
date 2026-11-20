"""Typed launch contracts for existing acceptance scripts; authority stays closed.

These builders reuse actual script parameters. Launches are intentionally blocked
until protected native authority is qualified. Credentials are never arguments.
Raw child output is discarded: only independently reopened evidence is usable.
A killed launcher may leave descendants/effects; timeout always means UNKNOWN.
"""
from dataclasses import dataclass
from pathlib import Path
import subprocess
from windows_native_lifecycle import RunContext, LifecycleError, _require
from windows_lifecycle_execution import native_execute


@dataclass(frozen=True)
class CandidateInputs:
    setup: Path
    manifest: Path
    evidence_directory: Path
    size_bytes: int

    def validate(self):
        _require(type(self.size_bytes) is int and 0 < self.size_bytes < 2**34, 'CANDIDATE_SIZE_INVALID')
        for path in (self.setup, self.manifest, self.evidence_directory):
            _require(isinstance(path, Path) and path.is_absolute() and
                     not any(c in str(path) for c in '\x00\r\n'), 'ADAPTER_PATH_INVALID')


@dataclass(frozen=True)
class ScriptContract:
    script: str
    role: str
    privilege: str
    side_effects: str
    evidence: str
    timeout_seconds: int
    recovery: str = 'UNKNOWN_REQUIRES_INDEPENDENT_REQUERY_NO_REPLAY'
    cleanup: str = 'IMMUTABLE_RUN_INVENTORY_AND_INDEPENDENT_RESOURCE_QUERIES'


CONTRACTS = {
    'environment': ScriptContract('qualify-windows-native-environment.ps1', 'subject', 'ordinary_user',
        'evidence_files', 'environment qualification JSON; independently verify candidate/token/session', 180),
    'journey': ScriptContract('test-windows-gui-journey.ps1', 'subject', 'ordinary_user',
        'install_gui_ipc_startup_processes', 'GUI and IPC evidence; independent product verification', 1500),
    'lifecycle': ScriptContract('test-windows-installed-uninstall.ps1', 'subject', 'ordinary_user',
        'shutdown_uninstall', 'installed lifecycle evidence; independent removal verification', 1500),
    'cleanup': ScriptContract('cleanup-windows-p013-test-user.ps1', 'controller', 'elevated',
        'account_profile_tasks_process_deletion', 'cleanup JSON is diagnostic only; inventory requery required', 900),
}


def build_arguments(kind, root, context: RunContext, inputs: CandidateInputs, *, approval,
                    startup='false', desktop='false', launch='false', test_user=''):
    import re
    context.validate()
    inputs.validate()
    _require(kind in CONTRACTS, 'ADAPTER_KIND_INVALID')
    _require(approval == 'I_APPROVE_DISPOSABLE_SYNVEIL_WINDOWS_ACCEPTANCE', 'HOST_MUTATION_APPROVAL_REQUIRED')
    script = Path(root).resolve() / 'scripts' / CONTRACTS[kind].script
    args = ['pwsh', '-NoLogo', '-NoProfile', '-NonInteractive', '-File', str(script),
            '-Setup', str(inputs.setup), '-ReleaseManifest', str(inputs.manifest),
            '-ExpectedSourceCommit', context.source_commit,
            '-ExpectedSetupSha256', context.candidate_sha256,
            '-ExpectedManifestSha256', context.manifest_sha256,
            '-ExpectedSetupSizeBytes', str(inputs.size_bytes),
            '-EvidenceDirectory', str(inputs.evidence_directory), '-OperatorApproval', approval]
    if kind in {'journey', 'lifecycle'}:
        args += ['-ExpectedHarnessCommit', context.harness_commit]
    if kind == 'journey':
        _require(all(v in {'true', 'false'} for v in (startup, desktop, launch)), 'ADAPTER_OPTION_INVALID')
        args += ['-StartupEnabled', startup, '-DesktopShortcutEnabled', desktop, '-LaunchAfterInstall', launch]
    if kind == 'cleanup':
        _require(bool(re.fullmatch(r'synveil_p013_[0-9a-f]{8}', test_user)), 'DISPOSABLE_SUBJECT_INVALID')
        args += ['-TestUserName', test_user]
    return tuple(args)


def launch(kind, root, context, inputs, **options):
    # First prove authority; currently always blocked BEFORE process creation.
    native_execute()
    args = build_arguments(kind, root, context, inputs, **options)
    try:
        result = subprocess.run(args, shell=False, stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=CONTRACTS[kind].timeout_seconds, check=False)
    except (subprocess.TimeoutExpired, OSError):
        raise LifecycleError('OPERATION_OUTCOME_UNKNOWN') from None
    # Exit code is never proof of side-effect success/failure or cleanup.
    return {'outcome': 'RECONCILIATION_REQUIRED', 'exit_code': result.returncode}
