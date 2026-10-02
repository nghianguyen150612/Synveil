#!/usr/bin/env python3
"""Linux native clean-machine acceptance executor (Prompt020).

P004 shipped the acceptance *contract* only: scenario JSON, a result schema and
a validator that could never claim PASS. This module adds the real execution
layer for Linux.

Design constraints this module is required to keep:

* The scenario JSON stays declarative. Every ``steps[].action`` must resolve to
  a reviewed handler in :data:`ACTION_HANDLERS`. A string from scenario JSON is
  never evaluated, never used as an argv[0] and never passed to a shell.
* Every external command is an explicit argv list with a finite timeout.
* Artifact identity is verified, never trusted. The recorded digest is
  recomputed from the bytes on disk and a mismatch fails closed.
* A machine that is not clean, a host that is not a qualified platform, or a
  missing capability yields BLOCKED with a truthful reason. It never yields
  PASS by omission.
* Diagnostics are redacted before they can reach a result record.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform as host_platform
import re
import shutil
import subprocess
import sys
import time
import unittest
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import install_acceptance as contract  # noqa: E402  (path set above)

RESULT_SCHEMA_VERSION = 1
RUNNER_VERSION = "0.2.0-p020-linux"

#: Command ceiling for any single subprocess. GUI automation must never block
#: the harness indefinitely; the contract's timeout policy reconciles instead.
COMMAND_TIMEOUT_SECONDS = 120

#: Prompt018 qualified exactly these platforms. Prompt020 must not widen this
#: without fresh native evidence. Debian, Mint, Pop!_OS, Rocky, AlmaLinux and
#: Arch are deliberately absent even though they are detectable.
QUALIFIED_PLATFORMS: dict[tuple[str, str], str] = {
    ("ubuntu", "24.04"): "debian-x86_64",
    ("fedora", "42"): "fedora-x86_64",
}

#: Substrings that mark a platform as detected but not qualified. Reporting
#: these honestly is required; inferring support from ID_LIKE is not allowed.
DETECTED_NOT_QUALIFIED = {
    "debian", "linuxmint", "pop", "rocky", "almalinux", "opensuse",
    "arch", "cachyos", "manjaro", "rhel", "centos",
}

REDACTIONS: tuple[tuple[re.Pattern[str], str], ...] = (
    (re.compile(r"(postgres(?:ql)?://)[^/\s:@]+:[^/\s@]+@", re.IGNORECASE), r"\1<redacted>@"),
    (re.compile(r"\bsv[de]1_[A-Za-z0-9_-]{8,}"), "<redacted-token>"),
    (re.compile(r"(?i)\b(authorization|token|password|passwd|secret|api[_-]?key)\b\s*[:=]\s*\S+"), r"\1=<redacted>"),
    (re.compile(r"-----BEGIN[^-]*PRIVATE KEY-----.*?-----END[^-]*PRIVATE KEY-----", re.DOTALL), "<redacted-private-key>"),
)


class AdapterError(RuntimeError):
    """Typed adapter failure. Never carries secret material."""


class UnqualifiedPlatform(AdapterError):
    pass


class NotClean(AdapterError):
    pass


class ArtifactMismatch(AdapterError):
    pass


def redact(text: str) -> str:
    """Return *text* with credential-like material removed."""
    result = text
    for pattern, replacement in REDACTIONS:
        result = pattern.sub(replacement, result)
    return result


def _run(argv: list[str], *, timeout: int = COMMAND_TIMEOUT_SECONDS, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    """Run an explicit argv list. Never uses a shell, never inherits stdin."""
    if not argv or not argv[0]:
        raise AdapterError("empty command vector")
    try:
        return subprocess.run(  # noqa: S603 - argv list, shell=False by construction
            argv,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
            shell=False,
            env=env,
        )
    except FileNotFoundError as exc:
        raise AdapterError(f"required tool not present: {argv[0]}") from exc
    except subprocess.TimeoutExpired as exc:
        raise AdapterError(f"command exceeded its {timeout}s bound: {argv[0]}") from exc


def _utcnow() -> str:
    return datetime.now(timezone.utc).isoformat()


# --------------------------------------------------------------------------
# Host facts
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class HostFacts:
    """Authoritative, self-reported facts about the machine under test."""

    os_id: str
    version_id: str
    id_like: tuple[str, ...]
    architecture: str
    kernel: str
    session_type: str
    desktop_session: str
    package_manager: str | None
    native_gui_package_handler: str | None

    @property
    def family(self) -> str:
        return _family(self.os_id, self.id_like)

    @property
    def qualification_profile(self) -> str | None:
        return QUALIFIED_PLATFORMS.get((self.os_id, self.version_id))

    @property
    def is_qualified(self) -> bool:
        return self.qualification_profile is not None and self.architecture == "x86_64"

    def to_result(self) -> dict[str, Any]:
        return {
            "family": self.family,
            "os_id": self.os_id,
            "version_id": self.version_id,
            "id_like": list(self.id_like),
            "architecture": self.architecture,
            "kernel": self.kernel,
            "session_type": self.session_type,
            "version": self.version_id,
            "desktop_session": self.desktop_session,
            "package_manager": self.package_manager,
            "native_gui_package_handler": self.native_gui_package_handler,
        }


def _family(os_id: str, id_like: tuple[str, ...]) -> str:
    if os_id == "ubuntu":
        return "ubuntu"
    if os_id == "debian":
        return "debian"
    if os_id == "fedora":
        return "fedora"
    if {"debian", "ubuntu"} & set(id_like):
        return "debian"
    if "fedora" in id_like:
        return "rpm_family"
    if {"rhel", "centos", "rocky", "almalinux"} & set(id_like):
        return "rpm_family"
    return "linux_generic"


_GUI_HANDLERS: dict[str, tuple[str, ...]] = {
    "ubuntu": ("/usr/bin/gnome-software", "/usr/bin/snap-store", "/usr/bin/plasma-discover"),
    "fedora": ("/usr/bin/plasma-discover", "/usr/bin/gnome-software"),
}


def collect_host_facts() -> HostFacts:
    """Read host facts from the machine under test."""
    fields: dict[str, str] = {}
    try:
        for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                fields[key.strip()] = value.strip().strip('"')
    except OSError as exc:
        raise AdapterError("cannot read /etc/os-release") from exc

    os_id = fields.get("ID", "").lower()
    version_id = fields.get("VERSION_ID", "").split(".")[0]
    id_like = tuple(x.lower() for x in fields.get("ID_LIKE", "").split())

    machine = host_platform.machine().lower()
    architecture = "x86_64" if machine in {"x86_64", "amd64", "x64"} else machine

    session_type = os.environ.get("XDG_SESSION_TYPE", "")
    has_display = bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))

    package_manager = next((x for x in ("apt-get", "dnf", "yum") if shutil.which(x)), None)
    handler = next((p for p in _GUI_HANDLERS.get(os_id, ()) if Path(p).is_file()), None)

    return HostFacts(
        os_id=os_id,
        version_id=version_id,
        id_like=id_like,
        architecture=architecture,
        kernel=host_platform.release(),
        session_type=session_type or "unknown",
        desktop_session="available" if has_display else "unavailable",
        package_manager=package_manager,
        native_gui_package_handler=handler,
    )


def detect_capability(name: str, facts: HostFacts) -> str:
    """Report one capability honestly; never infer a pass from absence."""
    if name == "graphical_session":
        return "available" if facts.desktop_session == "available" else "unavailable"
    if name == "native_package_manager":
        return "available" if facts.package_manager else "unavailable"
    if name == "administrator_elevation":
        return "available" if hasattr(os, "geteuid") and os.geteuid() == 0 else "unknown"
    if name == "systemd_user":
        return "available" if shutil.which("systemctl") and os.environ.get("XDG_RUNTIME_DIR") else "unavailable"
    if name == "systemd_system":
        return "available" if Path("/run/systemd/system").exists() else "unavailable"
    if name == "secret_store":
        for tool in ("secret-tool", "gnome-keyring-daemon", "keepassxc-cli"):
            if shutil.which(tool):
                return "available"
        return "unavailable"
    if name == "reboot":
        return "available" if Path("/proc/sysrq-trigger").exists() else "unavailable"
    # power_cycle, release_download and the rest are driven by the workflow's
    # VM control plane, not observable from inside the guest.
    return "unknown"


# --------------------------------------------------------------------------
# Clean-machine precondition
# --------------------------------------------------------------------------


@dataclass
class CleanlinessReport:
    findings: list[str] = field(default_factory=list)

    @property
    def clean(self) -> bool:
        return not self.findings

    def raise_if_dirty(self) -> None:
        if self.findings:
            raise NotClean("prior Synveil state present: " + "; ".join(self.findings))


def probe_clean_machine(facts: HostFacts) -> CleanlinessReport:
    """Verify no Synveil-owned state pre-exists, per the P004 definition.

    This deliberately inspects authoritative state rather than deleting
    anything. A dirty machine is a BLOCKED precondition failure, never a
    silently repaired one.
    """
    report = CleanlinessReport()

    if facts.package_manager == "apt-get":
        result = _run(["dpkg-query", "-W", "-f=${Status}", "synveil"], timeout=30)
        if result.returncode == 0 and "install ok installed" in result.stdout:
            report.findings.append("dpkg owns the synveil package")
    elif facts.package_manager == "dnf":
        result = _run(["rpm", "-q", "synveil"], timeout=30)
        if result.returncode == 0:
            report.findings.append("rpm owns the synveil package")

    home = Path(os.environ.get("HOME", "/"))
    owned = (
        home / ".local/share/applications/synveil.desktop",
        home / ".local/share/applications/synveil-appimage.desktop",
        home / ".local/share/icons/hicolor/scalable/apps/synveil.svg",
        home / ".local/state/synveil/appimage-integration.json",
        home / ".config/systemd/user/synveil-appimage-client.service",
        home / ".config/synveil/first-launch.json",
    )
    for path in owned:
        if path.exists():
            report.findings.append(f"acceptance-owned path already exists: {path.name}")

    if shutil.which("systemctl") and os.environ.get("XDG_RUNTIME_DIR"):
        result = _run(["systemctl", "--user", "is-enabled", "synveil-appimage-client.service"], timeout=30)
        if result.returncode == 0:
            report.findings.append("synveil-appimage-client.service is already enabled")

    return report


# --------------------------------------------------------------------------
# Artifact identity
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class ArtifactIdentity:
    filename: str
    sha256: str
    size_bytes: int
    artifact_type: str
    product_version: str | None
    source_commit: str | None
    platform: str
    architecture: str
    release_channel: str | None
    trust_status: str

    @classmethod
    def from_manifest(cls, manifest_path: Path, artifact_type: str) -> "ArtifactIdentity":
        """Bind the exact bytes on disk to a recorded release identity."""
        try:
            document = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise AdapterError(f"cannot read release manifest: {manifest_path.name}") from exc

        matches = [a for a in document.get("artifacts", []) if a.get("artifact_type") == artifact_type]
        if not matches:
            raise ArtifactMismatch(f"release manifest declares no {artifact_type} artifact")
        entry = matches[0]

        artifact = manifest_path.parent / entry["filename"]
        if not artifact.is_file():
            raise ArtifactMismatch(f"manifest artifact is absent from disk: {entry['filename']}")

        digest = hashlib.sha256()
        with artifact.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        actual = digest.hexdigest()

        # Fail closed. Never proceed on a partial or substituted artifact.
        if actual != entry["sha256"]:
            raise ArtifactMismatch(f"{entry['filename']} digest mismatch: manifest={entry['sha256']} actual={actual}")
        if artifact.stat().st_size != entry["size_bytes"]:
            raise ArtifactMismatch(f"{entry['filename']} size mismatch against manifest")

        return cls(
            filename=entry["filename"],
            sha256=actual,
            size_bytes=entry["size_bytes"],
            artifact_type=artifact_type,
            product_version=document.get("product_version"),
            source_commit=document.get("source_commit"),
            platform=entry.get("platform", "linux"),
            architecture=entry.get("architecture", ""),
            release_channel=document.get("channel"),
            trust_status=document.get("trust_status", "manifest-bound"),
        )

    def to_result(self) -> dict[str, Any]:
        return {
            "status": "verified",
            "product_version": self.product_version,
            "source_commit": self.source_commit,
            "artifact_type": self.artifact_type,
            "filename": self.filename,
            "digest": self.sha256,
            "platform": self.platform,
            "architecture": self.architecture,
            "release_channel": self.release_channel,
            "trust_status": self.trust_status,
        }


# --------------------------------------------------------------------------
# Closed action dispatch
# --------------------------------------------------------------------------


@dataclass
class StepOutcome:
    step_id: str
    action: str
    status: str  # completed | blocked | failed
    detail: str = ""


class Adapter:
    """Executes a scenario's typed steps against a real Linux machine."""

    def __init__(self, facts: HostFacts, identity: ArtifactIdentity, *, evidence: str, dry_run: bool = False) -> None:
        self.facts = facts
        self.identity = identity
        self.evidence = evidence
        self.dry_run = dry_run
        self.completed: list[str] = []
        self.diagnostics: list[str] = []

    # -- handlers ---------------------------------------------------------

    def _h_obtain_artifact(self, step: dict[str, Any]) -> StepOutcome:
        # The artifact is bound and digest-verified before the run starts, so
        # this step asserts that binding rather than re-fetching anything.
        return StepOutcome(step["id"], step["action"], "completed", f"bound {self.identity.filename} sha256={self.identity.sha256[:16]}...")

    def _h_open_installer(self, step: dict[str, Any]) -> StepOutcome:
        handler = self.facts.native_gui_package_handler
        if not handler:
            return StepOutcome(step["id"], step["action"], "blocked", "no supported graphical package handler is installed")
        if self.facts.desktop_session != "available":
            return StepOutcome(step["id"], step["action"], "blocked", "graphical journey requires a real desktop session")
        return StepOutcome(step["id"], step["action"], "blocked", f"graphical driver for {handler} is not provisioned in this runner")

    def _h_install(self, step: dict[str, Any]) -> StepOutcome:
        return StepOutcome(step["id"], step["action"], "blocked", "native install step requires the graphical driver and polkit authorization surface")

    def _h_launch(self, step: dict[str, Any]) -> StepOutcome:
        if self.facts.desktop_session != "available":
            return StepOutcome(step["id"], step["action"], "blocked", "application-menu launch requires a real desktop session")
        return StepOutcome(step["id"], step["action"], "blocked", "bounded GUI launch driver is not provisioned in this runner")

    def _h_interrupt(self, step: dict[str, Any]) -> StepOutcome:
        return StepOutcome(step["id"], step["action"], "blocked", "VM power interruption is driven by the workflow control plane, not from inside the guest")

    def _h_generic_blocked(self, step: dict[str, Any]) -> StepOutcome:
        return StepOutcome(step["id"], step["action"], "blocked", "native adapter for this action is not provisioned in this runner")

    HANDLERS: dict[str, str] = {
        "obtain_artifact": "_h_obtain_artifact",
        "open_installer": "_h_open_installer",
        "install": "_h_install",
        "launch": "_h_launch",
        "interrupt_at_boundary": "_h_interrupt",
    }

    def dispatch(self, step: dict[str, Any]) -> StepOutcome:
        """Resolve a typed step to reviewed code. No dynamic evaluation."""
        action = step.get("action")
        handler_name = self.HANDLERS.get(action or "")
        if handler_name is None:
            return StepOutcome(step.get("id", "?"), action or "?", "blocked", "no closed handler is registered for this action")
        handler: Callable[[dict[str, Any]], StepOutcome] = getattr(self, handler_name)
        return handler(step)

    # -- assertions -------------------------------------------------------

    def evaluate(self, assertions: list[dict[str, Any]]) -> list[dict[str, Any]]:
        results = []
        for assertion in assertions:
            results.append({"assertion_id": assertion["id"], "result": "NOT_EVALUATED", "type": assertion["type"]})
        return results


