#!/usr/bin/env python3
import copy, hashlib, importlib.util, io, json, sys, tempfile, unittest
import errno
import urllib.error
from pathlib import Path
from unittest import mock

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


class Opener(download.RawHttpsTransport):
    def __init__(self, responses): self.responses=iter(responses); self.calls=[]
    def request(self, request): self.calls.append(request.full_url); return next(self.responses)


class IncrementalResponse(Response):
    def __init__(self, body): super().__init__(body); self.read_sizes=[]
    def read(self, size=-1):
        if size < 0: raise AssertionError("unbounded read")
        self.read_sizes.append(size)
        return super().read(min(size, 3))


class ReleaseDownloadTests(unittest.TestCase):
    def setUp(self):
        self.payload=b"trusted artifact"
        self.artifact={"id":"linux-x86_64-deb", "artifact_type":"deb", "filename":"synveil.deb", "platform":"linux", "architecture":"x86_64", "role":"native_package", "product_version":"0.1.0", "size_bytes":len(self.payload), "sha256":hashlib.sha256(self.payload).hexdigest(), "components":["synveil-client"], "package_metadata":{"format":"deb","package_name":"synveil","package_version":"0.1.0","package_architecture":"amd64"}}
        self.manifest={"schema_version":1,"product":"Synveil","product_version":"0.1.0","source_commit":COMMIT,"artifacts":[self.artifact]}
        self.raw=json.dumps(self.manifest,sort_keys=True).encode(); self.pin=hashlib.sha256(self.raw).hexdigest()
        self.policy=download.ReleaseTrustPolicy(frozenset({"https://release.example"}),frozenset({"https://release.example"}),expected_manifest_sha256=self.pin)
        self.auth=download.authenticate_manifest(self.raw,self.policy)
        self.context=download.parse_authenticated_manifest(self.raw,self.auth)
        self.selection=download.select_artifact(self.context,platform="linux",architecture="x86_64",artifact_type="deb",role="native_package",expected_version="0.1.0")
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
    def test_08_downgrade_redirect(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,transport=Opener([Response(status=302,location="http://release.example/y")]))
    def test_09_cross_origin_redirect(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,transport=Opener([Response(status=302,location="https://evil.example/y")]))
    def test_10_allowlisted_redirect(self): self.assertEqual(b"ok",download.fetch_https("https://release.example/x",frozenset({"https://release.example","https://cdn.example"}),20,transport=Opener([Response(status=302,location="https://cdn.example/y"),Response(b"ok")])))
    def test_11_redirect_loop(self): self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,transport=Opener([Response(status=302,location="/x")]))
    def test_12_credentials_rejected(self): self.error("UNTRUSTED_ORIGIN",download.origin,"https://user:pass@release.example/x")
    def test_13_manifest_limit(self): self.error("MANIFEST_TOO_LARGE",download.authenticate_manifest,self.raw,self.policy.__class__(frozenset(),frozenset(),expected_manifest_sha256=self.pin,max_manifest_bytes=1))
    def test_14_version_match(self): self.assertEqual(self.manifest,download.parse_authenticated_manifest(self.raw,self.auth,"0.1.0").document)
    def test_15_version_mismatch(self): self.error("VERSION_MISMATCH",download.parse_authenticated_manifest,self.raw,self.auth,"0.2.0")
    def test_16_commit_match(self): download.parse_authenticated_manifest(self.raw,self.auth,expected_source_commit=COMMIT)
    def test_17_commit_mismatch(self): self.error("SOURCE_COMMIT_MISMATCH",download.parse_authenticated_manifest,self.raw,self.auth,None,"b"*40)
    def test_18_x86_aliases(self): self.assertEqual({"x86_64"},{download.normalize_architecture(x) for x in ("x86_64","amd64","x64")})
    def test_19_arm_aliases(self): self.assertEqual({"aarch64"},{download.normalize_architecture(x) for x in ("aarch64","arm64")})
    def test_20_bad_arch(self): self.error("UNSUPPORTED_ARCHITECTURE",download.normalize_architecture,"i686")
    def test_21_bad_platform(self): self.error("UNSUPPORTED_PLATFORM",download.normalize_platform,"darwin")
    def test_22_exact_selection(self): self.assertEqual(self.artifact,download.select_artifact(self.context,platform="Linux",architecture="amd64",artifact_type="deb",role="native_package",expected_version="0.1.0").artifact)
    def test_23_no_match(self): self.error("NO_MATCHING_ARTIFACT",download.select_artifact,self.context,platform="linux",architecture="arm64",artifact_type="deb",role="native_package",expected_version="0.1.0")
    def test_24_ambiguous(self):
        manifest=copy.deepcopy(self.manifest); duplicate=copy.deepcopy(self.artifact); duplicate["id"]="other"; manifest["artifacts"].append(duplicate)
        raw=json.dumps(manifest,sort_keys=True).encode(); policy=download.ReleaseTrustPolicy(frozenset(),frozenset(),expected_manifest_sha256=hashlib.sha256(raw).hexdigest())
        context=download.parse_authenticated_manifest(raw,download.authenticate_manifest(raw,policy))
        self.error("AMBIGUOUS_ARTIFACT",download.select_artifact,context,platform="linux",architecture="x64",artifact_type="deb",role="native_package",expected_version="0.1.0")
    def test_25_exact_size(self): self.assertEqual("VERIFIED",download.stage_artifact(io.BytesIO(self.payload),self.root,self.selection)["result"])
    def test_26_oversize(self): self.error("ARTIFACT_TOO_LARGE",download.stage_artifact,io.BytesIO(self.payload+b"x"),self.root,self.selection)
    def test_27_truncated(self): self.error("ARTIFACT_TRUNCATED",download.stage_artifact,io.BytesIO(self.payload[:-1]),self.root,self.selection)
    def test_28_digest_mismatch(self): self.error("ARTIFACT_DIGEST_MISMATCH",download.stage_artifact,io.BytesIO(b"x"*len(self.payload)),self.root,self.selection)
    def test_29_promoted(self): download.stage_artifact(io.BytesIO(self.payload),self.root,self.selection); self.assertEqual(self.payload,(self.root/"synveil.deb").read_bytes())
    def test_30_partial_not_promoted(self):
        self.error("ARTIFACT_TRUNCATED",download.stage_artifact,io.BytesIO(b"x"),self.root,self.selection); self.assertFalse((self.root/"synveil.deb").exists())
    def test_31_existing_verified(self): (self.root/"synveil.deb").write_bytes(self.payload); self.assertEqual("NOOP_ALREADY_VERIFIED",download.stage_artifact(io.BytesIO(b""),self.root,self.selection)["result"])
    def test_32_existing_conflict(self): (self.root/"synveil.deb").write_bytes(b"evil"); self.error("DESTINATION_CONFLICT",download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
    def test_33_traversal(self):
        bad=download.SelectedArtifact(self.context,0,dict(self.artifact,filename="../evil"))
        self.error("MANIFEST_AUTH_FAILED",download.stage_artifact,io.BytesIO(self.payload),self.root,bad)
    def test_34_symlink_escape(self):
        outside=self.root.parent/"outside-p006"; outside.write_bytes(b"x"); (self.root/"synveil.deb").symlink_to(outside)
        try: self.error("UNSAFE_PATH",download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        finally: outside.unlink(missing_ok=True)
    def test_35_unknown_auth_mode(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,download.ReleaseTrustPolicy(frozenset(),frozenset(),authentication_method="magic"))
    def signature_policy(self, schemes=frozenset({"ed25519"}), keys=frozenset({"test-key"})): return download.ReleaseTrustPolicy(frozenset(),frozenset(),authentication_method="detached_signature",trusted_key_ids=keys,allowed_signature_schemes=schemes)
    def descriptor(self,scheme="ed25519",key="test-key"): return {"signatures":[{"scheme":scheme,"key_id":key,"signature_filename":"manifest.sig"}]}
    def test_36_unknown_scheme(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor("rsa"))
    def test_37_unknown_key(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor(key="remote-key"))
    def test_38_unavailable_verifier(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor())
    def test_39_test_key_not_implicit(self): self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(keys=frozenset()),signature_descriptor=self.descriptor(),verifier=lambda *_:True)
    def test_40_result_no_secrets(self):
        result=download.stage_artifact(io.BytesIO(self.payload),self.root,self.selection)
        self.assertNotIn("token",json.dumps(result).lower()); self.assertEqual([],result["diagnostics_redacted"])
    def test_41_invalid_signature(self): self.error("MANIFEST_AUTH_FAILED",download.authenticate_manifest,self.raw,self.signature_policy(),signature_descriptor=self.descriptor(),verifier=lambda *_:False)
    def test_42_mock_valid_signature(self): self.assertEqual("AUTHENTICATED_SIGNATURE",download.authenticate_manifest(self.raw,self.signature_policy(),signature_descriptor=self.descriptor(),verifier=lambda *_:True).state)
    def test_43_manifest_fetch_limit(self): self.error("MANIFEST_TOO_LARGE",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,2,transport=Opener([Response(b"abc")]))
    def test_44_unauthenticated_cannot_stage(self):
        context=download.AuthenticatedManifest(self.raw,self.manifest,download.ManifestAuthentication("UNAUTHENTICATED","none",self.pin,len(self.raw)))
        self.error("MANIFEST_AUTH_FAILED",download.stage_artifact,io.BytesIO(self.payload),self.root,download.SelectedArtifact(context,0,self.artifact))
    def test_45_auth_a_manifest_b_rejected(self):
        self.error("MANIFEST_AUTH_FAILED",download.parse_authenticated_manifest,b"{}",self.auth)
    def test_46_artifact_b_rejected(self):
        forged=download.SelectedArtifact(self.context,0,dict(self.artifact,id="forged"))
        self.error("MANIFEST_AUTH_FAILED",download.stage_artifact,io.BytesIO(self.payload),self.root,forged)
    def test_47_invalid_auth_zero_requests(self):
        context=download.AuthenticatedManifest(self.raw,self.manifest,download.ManifestAuthentication("NO","none",self.pin,len(self.raw)))
        transport=Opener([Response(self.payload)])
        self.error("MANIFEST_AUTH_FAILED",download.download_artifact,"https://release.example",self.root,download.SelectedArtifact(context,0,self.artifact),self.policy,transport=transport)
        self.assertEqual([],transport.calls)
    def test_48_url_requires_authenticated_selection(self):
        context=download.AuthenticatedManifest(self.raw,self.manifest,download.ManifestAuthentication("NO","none",self.pin,len(self.raw)))
        self.error("MANIFEST_AUTH_FAILED",download.trusted_artifact_url,"https://release.example",download.SelectedArtifact(context,0,self.artifact),self.policy.allowed_artifact_origins)
    def test_49_destination_root_symlink(self):
        actual=self.root/"actual"; actual.mkdir(); link=self.root/"link"; link.symlink_to(actual,target_is_directory=True)
        self.error("UNSAFE_PATH",download.stage_artifact,io.BytesIO(self.payload),link,self.selection)
        self.assertEqual([],list(actual.iterdir()))
    def test_50_concurrent_target_no_clobber(self):
        class Racing(io.BytesIO):
            def read(inner,size=-1):
                value=super(Racing,inner).read(size)
                if not value and not (self.root/"synveil.deb").exists(): (self.root/"synveil.deb").write_bytes(b"racer")
                return value
        self.error("DESTINATION_CONFLICT",download.stage_artifact,Racing(self.payload),self.root,self.selection)
        self.assertEqual(b"racer",(self.root/"synveil.deb").read_bytes())
    def test_51_stream_consumed_incrementally(self):
        response=IncrementalResponse(self.payload); result=download.download_artifact("https://release.example",self.root,self.selection,self.policy,transport=Opener([response]))
        self.assertEqual("VERIFIED",result["result"]); self.assertTrue(response.read_sizes); self.assertNotIn(-1,response.read_sizes)
    def test_52_stream_oversize(self):
        response=IncrementalResponse(self.payload+b"x")
        self.error("ARTIFACT_TOO_LARGE",download.download_artifact,"https://release.example",self.root,self.selection,self.policy,transport=Opener([response]))
        self.assertFalse((self.root/"synveil.deb").exists())
    def test_53_stream_truncated(self):
        self.error("ARTIFACT_TRUNCATED",download.download_artifact,"https://release.example",self.root,self.selection,self.policy,transport=Opener([IncrementalResponse(self.payload[:-1])]))
    def test_54_stream_digest_mismatch(self):
        self.error("ARTIFACT_DIGEST_MISMATCH",download.download_artifact,"https://release.example",self.root,self.selection,self.policy,transport=Opener([IncrementalResponse(b"x"*len(self.payload))]))
    def test_55_existing_verified_zero_network(self):
        (self.root/"synveil.deb").write_bytes(self.payload); transport=Opener([])
        self.assertEqual("NOOP_ALREADY_VERIFIED",download.download_artifact("https://release.example",self.root,self.selection,self.policy,transport=transport)["result"]); self.assertEqual([],transport.calls)
    def test_56_existing_conflict_zero_network(self):
        (self.root/"synveil.deb").write_bytes(b"bad"); transport=Opener([])
        self.error("DESTINATION_CONFLICT",download.download_artifact,"https://release.example",self.root,self.selection,self.policy,transport=transport); self.assertEqual([],transport.calls)
    def rotation_descriptor(self, reverse=False):
        entries=[{"scheme":"ed25519","key_id":"old","signature_filename":"old.sig"},{"scheme":"ed25519","key_id":"new","signature_filename":"new.sig"}]
        return {"signatures":list(reversed(entries)) if reverse else entries}
    def test_57_rotation_old_only(self):
        auth=download.authenticate_manifest(self.raw,self.signature_policy(keys=frozenset({"old"})),signature_descriptor=self.rotation_descriptor(),verifier=lambda *_:True); self.assertEqual("old",auth.key_id)
    def test_58_rotation_new_only(self):
        auth=download.authenticate_manifest(self.raw,self.signature_policy(keys=frozenset({"new"})),signature_descriptor=self.rotation_descriptor(),verifier=lambda *_:True); self.assertEqual("new",auth.key_id)
    def test_59_rotation_order_independent(self):
        policy=self.signature_policy(keys=frozenset({"old"})); verify=lambda *_:True
        self.assertEqual(download.authenticate_manifest(self.raw,policy,signature_descriptor=self.rotation_descriptor(),verifier=verify).key_id,download.authenticate_manifest(self.raw,policy,signature_descriptor=self.rotation_descriptor(True),verifier=verify).key_id)
    def test_60_later_eligible_succeeds(self):
        auth=download.authenticate_manifest(self.raw,self.signature_policy(keys=frozenset({"old","new"})),signature_descriptor=self.rotation_descriptor(),verifier=lambda key,*_:key=="old"); self.assertEqual("old",auth.key_id)
    def test_61_all_eligible_fail(self):
        self.error("MANIFEST_AUTH_FAILED",download.authenticate_manifest,self.raw,self.signature_policy(keys=frozenset({"old","new"})),signature_descriptor=self.rotation_descriptor(),verifier=lambda *_:False)
    def test_62_none_eligible(self):
        self.error("UNSUPPORTED_AUTHENTICATION",download.authenticate_manifest,self.raw,self.signature_policy(keys=frozenset({"other"})),signature_descriptor=self.rotation_descriptor(),verifier=lambda *_:True)
    def test_63_cross_origin_not_requested(self):
        transport=Opener([Response(status=302,location="https://evil.example/x")]); self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,10,transport=transport); self.assertEqual(["https://release.example/x"],transport.calls)
    def test_64_http_target_not_requested(self):
        transport=Opener([Response(status=302,location="http://release.example/x")]); self.error("UNSAFE_REDIRECT",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,10,transport=transport); self.assertEqual(["https://release.example/x"],transport.calls)
    def test_65_normal_opener_rejected(self):
        with self.assertRaises(TypeError): download.fetch_https("https://release.example/x",self.policy.allowed_manifest_origins,10,transport=object())
    def test_66_result_single_context(self):
        result=download.stage_artifact(io.BytesIO(self.payload),self.root,self.selection)
        self.assertEqual((self.pin,"0.1.0",COMMIT,self.artifact["id"],self.artifact["sha256"],len(self.payload)),(result["manifest_identity"]["sha256"],result["product_version"],result["source_commit"],result["artifact_id"],result["artifact_sha256"],result["artifact_size"]))
    def test_67_transport_failure_is_typed(self):
        class Failed(download.UrllibNoRedirectTransport):
            def __init__(self):
                class Broken:
                    def open(*_args, **_kwargs): raise urllib.error.URLError("redacted")
                self._opener=Broken()
        self.error("NETWORK_ERROR",download.fetch_https,"https://release.example/x",self.policy.allowed_manifest_origins,20,transport=Failed())

