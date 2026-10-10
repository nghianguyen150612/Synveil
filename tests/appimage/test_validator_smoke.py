"""Regression tests for the host-decontaminated AppImage QML smoke."""
import importlib.util
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_appimage_build", ROOT / "scripts/validate-appimage-build.py"
)
VALIDATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VALIDATOR)


class AppImageSmokeTests(unittest.TestCase):
    def test_smoke_uses_bundled_xcb_plugin_in_isolated_xvfb(self):
        artifact = Path("/candidate/Synveil-0.1.0-x86_64.AppImage")
        private_dirs_exist = []
        with patch.object(VALIDATOR.shutil, "which", return_value="/usr/bin/xvfb-run"):
            with patch.object(VALIDATOR.subprocess, "run") as run:
                def observe_call(*args, **kwargs):
                    env = kwargs["env"]
                    private_dirs_exist.append(all(
                        Path(env[key]).is_dir() for key in (
                            "HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"
                        )
                    ))

                run.side_effect = observe_call
                VALIDATOR.run_smoke(artifact)

        command = run.call_args.args[0]
        env = run.call_args.kwargs["env"]
        self.assertEqual(command[0:2], ["/usr/bin/xvfb-run", "--auto-servernum"])
        self.assertTrue(command[2].startswith("--server-args=-screen 0 1280x800x24"))
        self.assertEqual(command[-2:], [str(artifact), "--qml-smoke-test"])
        self.assertEqual(env["QT_QPA_PLATFORM"], "xcb")
        self.assertEqual(env["APPIMAGE_EXTRACT_AND_RUN"], "1")
        for key in ("QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QML_IMPORT_PATH", "LD_LIBRARY_PATH"):
            self.assertNotIn(key, env)
        self.assertEqual(private_dirs_exist, [True])

    def test_smoke_fails_clearly_when_xvfb_runner_is_missing(self):
        with patch.object(VALIDATOR.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "APPIMAGE-18: xvfb-run is required"):
                VALIDATOR.run_smoke(Path("candidate.AppImage"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
