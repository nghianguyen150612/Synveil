#!/usr/bin/env python3
"""P006 Ed25519 backend. Only explicit, local public policy establishes trust."""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Mapping

SCHEME = "ed25519"
MAX_POLICY_BYTES = 64 * 1024
MAX_SIGNATURES = 16


class SignaturePolicyError(ValueError):
    """Bounded error; never carries key material, paths or remote text."""


def load_production_keys(path: Path) -> dict[str, bytes]:
    from release_download import read_bounded_regular_file
    raw = read_bounded_regular_file(path, MAX_POLICY_BYTES)
    if len(raw) > MAX_POLICY_BYTES:
        raise SignaturePolicyError("public trust policy exceeds its limit")
    try:
        data = json.loads(raw)
        if (not isinstance(data, dict) or set(data) != {"schema_version", "purpose", "keys"}
                or type(data["schema_version"]) is not int or data["schema_version"] != 1
                or data["purpose"] != "production-release-verification"
                or not isinstance(data["keys"], list) or not 1 <= len(data["keys"]) <= MAX_SIGNATURES):
            raise ValueError
        keys = {}
        for entry in data["keys"]:
            if (not isinstance(entry, dict) or set(entry) != {"key_id", "scheme", "public_key_hex"}
                    or entry["scheme"] != SCHEME or not isinstance(entry["public_key_hex"], str)
                    or not re.fullmatch(r"[0-9a-f]{64}", entry["public_key_hex"])):
                raise ValueError
            public_key = bytes.fromhex(entry["public_key_hex"])
            # Stable ID is cryptographically bound to public bytes. A test namespace,
            # filename, CI variable or remote key ID can never install a root.
            key_id = "synveil-release-" + hashlib.sha256(public_key).hexdigest()
            if entry["key_id"] != key_id or key_id in keys:
                raise ValueError
            keys[key_id] = public_key
        return keys
    except (ValueError, TypeError, KeyError) as error:
        raise SignaturePolicyError("invalid production public trust policy") from error


def verifier(public_keys: Mapping[str, bytes], signatures: Mapping[str, bytes]):
    """Verify detached 64-byte signatures over exact raw metadata, without fallback.

    Keys and signatures are copied at construction; metadata cannot add roots.
    The maintained cryptography/OpenSSL backend is imported only for signature
    mode so independently pinned bootstrap use keeps its stdlib-only boundary.
    """
    try:
        from cryptography.exceptions import InvalidSignature, UnsupportedAlgorithm
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError as error:
        raise SignaturePolicyError("Ed25519 backend unavailable") from error
    if not 1 <= len(public_keys) <= MAX_SIGNATURES or len(signatures) > MAX_SIGNATURES:
        raise SignaturePolicyError("invalid signature policy bounds")
    try:
        keys = {}
        for key_id, raw in public_keys.items():
            if not isinstance(raw, bytes) or len(raw) != 32:
                raise ValueError
            keys[key_id] = Ed25519PublicKey.from_public_bytes(raw)
    except (ValueError, TypeError, UnsupportedAlgorithm) as error:
        raise SignaturePolicyError("invalid Ed25519 public key or unavailable backend") from error
    detached = dict(signatures)

    def verify(key_id, scheme, raw, entry):
        signature = detached.get(entry.get("signature_filename"))
        if (scheme != SCHEME or key_id not in keys or not isinstance(raw, bytes)
                or not isinstance(signature, bytes) or len(signature) != 64):
            return False
        try:
            keys[key_id].verify(signature, raw)
            return True
        except (InvalidSignature, ValueError, TypeError, UnsupportedAlgorithm):
            return False

    return verify


def read_signature(path: Path) -> bytes:
    from release_download import read_bounded_regular_file
    # Bound even a malicious file or source; verifier rejects anything but 64.
    return read_bounded_regular_file(path, 64)
