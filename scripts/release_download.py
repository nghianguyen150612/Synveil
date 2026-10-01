#!/usr/bin/env python3
"""Fail-closed release manifest authentication and artifact staging (stdlib only)."""

from __future__ import annotations

import argparse
import hashlib
import hmac
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
NETWORK_TIMEOUT_SECONDS = 30
CHUNK_SIZE = 1024 * 1024
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


@dataclass(frozen=True)
class AuthenticatedManifest:
    """A validated document bound to the exact bytes which were authenticated."""

    raw_bytes: bytes
    document: dict
    authentication: ManifestAuthentication


@dataclass(frozen=True)
class SelectedArtifact:
    """An artifact identity inseparable from its authenticated manifest context."""

    manifest: AuthenticatedManifest
    artifact_index: int
    artifact: dict


class RawHttpsTransport:
    """Constrained transport whose request method must expose redirect responses.

    Injection is intentionally limited to this type. Unlike urllib opener injection,
    this boundary cannot accidentally accept an opener configured to auto-follow.
    """

    def request(self, request: urllib.request.Request):
        raise NotImplementedError


class UrllibNoRedirectTransport(RawHttpsTransport):
    class _NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None

    def __init__(self):
        self._opener = urllib.request.build_opener(self._NoRedirect())

    def request(self, request: urllib.request.Request):
        try:
            return self._opener.open(request, timeout=NETWORK_TIMEOUT_SECONDS)
        except urllib.error.HTTPError as error:
            return error
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            raise AcquisitionError("NETWORK_ERROR", "HTTPS request failed") from error


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


def _transport(value: RawHttpsTransport | None) -> RawHttpsTransport:
    if value is None:
        return UrllibNoRedirectTransport()
    if not isinstance(value, RawHttpsTransport):
        raise TypeError("transport must be a RawHttpsTransport")
    return value


def open_https(url: str, allowed: frozenset[str], max_redirects: int = MAX_REDIRECTS,
               transport: RawHttpsTransport | None = None):
    """Open a response after locally evaluating each exposed redirect."""
    raw_transport = _transport(transport)
    seen: set[str] = set()
    current = url
    for redirects in range(max_redirects + 1):
        require_allowed_url(current, allowed)
        if current in seen:
            raise AcquisitionError("UNSAFE_REDIRECT", "redirect loop")
        seen.add(current)
        response = raw_transport.request(urllib.request.Request(current, method="GET"))
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
                raise AcquisitionError(
                    "UNSAFE_REDIRECT", "redirect target is not trusted HTTPS"
                ) from error
            continue
        if status != 200:
            response.close()
            raise AcquisitionError("NETWORK_ERROR", f"unexpected HTTP status {status}")
        return response
    raise AcquisitionError("UNSAFE_REDIRECT", "redirect limit exceeded")


def fetch_https(url: str, allowed: frozenset[str], limit: int,
                max_redirects: int = MAX_REDIRECTS,
                transport: RawHttpsTransport | None = None) -> bytes:
    """Fetch a small bounded resource (principally the release manifest)."""
    response = open_https(url, allowed, max_redirects, transport)
    try:
        data = response.read(limit + 1)
    finally:
        response.close()
    if len(data) > limit:
        raise AcquisitionError("MANIFEST_TOO_LARGE", "response exceeds configured limit")
    return data


def authenticate_manifest(raw: bytes, policy: ReleaseTrustPolicy, *,
                          signature_descriptor=None,
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
        entries = signature_descriptor.get("signatures", []) if isinstance(signature_descriptor, dict) else []
        eligible = [entry for entry in entries if isinstance(entry, dict)
                    and entry.get("scheme") in policy.allowed_signature_schemes
                    and entry.get("key_id") in policy.trusted_key_ids]
        if not eligible:
            raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "no signature is locally trusted and allowed")
        if verifier is None:
            raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "approved signature verifier is unavailable")
        eligible.sort(key=lambda entry: (entry["key_id"], entry["scheme"],
                                         entry.get("signature_filename", ""),
                                         json.dumps(entry, sort_keys=True)))
        for entry in eligible:
            if verifier(entry["key_id"], entry["scheme"], raw, entry):
                return ManifestAuthentication("AUTHENTICATED_SIGNATURE", "detached_signature",
                                              digest, len(raw), entry["key_id"])
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "all eligible detached signatures failed")
    raise AcquisitionError("UNSUPPORTED_AUTHENTICATION", "unknown manifest authentication method")