if __name__ == "__main__": unittest.main()

class StageRaceSecurityTests(unittest.TestCase):
    setUp = ReleaseDownloadTests.setUp
    tearDown = ReleaseDownloadTests.tearDown
    error = ReleaseDownloadTests.error
    def test_68_parent_replacement_is_rejected_and_owned_temp_cleaned(self):
        if sys.platform == 'win32': self.skipTest('Unix directory descriptor race; Windows reparse tests are separate')
        import os
        stage=self.root/'stage'; stage.mkdir(mode=0o700); outside=self.root/'outside'; outside.mkdir(); moved=self.root/'moved'
        class Racing(io.BytesIO):
            def read(inner, size=-1):
                value=super().read(size)
                if not value and stage.is_dir() and not stage.is_symlink():
                    stage.rename(moved); stage.symlink_to(outside, target_is_directory=True)
                return value
        self.error('UNSAFE_PATH',download.stage_artifact,Racing(self.payload),stage,self.selection)
        self.assertEqual([],list(outside.iterdir())); self.assertEqual([],list(moved.iterdir()))

    def test_69_temporary_path_replacement_never_promotes_other_bytes(self):
        # Replace after the download handle closes: Windows correctly prevents
        # replacing an open file, while both platforms need this last-use guard.
        original_matches = download._matches
        def replace_before_last_use(path, size, digest):
            path.unlink(); path.write_bytes(b'evil')
            return original_matches(path, size, digest)
        with mock.patch.object(download, '_matches', side_effect=replace_before_last_use):
            self.error('UNSAFE_PATH',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertFalse((self.root/'synveil.deb').exists()); self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_70_enospc_during_temp_creation_has_no_final_artifact(self):
        with mock.patch.object(download.tempfile, 'mkstemp', side_effect=OSError(errno.ENOSPC, 'full')):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertFalse((self.root/'synveil.deb').exists())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_71_enospc_during_file_sync_cleans_only_owned_temp(self):
        with mock.patch.object(download.os, 'fsync', side_effect=OSError(errno.ENOSPC, 'full')):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertFalse((self.root/'synveil.deb').exists())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_72_enospc_during_promotion_never_promotes_partial_bytes(self):
        with mock.patch.object(download.os, 'link', side_effect=OSError(errno.ENOSPC, 'full')):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertFalse((self.root/'synveil.deb').exists())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_72a_promotion_failure_is_after_file_fsync(self):
        real_fsync = download.os.fsync
        calls = 0
        def observe_file_sync(fd):
            nonlocal calls
            calls += 1
            return real_fsync(fd)
        with mock.patch.object(download.os, 'fsync', side_effect=observe_file_sync), \
             mock.patch.object(download.os, 'link', side_effect=OSError(errno.ENOSPC, 'full')):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertGreaterEqual(calls, 1)
        self.assertFalse((self.root/'synveil.deb').exists())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_73_enospc_during_directory_sync_is_not_reported_verified(self):
        with mock.patch.object(download, '_sync_staging_parent',
                                side_effect=OSError(errno.ENOSPC, 'full')):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        target = self.root/'synveil.deb'
        self.assertEqual(self.payload,target.read_bytes())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))
        self.assertEqual('NOOP_ALREADY_VERIFIED',download.stage_artifact(io.BytesIO(b''),self.root,self.selection)['result'])

    def test_74_racing_existing_destination_survives_failed_promotion(self):
        target = self.root/'synveil.deb'
        class Racing(io.BytesIO):
            def read(inner, size=-1):
                value = super(Racing, inner).read(size)
                if value and not target.exists():
                    target.write_bytes(b'user destination')
                return value
        self.error('DESTINATION_CONFLICT',download.stage_artifact,Racing(self.payload),self.root,self.selection)
        self.assertEqual(b'user destination',target.read_bytes())

    def test_75_enospc_during_stream_write_cleans_partial_temp(self):
        original_fdopen = download.os.fdopen
        class FullOutput:
            def __init__(self, fd, mode): self.file = original_fdopen(fd, mode)
            def __enter__(self): return self
            def __exit__(self, *args): return self.file.__exit__(*args)
            def fileno(self): return self.file.fileno()
            def write(self, data):
                self.file.write(data[:1])
                raise OSError(errno.ENOSPC, 'full')
            def flush(self): return self.file.flush()
        with mock.patch.object(download.os, 'fdopen', side_effect=FullOutput):
            self.error('INSUFFICIENT_DISK_SPACE',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertFalse((self.root/'synveil.deb').exists())
        self.assertEqual([],list(self.root.glob('.synveil-download-*')))

    def test_76_directory_sync_failure_never_returns_verified(self):
        with mock.patch.object(download, '_sync_staging_parent',
                                side_effect=OSError(5, 'io failure')):
            self.error('STAGING_ERROR',download.stage_artifact,io.BytesIO(self.payload),self.root,self.selection)
        self.assertEqual(self.payload,(self.root/'synveil.deb').read_bytes())
