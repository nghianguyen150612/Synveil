#!/usr/bin/env python3
import copy
import hashlib
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("release_manifest", ROOT / "scripts/release_manifest.py")
manifest = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(manifest)
COMMIT = "a" * 40


class ReleaseManifestTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "synveil.deb").write_bytes(b"deb")

    def tearDown(self):
        self.temp.cleanup()

    def artifact(self, kind="deb", filename="synveil.deb", identity="debian-x86_64-deb"):
        path = self.root / filename
        if not path.exists(): path.write_bytes(kind.encode())
        item = {"id": identity, "artifact_type": kind, "filename": filename,
                "platform": "windows" if kind.startswith("windows_") else "linux",
                "architecture": "x86_64", "role": "native_package",
                "product_version": "0.1.0", "size_bytes": path.stat().st_size,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "components": ["synveil-desktop", "synveil-client"]}
        if kind == "deb": item["package_metadata"] = {"format":"deb", "package_name":"synveil", "package_version":"0.1.0", "package_architecture":"amd64"}
        if kind == "rpm": item["package_metadata"] = {"format":"rpm", "package_name":"synveil", "package_version":"0.1.0", "package_release":"1", "package_architecture":"x86_64"}
        if kind == "windows_portable_zip": item["role"] = "portable"
        if kind == "windows_installer": item["role"] = "primary_installer"
        return item

    def document(self, artifacts=None):
        return {"schema_version":1, "product":"Synveil", "product_version":"0.1.0", "source_commit":COMMIT, "artifacts":artifacts or []}

    def rejected(self, data, text=None, **kwargs):
        with self.assertRaises(manifest.ManifestError) as caught: manifest.validate(data, **kwargs)
        if text: self.assertIn(text, str(caught.exception))

    def test_01_empty_manifest(self): manifest.validate(self.document())
    def test_02_deb(self): manifest.validate(self.document([self.artifact()]), self.root)
    def test_03_rpm(self): manifest.validate(self.document([self.artifact("rpm", "synveil.rpm", "rpm-x86_64-rpm")]), self.root)
    def test_04_windows_portable(self): manifest.validate(self.document([self.artifact("windows_portable_zip", "synveil.zip", "windows-x86_64-portable")]), self.root)
    def test_05_appimage_supported(self): manifest.validate(self.document([self.artifact("appimage", "Synveil.AppImage", "linux-x86_64-appimage")]))
    def test_06_windows_installer_supported(self): manifest.validate(self.document([self.artifact("windows_installer", "SynveilSetup.exe", "windows-x86_64-installer")]))
    def test_07_future_schema_rejected(self): d=self.document(); d["schema_version"]=2; self.rejected(d,"unsupported schema_version")
    def test_08_missing_top_field(self): d=self.document(); del d["product"]; self.rejected(d,"missing required")
    def test_09_unknown_top_field(self): d=self.document(); d["channel"]="stable"; self.rejected(d,"unknown field")
    def test_10_unknown_artifact_field(self): a=self.artifact(); a["url"]="x"; self.rejected(self.document([a]),"unknown field")
    def test_11_duplicate_id(self): a=self.artifact(); self.rejected(self.document([a,copy.deepcopy(a)]),"duplicate artifact id")
    def test_12_traversal(self): a=self.artifact(); a["filename"]="../x"; self.rejected(self.document([a]),"unsafe artifact path")
    def test_13_unix_absolute(self): a=self.artifact(); a["filename"]="/x"; self.rejected(self.document([a]),"unsafe artifact path")
    def test_14_windows_absolute(self): a=self.artifact(); a["filename"]="C:\\x"; self.rejected(self.document([a]),"unsafe artifact path")
    def test_15_invalid_digest(self): a=self.artifact(); a["sha256"]="A"*64; self.rejected(self.document([a]),"invalid sha256")
    def test_16_wrong_size(self): a=self.artifact(); a["size_bytes"]+=1; self.rejected(self.document([a]),"size mismatch",artifact_root=self.root)
    def test_17_wrong_digest(self): a=self.artifact(); a["sha256"]="0"*64; self.rejected(self.document([a]),"digest mismatch",artifact_root=self.root)
    def test_18_missing_artifact(self): a=self.artifact(); (self.root/a["filename"]).unlink(); self.rejected(self.document([a]),"artifact missing",artifact_root=self.root)
    def test_19_symlink_escape(self):
        outside=self.root.parent/"outside-release-manifest"; outside.write_bytes(b"x")
        try:
            (self.root/"link.deb").symlink_to(outside); a=self.artifact(); a["filename"]="link.deb"; a["size_bytes"]=1; a["sha256"]=hashlib.sha256(b"x").hexdigest()
            self.rejected(self.document([a]),"artifact missing",artifact_root=self.root)
        finally: outside.unlink(missing_ok=True)
    def test_20_invalid_architecture(self): a=self.artifact(); a["architecture"]="amd64"; self.rejected(self.document([a]),"invalid architecture")
    def test_21_invalid_type(self): a=self.artifact(); a["artifact_type"]="msi"; self.rejected(self.document([a]),"unknown artifact_type")
    def test_22_wrong_native_metadata(self): a=self.artifact(); a["package_metadata"]["format"]="rpm"; self.rejected(self.document([a]),"combination")
    def test_23_version_inconsistency(self): a=self.artifact(); a["product_version"]="0.2.0"; self.rejected(self.document([a]),"product_version mismatch")
    def test_24_native_version_inconsistency(self): a=self.artifact(); a["package_metadata"]["package_version"]="2.0.0"; self.rejected(self.document([a]),"native package version")
    def test_25_source_commit_mode(self): self.rejected(self.document(),"source_commit mismatch",source_commit="b"*40)
    def test_26_order_rejected(self): self.rejected(self.document([self.artifact("rpm","z.rpm","z"),self.artifact("rpm","a.rpm","a")]),"not sorted")
    def test_27_create_is_byte_deterministic(self):
        spec=json.dumps({k:v for k,v in self.artifact().items() if k not in {"product_version","size_bytes","sha256"}})
        outputs=[]
        for name in ("one.json","two.json"):
            subprocess.run([str(ROOT/"scripts/release_manifest.py"),"create","--artifact-root",str(self.root),"--product-version","0.1.0","--source-commit",COMMIT,"--output",str(self.root/name),"--artifact",spec],check=True); outputs.append((self.root/name).read_bytes())
        self.assertEqual(outputs[0],outputs[1])
    def test_28_merge_mismatched_version(self):
        a=self.document(); b=self.document(); b["product_version"]="0.2.0"
        for name,data in (("a.json",a),("b.json",b)): (self.root/name).write_text(json.dumps(data))
        result=subprocess.run([str(ROOT/"scripts/release_manifest.py"),"merge","--output",str(self.root/"out"),str(self.root/"a.json"),str(self.root/"b.json")],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0); self.assertIn("mismatched product_version",result.stderr)
    def test_29_merge_mismatched_source(self):
        a=self.document(); b=self.document(); b["source_commit"]="b"*40; self._merge_rejected(a,b,"mismatched source_commit")
    def test_30_merge_duplicate_id(self):
        a=self.document([self.artifact()]); self._merge_rejected(a,a,"duplicate artifact id")
    def _merge_rejected(self,a,b,text):
        for name,data in (("a.json",a),("b.json",b)): (self.root/name).write_text(json.dumps(data))
        result=subprocess.run([str(ROOT/"scripts/release_manifest.py"),"merge","--output",str(self.root/"out"),str(self.root/"a.json"),str(self.root/"b.json")],capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0); self.assertIn(text,result.stderr)
    def test_31_schema_drift_guard(self):
        schema=json.loads((ROOT/"deploy/release/release-manifest-v1.schema.json").read_text())
        self.assertEqual(schema["properties"]["schema_version"]["const"],manifest.SCHEMA_VERSION)
        self.assertEqual(set(schema["$defs"]["artifact"]["properties"]["artifact_type"]["enum"]),manifest.TYPES)
        self.assertEqual(set(schema["required"]),manifest.TOP_FIELDS)
    def test_32_no_secret_fields(self):
        encoded=json.dumps(self.document([self.artifact()])).lower()
        for word in ("password","api_key","private_key","secretstore","token"): self.assertNotIn(word,encoded)

if __name__ == "__main__": unittest.main()