def _validate_context(context: AuthenticatedManifest) -> dict:
    authentication = context.authentication
    if (authentication.state not in AUTHENTICATED_STATES
            or authentication.manifest_size_bytes != len(context.raw_bytes)
            or not hmac.compare_digest(hashlib.sha256(context.raw_bytes).hexdigest(),
                                       authentication.manifest_sha256)):
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "exact manifest bytes are not authenticated")
    try:
        parsed = json.loads(context.raw_bytes.decode("utf-8"))
        release_manifest.validate(parsed)
    except (UnicodeError, json.JSONDecodeError, release_manifest.ManifestError) as error:
        raise AcquisitionError("INVALID_MANIFEST", "authenticated manifest is invalid") from error
    if parsed != context.document:
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "manifest document is detached from authenticated bytes")
    return parsed


def parse_authenticated_manifest(raw: bytes, authentication: ManifestAuthentication,
                                 expected_version: str | None = None,
                                 expected_source_commit: str | None = None) -> AuthenticatedManifest:
    # Parse only after proving the exact byte binding.
    if (authentication.state not in AUTHENTICATED_STATES
            or authentication.manifest_size_bytes != len(raw)
            or not hmac.compare_digest(hashlib.sha256(raw).hexdigest(), authentication.manifest_sha256)):
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
    return AuthenticatedManifest(bytes(raw), document, authentication)


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


def select_artifact(context: AuthenticatedManifest, *, platform: str, architecture: str,
                    artifact_type: str, role: str, expected_version: str) -> SelectedArtifact:
    manifest = _validate_context(context)
    if manifest["product_version"] != expected_version:
        raise AcquisitionError("VERSION_MISMATCH", "manifest product version differs from expectation")
    platform, architecture = normalize_platform(platform), normalize_architecture(architecture)
    matches = [(index, item) for index, item in enumerate(manifest["artifacts"]) if
               (item["platform"], item["architecture"], item["artifact_type"], item["role"], item["product_version"])
               == (platform, architecture, artifact_type, role, expected_version)]
    if not matches:
        raise AcquisitionError("NO_MATCHING_ARTIFACT", "no artifact has the requested exact identity")
    if len(matches) != 1:
        raise AcquisitionError("AMBIGUOUS_ARTIFACT", "more than one artifact has the requested identity")
    index, artifact = matches[0]
    return SelectedArtifact(context, index, artifact)


def _validate_selection(selection: SelectedArtifact) -> tuple[dict, dict]:
    manifest = _validate_context(selection.manifest)
    try:
        artifact = manifest["artifacts"][selection.artifact_index]
    except (IndexError, TypeError):
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "artifact is not in authenticated manifest") from None
    if artifact != selection.artifact:
        raise AcquisitionError("MANIFEST_AUTH_FAILED", "artifact is detached from authenticated manifest")
    return manifest, artifact


def trusted_artifact_url(base_url: str, selection: SelectedArtifact,
                         allowed: frozenset[str]) -> str:
    _, artifact = _validate_selection(selection)
    filename = artifact["filename"]
    if not release_manifest.safe_filename(filename):
        raise AcquisitionError("UNSAFE_PATH", "unsafe artifact filename")
    require_allowed_url(base_url, allowed)
    url = urllib.parse.urljoin(base_url.rstrip("/") + "/", urllib.parse.quote(filename))
    require_allowed_url(url, allowed)
    return url


def _safe_target(root: Path, filename: str) -> tuple[Path, Path]:
    if (not release_manifest.safe_filename(filename)
            or PureWindowsPath(filename).is_absolute()
            or PurePosixPath(filename).is_absolute()):
        raise AcquisitionError("UNSAFE_PATH", "unsafe artifact filename")
    supplied = Path(root)
    if supplied.is_symlink():
        raise AcquisitionError("UNSAFE_PATH", "destination root may not be a symlink")
    supplied.mkdir(parents=True, exist_ok=True)
    root = supplied.resolve()
    target = root / filename
    if target.is_symlink() or target.parent.resolve() != root:
        raise AcquisitionError("UNSAFE_PATH", "destination escapes trusted root")
    return root, target


