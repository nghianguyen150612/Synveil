from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
REPRODUCIBLE_SH = REPO_ROOT / "deploy/packages/common/reproducible.sh"


class ReleaseSourceFingerprintTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory(prefix="synveil-source-fingerprint-")
        self.repo = Path(self.temp_dir.name)
        self.github_env_dir = tempfile.TemporaryDirectory(prefix="synveil-source-fingerprint-env-")
        subprocess.run(["git", "init", "--quiet", str(self.repo)], check=True)
        subprocess.run(
            ["git", "-C", str(self.repo), "config", "user.email", "release-test@example.invalid"],
            check=True,
        )
        subprocess.run(
            ["git", "-C", str(self.repo), "config", "user.name", "Release Fingerprint Test"],
            check=True,
        )
        (self.repo / "tracked-source.txt").write_text("initial source\n", encoding="utf-8")
        subprocess.run(["git", "-C", str(self.repo), "add", "tracked-source.txt"], check=True)
        subprocess.run(["git", "-C", str(self.repo), "commit", "--quiet", "-m", "fixture source"], check=True)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()
        self.github_env_dir.cleanup()

    def bash(self, body: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", "-c", f'source "$1"; {body}', "release-source-test", str(REPRODUCIBLE_SH), str(self.repo)],
            cwd=self.repo,
            text=True,
            capture_output=True,
            check=False,
        )

    def fingerprint(self) -> str:
        result = self.bash('synveil_source_fingerprint "$2"')
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_source_fingerprint_ignores_generated_untracked_files(self) -> None:
        initial = self.fingerprint()
        (self.repo / "generated-build-input.txt").write_text("generated output\n", encoding="utf-8")
        self.assertEqual(self.fingerprint(), initial)

    def test_source_fingerprint_changes_for_dirty_tracked_source(self) -> None:
        initial = self.fingerprint()
        (self.repo / "tracked-source.txt").write_text("modified source\n", encoding="utf-8")
        self.assertNotEqual(self.fingerprint(), initial)

    def test_release_source_preflight_rejects_untracked_inputs(self) -> None:
        (self.repo / "untracked-source.txt").write_text("not committed\n", encoding="utf-8")
        result = self.bash('synveil_assert_release_source_preflight "$2"')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("non-ignored untracked release source inputs", result.stderr)
        self.assertIn("untracked-source.txt", result.stderr)

    def test_release_source_preflight_persists_root_and_revision(self) -> None:
        env_file = Path(self.github_env_dir.name) / "github-env.txt"
        result = subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; synveil_assert_release_source_preflight "$2"',
                "release-source-test",
                str(REPRODUCIBLE_SH),
                str(self.repo),
            ],
            cwd=self.repo,
            text=True,
            capture_output=True,
            check=False,
            env={**os.environ, "GITHUB_ENV": str(env_file)},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        content = env_file.read_text(encoding="utf-8")
        self.assertIn(f"SYNVEIL_RELEASE_SOURCE_PREFLIGHT_ROOT={self.repo}\n", content)
        revision = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
        self.assertIn(f"SYNVEIL_RELEASE_SOURCE_PREFLIGHT_SHA={revision}\n", content)


if __name__ == "__main__":
    unittest.main()
