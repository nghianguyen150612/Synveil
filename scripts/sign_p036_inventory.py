#!/usr/bin/env python3
"""Sign an already validated P036 inventory using the approved OpenPGP key."""

from __future__ import annotations

import base64
import binascii
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile

P036_SPEC = importlib.util.spec_from_file_location(
    "validate_p036_production_artifacts",
    Path(__file__).resolve().with_name("validate-p036-production-artifacts.py"),
)
p036 = importlib.util.module_from_spec(P036_SPEC)
assert P036_SPEC and P036_SPEC.loader
P036_SPEC.loader.exec_module(p036)

PRIVATE_KEY_ENV = "SYNVEIL_P036_INVENTORY_SIGNING_KEY_B64"
PASSPHRASE_ENV = "SYNVEIL_P036_INVENTORY_SIGNING_PASSPHRASE"
FINGERPRINT_ENV = "SYNVEIL_P036_INVENTORY_FINGERPRINT"
FINGERPRINT = re.compile(r"[0-9A-F]{40,64}\Z")


class SigningError(ValueError):
    pass


def _run(args: list[str], *, env: dict[str, str], timeout: int = 60,
         allow_failure: bool = False) -> subprocess.CompletedProcess:
    try:
        result = subprocess.run(args, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=timeout, check=False)
    except (OSError, subprocess.SubprocessError) as error:
        raise SigningError("P036 signing or verification tool failed to run") from error
    if result.returncode != 0 and not allow_failure:
        raise SigningError("P036 signing or verification command failed")
    return result


def _decode_private_key() -> bytes:
    encoded = os.environ.get(PRIVATE_KEY_ENV, "")
    if not encoded or len(encoded) > 8 * 1024 * 1024:
        raise SigningError("BLOCKED_SIGNING_INPUT: authorized P036 inventory key is not configured")
    try:
        content = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error) as error:
        raise SigningError("BLOCKED_SIGNING_INPUT: P036 inventory key is not valid base64") from error
    if not content or len(content) > 4 * 1024 * 1024:
        raise SigningError("BLOCKED_SIGNING_INPUT: P036 inventory key has an invalid size")
    return content


def _write_private(path: Path, content: bytes) -> None:
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def _primary_fingerprint(home: Path, env: dict[str, str]) -> str:
    result = _run(["gpg", "--batch", "--homedir", str(home), "--with-colons",
                   "--list-secret-keys", "--fingerprint"], env=env)
    found_primary = False
    for line in result.stdout.decode("utf-8", "replace").splitlines():
        fields = line.split(":")
        if fields[0] == "sec":
            if found_primary:
                raise SigningError("P036 signing key must contain one primary identity")
            found_primary = True
        elif fields[0] == "fpr" and found_primary:
            fingerprint = fields[9]
            if not FINGERPRINT.fullmatch(fingerprint):
                raise SigningError("GnuPG returned an invalid P036 key fingerprint")
            return fingerprint
    raise SigningError("authorized P036 inventory signing key is unavailable")


def _prevalidate(inventory: Path, artifact_root: Path, signature: Path) -> None:
    command = [sys.executable, str(Path(p036.__file__).resolve()),
               "--inventory", str(inventory), "--inventory-signature", str(signature),
               "--artifact-root", str(artifact_root), "--pre-signing-check"]
    result = _run(command, env=os.environ.copy(), timeout=600, allow_failure=True)
    if result.returncode:
        # The validator uses fixed, secret-free messages; retain its evidence.
        diagnostic = (result.stdout + result.stderr).decode("utf-8", "replace").strip()
        raise SigningError("P036 pre-signing validation failed: " + (diagnostic or "component identity gate rejected"))


