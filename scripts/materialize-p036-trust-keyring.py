#!/usr/bin/env python3
"""Materialize the independently provisioned public P036 publisher keyring."""
from __future__ import annotations

import base64
import binascii
import os
from pathlib import Path
import stat
import sys


def main() -> int:
    encoded = os.environ.get("SYNVEIL_P036_TRUSTED_KEYRING_B64", "")
    if not encoded:
        print("BLOCKED_BY_EXTERNAL_INPUT: trusted publisher keyring variable is absent", file=sys.stderr)
        return 2
    try:
        content = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError):
        print("BLOCKED_BY_EXTERNAL_INPUT: trusted publisher keyring is not valid base64", file=sys.stderr)
        return 2
    if not content or len(content) > 16 * 1024 * 1024:
        print("BLOCKED_BY_EXTERNAL_INPUT: trusted publisher keyring exceeds the allowed size", file=sys.stderr)
        return 2
    runner_temp = Path(os.environ.get("RUNNER_TEMP", ""))
    github_env = Path(os.environ.get("GITHUB_ENV", ""))
    try:
        directory = runner_temp.lstat()
        if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid()
                or directory.st_mode & 0o022 or not runner_temp.is_absolute()
                or not github_env.is_absolute()):
            raise ValueError
        target = runner_temp / "synveil-p036-trusted-keyring.gpg"
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        if "\n" in str(target) or "\r" in str(target):
            raise ValueError
        environment_info = github_env.lstat()
        if (not stat.S_ISREG(environment_info.st_mode) or environment_info.st_uid != os.getuid()
                or environment_info.st_nlink != 1 or environment_info.st_mode & 0o022):
            raise ValueError
        env_fd = os.open(github_env, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            opened_info = os.fstat(env_fd)
            if ((opened_info.st_dev, opened_info.st_ino) !=
                    (environment_info.st_dev, environment_info.st_ino)
                    or not stat.S_ISREG(opened_info.st_mode) or opened_info.st_nlink != 1
                    or opened_info.st_uid != os.getuid() or opened_info.st_mode & 0o022):
                raise ValueError
            line = f"SYNVEIL_P036_TRUSTED_KEYRING={target}\n".encode("utf-8")
            written = 0
            while written < len(line):
                amount = os.write(env_fd, line[written:])
                if amount <= 0:
                    raise OSError("GITHUB_ENV short write")
                written += amount
            os.fsync(env_fd)
        finally:
            os.close(env_fd)
    except (OSError, ValueError):
        print("BLOCKED_ENVIRONMENT: cannot safely materialize the P036 trust keyring", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
