#!/usr/bin/env python3
"""Prompt013 static and produced-DEB contract validation."""

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    raise SystemExit(f"DEB UX validation failed: {message}")


def text(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def validate_source() -> None:
    desktop = text("deploy/applications/synveil.desktop")
    required = {
        "Type": "Application",
        "Name": "Synveil",
        "Exec": "/usr/bin/synveil-desktop",
        "Icon": "synveil",
        "Terminal": "false",
    }
    entries = dict(
        line.split("=", 1) for line in desktop.splitlines() if "=" in line
    )
    for key, value in required.items():
        if entries.get(key) != value:
            fail(f"desktop entry {key} must be {value!r}")
    if any(token in entries["Exec"] for token in ("sh ", "bash", "target/", "sudo")):
        fail("desktop Exec crosses the production/privilege boundary")

    manifest = text("deploy/install/MANIFEST")
    expected = {
        "BINARY_DESKTOP  /usr/bin/synveil-desktop  0755  root  root  PACKAGE",
        "deploy/applications/synveil.desktop  /usr/share/applications/synveil.desktop  0644  root  root  PACKAGE",
        "deploy/icons/hicolor/scalable/apps/synveil.svg  /usr/share/icons/hicolor/scalable/apps/synveil.svg  0644  root  root  PACKAGE",
        "deploy/systemd-user/synveil-client.service  /usr/lib/systemd/user/synveil-client.service  0644  root  root  PACKAGE",
    }
    for record in expected:
        if record not in manifest:
            fail(f"canonical manifest record missing: {record}")

    build = text("deploy/packages/build.sh")
    match = re.search(r'^DEB_DEPENDS="([^"]+)"$', build, re.MULTILINE)
    if not match:
        fail("DEB_DEPENDS declaration missing")
    dependencies = set(match.group(1).split(", "))
    runtime = {
        "dbus-user-session", "gnome-keyring", "libdbus-1-3", "libsystemd0",
        "libqt6core6", "libqt6gui6", "libqt6qml6", "libqt6quick6",
        "libqt6quickcontrols2-6", "qt6-qpa-plugins", "qml6-module-qtqml",
        "qml6-module-qtquick", "qml6-module-qtquick-controls",
        "qml6-module-qtquick-layouts", "qml6-module-qtquick-window",
    }
    missing = sorted(runtime - dependencies)
    if missing:
        fail(f"required runtime dependencies missing: {missing}")
    forbidden = ("-dev", "compiler", "cargo", "rustc", "build-essential", "pkg-config")
    bad = sorted(dep for dep in dependencies if any(x in dep for x in forbidden))
    if bad:
        fail(f"development dependencies declared at runtime: {bad}")

    hooks = "\n".join(text(f"deploy/packages/debian/{name}") for name in ("postinst", "prerm", "postrm"))
    executable = "\n".join(
        line for line in hooks.splitlines() if line.strip() and not line.lstrip().startswith("#")
    )
    forbidden_hooks = {
        "desktop launch": r"(^|[;&|]\s*)synveil-desktop\b",
        "user service activation": r"systemctl\s+--user\s+(enable|start|restart|preset)",
        "autostart mutation": r"\.config/autostart|/etc/xdg/autostart",
        "home guessing": r"\$HOME|/home/",
    }
    for label, pattern in forbidden_hooks.items():
        if re.search(pattern, executable, re.MULTILINE):
            fail(f"maintainer hooks contain forbidden {label}")


def validate_workflow() -> None:
    workflow = text(".github/workflows/linux-packages.yml")
    producer_name = "      - name: Reproducible DEB and RPM rebuilds"
    consumer_name = "      - name: Validate release artifact provenance and metadata"
    producer = workflow.find(producer_name)
    consumer = workflow.find(consumer_name)
    if producer < 0 or consumer < 0 or producer >= consumer:
        fail("P013-CI-ORDER-1: real reproducible producer must precede provenance consumer")
    producer_block = workflow[producer:consumer]
    next_step = workflow.find("      - name:", consumer + len(consumer_name))
    consumer_block = workflow[consumer: next_step if next_step >= 0 else len(workflow)]
    if "./deploy/packages/build.sh --format=all" not in producer_block:
        fail("reproducible producer is not the canonical build.sh invocation")
    output = re.search(r"--output-dir=(target/[A-Za-z0-9_-]+)", producer_block)
    if not output:
        fail("reproducible producer output root is not explicit")
    root = output.group(1)
    required_outputs = (
        f"{root}/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt",
        f"{root}/SYNVEIL-RELEASE-MANIFEST.json",
    )
    for path in required_outputs:
        if path not in producer_block or path not in consumer_block:
            fail(f"P013-CI-ORDER-2: producer/consumer do not share {path}")
    if re.search(r"\b(touch|cp)\b.*packages-reproducible", producer_block):
        fail("reproducible evidence may not be synthesized")


def dpkg(deb: pathlib.Path, *arguments: str) -> str:
    if not arguments:
        fail("dpkg-deb operation missing")
    result = subprocess.run(
        ["dpkg-deb", arguments[0], str(deb), *arguments[1:]],
        text=True,
        capture_output=True,
    )
    if result.returncode:
        fail(f"dpkg-deb {' '.join(arguments)} failed: {result.stderr.strip()}")
    return result.stdout


def validate_deb(deb: pathlib.Path) -> None:
    if not deb.is_file():
        fail(f"DEB does not exist: {deb}")
    package = dpkg(deb, "--field", "Package").strip()
    version = dpkg(deb, "--field", "Version").strip()
    arch = dpkg(deb, "--field", "Architecture").strip()
    if package != "synveil" or arch != "amd64":
        fail(f"unexpected identity: package={package!r}, architecture={arch!r}")
    expected_name = f"synveil_{version}_amd64.deb"
    if deb.name != expected_name:
        fail(f"canonical filename is {expected_name}, got {deb.name}")
    listing = dpkg(deb, "--contents")
    expected_modes = {
        "./usr/bin/synveil-desktop": "-rwxr-xr-x",
        "./usr/bin/synveil-client": "-rwxr-xr-x",
        "./usr/lib/systemd/user/synveil-client.service": "-rw-r--r--",
        "./usr/share/applications/synveil.desktop": "-rw-r--r--",
        "./usr/share/icons/hicolor/scalable/apps/synveil.svg": "-rw-r--r--",
    }
    found = {}
    for line in listing.splitlines():
        fields = line.split()
        if len(fields) >= 6:
            found[fields[-1]] = (fields[0], fields[1])
    for path, mode in expected_modes.items():
        if path not in found:
            fail(f"payload path missing: {path}")
        actual_mode, owner = found[path]
        if actual_mode != mode or owner != "root/root":
            fail(f"payload policy mismatch for {path}: {actual_mode} {owner}")
    dependencies = dpkg(deb, "--field", "Depends")
    if "-dev" in dependencies:
        fail("built DEB includes a development dependency")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deb", type=pathlib.Path)
    args = parser.parse_args()
    validate_source()
    validate_workflow()
    if args.deb:
        validate_deb(args.deb.resolve())
    print("Prompt013 Debian package UX validation: PASS")


if __name__ == "__main__":
    main()
