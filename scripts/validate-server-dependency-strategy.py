#!/usr/bin/env python3
"""Focused, semantic static checks for the Prompt030 strategy lock."""

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]


def read(relative: str) -> str:
    path = ROOT / relative
    if not path.is_file():
        raise AssertionError(f"missing required file: {relative}")
    return path.read_text(encoding="utf-8")


strategy = read("docs/v0.2/SERVER_DEPENDENCY_STRATEGY.md")
adr = read("docs/adr/ADR-060-v0.2-managed-postgresql-dependency-strategy.md")
adr019 = read("docs/adr/ADR-019-managed-postgresql-lifecycle.md")
roadmap = read("docs/v0.2/ROADMAP.md")
manifest = read("docs/v0.2/PROMPT030_MANIFEST.md")
platform_en = read("docs/en/PLATFORM.md")
platform_vi = read("docs/vi/PLATFORM.md")

checks = {
    "canonical PostgreSQL authority": "sole canonical production server" in strategy,
    "managed PostgreSQL 17": "Synveil-managed private PostgreSQL 17 runtime" in strategy,
    "no normal Docker requirement": "No Docker, Docker Desktop, Podman, Compose" in strategy,
    "no arbitrary system PostgreSQL reuse": "does not silently reuse" in strategy,
    "external Advanced option": "Only Advanced / Server Mode offers **Use external PostgreSQL**" in strategy,
    "runtime differs from database": "POSTGRESQL_RUNTIME != SERVER_DATABASE" in strategy,
    "uninstall preserves database": "server-software removal" in strategy and "preserves" in strategy,
    "private listener": "never opens a PostgreSQL firewall port" in strategy,
    "DATABASE_URL hidden": "never requests or" in strategy and "`DATABASE_URL`" in strategy,
    "authenticated P006/P011 acquisition": all(token in strategy for token in ("P006", "P011", "authenticate metadata")),
    "Linux implementation targets": "Ubuntu 24.04" in strategy and "Fedora 42" in strategy,
    "Windows Host not qualified": "Windows 11" in strategy and "**NOT_YET_QUALIFIED**" in strategy,
    "ADR-060 accepted": "**Accepted / Chấp thuận — Prompt030 strategy contract**" in adr,
    "ADR-019 superseded": "Superseded by ADR-060" in adr019,
    "OD-PLAT-001 closed English": "ACCEPTED DECISION OD-PLAT-001" in platform_en,
    "OD-PLAT-001 closed Vietnamese": "ACCEPTED DECISION OD-PLAT-001" in platform_vi,
    "P031-P036 boundaries": all(f"**P0{n}**" in strategy for n in range(31, 37)),
    "roadmap strategy complete": "Complete (Prompt030 strategy)" in roadmap,
    "completion marker": "SYNVEIL_SERVER_DEPENDENCY_STRATEGY_LOCKED" in manifest,
    "no guided readiness marker": "SYNVEIL_V0_2_GUIDED_" + "SELF_HOSTING_READY" not in strategy + adr + manifest,
    "no Windows readiness marker": "SYNVEIL_V0_2_WINDOWS_" + "INSTALL_EXPERIENCE_READY" not in strategy + adr + manifest,
}

failed = [name for name, passed in checks.items() if not passed]
for name, passed in checks.items():
    print(f"P030-CONTRACT {'PASS' if passed else 'FAIL'}: {name}")
if failed:
    print(f"Prompt030 contract validation failed: {', '.join(failed)}", file=sys.stderr)
    sys.exit(1)
print("Prompt030 server dependency strategy validation passed")