def _matches(path: Path, size: int, digest: str) -> bool:
    if not path.is_file() or path.is_symlink() or path.stat().st_size != size:
        return False
    actual = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(CHUNK_SIZE), b""):
            actual.update(chunk)
    return hmac.compare_digest(actual.hexdigest(), digest)


def _existing_result(root: Path, artifact: dict) -> tuple[Path, str | None]:
    _, target = _safe_target(root, artifact["filename"])
    if target.exists() or target.is_symlink():
        if _matches(target, artifact["size_bytes"], artifact["sha256"]):
            return target, "NOOP_ALREADY_VERIFIED"
        raise AcquisitionError("DESTINATION_CONFLICT", "existing destination does not match authenticated artifact")
    return target, None


def stage_artifact(stream: BinaryIO, destination_root: Path,
                   selection: SelectedArtifact) -> dict:
    _, artifact = _validate_selection(selection)
    root, target = _safe_target(destination_root, artifact["filename"])
    expected_size, expected_digest = artifact["size_bytes"], artifact["sha256"]
    if target.exists() or target.is_symlink():
        if _matches(target, expected_size, expected_digest):
            return acquisition_result("NOOP_ALREADY_VERIFIED", target, selection)
        raise AcquisitionError("DESTINATION_CONFLICT", "existing destination does not match authenticated artifact")
    temp_path: Path | None = None
    try:
        fd, name = tempfile.mkstemp(prefix=".synveil-download-", dir=root)
        temp_path = Path(name)
        digest, count = hashlib.sha256(), 0
        with os.fdopen(fd, "wb") as output:
            while count <= expected_size:
                chunk = stream.read(min(CHUNK_SIZE, expected_size - count + 1))
                if not chunk:
                    break
                count += len(chunk)
                if count > expected_size:
                    raise AcquisitionError("ARTIFACT_TOO_LARGE", "artifact exceeds authenticated size")
                output.write(chunk)
                digest.update(chunk)
            output.flush()
            os.fsync(output.fileno())
        if count != expected_size:
            raise AcquisitionError("ARTIFACT_TRUNCATED", "artifact ended before authenticated size")
        if not hmac.compare_digest(digest.hexdigest(), expected_digest):
            raise AcquisitionError("ARTIFACT_DIGEST_MISMATCH", "artifact digest mismatch")
        try:
            os.link(temp_path, target)
        except FileExistsError as error:
            raise AcquisitionError("DESTINATION_CONFLICT", "destination appeared during download") from error
        except OSError as error:
            raise AcquisitionError("STAGING_ERROR", "atomic no-clobber promotion is unavailable") from error
        temp_path.unlink()
        temp_path = None
        try:
            directory_fd = os.open(root, os.O_RDONLY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        except OSError:
            pass
        return acquisition_result("VERIFIED", target, selection)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def download_artifact(base_url: str, destination_root: Path,
                      selection: SelectedArtifact, policy: ReleaseTrustPolicy, *,
                      transport: RawHttpsTransport | None = None) -> dict:
    """Preflight authenticated state and destination, then stream verified bytes."""
    _, artifact = _validate_selection(selection)
    target, existing = _existing_result(destination_root, artifact)
    if existing:
        return acquisition_result(existing, target, selection)
    url = trusted_artifact_url(base_url, selection, policy.allowed_artifact_origins)
    response = open_https(url, policy.allowed_artifact_origins, policy.max_redirects, transport)
    try:
        return stage_artifact(response, destination_root, selection)
    finally:
        response.close()


def acquisition_result(result: str, path: Path, selection: SelectedArtifact) -> dict:
    manifest, artifact = _validate_selection(selection)
    authentication = selection.manifest.authentication
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
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--expected-source-commit")
    args = parser.parse_args()
    try:
        raw = args.manifest.read_bytes()
        policy = ReleaseTrustPolicy(frozenset(), frozenset(), expected_manifest_sha256=args.expected_manifest_sha256)
        authentication = authenticate_manifest(raw, policy)
        context = parse_authenticated_manifest(raw, authentication, args.expected_version,
                                               args.expected_source_commit)
        print(json.dumps({"authentication": authentication.__dict__,
                          "manifest": context.document}, sort_keys=True))
        return 0
    except (OSError, AcquisitionError) as error:
        code = getattr(error, "code", "ERROR")
        print(json.dumps({"result": "FAILED", "error": code,
                          "diagnostics_redacted": []}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
