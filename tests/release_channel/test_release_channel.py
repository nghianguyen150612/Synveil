import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import release_channel as channel
import release_download as download

COMMIT_A = "a" * 40
COMMIT_B = "b" * 40


def release(version="0.1.0", *, fresh=True, upgrades=None, commit=COMMIT_A, size=10, digest=None):
    return {"product_version": version, "source_commit": commit,
            "manifest_filename": channel.MANIFEST_FILENAME, "manifest_size_bytes": size,
            "manifest_sha256": digest or "1" * 64, "fresh_install": fresh,
            "upgrade_from": [] if upgrades is None else upgrades}


def document(releases=None, generation=1):
    return {"schema_version": 1, "product": "Synveil", "channel": "stable",
            "generation": generation, "releases": [] if releases is None else releases}


def raw_doc(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def context(value=None, minimum=1):
    raw = raw_doc(document() if value is None else value)
    policy = channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest(), minimum_generation=minimum)
    return channel.parse_authenticated_channel(raw, channel.authenticate_channel(raw, policy), policy)


def fresh(ctx, stored=None):
    return channel.select_fresh_install(ctx, stored)


def upgrade(ctx, current, stored=None):
    return channel.select_upgrade(ctx, current, stored)


class ChannelTests(unittest.TestCase):
    def error(self, code, func, *args, **kwargs):
        with self.assertRaises(channel.ChannelError) as caught:
            func(*args, **kwargs)
        self.assertEqual(code, caught.exception.code)

    def test_01_empty_valid(self): self.assertEqual([], channel.validate_channel(document())["releases"])
    def test_02_invalid_json(self):
        raw=b"{"
        policy=channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest())
        auth=channel.authenticate_channel(raw,policy)
        self.error("INVALID_CHANNEL",channel.parse_authenticated_channel,raw,auth,policy)
    def test_03_invalid_utf8(self):
        raw=b"\xff"
        policy=channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest())
        auth=channel.authenticate_channel(raw,policy)
        self.error("INVALID_CHANNEL",channel.parse_authenticated_channel,raw,auth,policy)
    def mutate_error(self, code, key, value):
        item=document(); item[key]=value; self.error(code,channel.validate_channel,item)
    def test_04_unknown_top(self): self.mutate_error("INVALID_CHANNEL","url","https://evil")
    def test_05_missing_top(self):
        item=document(); del item["product"]; self.error("INVALID_CHANNEL",channel.validate_channel,item)
    def test_06_future_schema(self): self.mutate_error("UNSUPPORTED_CHANNEL_SCHEMA","schema_version",2)
    def test_07_wrong_product(self): self.mutate_error("UNSUPPORTED_CHANNEL","product","Other")
    def test_08_nonstable(self): self.mutate_error("UNSUPPORTED_CHANNEL","channel","beta")
    def test_09_generation_zero(self): self.mutate_error("INVALID_CHANNEL","generation",0)
    def test_10_generation_bool(self): self.mutate_error("INVALID_CHANNEL","generation",True)
    def test_11_generation_max(self): channel.validate_channel(document(generation=channel.MAX_GENERATION))
    def test_12_generation_too_large(self): self.mutate_error("INVALID_CHANNEL","generation",channel.MAX_GENERATION+1)
    def test_13_unknown_release_field(self):
        item=release(); item["url"]="https://evil"; self.error("INVALID_CHANNEL",channel.validate_channel,document([item]))
    def test_14_bad_commit(self):
        item=release(); item["source_commit"]="A"*40; self.error("INVALID_CHANNEL",channel.validate_channel,document([item]))
    def test_15_bad_digest(self):
        item=release(); item["manifest_sha256"]="A"*64; self.error("INVALID_CHANNEL",channel.validate_channel,document([item]))
    def test_16_bad_size_zero(self): self.error("INVALID_CHANNEL",channel.validate_channel,document([release(size=0)]))
    def test_17_bad_size_max(self): self.error("INVALID_CHANNEL",channel.validate_channel,document([release(size=1048577)]))
    def test_18_size_boundary(self): channel.validate_channel(document([release(size=1048576)]))
    def test_19_wrong_manifest_name(self):
        item=release(); item["manifest_filename"]="other.json"; self.error("INVALID_CHANNEL",channel.validate_channel,document([item]))
    def test_20_duplicate_version(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release(),release()]))
    def test_21_out_of_order(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("0.2.0"),release("0.1.0")]))
    def test_22_duplicate_upgrade(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("0.2.0",upgrades=["0.1.0","0.1.0"])]))
    def test_23_upgrade_self(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("0.2.0",upgrades=["0.2.0"])]))
    def test_24_upgrade_newer(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("0.2.0",upgrades=["0.3.0"])]))
    def test_25_upgrade_unsorted(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("0.3.0",upgrades=["0.2.0","0.1.0"])]))
    def test_26_release_limit(self): channel.validate_channel(document([release(f"0.0.{i}") for i in range(256)]))
    def test_27_release_limit_plus_one(self): self.error("INVALID_CHANNEL",channel.validate_channel,document([release(f"0.0.{i}") for i in range(257)]))
    def test_28_upgrade_limit(self): channel.validate_channel(document([release("1.0.0",upgrades=[f"0.0.{i}" for i in range(256)])]))
    def test_29_upgrade_limit_plus_one(self): self.error("INVALID_RELEASE_POLICY",channel.validate_channel,document([release("1.0.0",upgrades=[f"0.0.{i}" for i in range(257)])]))
    def test_30_pin_auth(self): self.assertEqual("AUTHENTICATED_PINNED_DIGEST",context().authentication.state)
    def test_31_wrong_pin(self): self.error("CHANNEL_AUTH_FAILED",channel.authenticate_channel,b"x",channel.ChannelTrustPolicy(expected_channel_sha256="0"*64))
    def test_32_bad_pin(self): self.error("CHANNEL_AUTH_FAILED",channel.authenticate_channel,b"x",channel.ChannelTrustPolicy(expected_channel_sha256="bad"))
    def test_33_channel_limit(self):
        raw=b"x"*10; self.error("CHANNEL_TOO_LARGE",channel.authenticate_channel,raw,channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest(),max_channel_bytes=9))
    def descriptor(self, raw, entries=None): return {"schema_version":1,"channel_filename":channel.CHANNEL_FILENAME,"channel_size_bytes":len(raw),"channel_sha256":hashlib.sha256(raw).hexdigest(),"signatures":entries or [{"scheme":"ed25519","key_id":"key","signature_filename":"channel.sig"}]}
    def sig_policy(self, keys=frozenset({"key"}), schemes=frozenset({"ed25519"})): return channel.ChannelTrustPolicy(authentication_method="detached_signature",trusted_key_ids=keys,allowed_signature_schemes=schemes)
    def test_34_signature_auth(self):
        raw=raw_doc(document()); self.assertEqual("key",channel.authenticate_channel(raw,self.sig_policy(),descriptor=self.descriptor(raw),verifier=lambda *_:True).key_id)
    def test_35_unknown_key(self):
        raw=raw_doc(document()); self.error("UNSUPPORTED_CHANNEL_AUTHENTICATION",channel.authenticate_channel,raw,self.sig_policy(frozenset({"other"})),descriptor=self.descriptor(raw),verifier=lambda *_:True)
    def test_36_unknown_scheme(self):
        raw=raw_doc(document()); self.error("UNSUPPORTED_CHANNEL_AUTHENTICATION",channel.authenticate_channel,raw,self.sig_policy(schemes=frozenset({"rsa"})),descriptor=self.descriptor(raw),verifier=lambda *_:True)
    def test_37_no_verifier(self):
        raw=raw_doc(document()); self.error("UNSUPPORTED_CHANNEL_AUTHENTICATION",channel.authenticate_channel,raw,self.sig_policy(),descriptor=self.descriptor(raw))
    def test_38_failed_signature(self):
        raw=raw_doc(document()); self.error("CHANNEL_AUTH_FAILED",channel.authenticate_channel,raw,self.sig_policy(),descriptor=self.descriptor(raw),verifier=lambda *_:False)
    def test_39_descriptor_wrong_bytes(self):
        raw=raw_doc(document()); desc=self.descriptor(raw); desc["channel_sha256"]="0"*64; self.error("CHANNEL_AUTH_FAILED",channel.authenticate_channel,raw,self.sig_policy(),descriptor=desc,verifier=lambda *_:True)
    def test_40_rotation_order_independent(self):
        raw=raw_doc(document()); entries=[{"scheme":"ed25519","key_id":x,"signature_filename":x+".sig"} for x in ("old","new")]
        policy=self.sig_policy(frozenset({"old","new"})); verifier=lambda key,*_:key=="old"
        a=channel.authenticate_channel(raw,policy,descriptor=self.descriptor(raw,entries),verifier=verifier)
        b=channel.authenticate_channel(raw,policy,descriptor=self.descriptor(raw,list(reversed(entries))),verifier=verifier)
        self.assertEqual(("old","old"),(a.key_id,b.key_id))
    def test_41_auth_a_bytes_b(self):
        a=context(document(generation=1)); raw=raw_doc(document(generation=2)); self.error("CHANNEL_AUTH_FAILED",channel.parse_authenticated_channel,raw,a.authentication,channel.ChannelTrustPolicy())
    def test_42_document_detached(self):
        ctx=context(document([release()])); forged=channel.AuthenticatedChannel(ctx.raw_bytes,document(),ctx.authentication); self.error("CHANNEL_AUTH_FAILED",fresh,forged)
    def test_43_minimum_generation(self):
        raw=raw_doc(document(generation=1)); policy=channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest(),minimum_generation=2)
        self.error("CHANNEL_ROLLBACK",channel.parse_authenticated_channel,raw,channel.authenticate_channel(raw,policy),policy)
    def test_44_highwater_initial(self): self.assertEqual(2,channel.evaluate_high_water(context(document(generation=2)),None).generation)
    def test_45_highwater_advance(self): self.assertEqual(2,channel.evaluate_high_water(context(document(generation=2)),channel.ChannelHighWater(1,"0"*64)).generation)
    def test_46_highwater_rollback(self): self.error("CHANNEL_ROLLBACK",channel.evaluate_high_water,context(document(generation=1)),channel.ChannelHighWater(2,"0"*64))
    def test_47_highwater_replay(self):
        ctx=context(document(generation=2)); stored=channel.ChannelHighWater(2,ctx.authentication.channel_sha256); self.assertIs(stored,channel.evaluate_high_water(ctx,stored))
    def test_48_highwater_equivocation(self):
        ctx=context(document([release()],generation=2)); self.error("CHANNEL_EQUIVOCATION",channel.evaluate_high_water,ctx,channel.ChannelHighWater(2,"0"*64))
    def test_49_fresh_one(self): self.assertEqual("0.1.0",fresh(context(document([release()]))).selected.release["product_version"])
    def test_50_fresh_highest(self): self.assertEqual("0.10.0",fresh(context(document([release("0.9.0"),release("0.10.0")]))).selected.release["product_version"])
    def test_51_fresh_skip_newer(self): self.assertEqual("0.1.0",fresh(context(document([release(),release("0.2.0",fresh=False)]))).selected.release["product_version"])
    def test_52_no_fresh(self): self.assertEqual("NO_RELEASE_AVAILABLE",fresh(context(document([release(fresh=False)]))).outcome)
    def test_53_empty_fresh(self): self.assertEqual("NO_RELEASE_AVAILABLE",fresh(context()).outcome)
    def test_54_upgrade_one(self): self.assertEqual("0.2.0",upgrade(context(document([release(),release("0.2.0",upgrades=["0.1.0"])])),"0.1.0").selected.release["product_version"])
    def test_55_upgrade_highest(self): self.assertEqual("0.3.0",upgrade(context(document([release(),release("0.2.0",upgrades=["0.1.0"]),release("0.3.0",upgrades=["0.1.0"])])),"0.1.0").selected.release["product_version"])
    def test_56_upgrade_skip_incompatible(self): self.assertEqual("0.2.0",upgrade(context(document([release(),release("0.2.0",upgrades=["0.1.0"]),release("0.3.0",upgrades=["0.2.0"])])),"0.1.0").selected.release["product_version"])
    def test_57_upgrade_same_skipped(self): self.assertEqual("NO_NEWER_COMPATIBLE_RELEASE",upgrade(context(document([release("0.1.0")])),"0.1.0").outcome)
    def test_58_upgrade_never_lower(self): self.assertEqual("NO_NEWER_COMPATIBLE_RELEASE",upgrade(context(document([release("0.1.0")])),"0.2.0").outcome)
    def test_59_upgrade_absent(self): self.assertEqual("NO_NEWER_COMPATIBLE_RELEASE",upgrade(context(document([release(),release("0.2.0")])),"0.1.0").outcome)
    def test_60_bad_current(self): self.error("INVALID_VERSION",upgrade,context(),"v0.1.0")
    def test_61_detached_release(self):
        selected=fresh(context(document([release()]))).selected
        forged=channel.SelectedRelease(selected.authenticated_channel,0,release("0.2.0"),"fresh_install")
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)
    def test_62_cross_channel_release(self):
        a=fresh(context(document([release()]))).selected
        b=fresh(context(document([release("0.2.0")],generation=2))).selected
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,channel.SelectedRelease(a.authenticated_channel,0,b.release,"fresh_install"))
    def manifest(self, version="0.1.0", commit=COMMIT_A): return {"schema_version":1,"product":"Synveil","product_version":version,"source_commit":commit,"artifacts":[]}
    def selected_for_manifest(self, manifest):
        raw=raw_doc(manifest); rel=release(manifest["product_version"],commit=manifest["source_commit"],size=len(raw),digest=hashlib.sha256(raw).hexdigest())
        return fresh(context(document([rel]))).selected,raw
    def test_63_manifest_bridge(self):
        selected,raw=self.selected_for_manifest(self.manifest()); self.assertEqual("0.1.0",channel.authenticate_selected_manifest(selected,raw).document["product_version"])
    def test_64_manifest_wrong_bytes(self):
        selected,raw=self.selected_for_manifest(self.manifest()); self.error("MANIFEST_BINDING_MISMATCH",channel.authenticate_selected_manifest,selected,raw[:-1]+b"x")
    def test_65_manifest_wrong_size(self):
        selected,raw=self.selected_for_manifest(self.manifest()); self.error("MANIFEST_BINDING_MISMATCH",channel.authenticate_selected_manifest,selected,raw+b"x")
    def test_66_manifest_wrong_version(self):
        selected,raw=self.selected_for_manifest(self.manifest()); altered=raw_doc(self.manifest("0.2.0")); forged=copy.deepcopy(selected.release); forged["manifest_size_bytes"]=len(altered); forged["manifest_sha256"]=hashlib.sha256(altered).hexdigest(); ctx=context(document([forged])); self.error("MANIFEST_BINDING_MISMATCH",channel.authenticate_selected_manifest,fresh(ctx).selected,altered)
    def test_67_manifest_wrong_commit(self):
        selected,raw=self.selected_for_manifest(self.manifest()); altered=raw_doc(self.manifest(commit=COMMIT_B)); forged=copy.deepcopy(selected.release); forged["manifest_size_bytes"]=len(altered); forged["manifest_sha256"]=hashlib.sha256(altered).hexdigest(); ctx=context(document([forged])); self.error("MANIFEST_BINDING_MISMATCH",channel.authenticate_selected_manifest,fresh(ctx).selected,altered)
    def test_68_evidence(self):
        selected=upgrade(context(document([release(),release("0.2.0",upgrades=["0.1.0"])])),"0.1.0").selected; evidence=channel.selection_evidence(selected)
        self.assertEqual(("stable","upgrade","0.1.0","0.2.0"),(evidence["channel"],evidence["selection_mode"],evidence["current_version"],evidence["selected_product_version"]))
    def test_69_auth_descriptor_unknown(self):
        raw=raw_doc(document()); desc=self.descriptor(raw); desc["url"]="x"; self.error("INVALID_CHANNEL",channel.validate_auth_descriptor,desc)
    def test_70_descriptor_unsafe_filename(self):
        raw=raw_doc(document()); desc=self.descriptor(raw); desc["signatures"][0]["signature_filename"]="../x"; self.error("INVALID_CHANNEL",channel.validate_auth_descriptor,desc)
    def test_71_builder_deterministic_and_derived(self):
        manifest=self.manifest(); raw=raw_doc(manifest)
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/channel.MANIFEST_FILENAME; path.write_bytes(raw)
            a=channel.build_channel(2,[(path,True,[])]); b=channel.build_channel(2,[(path,True,[])])
        self.assertEqual(a,b); item=json.loads(a)["releases"][0]; self.assertEqual((len(raw),hashlib.sha256(raw).hexdigest()),(item["manifest_size_bytes"],item["manifest_sha256"]))
    def test_72_builder_semantic_sort(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths=[]
            for version in ("0.10.0","0.9.0"):
                path=Path(tmp)/(version+".json"); path.write_bytes(raw_doc(self.manifest(version))); paths.append(path)
            built=json.loads(channel.build_channel(1,[(p,True,[]) for p in paths]))
        self.assertEqual(["0.9.0","0.10.0"],[x["product_version"] for x in built["releases"]])
    def test_73_p006_artifact_handoff(self):
        payload=b"artifact"; artifact={"id":"linux-x86_64-deb","artifact_type":"deb","filename":"synveil.deb","platform":"linux","architecture":"x86_64","role":"native_package","product_version":"0.1.0","size_bytes":len(payload),"sha256":hashlib.sha256(payload).hexdigest(),"components":["synveil-client"],"package_metadata":{"format":"deb","package_name":"synveil","package_version":"0.1.0","package_architecture":"amd64"}}
        manifest=dict(self.manifest(),artifacts=[artifact]); selected,raw=self.selected_for_manifest(manifest); authenticated=channel.authenticate_selected_manifest(selected,raw)
        chosen=download.select_artifact(authenticated,platform="linux",architecture="x86_64",artifact_type="deb",role="native_package",expected_version="0.1.0")
        self.assertEqual("linux-x86_64-deb",chosen.artifact["id"])
    def test_74_parse_enforces_size_limit(self):
        raw=b"x"*2; auth=channel.ChannelAuthentication("AUTHENTICATED_PINNED_DIGEST","pinned_sha256",hashlib.sha256(raw).hexdigest(),len(raw))
        self.error("CHANNEL_TOO_LARGE",channel.parse_authenticated_channel,raw,auth,channel.ChannelTrustPolicy(max_channel_bytes=1))
    def test_75_invalid_trusted_minimum(self):
        raw=raw_doc(document()); auth=channel.ChannelAuthentication("AUTHENTICATED_PINNED_DIGEST","pinned_sha256",hashlib.sha256(raw).hexdigest(),len(raw))
        self.error("INVALID_RELEASE_POLICY",channel.parse_authenticated_channel,raw,auth,channel.ChannelTrustPolicy(minimum_generation=0))


    def test_90_parse_rejects_pinned_policy_substitution(self):
        raw=raw_doc(document())
        trusted=channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest())
        authentication=channel.authenticate_channel(raw,trusted)
        other=channel.ChannelTrustPolicy(expected_channel_sha256="0"*64)
        self.error("CHANNEL_AUTH_FAILED",channel.parse_authenticated_channel,raw,authentication,other)

    def test_91_parse_rejects_signature_key_policy_substitution(self):
        raw=raw_doc(document())
        authentication=channel.authenticate_channel(raw,self.sig_policy(),descriptor=self.descriptor(raw),verifier=lambda *_:True)
        other=self.sig_policy(frozenset({"other"}))
        self.error("CHANNEL_AUTH_FAILED",channel.parse_authenticated_channel,raw,authentication,other)

    def test_92_parse_rejects_signature_scheme_policy_substitution(self):
        raw=raw_doc(document())
        authentication=channel.authenticate_channel(raw,self.sig_policy(),descriptor=self.descriptor(raw),verifier=lambda *_:True)
        other=self.sig_policy(schemes=frozenset({"rsa"}))
        self.error("CHANNEL_AUTH_FAILED",channel.parse_authenticated_channel,raw,authentication,other)

    def test_93_fresh_selection_enforces_highwater_rollback(self):
        ctx=context(document([release()],generation=1))
        self.error("CHANNEL_ROLLBACK",fresh,ctx,channel.ChannelHighWater(2,"0"*64))

    def test_94_upgrade_selection_enforces_highwater_rollback(self):
        ctx=context(document([release(),release("0.2.0",upgrades=["0.1.0"])],generation=1))
        self.error("CHANNEL_ROLLBACK",upgrade,ctx,"0.1.0",channel.ChannelHighWater(2,"0"*64))

    def test_95_selection_enforces_same_generation_equivocation(self):
        ctx=context(document([release()],generation=2))
        self.error("CHANNEL_EQUIVOCATION",fresh,ctx,channel.ChannelHighWater(2,"0"*64))

    def test_96_invalid_stored_highwater_rejected(self):
        ctx=context(document([release()],generation=2))
        self.error("INVALID_RELEASE_POLICY",fresh,ctx,channel.ChannelHighWater(1,"bad"))

    def test_97_forged_fresh_ineligible_release_rejected(self):
        ctx=context(document([release(fresh=False)]))
        forged=channel.SelectedRelease(ctx,0,ctx.document["releases"][0],"fresh_install")
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)

    def test_98_forged_fresh_with_current_version_rejected(self):
        ctx=context(document([release()]))
        forged=channel.SelectedRelease(ctx,0,ctx.document["releases"][0],"fresh_install","0.0.1")
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)

    def test_99_forged_upgrade_without_current_rejected(self):
        ctx=context(document([release("0.2.0",upgrades=["0.1.0"])]))
        forged=channel.SelectedRelease(ctx,0,ctx.document["releases"][0],"upgrade",None)
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)

    def test_100_forged_upgrade_incompatible_source_rejected(self):
        ctx=context(document([release("0.3.0",upgrades=["0.1.0"])]))
        forged=channel.SelectedRelease(ctx,0,ctx.document["releases"][0],"upgrade","0.2.0")
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)

    def test_101_forged_upgrade_downgrade_rejected(self):
        ctx=context(document([release("0.2.0",upgrades=["0.1.0"])]))
        forged=channel.SelectedRelease(ctx,0,ctx.document["releases"][0],"upgrade","0.3.0")
        self.error("SELECTED_RELEASE_DETACHED",channel.validate_selected_release,forged)


# Explicit numeric-order regressions and every rejected grammar spelling are dedicated cases.
for number,(left,right) in enumerate((("0.9.0","0.10.0"),("1.9.9","1.10.0"),("1.10.9","1.10.10"),("9.0.0","10.0.0")),76):
    setattr(ChannelTests,f"test_{number}_numeric_order",lambda self,l=left,r=right:self.assertLess(channel.parse_stable_version(l),channel.parse_stable_version(r)))
for number,value in enumerate(("0.2","v0.2.0","01.2.3","1.02.3","1.2.03","1.2.3-alpha","1.2.3-beta.1","1.2.3+build","latest","main"),80):
    setattr(ChannelTests,f"test_{number}_reject_version",lambda self,v=value:self.error("INVALID_VERSION",channel.parse_stable_version,v))

if __name__ == "__main__": unittest.main()
