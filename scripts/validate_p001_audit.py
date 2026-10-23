#!/usr/bin/env python3
"""Validate the scoped P001 audit docs without touching product state."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_FILES = {
    "docs/v0.3/audits/P001_V02_BASELINE_AUDIT.md",
    "docs/v0.3/audits/P001_COMPATIBILITY_BASELINE.md",
    "docs/v0.3/manifests/PROMPT001_MANIFEST.md",
    "scripts/validate_p001_audit.py",
}
REQUIRED_HEADINGS = {
    "docs/v0.3/audits/P001_V02_BASELINE_AUDIT.md": [
        "## Git preflight and immutable baseline",
        "## Audit A — v0.2 release state",
        "## Audit B — repository architecture and contracts",
        "## Audit C — installer and deployment",
        "## Audit D — persistent state and migrations",
        "## Audit E — compatibility and interface baseline",
        "## Audit F — existing security boundaries",
        "## CI, source validation and scope control",
        "## Remaining blockers and unknowns",
        "## P001 audit gate",
    ],
    "docs/v0.3/audits/P001_COMPATIBILITY_BASELINE.md": [
        "## Platform and client matrix",
        "## Major capability matrix",
        "## Compatibility invariants to preserve",
        "## Compatibility gaps and unknowns",
    ],
    "docs/v0.3/manifests/PROMPT001_MANIFEST.md": [
        "## Prompt identity and scope",
        "## Baseline SHAs and Git state",
        "## Files changed",
        "## Evidence inventory",
        "## Validation commands and outcomes",
        "## Verified findings",
        "## Risks, blockers and unresolved questions",
        "## PR URL and head SHA",
        "## Final audit gate",
    ],
}
MARKDOWN_LINK = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
RUN_URL = re.compile(r"https://github\.com/[^\s)]+/actions/runs/\d+")
SHA = re.compile(r"\b[0-9a-f]{40}\b")


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=ROOT, check=False, text=True, capture_output=True
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout


def changed_paths(base: str) -> set[str]:
    committed = set(run_git("diff", "--name-only", f"{base}...HEAD").splitlines())
    staged = set(run_git("diff", "--cached", "--name-only").splitlines())
    worktree = set(run_git("diff", "--name-only").splitlines())
    untracked = set(
        run_git("ls-files", "--others", "--exclude-standard").splitlines()
    )
    return {path for path in committed | staged | worktree | untracked if path}


def validate_docs(allow_pending_pr: bool) -> list[str]:
    errors: list[str] = []
    for relative, headings in REQUIRED_HEADINGS.items():
        path = ROOT / relative
        if not path.is_file():
            errors.append(f"missing artifact: {relative}")
            continue
        body = path.read_text(encoding="utf-8")
        for heading in headings:
            if heading not in body:
                errors.append(f"{relative}: missing required section {heading!r}")
        for target in MARKDOWN_LINK.findall(body):
            target = target.strip().split("#", 1)[0]
            if not target or re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target):
                continue
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                errors.append(f"{relative}: broken local evidence link {target!r}")
    manifest = ROOT / "docs/v0.3/manifests/PROMPT001_MANIFEST.md"
    if manifest.is_file():
        body = manifest.read_text(encoding="utf-8")
        if not SHA.search(body):
            errors.append("manifest: no full commit SHA recorded")
        if not RUN_URL.search(body):
            errors.append("manifest: no GitHub Actions run URL recorded")
        if "PENDING_OPEN" in body and not allow_pending_pr:
            errors.append("manifest: PR details are still marked PENDING_OPEN")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--base", default="origin/develop/v0.3", help="verified base ref for scope check"
    )
    parser.add_argument(
        "--allow-pending-pr",
        action="store_true",
        help="allow the pre-PR documentation checkpoint before PR creation",
    )
    args = parser.parse_args()

    errors = validate_docs(args.allow_pending_pr)
    try:
        changed = changed_paths(args.base)
    except RuntimeError as error:
        errors.append(str(error))
        changed = set()
    unexpected = changed - EXPECTED_FILES
    missing = EXPECTED_FILES - changed
    if unexpected:
        errors.append("unexpected changed paths: " + ", ".join(sorted(unexpected)))
    if missing:
        errors.append("expected audit files absent from change set: " + ", ".join(sorted(missing)))

    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("PASS: required audit sections and local evidence links are present")
    print("PASS: GitHub run URLs and full SHAs are recorded")
    print("PASS: only the four scoped audit files differ from the verified base")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
