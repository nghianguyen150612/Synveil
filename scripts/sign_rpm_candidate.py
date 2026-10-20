#!/usr/bin/env python3
"""Sign and independently verify the RPM in a candidate package manifest."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile

import release_manifest

PRIVATE_KEY_ENV = "SYNVEIL_RPM_SIGNING_PRIVATE_KEY_B64"
PUBLIC_KEY_ENV = "SYNVEIL_RPM_TRUSTED_PUBLIC_KEY_B64"
FINGERPRINT_ENV = "SYNVEIL_RPM_SIGNER_FINGERPRINT"
HEX_FINGERPRINT = re.compile(r"[0-9A-F]{40,64}\Z")


class SigningError(ValueError):
    pass


def _decode_environment(name: str, limit: int) -> bytes:
    encoded = os.environ.get(name, "")
    if not encoded:
        raise SigningError(f"BLOCKED_SIGNING_INPUT: {name} is not configured")
    if len(encoded) > limit * 2:
        raise SigningError(f"BLOCKED_SIGNING_INPUT: {name} exceeds its size limit")
    try:
        content = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error) as error:
        raise SigningError(f"BLOCKED_SIGNING_INPUT: {name} is not valid base64") from error
    if not content or len(content) > limit:
        raise SigningError(f"BLOCKED_SIGNING_INPUT: {name} is empty or exceeds its size limit")
    return content


def _write_private(path: Path, content: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    fd = os.open(path, flags, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def _run(args: list[str], *, env: dict[str, str], timeout: int = 60) -> subprocess.CompletedProcess:
    try:
        result = subprocess.run(args, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=timeout, check=False)
    except (OSError, subprocess.SubprocessError) as error:
        raise SigningError("RPM signing or verification tool failed to run") from error
    if result.returncode != 0:
        raise SigningError("RPM signing or verification command failed")
    return result


def _primary_fingerprint(home: Path, secret: bool, env: dict[str, str]) -> str:
    command = ["gpg", "--batch", "--homedir", str(home), "--with-colons"]
    command += ["--list-secret-keys", "--fingerprint"] if secret else ["--list-keys", "--fingerprint"]
    result = _run(command, env=env)
    expect = "sec" if secret else "pub"
    primary_seen = False
    for line in result.stdout.decode("utf-8", "replace").splitlines():
        fields = line.split(":")
        if fields[0] == expect:
            if primary_seen:
                raise SigningError("RPM trust policy must contain exactly one primary signing key")
            primary_seen = True
        elif fields[0] == "fpr" and primary_seen:
            fingerprint = fields[9]
            if not HEX_FINGERPRINT.fullmatch(fingerprint):
                raise SigningError("GPG returned an invalid primary fingerprint")
            return fingerprint
    raise SigningError("expected primary GPG signing key is unavailable")


def _checked_package_metadata(path: Path, item: dict, env: dict[str, str]) -> None:
    result = _run(["rpm", "-qp", "--qf", "%{NAME}\n%{VERSION}\n%{RELEASE}\n%{ARCH}\n", str(path)], env=env)
    values = result.stdout.decode("utf-8", "strict").splitlines()
    expected = item["package_metadata"]
    if values != [expected["package_name"], expected["package_version"],
                  expected["package_release"], expected["package_architecture"]]:
        raise SigningError("RPM native metadata differs from the candidate manifest")


def sign(manifest_path: Path, artifact_root: Path) -> dict:
    private_key = _decode_environment(PRIVATE_KEY_ENV, 1024 * 1024)
    public_key = _decode_environment(PUBLIC_KEY_ENV, 1024 * 1024)
    fingerprint = os.environ.get(FINGERPRINT_ENV, "")
    if not HEX_FINGERPRINT.fullmatch(fingerprint):
        raise SigningError(f"BLOCKED_SIGNING_INPUT: {FINGERPRINT_ENV} is missing or invalid")
    expected_fingerprint = fingerprint.upper()
    document = release_manifest.load(manifest_path)
    commit = document["source_commit"]
    version = document["product_version"]
    head = subprocess.run(["git", "rev-parse", "--verify", "HEAD^{commit}"],
                          cwd=Path(__file__).resolve().parents[1], stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True, timeout=10).stdout.decode("ascii").strip()
    if head != commit:
        raise SigningError("candidate source commit differs from this checkout")
    release_manifest.validate(document, artifact_root, commit)
    rpm_items = [item for item in document["artifacts"] if item["artifact_type"] == "rpm"]
    if len(rpm_items) != 1:
        raise SigningError("candidate manifest must contain exactly one RPM")
    rpm_item = rpm_items[0]
    original = artifact_root / rpm_item["filename"]
    if original.is_symlink() or not original.is_file():
        raise SigningError("RPM candidate is missing or redirected")

    with tempfile.TemporaryDirectory(prefix="synveil-rpm-signing-") as temporary:
        root = Path(temporary)
        root.chmod(0o700)
        home = root / "gnupg"
        home.mkdir(mode=0o700)
        key_in = root / "private.asc"
        key_pub = root / "public.asc"
        _write_private(key_in, private_key)
        _write_private(key_pub, public_key)
        env = dict(os.environ)
        env["GNUPGHOME"] = str(home)
        env["GPG_TTY"] = ""
        _run(["gpg", "--batch", "--homedir", str(home), "--import", str(key_pub)], env=env)
        public_fingerprint = _primary_fingerprint(home, False, env)
        if public_fingerprint != expected_fingerprint:
            raise SigningError("trusted RPM public key fingerprint differs from independent policy")
        _run(["gpg", "--batch", "--homedir", str(home), "--import", str(key_in)], env=env)
        secret_fingerprint = _primary_fingerprint(home, True, env)
        if secret_fingerprint != expected_fingerprint:
            raise SigningError("RPM signing key does not match independently trusted fingerprint")
        _checked_package_metadata(original, rpm_item, env)

        staged = root / "candidate.rpm"
        shutil.copyfile(original, staged)
        staged.chmod(stat.S_IMODE(original.stat().st_mode))
        _run(["rpmsign", "--define", "_signature gpg", "--define", f"_gpg_path {home}",
              "--define", f"_gpg_name {expected_fingerprint}", "--addsign", str(staged)], env=env, timeout=180)
        database = root / "rpmdb"
        database.mkdir(mode=0o700)
        _run(["rpm", "--dbpath", str(database), "--initdb"], env=env)
        _run(["rpmkeys", "--dbpath", str(database), "--import", str(key_pub)], env=env)
        verification = _run(["rpmkeys", "--dbpath", str(database), "--checksig", "--verbose", str(staged)], env=env)
        output = (verification.stdout + verification.stderr).decode("utf-8", "replace")
        if "OK" not in output or "NOKEY" in output or "NOT OK" in output:
            raise SigningError("RPM signature did not verify against the independently trusted public key")

        os.replace(staged, original)
        refreshed = []
        for item in document["artifacts"]:
            specification = {key: value for key, value in item.items()
                             if key not in {"product_version", "size_bytes", "sha256"}}
            refreshed.append(release_manifest.artifact_from_spec(artifact_root, version,
                                                                  json.dumps(specification, sort_keys=True)))
        document["artifacts"] = sorted(refreshed, key=lambda item: item["id"])
        release_manifest.write(manifest_path, document)
        release_manifest.validate(document, artifact_root, commit)
        rpm_size = original.stat().st_size
        rpm_digest = release_manifest.sha256(original)
        evidence = {
            "schema_version": 1,
            "status": "VERIFIED_SIGNED_CANDIDATE",
            "signature_scheme": "RPM OpenPGP package signature",
            "signer_fingerprint": expected_fingerprint,
            "filename": original.name,
            "size_bytes": rpm_size,
            "sha256": rpm_digest,
            "verification": "rpmkeys --checksig against isolated database containing only the independently trusted public key",
            "manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        }
        evidence_path = artifact_root / "rpm-signing-evidence.json"
        if evidence_path.exists() or evidence_path.is_symlink():
            raise SigningError("RPM signing evidence destination already exists")
        evidence_path.write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8", newline="\n")
        return evidence


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--artifact-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(sign(args.manifest, args.artifact_root), sort_keys=True))
        return 0
    except (SigningError, release_manifest.ManifestError, OSError, UnicodeError,
            ValueError, TypeError, subprocess.SubprocessError) as error:
        print(f"rpm-signing: BLOCKED_OR_REJECTED: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
