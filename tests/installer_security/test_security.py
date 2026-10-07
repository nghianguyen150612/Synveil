"""P043 adversarial cases supplement P005/P006/P011/P017/P027 suites, offline."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts'))
import release_signature as signatures
import release_download as download
import release_manifest as manifest
import release_channel as channel
if os.name == 'posix':
    import linux_quick_install as quick
import windows_lifecycle as windows
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


class SignatureSecurity(unittest.TestCase):
    def setUp(self):
        # Generated only in memory. No committed private material or CI trust root.
        self.private = {key: Ed25519PrivateKey.generate() for key in ('old', 'new', 'test')}
        self.keys = {key: value.public_key().public_bytes_raw() for key, value in self.private.items()}
        self.raw = b'exact authenticated metadata\n'
        self.detached = {key + '.sig': value.sign(self.raw) for key, value in self.private.items()}
        self.entries = [{'scheme': 'ed25519', 'key_id': key, 'signature_filename': key + '.sig'} for key in ('old', 'new')]

    def authenticate(self, keys, *, entries=None, detached=None, raw=None, pin=None):
        policy = download.ReleaseTrustPolicy(frozenset(), frozenset(), authentication_method='detached_signature',
            expected_manifest_sha256=pin, trusted_key_ids=frozenset(keys), allowed_signature_schemes=frozenset({'ed25519'}))
        verify = signatures.verifier(keys, self.detached if detached is None else detached)
        return download.authenticate_manifest(self.raw if raw is None else raw, policy,
            signature_descriptor={'signatures': self.entries if entries is None else entries}, verifier=verify)

    def test_rotation_real_old_new_overlap_and_reversed_order(self):
        for names, expected in ((('old',), 'old'), (('new',), 'new'), (('old', 'new'), 'new')):
            keys = {name: self.keys[name] for name in names}
            for entries in (self.entries, list(reversed(self.entries))):
                self.assertEqual(expected, self.authenticate(keys, entries=entries).key_id)

    def test_removed_key_unknown_key_and_scheme_fail(self):
        for entry in (dict(self.entries[0], key_id='retired'), dict(self.entries[0], scheme='rsa')):
            with self.assertRaises(download.AcquisitionError):
                self.authenticate({'new': self.keys['new']}, entries=[entry])

    def test_one_valid_among_invalid_and_all_invalid(self):
        keys = {k: self.keys[k] for k in ('old', 'new')}
        detached = dict(self.detached, **{'new.sig': b'x' * 64})
        self.assertEqual('old', self.authenticate(keys, detached=detached).key_id)
        detached['old.sig'] = b'x' * 64
        with self.assertRaises(download.AcquisitionError): self.authenticate(keys, detached=detached)

    def test_modified_bytes_no_pin_fallback(self):
        with self.assertRaises(download.AcquisitionError) as error:
            self.authenticate({'old': self.keys['old']}, raw=self.raw+b'evil', pin=hashlib.sha256(self.raw+b'evil').hexdigest())
        self.assertEqual('MANIFEST_AUTH_FAILED', error.exception.code)

    def test_signature_size_bounds_and_key_bounds(self):
        for value in (b'', b'x'*63, b'x'*65, b'x'*4096, 'not bytes'):
            with self.subTest(size=len(value)), self.assertRaises(download.AcquisitionError):
                self.authenticate({'old': self.keys['old']}, detached={'old.sig': value})
        with self.assertRaises(signatures.SignaturePolicyError): signatures.verifier({'old': b'bad'}, {})

    def test_backend_unavailable_is_closed(self):
        with mock.patch.dict(sys.modules, {'cryptography.hazmat.primitives.asymmetric.ed25519': None}):
            with self.assertRaises(signatures.SignaturePolicyError): signatures.verifier(self.keys, self.detached)

    def test_no_keys_from_ci_filename_or_descriptor(self):
        entry = {'scheme':'ed25519', 'key_id':'synveil-test-key', 'signature_filename':'production.sig'}
        with mock.patch.dict(os.environ, {'CI':'true', 'SYNVEIL_TRUST_ANY_KEY':'1'}), self.assertRaises(download.AcquisitionError):
            download.authenticate_manifest(self.raw, download.ReleaseTrustPolicy(frozenset(), frozenset(),
                authentication_method='detached_signature', allowed_signature_schemes=frozenset({'ed25519'})),
                signature_descriptor={'signatures':[entry]})

    def test_explicit_local_production_policy_and_substitution(self):
        key = self.keys['old']; key_id = 'synveil-release-' + hashlib.sha256(key).hexdigest()
        data = {'schema_version':1, 'purpose':'production-release-verification', 'keys':[
            {'key_id':key_id, 'scheme':'ed25519', 'public_key_hex':key.hex()}]}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'production.json'
            path.write_text(json.dumps(data))
            self.assertEqual({key_id:key}, signatures.load_production_keys(path))
            for field, value in (('purpose','test'), ('schema_version',2)):
                bad = dict(data, **{field:value}); path.write_text(json.dumps(bad))
                with self.assertRaises(signatures.SignaturePolicyError): signatures.load_production_keys(path)
            for key_id in ('synveil-test-key','production', 'synveil-release-'+'0'*64):
                data['keys'][0]['key_id'] = key_id; path.write_text(json.dumps(data))
                with self.assertRaises(signatures.SignaturePolicyError): signatures.load_production_keys(path)

    def test_descriptor_is_bounded_and_no_path_traversal(self):
        for entries in (self.entries*9, [dict(self.entries[0], signature_filename='../evil')], [{'key_id':[]}], 'not-array'):
            with self.assertRaises(download.AcquisitionError): self.authenticate(self.keys, entries=entries)

    def test_real_signature_channel_binding(self):
        document = {'schema_version':1,'product':'Synveil','channel':'stable','generation':42,'releases':[]}
        raw = json.dumps(document).encode()
        descriptor = {'schema_version':1, 'channel_filename':channel.CHANNEL_FILENAME,
            'channel_size_bytes':len(raw), 'channel_sha256':hashlib.sha256(raw).hexdigest(), 'signatures':self.entries}
        policy = channel.ChannelTrustPolicy(authentication_method='detached_signature', trusted_key_ids=frozenset({'old'}), allowed_signature_schemes=frozenset({'ed25519'}))
        verify = signatures.verifier(self.keys, {'old.sig':self.private['old'].sign(raw)})
        auth = channel.authenticate_channel(raw, policy, descriptor=descriptor, verifier=verify)
        self.assertEqual(42, channel.parse_authenticated_channel(raw, auth, policy).document['generation'])
        with self.assertRaises(channel.ChannelError): channel.authenticate_channel(raw+b' ', policy, descriptor=descriptor, verifier=verify)


class PathAndCommandSecurity(unittest.TestCase):
    def test_portable_basename_property_cases(self):
        for name in ('../evil','/absolute','C:\\absolute','C:evil','\\\\server\\share','nested/path','nested\\path',
                     '.', '..', '', 'a\x00b', 'a\nb', 'a\tb', 'name ', ' name', 'name.', 'NUL.exe', 'COM1',
                     'con.txt', 'a\x85b', '-option.deb', 'a'*256, 'a:stream', 'a//b', './name', 'name/'):
            with self.subTest(name=repr(name)): self.assertFalse(manifest.safe_filename(name))
        for name in ('Synveil release ☃.AppImage','synveil.deb','SynveilSetup.exe','a..b'):
            self.assertTrue(manifest.safe_filename(name))

    def test_url_parser_cannot_normalize_controls_or_userinfo(self):
        for url in (' https://good.example/x', 'https://good.example/\nx','https://user:token@good.example/x',
                    'http://good.example/x', 'https://good.example\\@evil.example/x'):
            with self.assertRaises(download.AcquisitionError): download.require_allowed_url(url, frozenset({'https://good.example'}))

    def test_windows_relative_component_property_and_ancestor_reparse(self):
        for name in ('..\\evil', 'x/../y', '.', 'x/./y', 'x//y', '\\rooted', 'C:relative',
                     'NUL.dll', 'x/\x85secret', 'x/COM1.txt', 'x/ space', 'x/trailing.', 'x/a:stream', 'x/\x00bad', 'x/\nsecret'):
            with self.subTest(name=repr(name)), self.assertRaises(ValueError): windows.obsolete_owned({name}, set())
        with self.assertRaises(ValueError): windows.obsolete_owned({'qml/Module/file.dll'}, set(), {'qml'})
        self.assertEqual({'qml\\Module\\file.dll'}, windows.obsolete_owned({'qml/Module/file.dll'}, set()))
        self.assertNotIn('user-note.txt', windows.obsolete_owned({'old.dll'}, set()))

    def test_strict_versions_and_upgrade_compatibility(self):
        for version in ('1','01.2.3','1.2.3-beta','1.2.3+build','1.2.3.4','١.2.3',' 1.2.3','4294967296.0.0','9'*10000):
            with self.subTest(version=version[:30]), self.assertRaises(ValueError): windows.StableVersion.parse(version)
            with self.assertRaises(channel.ChannelError): channel.parse_stable_version(version)
        with self.assertRaises(ValueError): windows.lifecycle('1.0.0','1.1.0',False)
        with self.assertRaises(ValueError): windows.lifecycle('1.1.0','1.0.0',False, compatible_sources=frozenset({'1.1.0'}))
        self.assertEqual('repair',windows.lifecycle('1.1.0','1.1.0',True))

    @unittest.skipUnless(os.name == 'posix', 'Linux native authority')
    def test_privileged_argv_environment_and_path_substitution(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory)/'apt-get'; fake.write_text('evil'); fake.chmod(0o755)
            with mock.patch.dict(os.environ, {'PATH':directory,'LD_PRELOAD':'secret','PYTHONPATH':'evil','HOME':'arbitrary','https_proxy':'https://user:secret@evil'}):
                self.assertNotEqual(str(fake), quick.system_executable('apt-get'))
                with mock.patch.object(quick.subprocess, 'run', return_value=subprocess.CompletedProcess([],0,'','')) as run, mock.patch.object(quick.os,'geteuid',return_value=1000), mock.patch.object(quick,'system_executable', side_effect=lambda n:'/usr/bin/'+n):
                    quick.NativeManager(quick.PROFILES['debian-x86_64'])._run(['apt-get','install','-y','--','/private/name;$(evil).deb'], mutate=True)
                command = run.call_args.args[0]
                self.assertEqual(['/usr/bin/sudo','--','/usr/bin/apt-get','install','-y','--','/private/name;$(evil).deb'],command)
                self.assertEqual({'PATH':'/usr/sbin:/usr/bin:/sbin:/bin','LANG':'C','LC_ALL':'C'},run.call_args.kwargs['env'])
                self.assertNotIn('shell',run.call_args.kwargs)

    @unittest.skipUnless(os.name == 'posix', 'Linux last-use guard')
    def test_last_use_replacement_and_symlink_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'synveil.deb'; path.write_bytes(b'verified')
            evidence = {'final_path':str(path),'artifact_size':8,'artifact_sha256':hashlib.sha256(b'verified').hexdigest()}
            quick.require_staged_evidence(evidence)
            path.write_bytes(b'replaced')
            with self.assertRaises(quick.QuickInstallError): quick.require_staged_evidence(evidence)
            path.unlink(); path.symlink_to(Path(directory)/'outside')
            with self.assertRaises(quick.QuickInstallError): quick.require_staged_evidence(evidence)
            native = quick.NativeManager(quick.PROFILES['debian-x86_64'])
            with self.assertRaises(quick.QuickInstallError): native.install(Path('-option.deb'),'0.1.0')

    @unittest.skipUnless(os.name == 'posix', 'Linux staged paths')
    def test_symlink_ancestor_and_shared_stage_rejected_before_write(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); outside=root/'outside'; outside.mkdir(); (root/'link').symlink_to(outside, target_is_directory=True)
            with self.assertRaises(download.AcquisitionError): download._safe_target(root/'link'/'nested', 'safe.deb')
            self.assertFalse((outside/'nested').exists())
            shared=root/'shared'; shared.mkdir(mode=0o777); shared.chmod(0o777)
            with self.assertRaises(download.AcquisitionError): download._safe_target(shared,'safe.deb')

    @unittest.skipUnless(os.name == 'posix', 'Linux durable high-water')
    def test_high_water_persistence_rollback_equivocation_unknown_schema(self):
        def context(generation, extra=False):
            data={'schema_version':1,'product':'Synveil','channel':'stable','generation':generation,'releases':[]}
            raw=json.dumps(data,indent=2 if extra else None).encode(); policy=channel.ChannelTrustPolicy(expected_channel_sha256=hashlib.sha256(raw).hexdigest())
            return channel.parse_authenticated_channel(raw,channel.authenticate_channel(raw,policy),policy)
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/'owned'
            quick.persist_channel_high_water(root,context(42))
            state=root/'channel-stable.json'; original=state.read_bytes()
            quick.persist_channel_high_water(root,context(42))
            for incoming, code in ((context(41),'CHANNEL_ROLLBACK'), (context(42,True),'CHANNEL_EQUIVOCATION')):
                with self.assertRaises(channel.ChannelError) as error: quick.persist_channel_high_water(root,incoming)
                self.assertEqual(code,error.exception.code); self.assertEqual(original,state.read_bytes())
            state.write_text('{"schema_version":2}')
            with self.assertRaises(channel.ChannelError): quick.persist_channel_high_water(root,context(43))
            self.assertEqual('{"schema_version":2}',state.read_text())

    def test_deb_preunpack_gate_has_zero_mutation(self):
        if os.name != 'posix': self.skipTest('POSIX hook')
        source=(ROOT/'deploy/packages/debian/preinst').read_text().replace('@SYNVEIL_DEB_VERSION@','0.1.0-1')
        with tempfile.TemporaryDirectory() as directory:
            hook=Path(directory)/'preinst'; hook.write_text(source)
            for old, status in (('',0),('0.1.0-1',0),('0.2.0-1',1),('0.0.9-1',1),('unknown',1),('$(secret)',1)):
                result=subprocess.run(['/bin/sh',str(hook),'upgrade',old],capture_output=True,text=True)
                self.assertEqual(status,result.returncode); self.assertNotIn(old or 'secret', result.stderr) if old else None
            self.assertEqual([hook],list(Path(directory).iterdir()))

class ManifestPropertySecurity(unittest.TestCase):
    def test_malformed_fields_are_typed_failures(self):
        item={'id':'linux-x86_64-deb','artifact_type':'deb','filename':'safe.deb','platform':'linux',
            'architecture':'x86_64','role':'native_package','product_version':'0.1.0','size_bytes':1,
            'sha256':'a'*64,'components':['synveil-client'], 'package_metadata':{'format':'deb',
            'package_name':'synveil','package_version':'0.1.0','package_architecture':'amd64'}}
        for field in ('artifact_type','platform','architecture','role','components','sha256','filename','size_bytes'):
            for malformed in (None, {}, [], [['nested']], True):
                if field=='components' and malformed==[]: continue
                with self.subTest(field=field,value=repr(malformed)), self.assertRaises(manifest.ManifestError):
                    manifest.validate_artifact(dict(item, **{field:malformed}), '0.1.0', None)

    def test_bootstrap_bundle_is_closed_deterministic_and_no_clobber(self):
        import importlib.util
        import tarfile
        spec=importlib.util.spec_from_file_location('bundle',ROOT/'scripts/build-quick-install-bundle.py')
        bundle=importlib.util.module_from_spec(spec); spec.loader.exec_module(bundle)
        with tempfile.TemporaryDirectory() as directory:
            one=Path(directory)/'one.tar.gz'; two=Path(directory)/'two.tar.gz'
            self.assertEqual(bundle.build(one),bundle.build(two))
            with tarfile.open(one) as archive:
                self.assertEqual(list(bundle.FILES), archive.getnames())
                self.assertTrue(all(member.isfile() and member.uid==0 and member.mtime==0 for member in archive.getmembers()))
            with self.assertRaises(FileExistsError): bundle.build(one)

if __name__ == '__main__': unittest.main()
