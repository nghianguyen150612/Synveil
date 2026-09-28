#!/usr/bin/env python3
"""Fail-closed release manifest authentication and artifact staging (stdlib only)."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import io
import json
import os
import re
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import BinaryIO, Callable

import release_manifest

MAX_MANIFEST_BYTES = 1024 * 1024
MAX_REDIRECTS = 5
AUTHENTICATED_STATES = {"AUTHENTICATED_PINNED_DIGEST", "AUTHENTICATED_SIGNATURE"}


class AcquisitionError(ValueError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class ReleaseTrustPolicy:
    """Trusted local bootstrap input; never construct this from remote metadata."""

    allowed_manifest_origins: frozenset[str]
    allowed_artifact_origins: frozenset[str]
    authentication_method: str = "pinned_sha256"
    expected_manifest_sha256: str | None = None
    trusted_key_ids: frozenset[str] = field(default_factory=frozenset)
    allowed_signature_schemes: frozenset[str] = field(default_factory=frozenset)
    max_manifest_bytes: int = MAX_MANIFEST_BYTES
    max_redirects: int = MAX_REDIRECTS


@dataclass(frozen=True)
class ManifestAuthentication:
    state: str
    method: str
    manifest_sha256: str
    manifest_size_bytes: int
    key_id: str | None = None


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def origin(url: str) -> str:
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https":
        raise AcquisitionError("UNTRUSTED_ORIGIN", "release URLs must use HTTPS")
    if parsed.username is not None or parsed.password is not None:
        raise AcquisitionError("UNTRUSTED_ORIGIN", "URL credentials are forbidden")
    if not parsed.hostname:
        raise AcquisitionError("UNTRUSTED_ORIGIN", "URL host is required")
    try:
        port = parsed.port
    except ValueError as error:
        raise AcquisitionError("UNTRUSTED_ORIGIN", "invalid URL port") from error
    host = parsed.hostname.lower()
    return f"https://{host}" + (f":{port}" if port not in (None, 443) else "")


def require_allowed_url(url: str, allowed: frozenset[str]) -> None:
    if origin(url) not in allowed:
        raise AcquisitionError("UNTRUSTED_ORIGIN", "URL origin is not trusted")


def fetch_https(url: str, allowed: frozenset[str], limit: int, max_redirects: int = MAX_REDIRECTS,
                opener=None) -> bytes:
    """Fetch bounded bytes while evaluating every redirect before following it."""
    opener = opener or urllib.request.build_opener(NoRedirect())
    seen: set[str] = set()
    current = url
    for redirects in range(max_redirects + 1):
        require_allowed_url(current, allowed)
        if current in seen:
            raise AcquisitionError("UNSAFE_REDIRECT", "redirect loop")
        seen.add(current)
        try:
            response = opener.open(urllib.request.Request(current, method="GET"))
        except urllib.error.HTTPError as error:
            response = error
        status = getattr(response, "status", response.getcode())
        if status in {301, 302, 303, 307, 308}:
            target = response.headers.get("Location")
            response.close()
            if not target or redirects == max_redirects:
                raise AcquisitionError("UNSAFE_REDIRECT", "missing or excessive redirect")
            current = urllib.parse.urljoin(current, target)
            try:
                require_allowed_url(current, allowed)
            except AcquisitionError as error:
                raise AcquisitionError("UNSAFE_REDIRECT", "redirect target is not trusted HTTPS") from error
            continue
        if status != 200:
            response.close()
            raise AcquisitionError("NETWORK_ERROR", f"unexpected HTTP status {status}")
        data = response.read(limit + 1)
        response.close()
        if len(data) > limit:
            raise AcquisitionError("MANIFEST_TOO_LARGE", "response exceeds configured limit")
        return data
    raise AcquisitionError("UNSAFE_REDIRECT", "redirect limit exceeded")


def download_artifact(base_url: str, destination_root: Path, artifact: dict, manifest: dict,
                      authentication: ManifestAuthentication, policy: ReleaseTrustPolicy,
                      *, opener=None) -> dict:
    """Acquire from the trusted origin and stage only fully verified bytes.

    The bounded HTTP reader requests at most the authenticated size plus one;
    staging then applies the same hard bound while hashing and writing the owned
    temporary file. The final name is never the HTTP write target.
    """
    url = trusted_artifact_url(base_url, artifact["filename"], policy.allowed_artifact_origins)
    raw = fetch_https(url, policy.allowed_artifact_origins, artifact["size_bytes"],
                      policy.max_redirects, opener)
    return stage_artifact(io.BytesIO(raw), destination_root, artifact, manifest, authentication)


def authenticate_manifest(raw: bytes, policy: ReleaseTrustPolicy, *, signature_descriptor=None,
                          verifier: Callable | None = None) -> ManifestAuthentication:
    digest = hashlib.sha256(raw).hexdigest()
    if len(raw) > policy.max_manifest_bytes:
        raise AcquisitionError("MANIFEST_TOO_LARGE", "manifest exceeds configured limit")
    if policy.authentication_method == "pinned_sha256":
        pin = policy.expected_manifest_sha256
        if not isinstance(pin, str) or not re.fullmatch(r"[0-9a-f]{64}", pin):
            raise AcquisitionError("MANIFEST_AUTH_FAILED", "trusted manifest pin must be lowercase SHA-256")
        if not hmac.compare_digest(digest, pin):
            raise AcquisitionError("MANIFEST_AUTH_FAILED", "manifest digest does not match trusted pin")
        return ManifestAuthentication("AUTHENTICATED_PINNED_DIGEST", "pinned_sha256", digest, len(raw))
    if policy.authentication_method == "detached_signature":
        if not isinstance(signature_descriptor, dict) or not signature_descriptor.get("signatures"):
            raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "detached signature descriptor is unavailable")
        entry = signature_descriptor["signatures"][0]
        scheme, key_id = entry.get("scheme"), entry.get("key_id")
        if scheme not in policy.allowed_signature_schemes or key_id not in policy.trusted_key_ids:
            raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "signature scheme or key ID is not locally trusted")
        if verifier is None:
            raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "approved signature verifier is unavailable")
        if not verifier(key_id, scheme, raw, entry):
            raise AcquisitionError("MANIFEST_AUTH_FAILED", "detached signature verification failed")
        return ManifestAuthentication("AUTHENTICATED_SIGNATURE", "detached_signature", digest, len(raw), key_id)
    raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "unknown manifest authentication method")


def parse_authenticated_manifest(raw: bytes, authentication: ManifestAuthentication,
                                 expected_version: str | None = None,
                                 expected_source_commit: str | None = None) -> dict:
    if authentication.state not in AUTHENTICATED_STATES or hashlib.sha256(raw).hexdigest() != authentication.manifest_sha256:
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "exact manifest bytes are not authenticated")
    try:
        document = json.loads(raw.decode("utf-8"))
        release_manifest.validate(document)
    except (UnicodeError, json.JSONDecodeError, release_manifest.ManifestError) as error:
        raise AcquisitionError("INVALID_MANIFEST", "authenticated manifest is invalid") from error
    if expected_version is not None and document["product_version"] != expected_version:
        raise AcquisitionError("VERSION_MISMATCH", "manifest product version differs from expectation")
    if expected_source_commit is not None and document["source_commit"] != expected_source_commit:
        raise AcquisitionError("SOURCE_COMMIT_MISMATCH", "manifest source commit differs from expectation")
    return document


def normalize_architecture(value: str) -> str:
    normalized = {"x86_64": "x86_64", "amd64": "x86_64", "x64": "x86_64",
                  "aarch64": "aarch64", "arm64": "aarch64"}.get(value.lower())
    if not normalized:
        raise AcquisitionError("UNSUPPORTED_ARCHITECTURE", "unsupported architecture")
    return normalized


def normalize_platform(value: str) -> str:
    normalized = {"linux": "linux", "windows": "windows"}.get(value.lower())
    if not normalized:
        raise AcquisitionError("UNSUPPORTED_PLATFORM", "unsupported platform")
    return normalized


def select_artifact(manifest: dict, *, platform: str, architecture: str, artifact_type: str,
                    role: str, expected_version: str) -> dict:
    if manifest["product_version"] != expected_version:
        raise AcquisitionError("VERSION_MISMATCH", "manifest product version differs from expectation")
    platform, architecture = normalize_platform(platform), normalize_architecture(architecture)
    matches = [item for item in manifest["artifacts"] if
               (item["platform"], item["architecture"], item["artifact_type"], item["role"], item["product_version"])
               == (platform, architecture, artifact_type, role, expected_version)]
    if not matches:
        raise AcquisitionError("NO_MATCHING_ARTIFACT", "no artifact has the requested exact identity")
    if len(matches) != 1:
        raise AcquisitionError("AMBIGUOUS_ARTIFACT", "more than one artifact has the requested identity")
    return matches[0]


def trusted_artifact_url(base_url: str, filename: str, allowed: frozenset[str]) -> str:
    if not release_manifest.safe_filename(filename):
        raise AcquisitionError("UNSAFE_PATH", "unsafe artifact filename")
    require_allowed_url(base_url, allowed)
    url = urllib.parse.urljoin(base_url.rstrip("/") + "/", urllib.parse.quote(filename))
    require_allowed_url(url, allowed)
    return url


def _safe_target(root: Path, filename: str) -> tuple[Path, Path]:
    if not release_manifest.safe_filename(filename) or PureWindowsPath(filename).is_absolute() or PurePosixPath(filename).is_absolute():
        raise AcquisitionError("UNSAFE_PATH", "unsafe artifact filename")
    root = root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if root.is_symlink():
        raise AcquisitionError("UNSAFE_PATH", "destination root may not be a symlink")
    target = root / filename
    if target.is_symlink() or target.parent.resolve() != root:
        raise AcquisitionError("UNSAFE_PATH", "destination escapes trusted root")
    return root, target


def _matches(path: Path, size: int, digest: str) -> bool:
    if not path.is_file() or path.is_symlink() or path.stat().st_size != size:
        return False
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return hmac.compare_digest(h.hexdigest(), digest)


def stage_artifact(stream: BinaryIO, destination_root: Path, artifact: dict,
                   manifest: dict, authentication: ManifestAuthentication) -> dict:
    if authentication.state not in AUTHENTICATED_STATES:
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "artifact use requires authenticated manifest")
    root, target = _safe_target(destination_root, artifact["filename"])
    expected_size, expected_digest = artifact["size_bytes"], artifact["sha256"]
    if target.exists():
        if _matches(target, expected_size, expected_digest):
            result = "NOOP_ALREADY_VERIFIED"
            return acquisition_result(result, target, artifact, manifest, authentication)
        raise AcquisitionError("DESTINATION_CONFLICT", "existing destination does not match authenticated artifact")
    temp_path: Path | None = None
    try:
        fd, name = tempfile.mkstemp(prefix=".synveil-download-", dir=root)
        temp_path = Path(name)
        digest, count = hashlib.sha256(), 0
        with os.fdopen(fd, "wb") as output:
            while True:
                chunk = stream.read(min(1024 * 1024, expected_size - count + 1))
                if not chunk:
                    break
                count += len(chunk)
                if count > expected_size:
                    raise AcquisitionError("ARTIFACT_TOO_LARGE", "artifact exceeds authenticated size")
                output.write(chunk); digest.update(chunk)
            output.flush(); os.fsync(output.fileno())
        if count != expected_size:
            raise AcquisitionError("ARTIFACT_TRUNCATED", "artifact ended before authenticated size")
        if not hmac.compare_digest(digest.hexdigest(), expected_digest):
            raise AcquisitionError("ARTIFACT_DIGEST_MISMATCH", "artifact digest mismatch")
        if target.exists() or target.is_symlink():
            raise AcquisitionError("DESTINATION_CONFLICT", "destination appeared during download")
        os.replace(temp_path, target)
        temp_path = None
        try:
            directory_fd = os.open(root, os.O_RDONLY)
            try: os.fsync(directory_fd)
            finally: os.close(directory_fd)
        except OSError:
            pass
        return acquisition_result("VERIFIED", target, artifact, manifest, authentication)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def acquisition_result(result: str, path: Path, artifact: dict, manifest: dict,
                       authentication: ManifestAuthentication) -> dict:
    return {"schema_version": 1, "manifest_identity": {"sha256": authentication.manifest_sha256,
            "size_bytes": authentication.manifest_size_bytes}, "manifest_authentication": {
            "state": authentication.state, "method": authentication.method, "key_id": authentication.key_id},
            "artifact_id": artifact["id"], "artifact_type": artifact["artifact_type"], "role": artifact["role"],
            "platform": artifact["platform"], "architecture": artifact["architecture"],
            "product_version": manifest["product_version"], "source_commit": manifest["source_commit"],
            "artifact_size": artifact["size_bytes"], "artifact_sha256": artifact["sha256"],
            "result": result, "final_path": str(path), "diagnostics_redacted": []}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path); parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--expected-version", required=True); parser.add_argument("--expected-source-commit")
    args = parser.parse_args()
    try:
        raw = args.manifest.read_bytes()
        policy = ReleaseTrustPolicy(frozenset(), frozenset(), expected_manifest_sha256=args.expected_manifest_sha256)
        auth = authenticate_manifest(raw, policy)
        document = parse_authenticated_manifest(raw, auth, args.expected_version, args.expected_source_commit)
        print(json.dumps({"authentication": auth.__dict__, "manifest": document}, sort_keys=True))
        return 0
    except (OSError, AcquisitionError) as error:
        code = getattr(error, "code", "ERROR")
        print(json.dumps({"result": "FAILED", "error": code, "diagnostics_redacted": []}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
