#!/usr/bin/env python3
"""Write the configured public release-signing trust policy into runner temp."""

from __future__ import annotations

import base64
import binascii
import os
from pathlib import Path
import stat
import sys

import release_signature


def materialize() -> Path:
    encoded = os.environ.get("SYNVEIL_RELEASE_ED25519_PUBLIC_TRUST_POLICY_B64", "")
    if not encoded or len(encoded) > 128 * 1024:
        raise ValueError("BLOCKED_SIGNING_INPUT: public release trust policy is not configured")
    try:
        content = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise ValueError("BLOCKED_SIGNING_INPUT: public release trust policy is invalid base64") from error
    if not content or len(content) > release_signature.MAX_POLICY_BYTES:
        raise ValueError("BLOCKED_SIGNING_INPUT: public release trust policy exceeds its size limit")
    runner_temp = Path(os.environ.get("RUNNER_TEMP", ""))
    if not runner_temp.is_absolute():
        raise ValueError("BLOCKED_ENVIRONMENT: runner temporary directory is unavailable")
    directory = runner_temp.lstat()
    if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid() or directory.st_mode & 0o022):
        raise ValueError("BLOCKED_ENVIRONMENT: runner temporary directory is not private")
    target = runner_temp / "synveil-release-public-trust-policy.json"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
    try:
        fd = os.open(target, flags, 0o600)
    except OSError as error:
        raise ValueError("BLOCKED_ENVIRONMENT: release trust policy destination is not new") from error
    with os.fdopen(fd, "wb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    try:
        release_signature.load_production_keys(target)
    except (release_signature.SignaturePolicyError, OSError) as error:
        target.unlink(missing_ok=True)
        raise ValueError("BLOCKED_SIGNING_INPUT: public trust policy does not match the release verifier schema") from error
    return target


def main() -> int:
    try:
        print(materialize())
        return 0
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
