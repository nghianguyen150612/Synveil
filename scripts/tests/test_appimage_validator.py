import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_appimage_build", ROOT / "scripts/validate-appimage-build.py"
)
validator = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(validator)


class AppImageIconValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.appdir = Path(self.temp.name) / "Synveil.AppDir"
        self.icon = self.appdir / "synveil.svg"
        self.bundled_icon = self.appdir / "usr/share/icons/hicolor/scalable/apps/synveil.svg"
        self.bundled_icon.parent.mkdir(parents=True)
        source = ROOT / "deploy/icons/hicolor/scalable/apps/synveil.svg"
        self.bundled_icon.write_bytes(source.read_bytes())

    def tearDown(self):
        self.temp.cleanup()

    def test_linuxdeploy_relative_root_symlink_to_exact_icon_is_valid(self):
        self.icon.symlink_to("usr/share/icons/hicolor/scalable/apps/synveil.svg")
        validator.validate_appdir_icon(self.appdir)

    def test_root_icon_cannot_escape_appdir_or_change_bytes(self):
        outside = Path(self.temp.name) / "outside.svg"
        outside.write_bytes(self.bundled_icon.read_bytes())
        self.icon.symlink_to(outside)
        with self.assertRaises(RuntimeError):
            validator.validate_appdir_icon(self.appdir)
        self.icon.unlink()
        self.icon.write_text("<svg>substituted</svg>")
        with self.assertRaises(RuntimeError):
            validator.validate_appdir_icon(self.appdir)


if __name__ == "__main__":
    unittest.main()
