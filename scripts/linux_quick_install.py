#!/usr/bin/env python3
"""Verified P017 acquisition and native Linux package installation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urljoin

import release_channel
import release_download

EXIT_USAGE = 2
EXIT_UNSUPPORTED = 10
EXIT_INTEGRITY = 20
EXIT_DOWNLOAD = 30
EXIT_AUTHORIZATION = 40
EXIT_PACKAGE_MANAGER = 50
EXIT_VERIFICATION = 60
EXIT_UNKNOWN = 70
EXIT_INTERNAL = 80


class QuickInstallError(RuntimeError):
    def __init__(self, category: str, message: str, action: str, exit_status: int):
        super().__init__(message)
        self.category = category
        self.action = action
        self.exit_status = exit_status


@dataclass(frozen=True)
class Profile:
    name: str
    artifact_type: str
    manager: str
    distro_ids: frozenset[str]


PROFILES = {
    "debian-x86_64": Profile("debian-x86_64", "deb", "APT", frozenset({"debian", "ubuntu"})),
    "fedora-x86_64": Profile("fedora-x86_64", "rpm", "DNF", frozenset({"fedora"})),
}
REQUIRED_PATHS = (
    "/usr/bin/synveil-desktop",
    "/usr/bin/synveil-client",
    "/usr/share/applications/synveil.desktop",
    "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
    "/usr/lib/systemd/user/synveil-client.service",
)


def resolve_profile(name: str, *, machine: str | None = None, os_release: Path = Path("/etc/os-release")) -> Profile:
    """Validate an explicit profile; never choose one from the host."""
    if name not in PROFILES:
        raise QuickInstallError("UnsupportedPlatform", "The explicit platform profile is unsupported.",
                                "Choose one of: debian-x86_64, fedora-x86_64.", EXIT_UNSUPPORTED)
    if (machine or platform.machine()).lower() not in {"x86_64", "amd64"}:
        raise QuickInstallError("UnsupportedPlatform", "The requested profile requires x86_64.",
                                "Use this installer only on a qualified x86_64 host.", EXIT_UNSUPPORTED)
    ids: set[str] = set()
    try:
        for line in os_release.read_text(encoding="utf-8").splitlines():
            key, separator, value = line.partition("=")
            if separator and key in {"ID", "ID_LIKE"}:
                ids.update(value.strip().strip('"').lower().split())
    except OSError as error:
        raise QuickInstallError("UnsupportedPlatform", "The host distribution could not be verified.",
                                "Run on the distribution named by the explicit profile.", EXIT_UNSUPPORTED) from error
    profile = PROFILES[name]
    if not ids.intersection(profile.distro_ids):
        raise QuickInstallError("UnsupportedPlatform", "The host does not match the explicit platform profile.",
                                "Select the correct qualified profile; no fallback was attempted.", EXIT_UNSUPPORTED)
    return profile


class NativeManager:
    def __init__(self, profile: Profile):
        self.profile = profile

    def _run(self, command: list[str], *, mutate: bool = False) -> subprocess.CompletedProcess[str]:
        if mutate and os.geteuid() != 0:
            command = ["sudo", *command]
        return subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)

    def installed_version(self) -> str | None:
        command = (["dpkg-query", "-W", "-f=${Status}\t${Version}", "synveil"]
                   if self.profile.artifact_type == "deb" else ["rpm", "-q", "--qf", "%{VERSION}", "synveil"])
        result = self._run(command)
        if result.returncode:
            return None
        value = result.stdout.strip()
        if self.profile.artifact_type == "deb":
            prefix = "install ok installed\t"
            return value[len(prefix):].split("-")[0] if value.startswith(prefix) else None
        return value

    def install(self, package: Path, expected_version: str) -> None:
        command = (["apt-get", "install", "-y", str(package)] if self.profile.artifact_type == "deb"
                   else ["dnf", "install", "-y", str(package)])
        try:
            result = self._run(command, mutate=True)
        except KeyboardInterrupt as error:
            raise QuickInstallError("OutcomeUnknown", "The package transaction was interrupted and its outcome is unknown.",
                                    "Inspect native package state before retrying.", EXIT_UNKNOWN) from error
        combined = (result.stdout + "\n" + result.stderr).lower()
        if result.returncode == 0:
            return
        if self.verify(expected_version):
            return
        installed = self.installed_version()
        payload_present = any(Path(item).exists() for item in REQUIRED_PATHS)
        if installed is not None or payload_present:
            raise QuickInstallError("OutcomeUnknown", "The failed package transaction left native state that is not fully verified.",
                                    "Inspect or repair native package state before retrying.", EXIT_UNKNOWN)
        if any(marker in combined for marker in ("could not get lock", "another app is currently holding", "waiting for process with pid")):
            raise QuickInstallError("PackageManagerBusy", "Another native package operation is running; no lock was removed.",
                                    "Wait for it to finish, then retry.", EXIT_PACKAGE_MANAGER)
        if result.returncode in {126, 127} or "not in the sudoers" in combined or "a password is required" in combined:
            raise QuickInstallError("AuthorizationFailed", "Administrator authorization was not granted.",
                                    "Retry and complete the visible sudo prompt.", EXIT_AUTHORIZATION)
        raise QuickInstallError("InstallationFailed", "The native package manager did not complete successfully.",
                                "Inspect native package state before retrying.", EXIT_PACKAGE_MANAGER)

    def verify(self, expected_version: str) -> bool:
        return self.installed_version() == expected_version and all(Path(item).is_file() for item in REQUIRED_PATHS)


def sha256_file(path: Path) -> tuple[int, str]:
    digest, size = hashlib.sha256(), 0
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def require_staged_evidence(evidence: dict) -> None:
    size, digest = sha256_file(Path(evidence["final_path"]))
    if size != evidence["artifact_size"] or digest != evidence["artifact_sha256"]:
        raise QuickInstallError("IntegrityVerificationFailed", "The verified staged package changed before installation.",
                                "Discard it and restart from trusted metadata.", EXIT_INTEGRITY)


def print_plan(evidence: dict, profile: Profile) -> None:
    print(f"Synveil {evidence['product_version']}")
    print(f"Package: {profile.artifact_type.upper()}")
    print("Architecture: x86_64")
    print("Verified release identity: yes")
    print(f"Package manager: {profile.manager}")
    print("User data will be preserved")
    print("Administrator authorization will be requested")


def acquire(args: argparse.Namespace, profile: Profile, staging: Path) -> dict:
    origins = frozenset(args.trusted_origin)
    channel_raw = release_download.fetch_https(args.channel_url, origins, release_channel.MAX_CHANNEL_BYTES)
    channel_policy = release_channel.ChannelTrustPolicy(expected_channel_sha256=args.trusted_channel_sha256,
                                                        minimum_generation=args.minimum_channel_generation)
    channel_auth = release_channel.authenticate_channel(channel_raw, channel_policy)
    channel = release_channel.parse_authenticated_channel(channel_raw, channel_auth, channel_policy)
    selected_result = release_channel.select_fresh_install(channel, None)
    if selected_result.selected is None:
        raise release_channel.ChannelError("NO_RELEASE_AVAILABLE", "authenticated channel has no fresh-install release")
    selected_release = selected_result.selected
    release = release_channel.validate_selected_release(selected_release)
    manifest_url = urljoin(args.channel_url, release["manifest_filename"])
    manifest_raw = release_download.fetch_https(manifest_url, origins, release_download.MAX_MANIFEST_BYTES)
    manifest = release_channel.authenticate_selected_manifest(selected_release, manifest_raw)
    selection = release_download.select_artifact(manifest, platform="linux", architecture="x86_64",
                                                 artifact_type=profile.artifact_type, role="native_package",
                                                 expected_version=release["product_version"])
    policy = release_download.ReleaseTrustPolicy(origins, origins,
                                                 expected_manifest_sha256=release["manifest_sha256"])
    base_url = urljoin(manifest_url, ".")
    return release_download.download_artifact(base_url, staging, selection, policy)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--platform-profile", required=True, choices=sorted(PROFILES))
    result.add_argument("--channel-url", required=True)
    result.add_argument("--trusted-channel-sha256", required=True)
    result.add_argument("--trusted-origin", action="append", required=True)
    result.add_argument("--minimum-channel-generation", type=int, required=True)
    result.add_argument("--yes", action="store_true", help="consent noninteractively after the plan is displayed")
    return result


def run(argv: list[str] | None = None, *, manager_factory=NativeManager) -> int:
    args = parser().parse_args(argv)
    staging: Path | None = None
    mutation_started = False
    preserve_staging = False
    try:
        if os.geteuid() == 0:
            raise QuickInstallError("AuthorizationRequired", "The whole installer must not run as root.",
                                    "Run as your ordinary user; sudo is requested only for installation.", EXIT_AUTHORIZATION)
        profile = resolve_profile(args.platform_profile)
        base = os.environ.get("XDG_RUNTIME_DIR")
        if base and Path(base).is_dir() and Path(base).stat().st_uid == os.getuid():
            staging = Path(tempfile.mkdtemp(prefix="synveil-install-", dir=base))
        else:
            staging = Path(tempfile.mkdtemp(prefix="synveil-install-"))
        os.chmod(staging, 0o700)
        evidence = acquire(args, profile, staging)
        manager = manager_factory(profile)
        installed = manager.installed_version()
        if installed == evidence["product_version"] and manager.verify(evidence["product_version"]):
            print(json.dumps({**evidence, "installation_result": "ALREADY_INSTALLED_VERIFIED"}, sort_keys=True))
            return 0
        if installed is not None:
            raise QuickInstallError("InstallationFailed", "A different or incomplete Synveil installation is present.",
                                    "Use the P009-compatible native upgrade or repair workflow.", EXIT_PACKAGE_MANAGER)
        print_plan(evidence, profile)
        if not args.yes:
            if not sys.stdin.isatty() or input("Install this verified package? [y/N] ").strip().lower() not in {"y", "yes"}:
                print("Installation cancelled; no package change was made.")
                return 0
        require_staged_evidence(evidence)
        mutation_started = True
        manager.install(Path(evidence["final_path"]), evidence["product_version"])
        if not manager.verify(evidence["product_version"]):
            raise QuickInstallError("VerificationFailed", "Native installation completed but required state was not verified.",
                                    "Inspect native package state and use the documented repair path.", EXIT_VERIFICATION)
        print(json.dumps({**evidence, "installation_result": "INSTALLED_VERIFIED"}, sort_keys=True))
        print("Synveil installed successfully.")
        print("Open Synveil from your application menu.")
        return 0
    except (release_channel.ChannelError, release_download.AcquisitionError) as error:
        network_codes = {"NETWORK_ERROR"}
        category = "DownloadFailed" if error.code in network_codes else "IntegrityVerificationFailed"
        status = EXIT_DOWNLOAD if error.code in network_codes else EXIT_INTEGRITY
        print(f"{category}: Trusted release acquisition failed ({error.code}).", file=sys.stderr)
        print("No package change was made. Check trusted bootstrap inputs and retry.", file=sys.stderr)
        return status
    except QuickInstallError as error:
        preserve_staging = error.category == "OutcomeUnknown"
        print(f"{error.category}: {error}", file=sys.stderr)
        print(("Package state may have changed. " if mutation_started else "No package change was made. ") + error.action,
              file=sys.stderr)
        return error.exit_status
    except (OSError, ValueError) as error:
        print("InternalFailure: The installer could not complete safely.", file=sys.stderr)
        print("No package change was assumed. Review bounded diagnostics and retry.", file=sys.stderr)
        return EXIT_INTERNAL
    finally:
        if staging is not None and not preserve_staging:
            shutil.rmtree(staging, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(run())
