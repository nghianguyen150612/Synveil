#!/usr/bin/env python3
"""Prompt014 static contracts and optional produced-RPM validation."""

import argparse
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    raise SystemExit(f"RPM UX validation failed: {message}")


def text(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def rpm_query(rpm: pathlib.Path, query: str, *extra: str) -> str:
    result = subprocess.run(
        ["rpm", "-qp", *extra, "--queryformat", query, str(rpm)],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        fail(f"rpm query failed: {result.stderr.strip()}")
    return result.stdout


def validate_source() -> None:
    spec = text("deploy/packages/rpm/synveil.spec.tmpl")
    required_headers = {
        "Name": "synveil",
        "BuildArch": "@SYNVEIL_ARCH@",
        "License": "MIT",
    }
    for header, expected in required_headers.items():
        match = re.search(rf"^{header}:\s*(.+)$", spec, re.MULTILINE)
        if not match or match.group(1).strip() != expected:
            fail(f"RPM-UX metadata {header} must be {expected!r}")
    for header in ("Version", "Release", "Summary", "Requires"):
        if not re.search(rf"^{header}:\s*\S", spec, re.MULTILINE):
            fail(f"RPM-UX metadata {header} is missing")

    install = spec[spec.index("%install"):spec.index("%files")]
    if "set -euo pipefail" in install or "set -eu" not in install:
        fail("RPM-UX-19: %install must use portable /bin/sh fail-fast semantics")

    requires = re.search(r"^Requires:\s*(.+)$", spec, re.MULTILINE)
    dependencies = {item.strip() for item in requires.group(1).split(",")}
    runtime = {
        "systemd", "systemd-libs", "glibc", "libgcc", "libstdc++",
        "dbus-daemon", "dbus-libs", "gnome-keyring", "qt6-qtbase",
        "qt6-qtbase-gui", "qt6-qtdeclarative", "qt6-qtquickcontrols2",
    }
    missing = sorted(runtime - dependencies)
    if missing:
        fail(f"RPM-UX-6: required Fedora runtime dependencies missing: {missing}")
    forbidden = ("-devel", "cargo", "rust", "compiler", "postgresql-server", "docker", "nginx")
    leaked = sorted(dep for dep in dependencies if any(word in dep.lower() for word in forbidden))
    if leaked:
        fail(f"RPM-UX-7: development/server dependencies leaked: {leaked}")

    desktop = text("deploy/applications/synveil.desktop")
    entries = dict(line.split("=", 1) for line in desktop.splitlines() if "=" in line)
    expected_desktop = {
        "Type": "Application", "Name": "Synveil",
        "Exec": "/usr/bin/synveil-desktop", "Icon": "synveil", "Terminal": "false",
    }
    for key, value in expected_desktop.items():
        if entries.get(key) != value:
            fail(f"RPM-UX desktop entry {key} must be {value!r}")

    required_files = {
        "/usr/bin/synveil-client", "/usr/bin/synveil-desktop",
        "/usr/bin/synveil-scheduled-maintenance-once",
        "/usr/lib/systemd/user/synveil-client.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer",
        "/usr/lib/sysusers.d/synveil.conf", "/usr/lib/tmpfiles.d/synveil.conf",
        "/usr/share/applications/synveil.desktop",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
        "/usr/share/doc/synveil/LICENSE", "/usr/share/doc/synveil/NOTICE",
    }
    for path in required_files:
        if path not in spec:
            fail(f"RPM-UX payload declaration missing: {path}")
    if "/usr/lib/systemd/system/synveil-client.service" in spec:
        fail("RPM-UX-8: client service must not be a system unit")

    scriptlets = spec[spec.index("%post"):spec.index("%changelog")]
    executable = "\n".join(
        line for line in scriptlets.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    )
    forbidden_scriptlets = {
        "desktop launch": r"(^|[;&|]\s*)synveil-desktop\b",
        "user service activation": r"systemctl\s+--user\s+(enable|start|restart|preset)",
        "client launch": r"(^|[;&|]\s*)synveil-client\b",
        "autostart mutation": r"\.config/autostart|/etc/xdg/autostart",
        "home guessing": r"\$HOME|\$USER|\$LOGNAME|/home/",
        "network access": r"\b(curl|wget)\b",
    }
    for label, pattern in forbidden_scriptlets.items():
        if re.search(pattern, executable, re.MULTILINE):
            fail(f"RPM-UX-9/10/11: scriptlets contain forbidden {label}")

    workflow = text(".github/workflows/linux-packages.yml")
    required_workflow = (
        "Prompt014 RPM UX source contract", "fedora:42",
        "DNF install exact RPM", "Installed non-root desktop smoke",
        "RPM erase and durable-state preservation",
    )
    for marker in required_workflow:
        if marker not in workflow:
            fail(f"hosted Fedora acceptance marker missing: {marker}")


def validate_rpm(rpm: pathlib.Path) -> None:
    if not rpm.is_file():
        fail(f"RPM does not exist: {rpm}")
    identity = rpm_query(rpm, "%{NAME}|%{VERSION}|%{RELEASE}|%{ARCH}|%{SUMMARY}|%{LICENSE}").split("|")
    if len(identity) != 6 or identity[0] != "synveil" or identity[3] != "x86_64":
        fail(f"RPM-UX-1/2: unexpected RPM identity: {identity}")
    if not all(identity[index].strip() for index in (1, 2, 4, 5)):
        fail("RPM metadata contains an empty required field")

    listing = rpm_query(
        rpm,
        "[%{FILENAMES}|%{FILEMODES:perms}|%{FILEUSERNAME}|%{FILEGROUPNAME}\\n]",
    )
    found = {}
    for line in listing.splitlines():
        fields = line.split("|")
        if len(fields) == 4:
            found[fields[0]] = tuple(fields[1:])
    expected_modes = {
        "/usr/bin/synveil-client": "-rwxr-xr-x",
        "/usr/bin/synveil-desktop": "-rwxr-xr-x",
        "/usr/bin/synveil-scheduled-maintenance-once": "-rwxr-xr-x",
        "/usr/lib/systemd/user/synveil-client.service": "-rw-r--r--",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.service": "-rw-r--r--",
        "/usr/lib/systemd/system/synveil-scheduled-maintenance.timer": "-rw-r--r--",
        "/usr/lib/sysusers.d/synveil.conf": "-rw-r--r--",
        "/usr/lib/tmpfiles.d/synveil.conf": "-rw-r--r--",
        "/usr/share/applications/synveil.desktop": "-rw-r--r--",
        "/usr/share/icons/hicolor/scalable/apps/synveil.svg": "-rw-r--r--",
        "/usr/share/doc/synveil/LICENSE": "-rw-r--r--",
        "/usr/share/doc/synveil/NOTICE": "-rw-r--r--",
    }
    for path, mode in expected_modes.items():
        actual = found.get(path)
        if actual != (mode, "root", "root"):
            fail(f"RPM-UX-12: {path} policy is {actual}, expected {(mode, 'root', 'root')}")

    requires = set(rpm_query(rpm, "[%{REQUIRENAME}\\n]").splitlines())
    for dependency in ("dbus-daemon", "gnome-keyring", "qt6-qtbase-gui", "qt6-qtdeclarative"):
        if dependency not in requires:
            fail(f"built RPM missing runtime requirement: {dependency}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rpm", type=pathlib.Path)
    args = parser.parse_args()
    validate_source()
    if args.rpm:
        validate_rpm(args.rpm.resolve())
    print("Prompt014 Fedora RPM package UX validation: PASS")


if __name__ == "__main__":
    main()
