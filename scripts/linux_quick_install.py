#!/usr/bin/env python3
"""Verified P017 acquisition and native Linux package installation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import re
import stat
import fcntl
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urljoin

import release_channel
import release_download
import linux_platform_detection

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


PROFILES = {
    "debian-x86_64": Profile("debian-x86_64", "deb", "APT"),
    "fedora-x86_64": Profile("fedora-x86_64", "rpm", "DNF"),
}
REQUIRED_PATHS = (
    "/usr/bin/synveil-desktop",
    "/usr/bin/synveil-client",
    "/usr/share/applications/synveil.desktop",
    "/usr/share/icons/hicolor/scalable/apps/synveil.svg",
    "/usr/lib/systemd/user/synveil-client.service",
)


def resolve_profile(asserted_name: str | None, detection: linux_platform_detection.DetectionResult) -> Profile:
    """Resolve a qualified detection; an explicit profile is only an assertion."""
    if detection.qualification_status != "QUALIFIED" or detection.profile not in PROFILES:
        if detection.qualification_status == "UNSUPPORTED_ARCHITECTURE":
            message = (f"Synveil detected architecture {detection.architecture}; "
                       "the v0.2 Linux installer currently qualifies x86_64 only.")
        elif detection.os_id and detection.version_id:
            message = (f"Synveil detected {detection.os_id} {detection.version_id} on "
                       f"{detection.architecture}, but this exact environment is not qualified.")
        else:
            message = "Synveil could not safely identify this Linux environment."
        raise QuickInstallError(detection.qualification_status, message,
                                f"Qualification stopped safely ({detection.reason_code}).", EXIT_UNSUPPORTED)
    if asserted_name is not None and asserted_name != detection.profile:
        raise QuickInstallError("UnsupportedPlatform", "The asserted platform profile does not match the qualified host.",
                                "Remove the assertion or use the matching profile; it cannot override host truth.",
                                EXIT_UNSUPPORTED)
    return PROFILES[detection.profile]


class NativeManager:
    def __init__(self, profile: Profile):
        self.profile = profile

    def _run(self, command: list[str], *, mutate: bool = False) -> subprocess.CompletedProcess[str]:
        command = [system_executable(command[0]), *command[1:]]
        if mutate:
            if os.geteuid() == 0:
                raise QuickInstallError("AuthorizationRequired", "The installer must run as an ordinary user.",
                                        "Use visible native package authorization.", EXIT_AUTHORIZATION)
            # Never preserve caller environment through sudo. The absolute manager
            # and a small OS-only PATH are also used before crossing that boundary.
            command = [system_executable("sudo"), "--", *command]
        environment = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LANG": "C", "LC_ALL": "C"}
        return subprocess.run(command, env=environment, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)

    def installed_version(self) -> str | None:
        command = (["dpkg-query", "-W", "-f=${Status}\t${Version}", "synveil"]
                   if self.profile.artifact_type == "deb" else ["rpm", "-q", "--qf", "%{VERSION}", "synveil"])
        result = self._run(command)
        if result.returncode == 1 and (not result.stdout.strip() if self.profile.artifact_type == "deb"
                                      else result.stdout.strip() == "package synveil is not installed"):
            return None  # native query's documented package-not-present result
        if result.returncode:
            raise QuickInstallError("OutcomeUnknown", "Native package identity could not be inspected.",
                                    "Reconcile native package state before installation.", EXIT_UNKNOWN)
        value = result.stdout.strip()
        if self.profile.artifact_type == "deb":
            prefix = "install ok installed\t"
            if not value.startswith(prefix):
                raise QuickInstallError("OutcomeUnknown", "Native package state is incomplete.",
                                        "Reconcile native package state before installation.", EXIT_UNKNOWN)
            value = value[len(prefix):]
        return native_product_version(value)

    def install(self, package: Path, expected_version: str) -> None:
        if not package.is_absolute() or package.name.startswith("-"):
            raise QuickInstallError("IntegrityVerificationFailed", "Unsafe package argument.",
                                    "Use the verified private stage.", EXIT_INTEGRITY)
        # The production adapter requires the P006 identity bridge, not a caller's
        # pathname alone. Rehash again at its last-use boundary.
        evidence = getattr(self, "artifact_evidence", None)
        if evidence is None or Path(evidence["final_path"]) != package:
            raise QuickInstallError("IntegrityVerificationFailed", "Verified package identity is required.",
                                    "Acquire trusted package evidence first.", EXIT_INTEGRITY)
        require_staged_evidence(evidence)
        command = (["apt-get", "install", "-y", "--", str(package)] if self.profile.artifact_type == "deb"
                   else ["dnf", "install", "-y", "--", str(package)])
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
        if self.installed_version() != expected_version or not all(Path(item).is_file() and not Path(item).is_symlink() for item in REQUIRED_PATHS):
            return False
        # Package database remains authority for the installed payload. Presence
        # and a version label alone do not prove its bytes survived mutation.
        command = ["dpkg", "--verify", "synveil"] if self.profile.artifact_type == "deb" else ["rpm", "-V", "synveil"]
        result = self._run(command)
        return result.returncode == 0 and not result.stdout.strip()


def sha256_file(path: Path) -> tuple[int, str]:
    digest, size = hashlib.sha256(), 0
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def require_staged_evidence(evidence: dict) -> None:
    path = Path(evidence["final_path"])
    release_download.require_safe_ancestry(path.parent)
    if (not path.is_absolute() or not release_download.release_manifest.safe_filename(path.name)
            or not release_download._matches(path, evidence["artifact_size"], evidence["artifact_sha256"])):
        raise QuickInstallError("IntegrityVerificationFailed", "The verified staged package changed before installation.",
                                "Discard it and restart from trusted metadata.", EXIT_INTEGRITY)


def native_product_version(value: str) -> str:
    match = re.fullmatch(r"((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))(?:-[1-9][0-9]*)?", value)
    if match is None:
        raise QuickInstallError("OutcomeUnknown", "Native package version is not supported.",
                                "Use a compatible lifecycle owner; do not reset native state.", EXIT_UNKNOWN)
    release_channel.parse_stable_version(match[1])
    return match[1]


def system_executable(name: str) -> str:
    if name not in {"sudo", "apt-get", "dpkg", "dpkg-query", "dnf", "rpm"}:
        raise ValueError("unapproved native executable")
    # /bin and /sbin may be usr-merge links; canonical OS-owned regular binaries
    # are accepted, while caller PATH and /usr/local never participate.
    for directory in ("/usr/bin", "/usr/sbin", "/bin", "/sbin"):
        candidate = Path(directory, name).resolve()
        try:
            info = candidate.stat()
            if (stat.S_ISREG(info.st_mode) and info.st_uid == 0 and not info.st_mode & 0o022
                    and all(parent.stat().st_uid == 0 and not parent.stat().st_mode & 0o022 for parent in candidate.parents)
                    and os.access(candidate, os.X_OK)):
                return str(candidate)
        except OSError:
            continue
    raise QuickInstallError("UnsupportedPlatform", "A trusted native package tool is unavailable.",
                            "Use the qualified platform's native package installation.", EXIT_UNSUPPORTED)


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
    descriptor, verify = None, None
    if args.trust_policy:
        import release_signature
        keys = release_signature.load_production_keys(args.trust_policy)
        descriptor_url = urljoin(args.channel_url, release_channel.CHANNEL_AUTH_FILENAME)
        descriptor = release_channel.validate_auth_descriptor(json.loads(
            release_download.fetch_https(descriptor_url, origins, release_channel.MAX_CHANNEL_BYTES)))
        signatures = {}
        for entry in descriptor["signatures"]:
            if entry["scheme"] == release_signature.SCHEME and entry["key_id"] in keys:
                signatures[entry["signature_filename"]] = release_download.fetch_https(
                    urljoin(descriptor_url, entry["signature_filename"]), origins, 64)
        verify = release_signature.verifier(keys, signatures)
        channel_policy = release_channel.ChannelTrustPolicy(authentication_method="detached_signature",
                        trusted_key_ids=frozenset(keys), allowed_signature_schemes=frozenset({release_signature.SCHEME}),
                        minimum_generation=args.minimum_channel_generation)
    channel_auth = release_channel.authenticate_channel(channel_raw, channel_policy, descriptor=descriptor, verifier=verify)
    channel = release_channel.parse_authenticated_channel(channel_raw, channel_auth, channel_policy)
    # Caller-owned persistence, with P011 remaining the only rollback authority.
    stored = accept_channel_high_water(channel)
    selected_result = release_channel.select_fresh_install(channel, stored)
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


def accept_channel_high_water(channel: release_channel.AuthenticatedChannel) -> release_channel.ChannelHighWater:
    base = Path(os.environ.get("XDG_STATE_HOME", str(Path.home() / ".local/state")))
    if not base.is_absolute():
        raise release_channel.ChannelError("INVALID_RELEASE_POLICY", "installer state root must be absolute")
    return persist_channel_high_water(base / "synveil/installer", channel)


def persist_channel_high_water(root: Path, channel: release_channel.AuthenticatedChannel) -> release_channel.ChannelHighWater:
    release_download.require_safe_ancestry(root)
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    release_download.require_safe_ancestry(root)
    info = root.stat()
    if info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise release_channel.ChannelError("INVALID_RELEASE_POLICY", "installer state must be private")
    flags = os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK
    fd = os.open(root / "channel-stable.lock", flags, 0o600)
    with os.fdopen(fd, "r+b") as lock:
        info = os.fstat(lock.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077 or info.st_nlink != 1:
            raise release_channel.ChannelError("INVALID_RELEASE_POLICY", "ambiguous channel state lock")
        fcntl.flock(lock, fcntl.LOCK_EX)
        state = root / "channel-stable.json"
        stored = None
        if state.exists() or state.is_symlink():
            info = state.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                    or info.st_mode & 0o077 or info.st_size > 512 or info.st_nlink != 1):
                raise release_channel.ChannelError("INVALID_RELEASE_POLICY", "ambiguous channel state")
            state_fd = os.open(state, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(state_fd, "rb") as stream:
                data = json.loads(stream.read(513))
            if (not isinstance(data, dict) or set(data) != {"schema_version", "generation", "channel_sha256"}
                    or type(data["schema_version"]) is not int or data["schema_version"] != 1):
                raise release_channel.ChannelError("INVALID_RELEASE_POLICY", "unknown channel state schema")
            stored = release_channel.ChannelHighWater(data["generation"], data["channel_sha256"])
        accepted = release_channel.evaluate_high_water(channel, stored)
        if accepted == stored:
            return accepted
        fd, name = tempfile.mkstemp(prefix=".channel-", dir=root)
        temporary = Path(name)
        try:
            with os.fdopen(fd, "wb") as output:
                output.write(json.dumps({"schema_version": 1, **accepted.__dict__}, sort_keys=True).encode())
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, state)  # exclusive owned lock; never follows a destination link
            directory_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        finally:
            temporary.unlink(missing_ok=True)
        return accepted


def parser() -> argparse.ArgumentParser:
    result = release_download.BoundedArgumentParser(description=__doc__)
    result.add_argument("--platform-profile", choices=sorted(PROFILES),
                        help="optional assertion; never overrides detected host qualification")
    result.add_argument("--detect-only", action="store_true",
                        help="print deterministic host qualification JSON and perform no other action")
    result.add_argument("--channel-url")
    trust = result.add_mutually_exclusive_group()
    trust.add_argument("--trusted-channel-sha256")
    trust.add_argument("--trust-policy", type=Path, help="explicit local production Ed25519 public policy")
    result.add_argument("--trusted-origin", action="append")
    result.add_argument("--minimum-channel-generation", type=int)
    result.add_argument("--yes", action="store_true", help="consent noninteractively after the plan is displayed")
    return result


def run(argv: list[str] | None = None, *, manager_factory=NativeManager,
        detector=linux_platform_detection.detect) -> int:
    argument_parser = parser()
    args = argument_parser.parse_args(argv)
    staging: Path | None = None
    mutation_started = False
    preserve_staging = False
    try:
        detection = detector()
        if args.detect_only:
            print(detection.to_json())
            return 0 if detection.qualification_status == "QUALIFIED" else EXIT_UNSUPPORTED
        missing = [name for name in ("channel_url", "trusted_origin",
                                      "minimum_channel_generation") if getattr(args, name) is None]
        if args.trusted_channel_sha256 is None and args.trust_policy is None:
            missing.append("trust_policy or trusted_channel_sha256")
        if missing:
            argument_parser.error("installation requires trusted release arguments: " + ", ".join(missing))
        profile = resolve_profile(args.platform_profile, detection)
        if os.geteuid() == 0:
            raise QuickInstallError("AuthorizationRequired", "The whole installer must not run as root.",
                                    "Run as your ordinary user; sudo is requested only for installation.", EXIT_AUTHORIZATION)
        base = os.environ.get("XDG_RUNTIME_DIR")
        if (base and Path(base).is_absolute() and Path(base).is_dir() and not Path(base).is_symlink()
                and Path(base).stat().st_uid == os.getuid() and not Path(base).stat().st_mode & 0o077):
            staging = Path(tempfile.mkdtemp(prefix="synveil-install-", dir=base))
        else:
            staging = Path(tempfile.mkdtemp(prefix="synveil-install-"))
        os.chmod(staging, 0o700)
        evidence = acquire(args, profile, staging)
        manager = manager_factory(profile)
        manager.artifact_evidence = evidence
        installed = manager.installed_version()
        release_channel.parse_stable_version(evidence["product_version"])
        if installed is not None:
            release_channel.parse_stable_version(installed)
            if release_channel.parse_stable_version(installed) > release_channel.parse_stable_version(evidence["product_version"]):
                raise QuickInstallError("InstallationFailed", "A newer Synveil version is installed; downgrade is rejected.",
                                        "Use a compatible version through the lifecycle owner.", EXIT_PACKAGE_MANAGER)
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
        if mutation_started:
            preserve_staging = True
            print("OutcomeUnknown: Native package state requires inspection before any retry.", file=sys.stderr)
            return EXIT_UNKNOWN
        print("InternalFailure: The installer could not complete safely.", file=sys.stderr)
        print("No package change was assumed. Review bounded diagnostics and retry.", file=sys.stderr)
        return EXIT_INTERNAL
    finally:
        if staging is not None and not preserve_staging:
            shutil.rmtree(staging, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(run())
