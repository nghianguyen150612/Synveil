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
    reproducible = REPRODUCIBLE.read_text(encoding="utf-8")
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
    for evidence in ("test-windows-installed-runtime.ps1", "QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "unrelatedCwd", "client probe", "missing-qwindows", "corrupt-dll", "unexpected-dll", "developer-file"):
        require(evidence in workflow or evidence in per_user_test, f"P024 hosted runtime evidence: {evidence}")
    require("VCToolsInstallDir" in workflow and "CompanyName" in workflow and "OriginalFilename" in workflow, "authenticated MSVC linker selection")
    require("LinkType" in workflow and "0x00004550" in workflow and "0x8664" in workflow, "regular AMD64 PE linker identity")
    require("$banner" not in workflow and "& $linker '/?'" not in workflow, "linker identity does not depend on localized help output")
    require("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER" in reproducible and '"${rustc_linker_args[@]}"' in reproducible, "direct rustc uses selected MSVC linker")
    require("CARGO_ENCODED_RUSTFLAGS" in reproducible and "$'\\x1f'" in reproducible, "lossless Cargo flag transport")
    require('rustc "${SYNVEIL_REPRODUCIBLE_RUSTC_FLAGS[@]}" "${rustc_linker_args[@]}"' in reproducible, "direct rustc receives discrete remaps")
    for runtime_rule in ("--compiler-runtime", "--qmldir", "platforms/qwindows.dll", "QmlImports=qml", "Qml2Imports=qml", "is_system_dll", "missing non-system import", "development directory leaked", 'rm -rf -- "$STAGING_DIR"'):
        require(runtime_rule in package, f"authoritative runtime rule: {runtime_rule}")
    for required in ("SYNVEIL-MANIFEST.txt", "unmanifested package file", "0x8664", "platforms/qwindows.dll"):
        require(required in installed_test, f"installed runtime verification: {required}")
    for evidence in ("synthetic_standard_user", "administrator_member", "installer_elevated", "integrity_sid", "RegistryView]::Registry64", "RegistryView]::Registry32", "machine PATH changed", "Synveil service created", "Synveil scheduled task created", "PER_USER_ACL_FAILURE", "second uninstaller", "state_preservation"):
        require(evidence in per_user_test, f"P025 per-user evidence: {evidence}")
    for evidence in ("SetPassword", "-Credential", "-LoadUserProfile", "WaitForExit(300000)", ".Delete('user'", "Remove-CimInstance", "::add-mask::"):
        require(evidence in per_user_invoker, f"P025 disposable-account harness: {evidence}")
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
    require("Root: HKCU64;" in iss, "trusted manifest identity writes to the 64-bit HKCU uninstall registry view")
    require("RegQueryStringValue(HKCU64," in iss, "recovery reads the 64-bit HKCU uninstall registry view")
    require("SetRegView(" not in iss, "installer does not call an unavailable registry-view function")
    require("DelTree(" not in iss and "{localappdata}\\Synveil" not in iss, "ordinary uninstall cannot recursively remove state")
    for evidence in ("Get-RegisteredUninstaller", "QuietUninstallString", "user-note.txt", "Snapshot-State",
                     "p027-obsolete-owned.txt", "downgrade-fixture", "reinstall-production",
                     "startup_preference_preserved", "authorized_scope"):
        require(evidence in lifecycle_test, f"P027 native lifecycle evidence: {evidence}")
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
