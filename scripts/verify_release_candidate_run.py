#!/usr/bin/env python3
"""Verify that an Actions artifact is the exact assembled release candidate.

Native acceptance workflows use this before copying candidate bytes into their
platform-specific harness. The run ID is fetched from GitHub and bound to the
candidate manifest; no runner-local rebuild can silently replace the subject.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

import release_manifest

EXPECTED_WORKFLOW = ".github/workflows/release-artifact-candidate.yml"
EXPECTED_ARTIFACT = "synveil-release-candidate"
EXPECTED_PRODUCT_VERSION = "0.2.0"
EXPECTED_TYPES = {"appimage", "deb", "rpm", "windows_installer", "windows_portable_zip"}
SHA1 = re.compile(r"[0-9a-f]{40}\Z")


class CandidateError(ValueError):
    """Candidate provenance or bytes do not meet the acceptance contract."""


def validate_run_record(run: object, run_id: str, repository: str) -> dict:
    if not isinstance(run, dict):
        raise CandidateError("candidate run metadata is not an object")
    if str(run.get("id")) != run_id:
        raise CandidateError("GitHub returned a different candidate run ID")
    workflow_path = run.get("path")
    if not isinstance(workflow_path, str) or workflow_path.split("@", 1)[0] != EXPECTED_WORKFLOW:
        raise CandidateError("candidate run was not produced by the release-artifact workflow")
    if run.get("event") != "workflow_dispatch":
        raise CandidateError("candidate run must be an explicitly dispatched release-artifact workflow")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise CandidateError("candidate producer workflow did not complete successfully")
    head_sha = run.get("head_sha")
    if not isinstance(head_sha, str) or SHA1.fullmatch(head_sha) is None:
        raise CandidateError("candidate producer source SHA is invalid")
    head_repository = run.get("head_repository")
    if not isinstance(head_repository, dict) or head_repository.get("full_name") != repository:
        raise CandidateError("candidate was not produced from this repository")
    return {"run_id": run_id, "source_commit": head_sha, "repository": repository}


def validate_candidate_files(root: Path, source_commit: str) -> dict:
    if root.is_symlink() or not root.is_dir():
        raise CandidateError("candidate artifact root must be a real directory")
    root = root.resolve(strict=True)
    manifest_path = root / "SYNVEIL-RELEASE-MANIFEST.json"
    evidence_path = root / "candidate-evidence.json"
    for path in (manifest_path, evidence_path):
        try:
            info = path.lstat()
        except OSError as error:
            raise CandidateError("candidate manifest or producer evidence is missing") from error
        if not path.is_file() or path.is_symlink() or info.st_nlink != 1:
            raise CandidateError("candidate manifest or producer evidence is redirected")
    try:
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        manifest = release_manifest.load(manifest_path)
    except (OSError, UnicodeError, json.JSONDecodeError, release_manifest.ManifestError) as error:
        raise CandidateError("candidate manifest or producer evidence is missing or invalid") from error

    try:
        release_manifest.validate(manifest, root, source_commit)
    except release_manifest.ManifestError as error:
        raise CandidateError(f"candidate manifest or product bytes failed validation: {error}") from error

    if not isinstance(evidence, dict) or evidence.get("schema_version") != 1:
        raise CandidateError("candidate producer evidence has an unsupported schema")
    if evidence.get("status") != "VERIFIED_UNSIGNED_CANDIDATE":
        raise CandidateError("candidate must be the explicitly unsigned assembled candidate")
    if evidence.get("source_commit") != source_commit:
        raise CandidateError("candidate evidence source differs from the producer run")
    if evidence.get("product_version") != manifest.get("product_version"):
        raise CandidateError("candidate evidence product version differs from the manifest")
    if manifest.get("product_version") != EXPECTED_PRODUCT_VERSION:
        raise CandidateError("candidate product version is not the planned v0.2.0 identity")

    artifacts = manifest.get("artifacts")
    if (not isinstance(artifacts, list)
            or {item.get("artifact_type") for item in artifacts if isinstance(item, dict)} != EXPECTED_TYPES
            or len(artifacts) != len(EXPECTED_TYPES)):
        raise CandidateError("candidate must contain exactly the five required desktop artifacts")
    evidence_artifacts = evidence.get("artifacts")
    if not isinstance(evidence_artifacts, list) or len(evidence_artifacts) != len(EXPECTED_TYPES):
        raise CandidateError("candidate evidence does not contain exactly five artifact identities")
    by_filename = {item.get("filename"): item for item in evidence_artifacts if isinstance(item, dict)}
    if len(by_filename) != len(evidence_artifacts):
        raise CandidateError("candidate evidence contains invalid or duplicate artifact identities")
    records = []
    for item in artifacts:
        recorded = by_filename.get(item["filename"])
        identity = {key: item[key] for key in ("id", "artifact_type", "filename", "platform", "architecture", "size_bytes", "sha256")}
        if identity["architecture"] != "x86_64":
            raise CandidateError(f"candidate architecture is not x86_64: {identity['filename']}")
        if recorded is None or any(
            recorded.get(key) != item[value]
            for key, value in (("artifact_type", "artifact_type"), ("filename", "filename"),
                               ("size_bytes", "size_bytes"), ("sha256", "sha256"))
        ):
            raise CandidateError(f"candidate evidence differs from manifest: {identity['filename']}")
        records.append(identity)

    return {
        "schema_version": 1,
        "status": "VERIFIED_UNSIGNED_CANDIDATE",
        "producer_workflow": EXPECTED_WORKFLOW,
        "producer_run_id": None,
        "source_commit": source_commit,
        "product_version": manifest["product_version"],
        "manifest_filename": manifest_path.name,
        "manifest_size_bytes": manifest_path.stat().st_size,
        "manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        "artifacts": records,
    }


def fetch_run(run_id: str, repository: str) -> dict:
    if not run_id.isdecimal() or len(run_id) > 20:
        raise CandidateError("candidate run ID must contain only decimal digits")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise CandidateError("repository must be an owner/name pair")
    token = os.environ.get("GITHUB_TOKEN", "")
    api_url = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
    if not token:
        raise CandidateError("GITHUB_TOKEN is unavailable for candidate provenance lookup")
    request = Request(
        f"{api_url}/repos/{repository}/actions/runs/{run_id}",
        headers={"Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}"},
    )
    try:
        with urlopen(request, timeout=20) as response:
            payload = response.read(1_048_577)
    except (HTTPError, URLError, TimeoutError, OSError) as error:
        raise CandidateError("candidate producer run metadata could not be retrieved") from error
    if len(payload) > 1_048_576:
        raise CandidateError("candidate producer run metadata exceeds the size limit")
    try:
        run = json.loads(payload.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise CandidateError("candidate producer run metadata is invalid JSON") from error
    return validate_run_record(run, run_id, repository)


def verify(
    run_id: str,
    artifact_name: str,
    repository: str,
    expected_source_commit: str,
    root: Path,
) -> dict:
    if artifact_name != EXPECTED_ARTIFACT:
        raise CandidateError(f"native acceptance only accepts {EXPECTED_ARTIFACT}")
    if not isinstance(expected_source_commit, str) or SHA1.fullmatch(expected_source_commit) is None:
        raise CandidateError("expected acceptance source SHA is invalid")
    run_identity = fetch_run(run_id, repository)
    if run_identity["source_commit"] != expected_source_commit:
        raise CandidateError("candidate source differs from the native acceptance workflow revision")
    candidate = validate_candidate_files(root, run_identity["source_commit"])
    candidate["producer_run_id"] = run_id
    candidate["repository"] = repository
    return candidate


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--artifact-name", required=True)
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--expected-source-commit", required=True)
    parser.add_argument("--artifact-root", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        result = verify(
            args.run_id,
            args.artifact_name,
            args.repository,
            args.expected_source_commit,
            args.artifact_root,
        )
        encoded = json.dumps(result, sort_keys=True, indent=2) + "\n"
        if args.output:
            args.output.write_text(encoded, encoding="utf-8", newline="\n")
        print(encoded, end="")
        return 0
    except (CandidateError, release_manifest.ManifestError, OSError, ValueError, TypeError) as error:
        print(f"release-candidate-verification: BLOCKED_OR_REJECTED: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
