#!/usr/bin/env python3
"""Network-free P022-P027 installer, runtime, startup, and lifecycle validator."""

from __future__ import annotations

import hashlib
import json
import re
import sys
import tempfile
from pathlib import Path, PurePosixPath, PureWindowsPath

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "deploy/windows/installer/toolchain.lock"
ISS = ROOT / "deploy/windows/installer/Synveil.iss"
BUILD = ROOT / "scripts/build-windows-installer.ps1"
WORKFLOW = ROOT / ".github/workflows/windows-installer.yml"
NATIVE_WORKFLOW = ROOT / ".github/workflows/windows-native-acceptance.yml"
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"
PACKAGE = ROOT / "deploy/packages/build-windows.sh"
INSTALLED_RUNTIME_TEST = ROOT / "scripts/test-windows-installed-runtime.ps1"
PER_USER_TEST = ROOT / "scripts/test-windows-per-user-installation.ps1"
PER_USER_INVOKER = ROOT / "scripts/invoke-windows-standard-user-test.ps1"
LIFECYCLE_TEST = ROOT / "scripts/test-windows-installer-lifecycle.ps1"
LIFECYCLE_MODEL = ROOT / "scripts/windows_lifecycle.py"
CLIENT_LAUNCH = ROOT / "crates/client/src/launch.rs"
CLIENT_CONFIG = ROOT / "crates/client/src/config.rs"
CLIENT_LIB = ROOT / "crates/client/src/lib.rs"
DESKTOP_BRIDGE = ROOT / "crates/desktop/src/bridge.rs"
REPRODUCIBLE = ROOT / "deploy/packages/common/reproducible.sh"
MSVC_TOOLCHAIN = ROOT / "scripts/select-msvc-linker.ps1"
APP_ID = "{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}"


def require(value: bool, message: str) -> None:
    if not value:
        raise AssertionError(message)


def parse_lock(path: Path = LOCK) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    require(set(data) == {"schema_version", "tool", "version", "url", "sha256", "compiler"}, "lock fields")
    require(data["schema_version"] == 1 and data["tool"] == "Inno Setup", "lock identity")
    require(data["version"] == "6.7.3", "Inno version pin")
    require(data["url"] == "https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe", "immutable official URL")
    require(re.fullmatch(r"[0-9a-f]{64}", data["sha256"]) is not None, "lock digest")
    require(data["compiler"] == "ISCC.exe", "compiler identity")
    return data


def verify_digest(path: Path, expected: str) -> None:
    require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, "SHA mismatch rejection")


def safe_relative(value: str) -> str:
    normalized = value.replace("\\", "/")
    path = PurePosixPath(normalized)
    require(not path.is_absolute() and not PureWindowsPath(value).is_absolute(), "absolute payload path")
    require(".." not in path.parts and value and "\0" not in value, "payload traversal")
    return normalized


def validate_names(values: list[str]) -> None:
    seen: set[str] = set()
    for value in values:
        key = safe_relative(value).casefold()
        require(not key.endswith(".exe") or key in {"synveil-desktop.exe", "synveil-client.exe"}, "unexpected executable")
        require(key not in seen, "payload duplicate/case collision")
        seen.add(key)


def expect_failure(function, *args) -> None:
    try:
        function(*args)
    except (AssertionError, json.JSONDecodeError):
        return
    raise AssertionError("invalid input was accepted")


