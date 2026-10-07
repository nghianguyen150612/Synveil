#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

python3 - <<'PY'
import json, pathlib, re, sys

root = pathlib.Path('.')
contract = json.loads((root/'deploy/linux/package-integration-v1.json').read_text())
assert contract['schema_version'] == 1
assert contract['product_version'] == '0.1.0'
assert {x['artifact_type'] for x in contract['formats']} == {'deb', 'rpm'}

manifest = (root/'deploy/install/MANIFEST').read_text()
manifest_ownership = {}
for line in manifest.splitlines():
    fields = line.split()
    if not fields or line.lstrip().startswith('#'):
        continue
    if len(fields) != 6:
        raise SystemExit(f"invalid MANIFEST record: {line}")
    manifest_ownership[fields[1]] = fields[5]
contract_paths = {surface['path'] for surface in contract['surfaces']}
manifest_package_paths = {
    path for path, lifecycle in manifest_ownership.items()
    if lifecycle in {'PACKAGE', 'TEMPLATE'}
}
if contract_paths != manifest_package_paths:
    missing = sorted(manifest_package_paths - contract_paths)
    extra = sorted(contract_paths - manifest_package_paths)
    raise SystemExit(f"package surface set drift: missing={missing}, extra={extra}")
for surface in contract['surfaces']:
    manifest_class = manifest_ownership.get(surface['path'])
    expected_owner = {'PACKAGE': 'PACKAGE_OWNED', 'TEMPLATE': 'PACKAGE_OWNED'}.get(manifest_class)
    if expected_owner is None or surface['owner'] != expected_owner:
        raise SystemExit(f"ownership mismatch for {surface['path']}: MANIFEST={manifest_class}, contract={surface['owner']}")

build = (root/'deploy/packages/build.sh').read_text()
deb_match = re.search(r'^DEB_DEPENDS="([^"]+)"$', build, re.MULTILINE)
if not deb_match:
    raise SystemExit('DEB_DEPENDS source not found')
deb_dependencies = deb_match.group(1).split(', ')
rpm_source = (root/'deploy/packages/rpm/synveil.spec.tmpl').read_text()
rpm_match = re.search(r'^Requires:\s+(.+)$', rpm_source, re.MULTILINE)
if not rpm_match:
    raise SystemExit('RPM Requires source not found')
rpm_dependencies = rpm_match.group(1).split(', ')
formats = {item['format']: item for item in contract['formats']}
if formats['DEB']['runtime_dependencies'] != deb_dependencies:
    raise SystemExit('DEB dependency drift between build.sh and package integration contract')
if formats['RPM']['runtime_dependencies'] != rpm_dependencies:
    raise SystemExit('RPM dependency drift between spec and package integration contract')

arch = (root/'deploy/packages/common/arch.sh').read_text()
if formats['DEB']['architectures'] != ['amd64', 'arm64'] or not all(x in arch for x in ('amd64', 'arm64')):
    raise SystemExit('DEB architecture contract drift')
if formats['RPM']['architectures'] != ['x86_64', 'aarch64'] or not all(x in arch for x in ('x86_64', 'aarch64')):
    raise SystemExit('RPM architecture contract drift')
if formats['DEB']['hooks'] != ['preinst', 'postinst', 'prerm', 'postrm']:
    raise SystemExit('DEB hook contract drift')
if formats['RPM']['hooks'] != ['pre', 'post', 'preun', 'postun']:
    raise SystemExit('RPM hook contract drift')

files = [root/'deploy/packages/debian/postinst', root/'deploy/packages/debian/prerm',
         root/'deploy/packages/debian/postrm', root/'deploy/packages/rpm/synveil.spec.tmpl']
for path in files:
    text = path.read_text()
    executable = '\n'.join(line for line in text.splitlines()
                           if line.strip() and not line.lstrip().startswith('#'))
    forbidden = {
      'network download': r'(^|[;&|]\s*)(curl|wget)\s',
      'recursive deletion': r'\brm\s+(?:-[A-Za-z]*r[A-Za-z]*f?|-[A-Za-z]*f[A-Za-z]*r)\b',
      'user-home mutation': r'(?<![A-Za-z_])(\$HOME|\$USER|\$LOGNAME|/home/)',
      'shell command interpolation': r'\b(sh|bash)\s+-c\b',
      'automatic enablement': r'(^|[;&|]\s*)\s*systemctl\s+(?:--user\s+)?enable\b',
    }
    for label, pattern in forbidden.items():
        if re.search(pattern, executable, re.MULTILINE):
            raise SystemExit(f"{path}: forbidden {label}")

desktop = (root/'deploy/applications/synveil.desktop').read_text()
assert 'Exec=/usr/bin/synveil-desktop' in desktop
assert 'Icon=synveil' in desktop
assert '/usr/lib/systemd/system/synveil-client.service' not in manifest

deb = (root/'deploy/packages/debian/control.tmpl').read_text()
rpm = (root/'deploy/packages/rpm/synveil.spec.tmpl').read_text()
assert 'Package: synveil' in deb and 'Name:           synveil' in rpm
assert 'NOT auto-enabled' in deb and 'NOT auto-enabled' in rpm
print('linux package contract/static hook audit: PASS')
PY

cargo test -p synveil-install-engine --test linux_package_integration --locked
cargo test -p synveil-metadata \
  --test linux_native_packaging_units \
  --locked \
  -- --nocapture
