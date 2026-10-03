import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from windows_lifecycle import PRESERVED_CLASSES, authorize_purge, lifecycle, obsolete_owned


class WindowsLifecycleTests(unittest.TestCase):
    def test_same_version_explicit_repair_is_idempotent(self):
        self.assertEqual(lifecycle("0.1.0", "0.1.0", True), "repair")
        self.assertEqual(lifecycle("0.1.0", "0.1.0", True), "repair")

    def test_repair_requires_existing_exact_identity(self):
        with self.assertRaises(ValueError): lifecycle(None, "0.1.0", True)
        with self.assertRaises(ValueError): lifecycle("0.1.0", "0.2.0", True)
        with self.assertRaises(ValueError): lifecycle("0.1.0", "0.1.0", False)

    def test_upgrade_and_downgrade(self):
        self.assertEqual(lifecycle("1.0.0", "1.1.0", False), "upgrade")
        with self.assertRaises(ValueError): lifecycle("1.1.0", "1.0.0", False)
        with self.assertRaises(ValueError): lifecycle("unknown", "1.0.0", False)

    def test_only_old_owned_files_are_obsolete(self):
        self.assertEqual(obsolete_owned({"old.dll", "keep.dll"}, {"keep.dll"}), {"old.dll"})
        self.assertNotIn("user-note.txt", obsolete_owned({"old.dll"}, set()))
        with self.assertRaises(ValueError): obsolete_owned({"..\\escape"}, set())
        with self.assertRaises(ValueError): obsolete_owned({"linked.dll"}, set(), {"linked.dll"})

    def test_durable_classes_are_preserved(self):
        self.assertEqual(len(PRESERVED_CLASSES), 7)
        self.assertIn("USER_LIBRARY", PRESERVED_CLASSES)
        self.assertIn("CREDENTIAL_STATE", PRESERVED_CLASSES)

    def test_purge_is_explicit_and_bounded(self):
        self.assertEqual(authorize_purge({"APPLICATION_CONFIG"}, True), {"APPLICATION_CONFIG"})
        for rejected in ({"USER_LIBRARY"}, {"SERVER_DATABASE"}, {"CREDENTIAL_STATE"}):
            with self.assertRaises(ValueError): authorize_purge(rejected, True)
        with self.assertRaises(ValueError): authorize_purge({"APPLICATION_CONFIG"}, False)


if __name__ == "__main__":
    unittest.main()
