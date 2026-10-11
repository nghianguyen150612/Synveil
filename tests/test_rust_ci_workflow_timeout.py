#!/usr/bin/env python3
"""Keep the cold cross-platform Rust test budget above observed Windows time."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/ci.yml"


class RustCiWorkflowTimeoutTests(unittest.TestCase):
    def test_cross_platform_test_budget_covers_cold_windows_build(self) -> None:
        lines = WORKFLOW.read_text(encoding="utf-8").splitlines()
        start = lines.index("  cross-platform-test:")
        end = next(
            (i for i in range(start + 1, len(lines)) if re.fullmatch(r"  [a-z0-9_-]+:", lines[i])),
            len(lines),
        )
        job = "\n".join(lines[start:end])
        timeout = re.search(r"(?m)^    timeout-minutes:\s*(\d+)\s*$", job)

        self.assertIsNotNone(timeout, "cross-platform-test must have an explicit timeout")
        assert timeout is not None
        self.assertGreaterEqual(
            int(timeout.group(1)),
            30,
            "A cold Windows workspace build reached the 15-minute job timeout before focused tests completed.",
        )
        self.assertIn("Test desktop sync host/controller", job)


if __name__ == "__main__":
    unittest.main()
