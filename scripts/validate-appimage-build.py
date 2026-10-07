#!/usr/bin/env python3
"""Validate P015 source contracts and, when supplied, a real AppDir/AppImage."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION_RE = re.compile(r'^version\s*=\s*"([^"]+)"', re.M)


def fail(message: str) -> None:
    raise RuntimeError(message)


def version() -> str:
    text = (ROOT / "Cargo.toml").read_text()
    section = text.split("[workspace.package]", 1)[1].split("[", 1)[0]
    match = VERSION_RE.search(section)
    if not match:
        fail("canonical workspace version is missing")
    return match.group(1)


def contains(root: Path, pattern: str) -> bool:
    return any(path.is_file() for path in root.glob(pattern))


def validate_appdir(appdir: Path) -> None:
    required = ["AppRun", "synveil.desktop", "synveil.svg", "usr/bin/synveil-desktop", "usr/bin/synveil-client", "usr/bin/synveil-appimage-integration"]
    for relative in required:
        path = appdir / relative
        if not path.is_file():
            fail(f"APPIMAGE-4–8: required AppDir file missing: {relative}")
    apprun = (appdir / "AppRun").read_text()
    desktop = (appdir / "synveil.desktop").read_text()
    if 'exec "$APPDIR/usr/bin/synveil-desktop" "$@"' not in apprun:
        fail("APPIMAGE-8: AppRun does not exec packaged desktop")
    if "Name=Synveil" not in desktop or "Exec=AppRun" not in desktop or "/usr/bin" in desktop:
        fail("APPIMAGE-6/8: non-portable desktop identity")
    forbidden = ("sudo", "systemctl", ".local/share/applications", "target/release", "CARGO_HOME")
    if any(token in apprun for token in forbidden):
        fail("APPIMAGE-9/16/17: AppRun contains a forbidden build/system integration token")
    libraries = [p.name for p in (appdir / "usr/lib").rglob("*.so*") if p.is_file()]
    for qt in ("Qt6Core", "Qt6Gui", "Qt6Network", "Qt6Qml", "Qt6Quick"):
        if not any(qt in name for name in libraries):
            fail(f"APPIMAGE-10: bundled {qt} library missing")
    if not contains(appdir, "usr/qml/**/qmldir"):
        fail("APPIMAGE-11: bundled QML module metadata missing")
    if not contains(appdir, "usr/plugins/platforms/libqxcb.so"):
        fail("APPIMAGE-12: bundled graphical Qt xcb platform plugin missing")


def run_smoke(artifact: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="synveil-appimage-smoke.") as work:
        base = Path(work)
        for name in ("config", "data", "cache", "runtime", "home"):
            (base / name).mkdir(mode=0o700)
        env = {k: v for k, v in os.environ.items() if k not in {
            "QML2_IMPORT_PATH", "QML_IMPORT_PATH", "QT_PLUGIN_PATH", "LD_LIBRARY_PATH", "CARGO_HOME", "CARGO_TARGET_DIR"
        }}
        env.update(HOME=str(base / "home"), XDG_CONFIG_HOME=str(base / "config"),
                   XDG_DATA_HOME=str(base / "data"), XDG_CACHE_HOME=str(base / "cache"),
                   XDG_RUNTIME_DIR=str(base / "runtime"), QT_QPA_PLATFORM="xcb",
                   QML_DISABLE_DISK_CACHE="1", APPIMAGE_EXTRACT_AND_RUN="1")
        # The production artifact deliberately ships xcb, not a developer-only
        # offscreen plugin. Exercise that exact bundled plugin on a disposable
        # display. This remains CI smoke, never native-clean-machine evidence.
        subprocess.run(["xvfb-run", "-a", "-s", "-screen 0 1280x800x24", "timeout", "20s",
                        str(artifact), "--qml-smoke-test"], cwd=base, env=env, check=True, timeout=35)


def inspect_artifact(artifact: Path, smoke: bool) -> None:
    if artifact.name != f"Synveil-{version()}-x86_64.AppImage":
        fail("APPIMAGE-1–3: artifact filename is not canonical/version-derived")
    if not artifact.is_file() or not artifact.stat().st_mode & stat.S_IXUSR:
        fail("artifact is absent or not executable")
    with tempfile.TemporaryDirectory(prefix="synveil-appimage-inspect.") as work:
        extract_env = dict(os.environ)
        extract_env.pop("APPIMAGE_EXTRACT_AND_RUN", None)
        subprocess.run([str(artifact), "--appimage-extract"], cwd=work,
                       env=extract_env, stdout=subprocess.DEVNULL, check=True)
        validate_appdir(Path(work) / "squashfs-root")
    if smoke:
        run_smoke(artifact)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--appdir", type=Path)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--smoke", action="store_true")
    args = parser.parse_args()
    # Static source contract always runs.
    for relative in ("deploy/packages/build-appimage.sh", "deploy/packages/appimage/AppRun",
                     "deploy/packages/appimage/synveil.desktop", "deploy/icons/hicolor/scalable/apps/synveil.svg"):
        if not (ROOT / relative).is_file():
            fail(f"static contract file missing: {relative}")
    builder = (ROOT / "deploy/packages/build-appimage.sh").read_text()
    for token in ("synveil_cargo_version", "SOURCE_DATE_EPOCH", "x86_64", "sha256sum --check", "cmp -s", "--custom-apprun"):
        if token not in builder:
            fail(f"static builder contract missing: {token}")
    if builder.count("--custom-apprun") < 2:
        fail("static builder contract must preserve the reviewed AppRun during deployment and output")
    print("APPIMAGE static/source contract: PASS")
    if args.appdir:
        validate_appdir(args.appdir.resolve()); print("AppDir inspection: PASS")
    if args.artifact:
        inspect_artifact(args.artifact.resolve(), args.smoke); print("AppImage inspection: PASS")
    else:
        print("AppImage inspection/runtime: SKIPPED (no --artifact supplied)")
    if args.manifest:
        data = json.loads(args.manifest.read_text())
        expected = next((a for a in data["artifacts"] if a["id"] == "appimage-linux-x86_64"), None)
        if not expected or not args.artifact:
            fail("APPIMAGE-20: manifest/artifact pair missing")
        digest = hashlib.sha256(args.artifact.read_bytes()).hexdigest()
        if expected["sha256"] != digest or expected["size_bytes"] != args.artifact.stat().st_size:
            fail("APPIMAGE-20: manifest identity does not match artifact bytes")
        print("Release manifest AppImage identity: PASS")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, subprocess.CalledProcessError, KeyError, json.JSONDecodeError) as error:
        print(f"validate-appimage-build: ERROR: {error}", file=sys.stderr)
        raise SystemExit(1)