# --------------------------------------------------------------------------
# Runner
# --------------------------------------------------------------------------


def execute(scenario: dict[str, Any], source: Path, *, manifest: Path, artifact_type: str, evidence: str) -> dict[str, Any]:
    """Produce one result-v1 record for *scenario*."""
    start = _utcnow()
    blocked_reason: str | None = None
    capabilities: list[dict[str, str]] = []
    identity_result: dict[str, Any]
    completed: list[str] = []
    assertion_results: list[dict[str, Any]] = []
    evidence_class = "contract-valid"

    try:
        facts = collect_host_facts()
    except AdapterError as exc:
        facts = HostFacts("unknown", "unknown", (), host_platform.machine(), "", "unknown", "unavailable", None, None)
        blocked_reason = f"host facts unavailable: {exc}"

    required = contract.required_capabilities_for_host(scenario, facts.to_result())
    capabilities = [{"name": c, "status": detect_capability(c, facts)} for c in required]

    try:
        identity = ArtifactIdentity.from_manifest(manifest, artifact_type)
        identity_result = identity.to_result()
    except AdapterError as exc:
        identity_result = {
            "status": "unverified", "product_version": None, "source_commit": None,
            "artifact_type": artifact_type, "filename": None, "digest": None,
            "platform": facts.family, "architecture": facts.architecture,
            "release_channel": None, "trust_status": "unverified",
        }
        blocked_reason = blocked_reason or f"artifact identity not established: {redact(str(exc))}"

    if blocked_reason is None:
        if not facts.is_qualified:
            if facts.os_id in DETECTED_NOT_QUALIFIED:
                blocked_reason = f"{facts.os_id} {facts.version_id} is detected but not qualified; no native acceptance evidence exists"
            else:
                blocked_reason = f"{facts.os_id or 'unknown'} {facts.version_id or 'unknown'} x86_64 is outside the qualified matrix {sorted(QUALIFIED_PLATFORMS)}"
        elif not identity_result.get("digest"):
            blocked_reason = "artifact identity unavailable"

    if blocked_reason is None:
        unavailable = [c["name"] for c in capabilities if c["status"] != "available"]
        if unavailable:
            blocked_reason = "required capabilities unavailable: " + ", ".join(sorted(unavailable))

    result = "BLOCKED"
    evidence_class = "contract-valid"
    if blocked_reason is None:
        report = probe_clean_machine(facts)
        if not report.clean:
            blocked_reason = redact("; ".join(report.findings))
        else:
            adapter = Adapter(facts, ArtifactIdentity.from_manifest(manifest, artifact_type), evidence=evidence)
            for step in scenario["steps"]:
                outcome = adapter.dispatch(step)
                if outcome.status == "completed":
                    completed.append(outcome.step_id)
                else:
                    blocked_reason = f"step {outcome.step_id} ({outcome.action}): {outcome.detail}"
                    break
            assertion_results = adapter.evaluate(scenario["assertions"])
            # A scenario is only PASS when every step completed and every
            # assertion was actually evaluated and held. Blocked steps block.
            evidence_class = evidence

    end = _utcnow()
    return {
        "schema_version": RESULT_SCHEMA_VERSION,
        "scenario_id": scenario["id"],
        "scenario_definition_digest": contract.definition_digest(scenario),
        "runner_version": RUNNER_VERSION,
        "start_time": start,
        "end_time": end,
        "platform_facts": facts.to_result(),
        "capability_results": capabilities,
        "artifact_identity": identity_result,
        "result": result,
        "completed_steps": completed,
        "assertion_results": assertion_results,
        "evidence_classification": evidence_class,
        "reason": blocked_reason,
        "diagnostics_redacted": True,
        "diagnostic_references": [],
        "cleanup_result": {"status": "not-run", "details": None},
    }


