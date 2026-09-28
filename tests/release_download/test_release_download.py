#!/usr/bin/env python3
import copy, hashlib, importlib.util, io, json, sys, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("release_download", ROOT / "scripts/release_download.py")
download = importlib.util.module_from_spec(SPEC); sys.modules[SPEC.name] = download
assert SPEC.loader; SPEC.loader.exec_module(download)
COMMIT = "a" * 40


class Response(io.BytesIO):
    def __init__(self, body=b"", status=200, location=None):
        super().__init__(body); self.status=status; self.headers={}
        if location: self.headers["Location"] = location
    def getcode(self): return self.status


class Opener:
    def __init__(self, responses): self.responses=iter(responses)
    def open(self, request): return next(self.responses)


class ReleaseDownloadTests(unittest.TestCase):
    def setUp(self):
        self.payload=b"trusted artifact"
        self.artifact={"id":"linux-x86_64-deb", "artifact_type":"deb", "filename":"synveil.deb", "platform":"linux", "architecture":"x86_64", "role":"native_package", "product_version":"0.1.0", "size_bytes":len(self.payload), "sha256":hashlib.sha256(self.payload).hexdigest(), "components":["synveil-client"], "package_metadata":{"format":"deb","package_name":"synveil","package_version":"0.1.0","package_architecture":"amd64"}}
        self.manifest={"schema_version":1,"product":"Synveil","product_version":"0.1.0","source_commit":COMMIT,"artifacts":[self.artifact]}
        self.raw=json.dumps(self.manifest,sort_keys=True).encode(); self.pin=hashlib.sha256(self.raw).hexdigest()
        self.policy=download.ReleaseTrustPolicy(frozenset({"https://release.example"}),frozenset({"https://release.example"}),expected_manifest_sha256=self.pin)
        self.auth=download.authenticate_manifest(self.raw,self.policy)
        self.temp=tempfile.TemporaryDirectory(); self.root=Path(self.temp.name)
    def tearDown(self): self.temp.cleanup()
    def error(self, code, function, *args, **kwargs):
        with self.assertRaises(download.AcquisitionError) as caught: function(*args,**kwargs)
        self.assertEqual(code,caught.exception.code)
    def test_01_valid_pin(self): self.assertEqual("AUTHENTICATED_PINNED_DIGEST",self.auth.state)
    def test_02_wrong_pin(self): self.error("MANIFEST_AUTH_FAILED",download.authenticate_manifest,self.raw,download.ReleaseTrustPolicy(frozenset(),frozenset(),expected_manifest_sha256="0"*64))
    def test_03_malformed_pin(self): self.error("MANIFEST_AUTH_FAILED",download.authenticate_manifest,self.raw,download.ReleaseTrustPolicy(frozenset(),frozenset(),expected_manifest_sha256="A"*64))
    def test_04_semantics_after_authentication(self): self.error("MANIFEST_AUTH_FAILED",download.parse_authenticated_manifest,b"{}",self.auth)
    def test_05_http_rejected(self): self.error("UNTRUSTED_ORIGIN",download.require_allowed_url,"http://release.example/x",self.policy.allowed_manifest_origins)
    def test_06_https_allowed(self): download.require_allowed_url("https://release.example/x",self.policy.allowed_manifest_origins)
    def test_07_untrusted_origin(self): self.error("UNTRUSTED_ORIGIN",download.require_allowed_url,"https://evil.example/x",self.policy.allowed_manifest_origins)
    def test_08_downgrade_redirect(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,opener=Opener([Response(status=302,location="http://release.example/y")]))
    def test_09_cross_origin_redirect(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,opener=Opener([Response(status=302,location="https://evil.example/y")]))
    def test_10_allowlisted_redirect(self): self.assertEqual(b"ok",download.fetch_https("https://release.example/x",frozenset({"https://release.example","https://cdn.example"}),20,opener=Opener([Response(status=302,location="https://cdn.example/y"),Response(b"ok")])))
    def test_11_redirect_loop(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,opener=Opener([Response(status=302,location="/x")]))
    def test_12_credentials_rejected(self): self.error("UNTRUSTED_ORIGIN",download.origin,"https://user:pass@release.example/x")
    def test_13_manifest_limit(self): self.error("MANIFEST_TOO_LARGE",download.authenticate_manifest,self.raw,self.policy.__class__(frozenset(),frozenset(),expected_manifest_sha256=self.pin,max_manifest_bytes=1))
    def test_14_version_match(self): self.assertEqual(self.manifest,download.parse_authenticated_manifest(self.raw,self.auth,"0.1.0"))
    def test_15_version_mismatch(self): self.error("VERSION_MISMATCH",download.parse_authenticated_manifest,self.raw,self.auth,"0.2.0")
    def test_16_commit_match(self): download.parse_authenticated_manifest(self.raw,self.auth,expected_source_commit=COMMIT)
    def test_17_commit_mismatch(self): self.error("SOURCE_COMMIT_MISMATCH",download.parse_authenticated_manifest,self.raw,self.auth,None,"b"*40)
    def test_18_x86_aliases(self): self.assertEqual({"x86_64"},{download.normalize_architecture(x) for x in ("x86_64","amd64","x64")})
    def test_19_arm_aliases(self): self.assertEqual({"aarch64"},{download.normalize_architecture(x) for x in ("aarch64","arm64")})
    def test_20_bad_arch(self): self.error("UNSUPPORTED_ARCHITECTURE",download.normalize_architecture,"i686")
    def test_21_bad_platform(self): self.error("UNSUPPORTED_PLATFORM",download.normalize_platform,"darwin")
    def test_22_exact_selection(self): self.assertEqual(self.artifact,download.select_artifact(self.manifest,platform="Linux",architecture="amd64",artifact_type="deb",role="native_package",expected_version="0.1.0"))
    def test_23_no_match(self): self.error("NO_MATCHING_ARTIFACT",download.select_artifact,self.manifest,platform="linux",architecture="arm64",artifact_type="deb",role="native_package",expected_version="0.1.0")
    def test_24_ambiguous(self):
        manifest=copy.deepcopy(self.manifest); duplicate=copy.deepcopy(self.artifact); duplicate["id"]="other"; manifest["artifacts"].append(duplicate)
        self.error("AMBIGUOUS_ARTIFACT",download.select_artifact,manifest,platform="linux",architecture="x64",artifact_type="deb",role="native_package",expected_version="0.1.0")
    def test_25_exact_size(self): self.assertEqual("VERIFIED",download.stage_artifact(io.BytesIO(self.payload),self.root,self.artifact,self.manifest,self.auth)["result"])
    def test_26_oversize(self): self.error("ARTIFACT_TOO_LARGE",download.stage_artifact,io.BytesIO(self.payload+b"x"),self.root,self.artifact,self.manifest,self.auth)
    def test_27_truncated(self): self.error("ARTIFACT_TRUNCATED",download.stage_artifact,io.BytesIO(self.payload[:-1]),self.root,self.artifact,self.manifest,self.auth)
    def test_28_digest_mismatch(self): self.error("ARTIFACT_DIGEST_MISMATCH",download.stage_artifact,io.BytesIO(b"x"*len(self.payload)),self.root,self.artifact,self.manifest,self.auth)
    def test_29_promoted(self): download.stage_artifact(io.BytesIO(self.payload),self.root,self.artifact,self.manifest,self.auth); self.assertEqual(self.payload,(self.root/"synveil.deb").read_bytes())
    def test_30_partial_not_promoted(self):
        self.error("ARTIFACT_TRUNCATED",download.stage_artifact,io.BytesIO(b"x"),self.root,self.artifact,self.manifest,self.auth); self.assertFalse((self.root/"synveil.deb").exists())
    def test_31_existing_verified(self): (self.root/"synveil.deb").write_bytes(self.payload); self.assertEqual("NOOP_ALREADY_VERIFIED",download.stage_artifact(io.BytesIO(b""),self.root,self.artifact,self.manifest,self.auth)["result"])
    def test_32_existing_conflict(self): (self.root/"synveil.deb").write_bytes(b"evil"); self.error("DESTINATION_CONFLICT",download.stage_artifact,io.BytesIO(self.payload),self.root,self.artifact,self.manifest,self.auth)
    def test_33_traversal(self): bad=dict(self.artifact,filename="../evil"); self.error("UNSAFE_PATH",download.stage_artifact,io.BytesIO(self.payload),self.root,bad,self.manifest,self.auth)
    def test_34_symlink_escape(self):
        outside=self.root.parent/"outside-p006"; outside.write_bytes(b"x"); (self.root/"synveil.deb").symlink_to(outside)
        try: self.error("UNSAFE_PATH",download.stage_artifact,io.BytesIO(self.payload),self.root,self.artifact,self.manifest,self.auth)
        finally: outside.unlink(missing_ok=True)
    def test_35_unknown_auth_mode(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,download.ReleaseTrustPolicy(frozenset(),frozenset(),authentication_method="magic"))
    def signature_policy(self, schemes=frozenset({"ed25519"}), keys=frozenset({"test-key"})): return download.ReleaseTrustPolicy(frozenset(),frozenset(),authentication_method="detached_signature",trusted_key_ids=keys,allowed_signature_schemes=schemes)
    def descriptor(self,scheme="ed25519",key="test-key"): return {"signatures":[{"scheme":scheme,"key_id":key,"signature_filename":"manifest.sig"}]}
    def test_36_unknown_scheme(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor("rsa"))
    def test_37_unknown_key(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor(key="remote-key"))
    def test_38_unavailable_verifier(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor())
    def test_39_test_key_not_implicit(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(keys=frozenset()),signature_descriptor=self.descriptor(),verifier=lambda *_:True)
    def test_40_result_no_secrets(self):
        result=download.stage_artifact(io.BytesIO(self.payload),self.root,self.artifact,self.manifest,self.auth)
        self.assertNotIn("token",json.dumps(result).lower()); self.assertEqual([],result["diagnostics_redacted"])
    def test_41_invalid_signature(self): self.error("MANIFEST_AUTH_FAILED",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor(),verifier=lambda *_:False)
    def test_42_mock_valid_signature(self): self.assertEqual("AUTHENTICATED_SIGNATURE",download.authenticate_manifest(self.raw,self.signature_policy(),signature_descriptor=self.descriptor(),verifier=lambda *_:True).state)
    def test_43_manifest_fetch_limit(self): self.error("MANIFEST_TOO_LARGE",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,2,opener=Opener([Response(b"abc")]))
    def test_44_unauthenticated_cannot_stage(self): self.error("MANIFEST_AUTH_FAILED",download.stage_artifact,io.BytesIO(self.payload),self.root,self.artifact,self.manifest,download.ManifestAuthentication("UNAUTHENTICATED","none",self.pin,len(self.raw)))

if __name__ == "__main__": unittest.main()
