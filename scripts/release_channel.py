#!/usr/bin/env python3
"""Authenticated stable-channel validation and deterministic release selection."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

import release_download
import release_manifest

SCHEMA_VERSION = 1
AUTH_SCHEMA_VERSION = 1
CHANNEL_FILENAME = "SYNVEIL-RELEASE-CHANNEL.json"
CHANNEL_AUTH_FILENAME = "SYNVEIL-RELEASE-CHANNEL-AUTH.json"
MANIFEST_FILENAME = "SYNVEIL-RELEASE-MANIFEST.json"
MAX_CHANNEL_BYTES = 1_048_576
MAX_RELEASES = 256
MAX_UPGRADE_SOURCES_PER_RELEASE = 256
MAX_GENERATION = 2**63 - 1
TOP_FIELDS = {"schema_version", "product", "channel", "generation", "releases"}
RELEASE_FIELDS = {"product_version", "source_commit", "manifest_filename", "manifest_size_bytes", "manifest_sha256", "fresh_install", "upgrade_from"}
AUTH_FIELDS = {"schema_version", "channel_filename", "channel_size_bytes", "channel_sha256", "signatures"}
SIGNATURE_FIELDS = {"scheme", "key_id", "signature_filename"}
AUTHENTICATED_STATES = {"AUTHENTICATED_PINNED_DIGEST", "AUTHENTICATED_SIGNATURE"}
VERSION_RE = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
HEX40_RE = re.compile(r"[0-9a-f]{40}")
HEX64_RE = re.compile(r"[0-9a-f]{64}")
TOKEN_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")


class ChannelError(ValueError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class ChannelTrustPolicy:
    authentication_method: str = "pinned_sha256"
    expected_channel_sha256: str | None = None
    trusted_key_ids: frozenset[str] = field(default_factory=frozenset)
    allowed_signature_schemes: frozenset[str] = field(default_factory=frozenset)
    minimum_generation: int = 1
    max_channel_bytes: int = MAX_CHANNEL_BYTES


@dataclass(frozen=True)
class ChannelAuthentication:
    state: str
    method: str
    channel_sha256: str
    channel_size_bytes: int
    key_id: str | None = None
    scheme: str | None = None


@dataclass(frozen=True)
class AuthenticatedChannel:
    raw_bytes: bytes
    document: dict
    authentication: ChannelAuthentication


@dataclass(frozen=True)
class ChannelHighWater:
    generation: int
    channel_sha256: str


@dataclass(frozen=True)
class SelectedRelease:
    authenticated_channel: AuthenticatedChannel
    release_index: int
    release: dict
    selection_mode: str
    current_version: str | None = None


@dataclass(frozen=True)
class SelectionResult:
    outcome: str
    selected: SelectedRelease | None = None


def fail(code: str, message: str):
    raise ChannelError(code, message)


def parse_stable_version(value: object) -> tuple[int, int, int]:
    if not isinstance(value, str) or not (match := VERSION_RE.fullmatch(value)):
        fail("INVALID_VERSION", "version must be stable MAJOR.MINOR.PATCH without leading zeroes")
    return tuple(map(int, match.groups()))


def _fields(value: object, exact: set[str], where: str) -> dict:
    if not isinstance(value, dict) or set(value) != exact:
        fail("INVALID_CHANNEL", f"{where} must contain exactly {sorted(exact)}")
    return value


def validate_auth_descriptor(value: object) -> dict:
    data = _fields(value, AUTH_FIELDS, "channel authentication descriptor")
    if type(data["schema_version"]) is not int or data["schema_version"] != AUTH_SCHEMA_VERSION:
        fail("UNSUPPORTED_CHANNEL_AUTHENTICATION", "unsupported channel authentication schema")
    if data["channel_filename"] != CHANNEL_FILENAME:
        fail("INVALID_CHANNEL", "authentication descriptor has wrong channel filename")
    if type(data["channel_size_bytes"]) is not int or not 0 <= data["channel_size_bytes"] <= MAX_CHANNEL_BYTES:
        fail("INVALID_CHANNEL", "invalid authenticated channel size")
    if not isinstance(data["channel_sha256"], str) or not HEX64_RE.fullmatch(data["channel_sha256"]):
        fail("INVALID_CHANNEL", "invalid authenticated channel digest")
    if not isinstance(data["signatures"], list) or not data["signatures"]:
        fail("INVALID_CHANNEL", "at least one signature is required")
    for entry in data["signatures"]:
        entry = _fields(entry, SIGNATURE_FIELDS, "signature")
        if any(not isinstance(entry[k], str) or not TOKEN_RE.fullmatch(entry[k]) for k in SIGNATURE_FIELDS):
            fail("INVALID_CHANNEL", "unsafe signature metadata")
    return data


def validate_channel(value: object) -> dict:
    data = _fields(value, TOP_FIELDS, "channel")
    if type(data["schema_version"]) is not int or data["schema_version"] != SCHEMA_VERSION:
        fail("UNSUPPORTED_CHANNEL_SCHEMA", "unsupported channel schema")
    if data["product"] != "Synveil" or data["channel"] != "stable":
        fail("UNSUPPORTED_CHANNEL", "only the Synveil stable channel is supported")
    if type(data["generation"]) is not int or not 1 <= data["generation"] <= MAX_GENERATION:
        fail("INVALID_CHANNEL", "generation must be a positive bounded integer")
    releases = data["releases"]
    if not isinstance(releases, list) or len(releases) > MAX_RELEASES:
        fail("INVALID_CHANNEL", "invalid release collection size")
    prior = None
    for item in releases:
        item = _fields(item, RELEASE_FIELDS, "release")
        version = parse_stable_version(item["product_version"])
        if prior is not None and version <= prior:
            fail("INVALID_RELEASE_POLICY", "releases must be strictly ascending and unique")
        prior = version
        if not isinstance(item["source_commit"], str) or not HEX40_RE.fullmatch(item["source_commit"]):
            fail("INVALID_CHANNEL", "invalid source commit")
        if item["manifest_filename"] != MANIFEST_FILENAME:
            fail("INVALID_CHANNEL", "invalid manifest filename")
        if type(item["manifest_size_bytes"]) is not int or not 1 <= item["manifest_size_bytes"] <= release_download.MAX_MANIFEST_BYTES:
            fail("INVALID_CHANNEL", "invalid manifest size")
        if not isinstance(item["manifest_sha256"], str) or not HEX64_RE.fullmatch(item["manifest_sha256"]):
            fail("INVALID_CHANNEL", "invalid manifest digest")
        if type(item["fresh_install"]) is not bool:
            fail("INVALID_RELEASE_POLICY", "fresh_install must be boolean")
        sources = item["upgrade_from"]
        if not isinstance(sources, list) or len(sources) > MAX_UPGRADE_SOURCES_PER_RELEASE:
            fail("INVALID_RELEASE_POLICY", "invalid upgrade source collection size")
        parsed = [parse_stable_version(source) for source in sources]
        if parsed != sorted(set(parsed)) or any(source >= version for source in parsed):
            fail("INVALID_RELEASE_POLICY", "upgrade_from must be sorted, unique, and older than target")
    return data


def authenticate_channel(raw: bytes, policy: ChannelTrustPolicy, *, descriptor=None,
                         verifier: Callable | None = None) -> ChannelAuthentication:
    if not isinstance(raw, bytes) or len(raw) > policy.max_channel_bytes:
        fail("CHANNEL_TOO_LARGE", "channel exceeds configured byte limit")
    digest = hashlib.sha256(raw).hexdigest()
    if policy.authentication_method == "pinned_sha256":
        pin = policy.expected_channel_sha256
        if not isinstance(pin, str) or not HEX64_RE.fullmatch(pin) or not hmac.compare_digest(pin, digest):
            fail("CHANNEL_AUTH_FAILED", "channel digest does not match trusted pin")
        return ChannelAuthentication("AUTHENTICATED_PINNED_DIGEST", "pinned_sha256", digest, len(raw))
    if policy.authentication_method != "detached_signature":
        fail("UNSUPPORTED_CHANNEL_AUTHENTICATION", "unknown channel authentication method")
    descriptor = validate_auth_descriptor(descriptor)
    if descriptor["channel_size_bytes"] != len(raw) or not hmac.compare_digest(descriptor["channel_sha256"], digest):
        fail("CHANNEL_AUTH_FAILED", "descriptor does not bind exact channel bytes")
    eligible = [entry for entry in descriptor["signatures"] if entry["scheme"] in policy.allowed_signature_schemes and entry["key_id"] in policy.trusted_key_ids]
    if not eligible or verifier is None:
        fail("UNSUPPORTED_CHANNEL_AUTHENTICATION", "no locally trusted signature verifier is available")
    eligible.sort(key=lambda entry: (entry["key_id"], entry["scheme"], entry["signature_filename"]))
    for entry in eligible:
        if verifier(entry["key_id"], entry["scheme"], raw, entry):
            return ChannelAuthentication(
                "AUTHENTICATED_SIGNATURE",
                "detached_signature",
                digest,
                len(raw),
                entry["key_id"],
                entry["scheme"],
            )
    fail("CHANNEL_AUTH_FAILED", "all eligible channel signatures failed")


def parse_authenticated_channel(raw: bytes, authentication: ChannelAuthentication,
                                policy: ChannelTrustPolicy) -> AuthenticatedChannel:
    if not isinstance(raw, bytes) or len(raw) > policy.max_channel_bytes:
        fail("CHANNEL_TOO_LARGE", "channel exceeds configured byte limit")
    if type(policy.minimum_generation) is not int or not 1 <= policy.minimum_generation <= MAX_GENERATION:
        fail("INVALID_RELEASE_POLICY", "trusted minimum generation is invalid")
    digest = hashlib.sha256(raw).hexdigest()
    if authentication.state not in AUTHENTICATED_STATES or authentication.channel_size_bytes != len(raw) or not hmac.compare_digest(authentication.channel_sha256, digest):
        fail("CHANNEL_AUTH_FAILED", "exact channel bytes are not authenticated")
    if authentication.method != policy.authentication_method:
        fail("CHANNEL_AUTH_FAILED", "authentication evidence does not match local trust policy")
    if policy.authentication_method == "pinned_sha256":
        pin = policy.expected_channel_sha256
        if (authentication.state != "AUTHENTICATED_PINNED_DIGEST"
                or not isinstance(pin, str)
                or not HEX64_RE.fullmatch(pin)
                or not hmac.compare_digest(pin, digest)
                or authentication.key_id is not None
                or authentication.scheme is not None):
            fail("CHANNEL_AUTH_FAILED", "pinned channel evidence does not match local trust policy")
    elif policy.authentication_method == "detached_signature":
        if (authentication.state != "AUTHENTICATED_SIGNATURE"
                or not isinstance(authentication.key_id, str)
                or authentication.key_id not in policy.trusted_key_ids
                or not isinstance(authentication.scheme, str)
                or authentication.scheme not in policy.allowed_signature_schemes):
            fail("CHANNEL_AUTH_FAILED", "signature evidence does not match local trust policy")
    else:
        fail("UNSUPPORTED_CHANNEL_AUTHENTICATION", "unknown channel authentication method")
    try:
        document = json.loads(raw.decode("utf-8"))
        validate_channel(document)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ChannelError("INVALID_CHANNEL", "authenticated channel is not valid UTF-8 JSON") from error
    if document["generation"] < policy.minimum_generation:
        fail("CHANNEL_ROLLBACK", "channel generation is below trusted minimum")
    return AuthenticatedChannel(bytes(raw), document, authentication)


def _validated_context(context: AuthenticatedChannel) -> dict:
    authentication = context.authentication
    if authentication.method == "pinned_sha256":
        policy = ChannelTrustPolicy(
            authentication_method="pinned_sha256",
            expected_channel_sha256=authentication.channel_sha256,
            minimum_generation=1,
        )
    elif authentication.method == "detached_signature":
        policy = ChannelTrustPolicy(
            authentication_method="detached_signature",
            trusted_key_ids=frozenset({authentication.key_id}) if authentication.key_id else frozenset(),
            allowed_signature_schemes=frozenset({authentication.scheme}) if authentication.scheme else frozenset(),
            minimum_generation=1,
        )
    else:
        fail("UNSUPPORTED_CHANNEL_AUTHENTICATION", "unknown channel authentication method")
    parsed = parse_authenticated_channel(context.raw_bytes, authentication, policy)
    if parsed.document != context.document:
        fail("CHANNEL_AUTH_FAILED", "channel document is detached from authenticated bytes")
    return parsed.document


def evaluate_high_water(context: AuthenticatedChannel, stored: ChannelHighWater | None) -> ChannelHighWater:
    document = _validated_context(context)
    if stored is not None:
        if (type(stored.generation) is not int
                or not 1 <= stored.generation <= MAX_GENERATION
                or not isinstance(stored.channel_sha256, str)
                or not HEX64_RE.fullmatch(stored.channel_sha256)):
            fail("INVALID_RELEASE_POLICY", "stored channel high-water state is invalid")
    incoming = ChannelHighWater(document["generation"], context.authentication.channel_sha256)
    if stored is None or incoming.generation > stored.generation:
        return incoming
    if incoming.generation < stored.generation:
        fail("CHANNEL_ROLLBACK", "channel generation regressed")
    if not hmac.compare_digest(incoming.channel_sha256, stored.channel_sha256):
        fail("CHANNEL_EQUIVOCATION", "same generation has different authenticated bytes")
    return stored


def _selected(context, index, mode, current=None):
    release = context.document["releases"][index]
    return SelectedRelease(context, index, release, mode, current)


def select_fresh_install(context: AuthenticatedChannel,
                         stored_high_water: ChannelHighWater | None) -> SelectionResult:
    evaluate_high_water(context, stored_high_water)
    document = _validated_context(context)
    eligible = [index for index, item in enumerate(document["releases"]) if item["fresh_install"]]
    return SelectionResult("SELECTED", _selected(context, eligible[-1], "fresh_install")) if eligible else SelectionResult("NO_RELEASE_AVAILABLE")


def select_upgrade(context: AuthenticatedChannel, current_version: str,
                   stored_high_water: ChannelHighWater | None) -> SelectionResult:
    evaluate_high_water(context, stored_high_water)
    document = _validated_context(context)
    current = parse_stable_version(current_version)
    eligible = [index for index, item in enumerate(document["releases"])
                if parse_stable_version(item["product_version"]) > current and current_version in item["upgrade_from"]]
    return SelectionResult("SELECTED", _selected(context, eligible[-1], "upgrade", current_version)) if eligible else SelectionResult("NO_NEWER_COMPATIBLE_RELEASE")


def validate_selected_release(selected: SelectedRelease) -> dict:
    document = _validated_context(selected.authenticated_channel)
    if type(selected.release_index) is not int or not 0 <= selected.release_index < len(document["releases"]):
        fail("SELECTED_RELEASE_DETACHED", "release index is not in authenticated channel")
    release = document["releases"][selected.release_index]
    if release != selected.release:
        fail("SELECTED_RELEASE_DETACHED", "release is detached from authenticated channel")
    if selected.selection_mode == "fresh_install":
        if selected.current_version is not None or not release["fresh_install"]:
            fail("SELECTED_RELEASE_DETACHED", "fresh-install selection is not eligible")
    elif selected.selection_mode == "upgrade":
        if not isinstance(selected.current_version, str):
            fail("SELECTED_RELEASE_DETACHED", "upgrade selection lacks exact current version")
        try:
            current = parse_stable_version(selected.current_version)
        except ChannelError as error:
            raise ChannelError("SELECTED_RELEASE_DETACHED", "upgrade selection has invalid current version") from error
        target = parse_stable_version(release["product_version"])
        if target <= current or selected.current_version not in release["upgrade_from"]:
            fail("SELECTED_RELEASE_DETACHED", "upgrade selection is not compatible with current version")
    else:
        fail("SELECTED_RELEASE_DETACHED", "unknown selection mode")
    return release


def selection_evidence(selected: SelectedRelease) -> dict:
    release = validate_selected_release(selected)
    context, auth = selected.authenticated_channel, selected.authenticated_channel.authentication
    return {"schema_version": 1, "channel": "stable", "channel_generation": context.document["generation"],
            "channel_sha256": auth.channel_sha256, "channel_authentication_state": auth.state,
            "channel_authentication_method": auth.method, "channel_key_id": auth.key_id,
            "selection_mode": selected.selection_mode, "current_version": selected.current_version,
            "selected_product_version": release["product_version"], "selected_source_commit": release["source_commit"],
            "manifest_filename": release["manifest_filename"], "manifest_size_bytes": release["manifest_size_bytes"],
            "manifest_sha256": release["manifest_sha256"]}


def authenticate_selected_manifest(selected: SelectedRelease, raw_manifest: bytes):
    release = validate_selected_release(selected)
    if len(raw_manifest) != release["manifest_size_bytes"]:
        fail("MANIFEST_BINDING_MISMATCH", "manifest size differs from selected release")
    policy = release_download.ReleaseTrustPolicy(frozenset(), frozenset(), expected_manifest_sha256=release["manifest_sha256"])
    try:
        authentication = release_download.authenticate_manifest(raw_manifest, policy)
        return release_download.parse_authenticated_manifest(raw_manifest, authentication,
            expected_version=release["product_version"], expected_source_commit=release["source_commit"])
    except release_download.AcquisitionError as error:
        raise ChannelError("MANIFEST_BINDING_MISMATCH", str(error)) from error


def build_channel(generation: int, policies: list[tuple[Path, bool, list[str]]]) -> bytes:
    releases = []
    for path, fresh_install, upgrade_from in policies:
        raw = path.read_bytes()
        try:
            manifest = json.loads(raw.decode("utf-8"))
            release_manifest.validate(manifest)
        except (UnicodeError, json.JSONDecodeError, release_manifest.ManifestError) as error:
            raise ChannelError("INVALID_CHANNEL", f"invalid P005 manifest: {path.name}") from error
        releases.append({"product_version": manifest["product_version"], "source_commit": manifest["source_commit"],
            "manifest_filename": MANIFEST_FILENAME, "manifest_size_bytes": len(raw),
            "manifest_sha256": hashlib.sha256(raw).hexdigest(), "fresh_install": fresh_install,
            "upgrade_from": list(upgrade_from)})
    releases.sort(key=lambda item: parse_stable_version(item["product_version"]))
    document = {"schema_version": 1, "product": "Synveil", "channel": "stable", "generation": generation, "releases": releases}
    validate_channel(document)
    return (json.dumps(document, sort_keys=True, indent=2) + "\n").encode()


def _load(path: str):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("validate", "inspect"):
        command = sub.add_parser(name); command.add_argument("channel")
    create = sub.add_parser("create")
    create.add_argument("--generation", required=True, type=int); create.add_argument("--output", required=True)
    create.add_argument("--release", action="append", required=True, metavar="MANIFEST,FRESH,UPGRADE_FROM")
    for name in ("select-fresh", "select-upgrade"):
        command = sub.add_parser(name); command.add_argument("channel"); command.add_argument("--trusted-sha256", required=True)
        if name == "select-upgrade": command.add_argument("--current-version", required=True)
    args = parser.parse_args(argv)
    try:
        if args.command in {"validate", "inspect"}:
            document = validate_channel(_load(args.channel))
            print(json.dumps(document if args.command == "inspect" else {"status": "VALID", "release_count": len(document["releases"])}, sort_keys=True, indent=2)); return 0
        if args.command == "create":
            policies = []
            for spec in args.release:
                path, fresh, sources = (spec.split(",", 2) + [""])[:3]
                policies.append((Path(path), fresh.lower() == "true", [x for x in sources.split(";") if x]))
            Path(args.output).write_bytes(build_channel(args.generation, policies)); return 0
        raw = Path(args.channel).read_bytes(); policy = ChannelTrustPolicy(expected_channel_sha256=args.trusted_sha256)
        context = parse_authenticated_channel(raw, authenticate_channel(raw, policy), policy)
        result = select_fresh_install(context) if args.command == "select-fresh" else select_upgrade(context, args.current_version)
        print(json.dumps({"outcome": result.outcome, "evidence": selection_evidence(result.selected) if result.selected else None}, sort_keys=True, indent=2)); return 0
    except (ChannelError, OSError, json.JSONDecodeError) as error:
        print(f"release_channel: {getattr(error, 'code', 'INVALID_CHANNEL')}: {error}", file=sys.stderr); return 2


if __name__ == "__main__":
    raise SystemExit(main())
