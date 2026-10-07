#!/usr/bin/env python3
"""P043 structural regression gate, using AST boundaries and executable suites."""
from __future__ import annotations
import ast
from pathlib import Path
import re
import sys

ROOT=Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    if not condition: raise AssertionError(message)


def read(name: str) -> str:
    return (ROOT/name).read_text(encoding='utf-8')


def function(source: str, name: str) -> ast.FunctionDef:
    matches=[node for node in ast.walk(ast.parse(source)) if isinstance(node, ast.FunctionDef) and node.name==name]
    require(len(matches)==1, 'missing/ambiguous function: '+name)
    return matches[0]


def calls(node) -> list[str]:
    return [ast.unparse(item.func) for item in ast.walk(node) if isinstance(item, ast.Call)]


def main() -> int:
    production=('scripts/release_download.py','scripts/release_manifest.py','scripts/release_channel.py',
        'scripts/release_signature.py','scripts/linux_quick_install.py','deploy/install/quick-install.sh',
        'deploy/windows/installer/Synveil.iss','crates/install-engine/src/appimage.rs',
        'crates/install-engine/src/lifecycle.rs','crates/install-engine/src/journal.rs')
    forbidden=re.compile(r'--(?:insecure|skip-verify|ignore-signature|ignore-hash|allow-http|force-downgrade|trust-any-key|disable-tls|force-install-unknown|ignore-ownership)\b')
    for path in production:
        source=read(path)
        # Ignore comments/docstrings for executable Python; command-line literals
        # and callsites remain checked, so prose prohibitions do not create green.
        if path.endswith('.py'):
            tree=ast.parse(source)
            strings='\n'.join(n.value for n in ast.walk(tree) if isinstance(n,ast.Constant) and isinstance(n.value,str))
            require(not forbidden.search(strings), 'production bypass string: '+path)
            for node in ast.walk(tree):
                if isinstance(node,ast.Call):
                    name=ast.unparse(node.func)
                    require(name not in {'eval','exec','os.system','os.popen'}, 'dynamic execution: '+path)
                    if name.startswith('subprocess.'):
                        require(not any(k.arg=='shell' and not (isinstance(k.value,ast.Constant) and k.value.value is False) for k in node.keywords), 'shell execution: '+path)
        else:
            executable='\n'.join(line for line in source.splitlines() if not line.lstrip().startswith(('#','//')))
            require(not forbidden.search(executable), 'production bypass flag: '+path)
            require(not re.search(r'\b(?:curl|wget)\b[^\n]*\|\s*(?:sh|bash)\b', executable), 'remote shell pipeline: '+path)
            require(not re.search(r'\b(?:pkill|killall|taskkill)\b|Command::new\("(?:sh|bash|cmd|powershell)"\)', executable), 'unowned process/shell authority: '+path)
    acquisition=read('scripts/release_download.py')
    selected=function(acquisition,'select_artifact')
    require(isinstance(selected.body[0], ast.Assign) and ast.unparse(selected.body[0].value)=='_validate_context(context)', 'selection must begin with authenticated context validation')
    stage=function(acquisition,'stage_artifact')
    require('_validate_selection' in calls(stage) and 'os.link' in calls(stage) and 'os.fsync' in calls(stage), 'authenticated no-clobber staging lost')
    require('src_dir_fd=directory_fd' in acquisition and 'O_NOFOLLOW' in acquisition and 'st_file_attributes' in acquisition, 'staging ancestry/identity guards lost')
    quick=read('scripts/linux_quick_install.py')
    native=function(quick,'_run')
    require('system_executable' in calls(native), 'native executable resolution lost')
    run=[n for n in ast.walk(native) if isinstance(n,ast.Call) and ast.unparse(n.func)=='subprocess.run']
    require(len(run)==1 and any(k.arg=='env' and isinstance(k.value,ast.Name) and k.value.id=='environment' for k in run[0].keywords), 'scoped native environment lost')
    install=function(quick,'install')
    require('require_staged_evidence' in calls(install) and any(isinstance(n,ast.Constant) and n.value=='--' for n in ast.walk(install)), 'native last-use/option boundary lost')
    require('persist_channel_high_water' in calls(function(quick,'accept_channel_high_water')), 'production high-water handoff lost')
    channel=read('scripts/release_channel.py')
    high=function(channel,'evaluate_high_water')
    require(all(code in ast.unparse(high) for code in ('CHANNEL_ROLLBACK','CHANNEL_EQUIVOCATION')), 'P011 anti-rollback checks lost')
    signature=read('scripts/release_signature.py')
    require('Ed25519PublicKey.from_public_bytes' in signature and '.verify(signature, raw)' in signature, 'reviewed exact-byte backend lost')
    require('os.environ' not in signature and 'synveil-release-' in signature and 'hashlib.sha256(public_key)' in signature, 'remote/environment trust root admission')
    lifecycle=read('crates/install-engine/src/lifecycle.rs')
    require(lifecycle.index('if target < source') < lifecycle.index('match upgrade.compatibility.disposition'), 'caller compatibility can bypass downgrade comparison')
    require('if target == source' in lifecycle and 'ActiveTransaction' in lifecycle and 'NewerThanSupported' in lifecycle, 'same-version/state/reconciliation guards lost')
    journal=read('crates/install-engine/src/journal.rs')
    require('JournalUnsupportedSchema' in journal and 'meta.file_attributes() & 0x400' in journal and 'path.ancestors()' in journal, 'journal schema/ancestry guard lost')
    require(not re.search(r'remove_dir_all|\.clear\(\)', journal), 'destructive journal reset')
    app=read('crates/install-engine/src/appimage.rs')
    require('let previous = self.existing_record()?' in app and 'reject_symlink_ancestors(path)?' in app, 'AppImage ownership validation lost')
    launch=read('crates/client/src/launch.rs')
    require('GetSystemDirectoryW' in launch and 'SHGetKnownFolderPath' in launch and 'share_mode(1)' in launch,
            'Windows native tool/private locked XML boundary lost')
    scheduler=launch.split('fn schtasks_path()')[1].split('fn schtasks_command')[0]
    require('var_os("SystemRoot")' not in scheduler and 'std::env::temp_dir()' not in scheduler,
            'Windows environment substitution reintroduced')
    iss=read('deploy/windows/installer/Synveil.iss')
    for token in ('PrivilegesRequired=lowest','UsePreviousAppDir=no','ValidateSecurityOptions','RequireInstallRoot','RequireNoReparseAncestry','IsExplicitUpgradeSource','RequireOwnedExecutable','PrepareToInstall'):
        require(token in iss,'Windows security contract lost: '+token)
    require('Pos(Uppercase(Root), Uppercase(Candidate))' not in iss and 'DelTree(' not in iss, 'Windows cleanup containment/regressive deletion')
    for hook in ('deploy/packages/debian/postinst','deploy/packages/debian/prerm','deploy/packages/debian/postrm','deploy/packages/rpm/synveil.spec.tmpl'):
        source=read(hook)
        executable='\n'.join(line for line in source.splitlines() if not line.lstrip().startswith('#'))
        require('PATH=/usr/sbin:/usr/bin:/sbin:/bin' in executable, 'privileged hook PATH policy missing')
        require(not re.search(r'\$HOME|/home/|client\.db|systemctl\s+--user\s+(?:start|enable)|rm\s+.*lock', executable), 'privileged user-state/lock mutation')
    for name in ('INSTALLER_SECURITY_HARDENING.md','PROMPT043_MANIFEST.md'):
        require((ROOT/'docs/v0.2'/name).is_file(), 'P043 documentation absent')
    require((ROOT/'docs/v0.2/PROMPT044_MANIFEST.md').is_file(), 'P044 evidence manifest absent')
    require((ROOT/'docs/v0.2/INSTALLATION_RESILIENCE_HARDENING.md').is_file(), 'P044 resilience evidence absent')
    require((ROOT/'scripts/validate-installation-resilience-hardening.py').is_file(), 'P044 static validator absent')
    roadmap=read('docs/v0.2/ROADMAP.md')
    phase=roadmap.split('### P044 —')[1].split('## Release acceptance')[0]
    require('**Implemented' in phase,'P044 source/evidence status missing')
    p045=roadmap.split('### P045 —')[1].split('### P046 —')[0]
    require('**Implemented' not in p045,'P045 must remain deferred')
    print('installer security hardening: PASS')
    return 0


if __name__=='__main__':
    try: sys.exit(main())
    except (AssertionError,OSError,SyntaxError) as error:
        print('installer security hardening: FAIL: '+str(error),file=sys.stderr)
        sys.exit(1)