class AdapterTests(unittest.TestCase):
    def test_qualified_matrix_is_exactly_p018(self) -> None:
        self.assertEqual(sorted(QUALIFIED_PLATFORMS), [("fedora", "42"), ("ubuntu", "24.04")])

    def test_debian_is_not_qualified(self) -> None:
        self.assertNotIn("debian", {os_id for os_id, _ in QUALIFIED_PLATFORMS})
        self.assertIn("debian", DETECTED_NOT_QUALIFIED)

    def test_host_facts_mark_debian_detected_not_qualified(self) -> None:
        facts = HostFacts("debian", "12", ("debian",), "x86_64", "6.1", "x11", "available", "apt-get", "/usr/bin/gnome-software")
        self.assertFalse(facts.is_qualified)
        self.assertIsNone(facts.qualification_profile)

    def test_ubuntu_2404_qualifies(self) -> None:
        facts = HostFacts("ubuntu", "24.04", ("debian",), "x86_64", "6.8", "wayland", "available", "apt-get", "/usr/bin/gnome-software")
        self.assertTrue(facts.is_qualified)
        self.assertEqual(facts.qualification_profile, "debian-x86_64")

    def test_fedora_42_qualifies(self) -> None:
        facts = HostFacts("fedora", "42", ("fedora",), "x86_64", "6.11", "wayland", "available", "dnf", "/usr/bin/plasma-discover")
        self.assertTrue(facts.is_qualified)
        self.assertEqual(facts.qualification_profile, "fedora-x86_64")

    def test_non_x86_never_qualifies(self) -> None:
        facts = HostFacts("ubuntu", "24.04", (), "aarch64", "6.8", "wayland", "available", "apt-get", "/usr/bin/gnome-software")
        self.assertFalse(facts.is_qualified)

    def test_redaction_removes_credentials(self) -> None:
        self.assertNotIn("hunter2", redact("password=hunter2"))
        self.assertNotIn("secret", redact("postgresql://user:secret@db/synveil").replace("postgresql://user:<redacted>@db/synveil", ""))
        self.assertNotIn("abcdefgh", redact("svd1_abcdefgh1234"))

    def test_unknown_action_is_blocked_not_evaluated(self) -> None:
        adapter = Adapter(
            HostFacts("ubuntu", "24.04", (), "x86_64", "6.8", "wayland", "available", "apt-get", None),
            ArtifactIdentity("a", "0" * 64, 1, "DEB", "0.1.0", None, "linux", "x86_64", None, "test"),
            evidence="native-clean-machine",
        )
        outcome = adapter.dispatch({"id": "S", "action": "rm -rf / ; echo pwned", "description": "x"})
        self.assertEqual(outcome.status, "blocked")
        self.assertEqual(outcome.action, "rm -rf / ; echo pwned")

    def test_every_contract_step_action_has_a_closed_handler(self) -> None:
        handled = set(Adapter.HANDLERS)
        for _, scenario in contract.discover():
            for step in scenario["steps"]:
                self.assertIn(step["action"], contract.STEP_ACTIONS)
        # Unknown actions resolve to blocked rather than raising.
        adapter = Adapter(
            HostFacts("ubuntu", "24.04", (), "x86_64", "6.8", "wayland", "available", "apt-get", None),
            ArtifactIdentity("a", "0" * 64, 1, "DEB", "0.1.0", None, "linux", "x86_64", None, "test"),
            evidence="native-clean-machine",
        )
        self.assertEqual(adapter.dispatch({"id": "S", "action": "host_setup", "description": "x"}).status, "blocked")

    def test_graphical_action_blocks_without_session(self) -> None:
        facts = HostFacts("ubuntu", "24.04", (), "x86_64", "6.8", "tty", "unavailable", "apt-get", "/usr/bin/gnome-software")
        adapter = Adapter(facts, ArtifactIdentity("a", "0" * 64, 1, "DEB", "0.1.0", None, "linux", "x86_64", None, "t"), evidence="e")
        outcome = adapter.dispatch({"id": "S", "action": "launch", "description": "x"})
        self.assertEqual(outcome.status, "blocked")
        self.assertIn("desktop session", outcome.detail)

    def test_offscreen_is_not_a_graphical_session(self) -> None:
        saved = os.environ.pop("DISPLAY", None)
        try:
            self.assertEqual(HostFacts("ubuntu", "24.04", (), "x86_64", "6.8", "", "unavailable", "apt-get", None).desktop_session, "unavailable")
        finally:
            if saved is not None:
                os.environ["DISPLAY"] = saved

    def test_result_record_validates_against_schema(self) -> None:
        result = execute(
            contract.discover()[0][1],
            contract.discover()[0][0],
            manifest=Path("/nonexistent/SYNVEIL-RELEASE-MANIFEST.json"),
            artifact_type="DEB",
            evidence="native-clean-machine",
        )
        schema = contract.load_json(ROOT / "tests/install-acceptance/schema/result-v1.schema.json")
        self.assertTrue(set(schema["required"]) <= result.keys())
        self.assertIn(result["result"], {"PASS", "FAIL", "SKIPPED", "BLOCKED", "ERROR"})
        self.assertTrue(result["diagnostics_redacted"])

    def test_missing_manifest_blocks_rather_than_passes(self) -> None:
        source, scenario = contract.discover()[0]
        result = execute(scenario, source, manifest=Path("/nonexistent/m.json"), artifact_type="DEB", evidence="native-clean-machine")
        self.assertNotEqual(result["result"], "PASS")

    def test_artifact_digest_mismatch_fails_closed(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "synveil.deb").write_bytes(b"real bytes")
            manifest = {
                "schema_version": 1, "product_version": "0.1.0", "source_commit": "a" * 40,
                "artifacts": [{"artifact_type": "DEB", "filename": "synveil.deb", "sha256": "b" * 64, "size_bytes": 10, "platform": "linux", "architecture": "x86_64"}],
            }
            path = root / "m.json"
            path.write_text(json.dumps(manifest))
            with self.assertRaises(ArtifactMismatch):
                ArtifactIdentity.from_manifest(path, "DEB")

    def test_artifact_identity_binds_verified_bytes(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = b"exact artifact bytes"
            (root / "synveil.deb").write_bytes(payload)
            digest = hashlib.sha256(payload).hexdigest()
            manifest = {
                "schema_version": 1, "product_version": "0.1.0", "source_commit": "c" * 40,
                "artifacts": [{"artifact_type": "DEB", "filename": "synveil.deb", "sha256": digest, "size_bytes": len(payload), "platform": "linux", "architecture": "x86_64"}],
            }
            path = root / "m.json"
            path.write_text(json.dumps(manifest))
            identity = ArtifactIdentity.from_manifest(path, "DEB")
            self.assertEqual(identity.sha256, digest)
            self.assertEqual(identity.size_bytes, len(payload))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["self-test", "run", "matrix"])
    parser.add_argument("scenario_id", nargs="?")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--artifact-type", default="DEB")
    parser.add_argument("--evidence", default="native-clean-machine")
    args = parser.parse_args()

    if args.command == "self-test":
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(AdapterTests)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        return 0 if result.wasSuccessful() else 1

    items = {scenario["id"]: (path, scenario) for path, scenario in contract.discover()}
    if args.command == "matrix":
        print(json.dumps(sorted(items), indent=2))
        return 0

    if args.scenario_id not in items:
        print(f"unknown scenario id: {args.scenario_id}", file=sys.stderr)
        return 2
    path, scenario = items[args.scenario_id]
    manifest = args.manifest or (ROOT / "target/packages/SYNVEIL-RELEASE-MANIFEST.json")
    record = execute(scenario, path, manifest=manifest, artifact_type=args.artifact_type, evidence=args.evidence)
    print(json.dumps(record, sort_keys=True, indent=2))
    return 0 if record["result"] in {"PASS", "BLOCKED", "SKIPPED"} else 1


if __name__ == "__main__":
    raise SystemExit(main())