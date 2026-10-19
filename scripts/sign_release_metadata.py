#!/usr/bin/env python3
"""Sign release manifests/channels with the trusted Ed25519 publisher key."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile

import release_channel
import release_download
import release_manifest
import release_signature

PRIVATE_KEY_ENV = "SYNVEIL_RELEASE_ED25519_PRIVATE_KEY_B64"
MAX_INPUT_BYTES = 1_048_576
KINDS = {
    "manifest": {
        "filename": "SYNVEIL-RELEASE-MANIFEST.json",
        "signature": "SYNVEIL-RELEASE-MANIFEST.sig",
        "descriptor": "SYNVEIL-RELEASE-MANIFEST-AUTH.json",
    },
    "channel": {
        "filename": release_channel.CHANNEL_FILENAME,
        "signature": "SYNVEIL-RELEASE-CHANNEL.sig",
        "descriptor": release_channel.CHANNEL_AUTH_FILENAME,
    },
}


class SigningError(ValueError):
    pass


def _unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise SigningError("metadata contains a duplicate JSON field")
        result[key] = value
    return result


def _load_document(raw: bytes, kind: str) -> dict:
    try:
        document = json.loads(raw.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise SigningError("metadata is not valid UTF-8 JSON") from error
    if kind == "manifest":
        return release_manifest.validate(document)
    return release_channel.validate_channel(document)


def _descriptor(kind: str, filename: str, signature_filename: str,
                raw: bytes, key_id: str) -> dict:
    base = {
        "schema_version": 1,
        "signatures": [{"scheme": release_signature.SCHEME,
                        "key_id": key_id,
                        "signature_filename": signature_filename}],
    }
    if kind == "manifest":
        base.update(manifest_filename=filename,
                    manifest_size_bytes=len(raw),
                    manifest_sha256=hashlib.sha256(raw).hexdigest())
    else:
        base.update(channel_filename=filename,
                    channel_size_bytes=len(raw),
                    channel_sha256=hashlib.sha256(raw).hexdigest())
    return base


def _validate_signature(kind: str, raw: bytes, signature: bytes,
                        descriptor: dict, keys: dict[str, bytes], key_id: str) -> None:
    verify = release_signature.verifier(keys, {descriptor["signatures"][0]["signature_filename"]: signature})
    if kind == "manifest":
        policy = release_download.ReleaseTrustPolicy(
            frozenset(), frozenset(), authentication_method="detached_signature",
            trusted_key_ids=frozenset({key_id}),
            allowed_signature_schemes=frozenset({release_signature.SCHEME}),
        )
        authentication = release_download.authenticate_manifest(
            raw, policy, signature_descriptor=descriptor, verifier=verify
        )
        release_download.parse_authenticated_manifest(raw, authentication)
    else:
        release_channel.validate_auth_descriptor(descriptor)
        policy = release_channel.ChannelTrustPolicy(
            authentication_method="detached_signature",
            trusted_key_ids=frozenset({key_id}),
            allowed_signature_schemes=frozenset({release_signature.SCHEME}),
        )
        authentication = release_channel.authenticate_channel(
            raw, policy, descriptor=descriptor, verifier=verify
        )
        release_channel.parse_authenticated_channel(raw, authentication, policy)


def _read_private_key() -> bytes:
    encoded = os.environ.get(PRIVATE_KEY_ENV, "")
    if not encoded:
        raise SigningError("BLOCKED_SIGNING_INPUT: protected Ed25519 private key is not configured")
    if len(encoded) > 256:
        raise SigningError("BLOCKED_SIGNING_INPUT: protected Ed25519 private key has invalid length")
    try:
        private_seed = base64.b64decode(encoded, validate=True)
    except (ValueError, base64.binascii.Error) as error:
        raise SigningError("BLOCKED_SIGNING_INPUT: protected Ed25519 private key encoding is invalid") from error
    if len(private_seed) != 32:
        raise SigningError("BLOCKED_SIGNING_INPUT: expected a 32-byte Ed25519 seed")
    return private_seed


def _publish_pair(signature_path: Path, signature: bytes,
                  descriptor_path: Path, descriptor: bytes) -> None:
    signature_path = signature_path.absolute()
    descriptor_path = descriptor_path.absolute()
    if signature_path.parent != descriptor_path.parent:
        raise SigningError("signature and descriptor must share a staging directory")
    if signature_path == descriptor_path or signature_path.exists() or descriptor_path.exists():
        raise SigningError("existing signature output is preserved")
    if signature_path.parent.is_symlink() or any(parent.is_symlink() for parent in signature_path.parent.parents):
        raise SigningError("signature output directory contains a symlink")
    signature_path.parent.mkdir(parents=True, exist_ok=True)
    staged: list[Path] = []
    linked: list[Path] = []
    try:
        for data in (signature, descriptor):
            fd, name = tempfile.mkstemp(prefix=".synveil-signature.", dir=signature_path.parent)
            path = Path(name)
            staged.append(path)
            with os.fdopen(fd, "wb") as stream:
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
            path.chmod(0o644)
        for source, destination in zip(staged, (signature_path, descriptor_path), strict=True):
            os.link(source, destination, follow_symlinks=False)
            linked.append(destination)
        directory_fd = os.open(signature_path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except OSError as error:
        for path in linked:
            path.unlink(missing_ok=True)
        raise SigningError("signature metadata could not be published atomically") from error
    finally:
        for path in staged:
            path.unlink(missing_ok=True)


def sign(kind: str, input_path: Path, signature_path: Path,
         descriptor_path: Path, trust_policy_path: Path) -> dict:
    if kind not in KINDS:
        raise SigningError("unsupported metadata signing purpose")
    expected = KINDS[kind]
    if input_path.name != expected["filename"]:
        raise SigningError(f"{kind} input must be named {expected['filename']}")
    if signature_path.name != expected["signature"] or descriptor_path.name != expected["descriptor"]:
        raise SigningError("signature and authentication descriptor filenames must match the release contract")
    raw = release_download.read_bounded_regular_file(input_path, MAX_INPUT_BYTES)
    if not raw:
        raise SigningError("metadata input is empty")
    _load_document(raw, kind)
    seed = _read_private_key()
    keys = release_signature.load_production_keys(trust_policy_path)
    try:
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    except ImportError as error:
        raise SigningError("Ed25519 cryptographic backend is unavailable") from error
    private = Ed25519PrivateKey.from_private_bytes(seed)
    public = private.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    key_id = "synveil-release-" + hashlib.sha256(public).hexdigest()
    if keys.get(key_id) != public:
        raise SigningError("protected private key does not match an independently configured trusted public key")
    signature = private.sign(raw)
    descriptor = _descriptor(kind, expected["filename"], expected["signature"], raw, key_id)
    _validate_signature(kind, raw, signature, descriptor, keys, key_id)
    encoded_descriptor = (json.dumps(descriptor, sort_keys=True, indent=2) + "\n").encode("utf-8")
    _publish_pair(signature_path, signature, descriptor_path, encoded_descriptor)
    return {"status": "VERIFIED_SIGNED_CANDIDATE", "kind": kind,
            "key_id": key_id, "signature_sha256": hashlib.sha256(signature).hexdigest(),
            "metadata_sha256": hashlib.sha256(raw).hexdigest(), "metadata_size_bytes": len(raw)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=tuple(KINDS), required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--signature", type=Path, required=True)
    parser.add_argument("--descriptor", type=Path, required=True)
    parser.add_argument("--trust-policy", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = sign(args.kind, args.input, args.signature, args.descriptor, args.trust_policy)
    except (SigningError, release_signature.SignaturePolicyError,
            release_download.AcquisitionError, release_manifest.ManifestError,
            release_channel.ChannelError, OSError, ValueError, TypeError) as error:
        print(f"release-signing: BLOCKED_OR_REJECTED: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