def main() -> int:
    lock = parse_lock()
    iss = ISS.read_text(encoding="utf-8")
    build = BUILD.read_text(encoding="utf-8")
    workflow = WORKFLOW.read_text(encoding="utf-8")
    native_workflow = NATIVE_WORKFLOW.read_text(encoding="utf-8")
    ci_workflow = CI_WORKFLOW.read_text(encoding="utf-8")
    reproducible = REPRODUCIBLE.read_text(encoding="utf-8")
    msvc_toolchain = MSVC_TOOLCHAIN.read_text(encoding="utf-8")
    package = PACKAGE.read_text(encoding="utf-8")
    installed_test = INSTALLED_RUNTIME_TEST.read_text(encoding="utf-8")
    per_user_test = PER_USER_TEST.read_text(encoding="utf-8")
    per_user_invoker = PER_USER_INVOKER.read_text(encoding="utf-8")
    lifecycle_test = LIFECYCLE_TEST.read_text(encoding="utf-8")
    lifecycle_model = LIFECYCLE_MODEL.read_text(encoding="utf-8")
    launch = CLIENT_LAUNCH.read_text(encoding="utf-8")
    config = CLIENT_CONFIG.read_text(encoding="utf-8")
    client_lib = CLIENT_LIB.read_text(encoding="utf-8")
    desktop_bridge = DESKTOP_BRIDGE.read_text(encoding="utf-8")
    controller_start = desktop_bridge.index("fn start_controller(mut self: Pin<&mut Self>)")
    controller_end = desktop_bridge.index("fn install_tray(mut self: Pin<&mut Self>)", controller_start)
    controller_startup = desktop_bridge[controller_start:controller_end]
    lower = iss.lower()
    require(f"AppId={{{APP_ID}" in iss, "stable AppId")
    require("DefaultDirName={localappdata}\\Programs\\Synveil" in iss, "per-user path")
    require("PrivilegesRequired=lowest" in iss and not re.search(r"(?m)^PrivilegesRequiredOverridesAllowed=", iss), "no elevation override (Inno blank default)")
    require("ArchitecturesAllowed=x64" in iss and "ArchitecturesInstallIn64BitMode=x64" in iss, "x86_64 constraint")
    require('Filename: "{app}\\synveil-desktop.exe"' in iss, "absolute installed shortcut target")
    for directive in ("DisableWelcomePage=no", "DisableDirPage=yes", "DisableProgramGroupPage=yes", "DisableReadyPage=yes"):
        require(directive in iss, f"four-page topology directive: {directive}")
    require("LicenseFile={#SynveilPayloadDir}\\LICENSE" in iss, "repository license page")
    for control in ("StartupRequestedCheckBox", "DesktopIconRequestedCheckBox", "LaunchRequestedCheckBox"):
        require(control in iss, f"stable native option control: {control}")
    for state in ("StartupRequested", "DesktopIconRequested", "LaunchRequested"):
        require(state in iss, f"distinct option state: {state}")
    require("not WizardSilent" in iss, "interactive selected and silent fail-closed defaults")
    require('Name: "{userdesktop}\\Synveil"' in iss and "Check: ShouldCreateDesktopIcon" in iss, "conditional current-user desktop shortcut")
    require("{commondesktop}" not in lower, "no common desktop shortcut")
    require("[Run]" not in iss and iss.count("Exec(ExpandConstant('{app}\\synveil-desktop.exe')") == 1, "single absolute launch authority")
    require("LaunchRequested and (not WizardSilent)" in iss, "silent mode cannot launch")
    require("P026" in iss and "StartupRequested" in iss, "P026 startup handoff boundary")
    require("--startup-preference " in iss and "synveil-client.exe" in iss and "ewWaitUntilTerminated" in iss,
            "bounded installed-client startup handoff")
    require("Silent installation requires /STARTUP=0 or /STARTUP=1" in iss, "silent startup choice required")
    for forbidden in ("schtasks", "currentversion\\run", "programdata", "{commonprograms}", "create service"):
        require(forbidden not in lower, f"forbidden installer authority: {forbidden}")
    for forbidden in ("privilegesrequired=admin", "privilegesrequired=poweruser", "requireadministrator", "highestavailable", "runas", "{autopf}", "{pf}"):
        require(forbidden not in lower, f"forbidden per-machine/elevation authority: {forbidden}")
    require("http://" not in lower and "https://" not in lower, "no install-time network authority")
    require('source: "*"' not in lower, "broad wildcard")
    require('#include GeneratedDir + "\\\\files.iss"' in iss, "installer files derive from closed generated inventory")
    for option in ("STARTUP", "DESKTOPICON", "LAUNCH"):
        require(f"ParseBooleanOption('{option}'" in iss, f"silent option parser: {option}")
    require("accepts only 0 or 1" in iss, "malformed silent option rejection")
    require("%appdata%" not in lower and "%localappdata%\\synveil" not in lower and "deltree" not in lower, "state removal")
    for token in ("TOOLCHAIN_ACQUISITION_FAILURE", "TOOLCHAIN_INTEGRITY_FAILURE", "PAYLOAD_BUILD_FAILURE", "PAYLOAD_IDENTITY_FAILURE", "INSTALLER_COMPILE_FAILURE", "INSTALLER_VERIFY_FAILURE"):
        require(token in build, f"typed failure: {token}")
    for unsafe in ("Invoke-Expression", "cmd /c", "Start-Process"):
        require(unsafe not in build, f"unsafe PowerShell primitive: {unsafe}")
    require("Get-FileHash" in build and "-cne $Lock.sha256" in build, "digest before execution")
    require("windows-x86_64-installer" in build and '"windows_installer"' in build and '"SynveilSetup.exe"' in build and '"primary_installer"' in build, "artifact manifest entry")
    require("windows-latest" in workflow and "/VERYSILENT" in per_user_test and "state-sentinel" in per_user_test, "native smoke contract")
    require("fn should_start_background_client(smoke_test: bool)" in desktop_bridge and
            "if !should_start_background_client(smoke_test)" in controller_startup and
            controller_startup.index("if !should_start_background_client(smoke_test)") <
            controller_startup.index("launch_manager.ensure_running().await"),
            "bounded QML smoke must not leave a detached background client")
    require("profile_id=not-a-profile" in per_user_test and
            "$env:SYNVEIL_CLIENT_CONFIG = $clientProbeConfig" in per_user_test and
            "78 'client probe'" in per_user_test,
            "standard-user client probe uses an isolated invalid config fixture")
    require('SYNVEIL_ENABLE_QMLCACHEGEN_WRAPPER: "1"' in workflow,
            "Windows installer candidate enables canonical QML source and RCC staging")
    require('SYNVEIL_ENABLE_QMLCACHEGEN_WRAPPER: "1"' in native_workflow,
            "Windows P028 candidate enables canonical QML source and RCC staging")
    for candidate_workflow in (workflow, native_workflow):
        require("target/windows-installer-runtime/SYNVEIL-MANIFEST.txt" in candidate_workflow and
                "target/windows-installer-generated/payload-inventory.json" in candidate_workflow,
                "Windows candidate uploads its exact runtime input inventory for cross-producer diagnosis")
    for evidence in ("test-windows-installed-runtime.ps1", "QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "unrelatedCwd", "client probe", "missing-qwindows", "corrupt-dll", "unexpected-dll", "developer-file"):
        require(evidence in workflow or evidence in per_user_test, f"P024 hosted runtime evidence: {evidence}")
    require("select-msvc-linker.ps1" in workflow and "VCToolsInstallDir" in msvc_toolchain and "CompanyName" in msvc_toolchain and "OriginalFilename" in msvc_toolchain, "authenticated MSVC linker selection")
    require("LinkType" in msvc_toolchain and "0x00004550" in msvc_toolchain and "0x8664" in msvc_toolchain, "regular AMD64 PE linker identity")
    require("VCToolsRedistDir" in msvc_toolchain and "VCToolsVersion" in msvc_toolchain and "SYNVEIL_MSVC_CRT_DIR" in msvc_toolchain and "Microsoft.VC*.CRT" in msvc_toolchain and "$crtCandidates.Count -ne 1" in msvc_toolchain, "authenticated exact MSVC runtime selection")
    require("VSINSTALLDIR" in msvc_toolchain and
            "VC\\Tools\\Llvm\\x64\\bin\\llvm-ar.exe" in msvc_toolchain and
            "SYNVEIL_MSVC_AR" in msvc_toolchain and "--version" in msvc_toolchain and
            "LLVM archiver SHA-256" in msvc_toolchain and
            "MSVC_ARCHIVER_SELECTION_FAILURE" in msvc_toolchain,
            "deterministic MSVC archiver is selected and identified from the active Visual Studio installation")
    require("MSVC reproducible builds require SYNVEIL_MSVC_AR" in reproducible and
            "AR conflicts with the selected deterministic MSVC archiver" in reproducible and
            "synveil_append_msys_env_conversion_exclusion AR" in reproducible and
            "synveil_append_msys_env_conversion_exclusion SYNVEIL_MSVC_AR" in reproducible and
            "for build_flag_var in CFLAGS CXXFLAGS" in reproducible and
            "synveil_append_msys_env_conversion_exclusion \"$build_flag_var\"" in reproducible and
            'native_repro_flags=("/Brepro" "/experimental:deterministic")' in reproducible and
            '"/pathmap:${native_repo_root}=/usr/src/synveil"' in reproducible and
            '"/pathmap:${native_target_dir}=/usr/src/synveil-target"' in reproducible and
            'existing_native_flags+="$native_flag"' in reproducible and
            '"/experimental:deterministic"' in (ROOT / "scripts/test-windows-rustflags.sh").read_text(encoding="utf-8") and
            "cqD" in reproducible and "unselected AR executable" in
            (ROOT / "scripts/test-windows-rustflags.sh").read_text(encoding="utf-8"),
            "MSVC uses deterministic archives, compiler mode, source/output path maps, and MSYS-safe C/C++ flags")
    require(reproducible.index('"/pathmap:${native_target_dir}=/usr/src/synveil-target"') <
            reproducible.index('"/pathmap:${native_repo_root}=/usr/src/synveil"'),
            "MSVC maps nested target roots before the broader checkout path")
    require("$banner" not in workflow and "& $linker '/?'" not in workflow, "linker identity does not depend on localized help output")
    require("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER" in reproducible and '"${rustc_linker_args[@]}"' in reproducible, "direct rustc uses selected MSVC linker")
    require("CARGO_ENCODED_RUSTFLAGS" in reproducible and "$'\\x1f'" in reproducible, "lossless Cargo flag transport")
    require('synveil_run_direct_rustc "$host_toolchain"' in reproducible and
            '"${SYNVEIL_REPRODUCIBLE_RUSTC_FLAGS[@]}" "${rustc_linker_args[@]}"' in reproducible,
            "direct rustc receives discrete remaps through the selected toolchain wrapper")
    require("host_toolchain\" == *\"msvc\"" in reproducible and "link-arg=/Brepro" in reproducible,
            "MSVC release links use the deterministic PE timestamp mode")
    require('MSYS2_ARG_CONV_EXCL="$arg_conversion_exclusions" rustc "$@"' in reproducible and
            'arg_conversion_exclusions+="link-arg=/Brepro"' in reproducible and
            'synveil_run_direct_rustc' in (ROOT / "scripts/test-windows-rustflags.sh").read_text(encoding="utf-8"),
            "Git Bash preserves the MSVC /Brepro linker argument across direct rustc invocation")
    require("test-windows-rustflags.sh" in ci_workflow,
            "Windows Qt reproducibility CI runs the MSVC linker-flag regression")
    rustflags_test = (ROOT / "scripts/test-windows-rustflags.sh").read_text(encoding="utf-8")
    qmake_fixture = rustflags_test.index('QMAKE="$fixture_root/no-qmake"')
    qmake_reset = rustflags_test.index("unset AR SYNVEIL_MSVC_AR MSYSTEM QMAKE")
    cargo_home_cases = rustflags_test.index("# Exercise explicit and default Windows Cargo homes.")
    require(rustflags_test.count("unset QMAKE") == 0 and
            qmake_fixture < cargo_home_cases < qmake_reset and
            "Keep Qt wrapper discovery disabled for every simulated Cargo-home case below." in rustflags_test,
            "Windows flag regression keeps Qt wrapper discovery isolated when run after Qt installation")
    require('"${VCToolsInstallDir:?VCToolsInstallDir is required for the MSVC path-map probe}"' in rustflags_test and
            "/bin/Hostx64/x64/cl.exe" in rustflags_test and
            "/nologo /FC /Od /c" in rustflags_test and
            "Keep debug metadata disabled" in rustflags_test and
            'PROBE_OBJECT="$probe_object" python3' in rustflags_test and
            'synveil-repro-msvc-pathmap.' in rustflags_test,
            "Windows path-map regression compiles and inspects an object with the active MSVC compiler")
    for runtime_rule in ("SYNVEIL_MSVC_CRT_DIR", "VCToolsRedistDir/x64/%s", "MSVCP140", "copy_missing_msvc_runtime_imports", "--qmldir", "platforms/qwindows.dll", "QmlImports=qml", "Qml2Imports=qml", "is_system_dll", "missing non-system import", "development directory leaked", 'rm -rf -- "$STAGING_DIR"'):
        require(runtime_rule in package, f"authoritative runtime rule: {runtime_rule}")
    require("--compiler-runtime" not in package and "--no-compiler-runtime" in package,
            "Windows Qt deployment must explicitly exclude the VC redistributable installer")
    require(package.rindex("copy_missing_msvc_runtime_imports") < package.index("# Audit every shipped PE's imports"),
            "authenticated CRT closure must be copied before the complete import audit")
    for required in ("SYNVEIL-MANIFEST.txt", "unmanifested package file", "0x8664", "platforms/qwindows.dll"):
        require(required in installed_test, f"installed runtime verification: {required}")
    for evidence in ("synthetic_standard_user", "administrator_member", "installer_elevated", "integrity_sid", "RegistryView]::Registry64", "RegistryView]::Registry32", "machine PATH changed", "Synveil service created", "Synveil scheduled task created", "PER_USER_ACL_FAILURE", "second uninstaller", "state_preservation"):
        require(evidence in per_user_test, f"P025 per-user evidence: {evidence}")
    for evidence in ("[Environment+SpecialFolder]::UserProfile", "LocalApplicationData", "ApplicationData",
                     "Resolve-ProfileScopedFolder", "$env:LOCALAPPDATA = $localAppData",
                     "$env:APPDATA = $roamingAppData", "$env:TEMP = $userTemp",
                     "[Environment+SpecialFolder]::Desktop", "Join-Path $profileRoot 'Desktop'",
                     "$desktop = Join-Path $desktopFolder 'Synveil.lnk'",
                     "PER_USER_PROFILE_FAILURE"):
        require(evidence in per_user_test, f"P025 standard-user profile environment: {evidence}")
    for evidence in ("SetPassword", "-Credential", "-LoadUserProfile", "WaitForExit(300000)", ".Delete('user'", "Remove-CimInstance", "::add-mask::"):
        require(evidence in per_user_invoker, f"P025 disposable-account harness: {evidence}")
    for evidence in ("LIFECYCLE_TIMEOUT_DIAGNOSTIC", "Win32_Thread", "wait_states=$waitSummary"):
        require(evidence in LIFECYCLE_TEST.read_text(encoding="utf-8"),
                f"P027 timeout diagnostics include process state: {evidence}")
    uninstaller_wait_start = lifecycle_test.index("function Wait-ForUninstallerSelfRemoval")
    uninstaller_wait_end = lifecycle_test.index("\n}", uninstaller_wait_start) + 2
    uninstaller_wait = lifecycle_test[uninstaller_wait_start:uninstaller_wait_end]
    require("AddSeconds(30)" in uninstaller_wait and
            "Start-Sleep -Milliseconds 100" in uninstaller_wait and
            "Get-ChildItem -LiteralPath $rootPath -Force" in uninstaller_wait and
            "registered uninstaller escaped the package root" in uninstaller_wait and
            "unexpected residual package entries" in uninstaller_wait and
            "registered uninstaller remains after bounded self-cleanup wait" in uninstaller_wait and
            "-Recurse" not in uninstaller_wait,
            "standard-user lifecycle waits boundedly for only the registered Inno uninstaller self-cleanup")
    for uninstaller_name in ("productionUninstaller", "fixtureUninstaller", "reinstalledUninstaller"):
        require(f"Wait-ForUninstallerSelfRemoval ${uninstaller_name}" in lifecycle_test,
                f"standard-user lifecycle checks bounded self-cleanup after {uninstaller_name}")
    account_set_info = per_user_invoker.index("$account.SetInfo()")
    account_created_flag = per_user_invoker.index("$accountCreated = $true")
    require("$accountCreationAttempted" in per_user_invoker and account_created_flag > account_set_info,
            "P025 cleanup records account persistence only after SetInfo")
    delete_account = per_user_invoker.index(".Delete('user',$user)")
    account_lookup = per_user_invoker.index("Get-CimInstance -ClassName Win32_UserAccount")
    account_identity_guard = per_user_invoker.index("$localAccount.SID -cne $sid")
    require(account_lookup < account_identity_guard < delete_account and
            "refusing to delete an account with an unexpected identity" in per_user_invoker,
            "P025 cleanup must verify the exact local account identity before deletion")
    failure_catch = per_user_invoker.index("} catch {", per_user_invoker.index("$stage = 'create_account'"))
    cleanup_finally = per_user_invoker.index("} finally {", failure_catch)
    primary_rethrow = per_user_invoker.index("if ($null -ne $primaryFailure)", cleanup_finally)
    require("windows-per-user-failure.json" in per_user_invoker and
            "STANDARD_USER_TEST_FAILURE stage=$stage" in per_user_invoker and
            "Record-CleanupFailure" in per_user_invoker and
            failure_catch < cleanup_finally < primary_rethrow,
            "P025 diagnostics must preserve the primary test error and record cleanup failures separately")
    for evidence in ("Inspect compiled Setup execution level", "requestedExecutionLevel", "asInvoker", "invoke-windows-standard-user-test.ps1", "windows-per-user-evidence"):
        require(evidence in workflow, f"P025 hosted workflow evidence: {evidence}")
    for evidence in (r"\Synveil\BackgroundClient\profile-", "InteractiveToken", "LeastPrivilege", "<LogonTrigger>",
                     "MultipleInstancesPolicy>IgnoreNew", "windows_task_xml_is_authoritative", "UnsafeState",
                     "GetSystemDirectoryW", "SHGetKnownFolderPath", "share_mode(1)",
                     "schtasks.exe", "create_new(true)", "file.sync_all()"):
        require(evidence in launch, f"P026 Task Scheduler authority: {evidence}")
    for forbidden in ("cmd.exe", "powershell.exe", "CurrentVersion\\Run"):
        require(forbidden.lower() not in launch.lower(), f"P026 forbidden startup mechanism: {forbidden}")
    for evidence in ("StartupPreference", "startup-preference.conf", "version=1", "state=enabled", "state=disabled"):
        require(evidence in config, f"P026 durable typed preference: {evidence}")
    for evidence in ("--startup-preference", "--cleanup-startup-integration", "existing_profile_id", "enable_autostart", "disable_autostart"):
        require(evidence in client_lib, f"P026 bounded runtime handoff: {evidence}")
    require("StartupPreferenceStore::current" in desktop_bridge and "apply_startup_choice" in desktop_bridge,
            "installer and Settings share startup preference authority")
    for evidence in ("/REPAIR accepts only 1", "RepairMode and FreshInstall", "CompareStrictVersion",
                     "Downgrade is not supported", "Silent same-version Setup requires /REPAIR=1",
                     "LoadTrustedPreviousManifest", "SynveilManifestSha256", "GetSHA256OfFile",
                     "RemoveProvenObsoleteFiles", "FileAttributeReparsePoint",
                     "--cleanup-startup-integration", "CurUninstallStepChanged"):
        require(evidence in iss, f"P027 installer lifecycle contract: {evidence}")
    current_manifest_lookup = iss[iss.index("function CurrentManifestOwns"):iss.index("procedure LoadCurrentManifestEntries")]
    current_manifest_loader = iss[iss.index("procedure LoadCurrentManifestEntries"):iss.index("procedure RemoveProvenObsoleteFiles")]
    obsolete_file_reconciliation = iss[iss.index("procedure RemoveProvenObsoleteFiles"):iss.index("function IsExplicitUpgradeSource")]
    require("CurrentManifestEntries.IndexOf(Relative)" in current_manifest_lookup and
            current_manifest_lookup.count("GetSHA256OfFile") == 1 and "LoadFromFile" not in current_manifest_lookup,
            "P027 ownership lookup verifies manifest identity and uses its authenticated in-memory index")
    require("CurrentManifestEntries.CaseSensitive := False" in current_manifest_loader and
            "CurrentManifestEntries.Sorted := True" in current_manifest_loader and
            current_manifest_loader.count("GetSHA256OfFile") == 2 and
            "duplicate file identities" in current_manifest_loader,
            "P027 target manifest is hash-verified around one strict, indexed load")
    require(obsolete_file_reconciliation.count("LoadCurrentManifestEntries") == 1 and
            "reconciliation progress" in obsolete_file_reconciliation and
            "changed during ownership reconciliation" in obsolete_file_reconciliation,
            "P027 repair reconciles the owned-file manifest in a bounded indexed pass")
    require("Root: HKCU64;" in iss, "trusted manifest identity writes to the 64-bit HKCU uninstall registry view")
    require("RegQueryStringValue(HKCU64," in iss, "recovery reads the 64-bit HKCU uninstall registry view")
    require("RegWriteStringValue(HKCU64, UninstallKey, 'SynveilManifestSha256'" in iss
            and "CommitInstalledManifestIdentity" in iss,
            "Setup must commit target ownership identity after installing payload")
    require("SetRegView(" not in iss, "installer does not call an unavailable registry-view function")
    require("DelTree(" not in iss and "{localappdata}\\Synveil" not in iss, "ordinary uninstall cannot recursively remove state")
    for evidence in ("Get-RegisteredUninstaller", "QuietUninstallString", "user-note.txt", "Snapshot-State",
                     "p027-obsolete-owned.txt", "downgrade-fixture", "reinstall-production",
                     "startup_preference_preserved", "authorized_scope"):
        require(evidence in lifecycle_test, f"P027 native lifecycle evidence: {evidence}")
    for evidence in ("Get-SetupProcessTree", "LIFECYCLE_TIMEOUT_PROCESS name=", "$process.Kill($true)",
                     "LIFECYCLE_TIMEOUT_RECOVERY", "LIFECYCLE_TIMEOUT_DIAGNOSTIC",
                     "process_tree_terminated=$treeTerminated"):
        require(evidence in lifecycle_test, f"P027 timeout cleanup retains child-process evidence: {evidence}")
    for evidence in ("Interrupt-UpgradeAfterOwnedPayloadCopy", "NewerLicenseHash",
                     "LIFECYCLE_INTERRUPTION_FAILURE", "process_termination='forced'",
                     "prior_manifest_and_registration_matched"):
        require(evidence in lifecycle_test, f"P044 Windows interruption evidence: {evidence}")
    require("P044_NEWER_LICENSE_HASH" in workflow,
            "P044 Windows interruption fixture does not bind its target payload identity")
    require("unins000.exe" not in lifecycle_test.lower(), "registered uninstaller must not be hard-coded")
    for evidence in ("PURGE_CLASSES", 'frozenset({"APPLICATION_CONFIG"})', "confirmed",
                     "USER_LIBRARY", "CREDENTIAL_STATE", "SERVER_DATABASE"):
        require(evidence in lifecycle_model, f"P027 separate bounded purge model: {evidence}")
    require("LifecycleFixtureVersion" in build and "if (!$env:CI)" in build and "if (!$LifecycleFixtureVersion)" in build,
            "test-only fixture cannot alter production version/release manifest")
    require("Assert-NoSecretLikeBytes" in build and "runtime-payload.bin" in build and
            "secret-like marker '$marker' in $Label" in build,
            "P044 fixture secret-marker scan remains present")
    fixture_secret_scan = build[build.index("function Test-FixtureSecretScanPath") :
                                build.index("function Assert-NoPrivateSourcePath")]
    extension_policy = re.search(r"\$textExtensions = @\(([^)]*)\)", fixture_secret_scan)
    require("$leaf -in @('synveil-desktop.exe','synveil-client.exe','LICENSE','NOTICE')" in fixture_secret_scan and
            extension_policy is not None and "'.dll'" not in extension_policy.group(1) and
            "private-key-marker-text" in workflow and "LifecycleFixtureVersion" in workflow and
            "BEGIN PRIVATE KEY" in workflow,
            "fixture scans first-party/text payloads and still rejects secret markers in text")
    private_path_scan = build[build.index("function Assert-NoPrivateSourcePath") :
                              build.index("function Get-WorkspaceVersion")]
    require("[Text.Encoding]::ASCII.GetString($Content)" in private_path_scan and
            "[Text.Encoding]::Unicode.GetString($Content)" in private_path_scan and
            "Assert-NoPrivateSourcePath $bytes $repo" in build and
            build.index("Assert-NoPrivateSourcePath $bytes $repo") > build.index("if (!$LifecycleFixtureVersion)"),
            "private source path scan must inspect every compiled Setup, including P044 fixtures")
    require("Build isolated lifecycle Setup fixtures" in workflow and "OlderFixtureSetup" in workflow,
            "hosted standard-user lifecycle execution")
    with tempfile.TemporaryDirectory() as directory:
        sample = Path(directory) / "sample"; sample.write_bytes(b"locked")
        verify_digest(sample, hashlib.sha256(b"locked").hexdigest())
        expect_failure(verify_digest, sample, "0" * 64)
        bad = Path(directory) / "bad.lock"; bad.write_text("{}", encoding="utf-8")
        expect_failure(parse_lock, bad)
    validate_names(["a.dll", "qml/Module/qmldir"])
    expect_failure(validate_names, ["../escape.dll"])
    expect_failure(validate_names, ["Qt6Core.dll", "qt6core.DLL"])
    expect_failure(validate_names, ["surprise.exe"])
    require(lock["sha256"] == "9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732", "reviewed upstream digest")
    print("windows installer contract: PASS")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, OSError, UnicodeError, json.JSONDecodeError) as error:
        print(f"windows installer contract: FAIL: {error}", file=sys.stderr)
        sys.exit(1)
