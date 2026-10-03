#!/usr/bin/env python3
"""Static product-contract regression checks for Prompt029."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/v0.2/SERVER_SETUP_PRODUCT_CONTRACT.md"

REQUIRED = (
    "Host Synveil on this device",
    "Connect to existing Synveil",
    "Personal / Home Mode",
    "Advanced / Server Mode",
    "PostgreSQL remains Synveil's canonical server metadata and transaction authority",
    "Server storage and client library are different",
    "Ordinary uninstall MUST preserve SERVER_CONFIG, SERVER_DATABASE, and SERVER_OBJECT_DATA",
    "SERVER_PACKAGE",
    "SERVER_SECRET",
    "SERVER_SERVICE_INTEGRATION",
    "SERVER_NETWORK_INTEGRATION",
    "SERVER_RUNTIME",
    "EXTERNAL_DEPENDENCY",
    "no silent public exposure",
    "no mandatory proprietary relay",
    "no certificate-trust bypass",
    "no plaintext credential fallback",
    "ROOT UNAVAILABLE != DELETE EVERYTHING",
    "Unknown/future schema fails closed",
    "Server Bootstrap Coordinator",
    "Server Bootstrap Coordinator is separate from the Installation Coordinator",
    "MUST NOT implement file synchronization",
    "Connect MUST NOT install PostgreSQL",
    "Server Ready",
    "TCP listener or process start alone is never",
    "PostgreSQL is reachable",
    "INFRASTRUCTURE_READY",
    "ADMIN_BOOTSTRAP_COMPLETE",
    "OutcomeUnknown",
    "confirmation was lost",
    "reconcile by effect identity before retry",
    "Before mutation, cancellation is safe",
    "global rollback promise",
    "Server Dependency Strategy",
    "Managed Server Configuration",
    "Storage Location Wizard",
    "Server Service Installation",
    "Server Network and Reachability Setup",
    "Server First-Admin Bootstrap",
    "End-to-End Self-Host Wizard",
)

CONTRADICTIONS = (
    r"SQLite is Synveil's canonical server metadata and transaction authority",
    r"SQLite is the production server database",
    r"Connect installs PostgreSQL",
    r"Connect provisions server services",
    r"Server Ready means (?:only )?a TCP listener",
    r"Ordinary uninstall deletes SERVER_DATABASE",
    r"Ordinary uninstall deletes SERVER_OBJECT_DATA",
    r"all setup effects roll back",
    r"retry (?:an )?(?:unknown|uncertain) effect without reconciliation",
    r"server storage and client library are the same",
    r"Host mode bypasses authentication",
)


def main() -> int:
    if not CONTRACT.is_file():
        print(f"missing product contract: {CONTRACT.relative_to(ROOT)}", file=sys.stderr)
        return 1

    content = CONTRACT.read_text(encoding="utf-8")
    missing = [phrase for phrase in REQUIRED if phrase.casefold() not in content.casefold()]
    contradicted = [
        pattern for pattern in CONTRADICTIONS
        if re.search(pattern, content, flags=re.IGNORECASE)
    ]

    if missing or contradicted:
        for phrase in missing:
            print(f"missing required contract phrase: {phrase}", file=sys.stderr)
        for pattern in contradicted:
            print(f"contradictory contract statement: {pattern}", file=sys.stderr)
        return 1

    print(f"Prompt029 server setup contract: PASS ({len(REQUIRED)} required checks)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