def sign_inventory(inventory: Path, signature: Path, artifact_root: Path) -> dict:
    fingerprint = os.environ.get(FINGERPRINT_ENV, "")
    if not FINGERPRINT.fullmatch(fingerprint):
        raise SigningError("BLOCKED_SIGNING_INPUT: approved P036 inventory fingerprint is not configured")
    fingerprint = fingerprint.upper()
    keyring_path = os.environ.get("SYNVEIL_P036_TRUSTED_KEYRING", "")
    if not keyring_path:
        raise SigningError("BLOCKED_TRUST_INPUT: independently configured P036 public keyring is not materialized")
    if not inventory.is_file():
        raise SigningError("BLOCKED_BY_EXTERNAL_INPUT: acceptance/p036/production-artifacts.json has not been supplied")
    if not artifact_root.is_dir():
        raise SigningError("BLOCKED_BY_EXTERNAL_INPUT: P036 production artifact files have not been supplied")
    inventory = inventory.resolve(strict=True)
    artifact_root = artifact_root.resolve(strict=True)
    signature = signature.absolute()
    if signature.exists() or signature.is_symlink():
        raise SigningError("P036 signature output already exists; existing signature is preserved")
    _prevalidate(inventory, artifact_root, signature)
    private_key = _decode_private_key()
    passphrase = os.environ.get(PASSPHRASE_ENV, "")
    if len(passphrase) > 4096:
        raise SigningError("BLOCKED_SIGNING_INPUT: P036 signing passphrase exceeds its size limit")

    signature.parent.mkdir(parents=True, exist_ok=True)
    if signature.parent.is_symlink() or any(parent.is_symlink() for parent in signature.parent.parents):
        raise SigningError("P036 signature output ancestry contains a symlink")
    with tempfile.TemporaryDirectory(prefix="synveil-p036-signing-") as temporary:
        root = Path(temporary)
        root.chmod(0o700)
        home = root / "gnupg"
        home.mkdir(mode=0o700)
        private_path = root / "private.asc"
        passphrase_path = root / "passphrase"
        public_ring = root / "trusted-publishers.gpg"
        _write_private(private_path, private_key)
        _write_private(passphrase_path, passphrase.encode("utf-8"))
        public_encoded = os.environ.get("SYNVEIL_P036_TRUSTED_KEYRING_B64", "")
        if not public_encoded or len(public_encoded) > 32 * 1024 * 1024:
            raise SigningError("BLOCKED_TRUST_INPUT: independently approved P036 public keyring is not configured")
        try:
            public_bytes = base64.b64decode(public_encoded, validate=True)
        except (ValueError, binascii.Error) as error:
            raise SigningError("BLOCKED_TRUST_INPUT: P036 public keyring is invalid base64") from error
        if not public_bytes or len(public_bytes) > 16 * 1024 * 1024:
            raise SigningError("BLOCKED_TRUST_INPUT: P036 public keyring has an invalid size")
        _write_private(public_ring, public_bytes)

        env = dict(os.environ)
        env["GNUPGHOME"] = str(home)
        env["GPG_TTY"] = ""
        _run(["gpg", "--batch", "--homedir", str(home), "--import", str(private_path)], env=env)
        actual_fingerprint = _primary_fingerprint(home, env)
        if actual_fingerprint != fingerprint:
            raise SigningError("P036 signing key does not match independently configured inventory fingerprint")

        temp_signature = root / "production-artifacts.json.sig"
        sign_args = ["gpg", "--batch", "--no-tty", "--homedir", str(home),
                     "--pinentry-mode", "loopback", "--passphrase-file", str(passphrase_path),
                     "--local-user", fingerprint, "--detach-sign", "--output", str(temp_signature), str(inventory)]
        _run(sign_args, env=env, timeout=120)
        if not temp_signature.is_file() or temp_signature.stat().st_size == 0:
            raise SigningError("GnuPG did not create a detached P036 inventory signature")

        try:
            p036.verify_signature(public_ring, temp_signature, inventory, fingerprint)
        except p036.InventoryError as error:
            raise SigningError("P036 detached signature did not verify to the independently trusted fingerprint")

        fd = os.open(signature, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o644)
        try:
            with os.fdopen(fd, "wb") as stream, temp_signature.open("rb") as source:
                shutil.copyfileobj(source, stream)
                stream.flush()
                os.fsync(stream.fileno())
            verify = _run([sys.executable, str(Path(p036.__file__).resolve()),
                           "--inventory", str(inventory), "--inventory-signature", str(signature),
                           "--artifact-root", str(artifact_root)], env=env, timeout=600, allow_failure=True)
            if verify.returncode:
                signature.unlink(missing_ok=True)
                diagnostic = (verify.stdout + verify.stderr).decode("utf-8", "replace").strip()
                raise SigningError("new P036 signature failed the production validator: " + (diagnostic or "validation rejected"))
            raw = inventory.read_bytes()
            return {"schema_version": 1, "status": "VERIFIED_SIGNED_CANDIDATE",
                    "signer_fingerprint": fingerprint, "inventory_size_bytes": len(raw),
                    "inventory_sha256": hashlib.sha256(raw).hexdigest(),
                    "signature_size_bytes": signature.stat().st_size,
                    "signature_sha256": hashlib.sha256(signature.read_bytes()).hexdigest()}
        except Exception:
            signature.unlink(missing_ok=True)
            raise


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", type=Path, default=p036.ROOT / "acceptance/p036/production-artifacts.json")
    parser.add_argument("--signature", type=Path, required=True)
    parser.add_argument("--artifact-root", type=Path, default=p036.ROOT / "acceptance/p036/artifacts")
    args = parser.parse_args()
    try:
        print(json.dumps(sign_inventory(args.inventory, args.signature, args.artifact_root), sort_keys=True))
        return 0
    except (SigningError, p036.InventoryError, p036.ExternalInputError, OSError,
            UnicodeError, ValueError, TypeError, subprocess.SubprocessError) as error:
        print(f"p036-inventory-signing: BLOCKED_OR_REJECTED: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
