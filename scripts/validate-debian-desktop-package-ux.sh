#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACT="$ROOT/deploy/linux/debian-desktop-ux-v1.json"
DEB="${SYNVEIL_DEB_ARTIFACT:-${1:-}}"

python3 - "$ROOT" "$CONTRACT" <<'PY'
import json, pathlib, re, sys
root, path = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
d = json.loads(path.read_text())
expected = {
 "schema_version": 1, "package_format": "DEB", "product": "Synveil",
 "supported_product_architecture": "x86_64", "native_package_architecture": "amd64",
 "package_name": "synveil", "desktop_entry_path": "/usr/share/applications/synveil.desktop",
 "desktop_exec": "/usr/bin/synveil-desktop", "desktop_icon": "synveil", "terminal": False,
 "native_dependency_resolution": True, "interactive_maintainer_scripts": False,
 "auto_launch_after_install": False, "auto_start_after_install": False,
}
if d != expected: raise SystemExit("closed Prompt013 contract mismatch (unknown, missing, or invalid field)")
desktop = (root/'deploy/applications/synveil.desktop').read_text().splitlines()
for line in ('[Desktop Entry]','Type=Application','Name=Synveil','Exec='+d['desktop_exec'],
             'Icon='+d['desktop_icon'],'Terminal=false','Categories=Utility;Security;FileTransfer;'):
    if line not in desktop: raise SystemExit('desktop entry mismatch: '+line)
text='\n'.join(desktop)
for bad in ('sudo','pkexec','sh -c','bash -c','NoDisplay=true','Hidden=true','OnlyShowIn='):
    if bad in text: raise SystemExit('unsafe/hidden desktop entry token: '+bad)
control=(root/'deploy/packages/debian/control.tmpl').read_text()
if not control.startswith('Package: '+d['package_name']+'\n'): raise SystemExit('control package drift')
build=(root/'deploy/packages/build.sh').read_text()
if 'DEB_DEPENDS=' not in build or 'amd64' not in (root/'deploy/packages/common/arch.sh').read_text():
    raise SystemExit('DEB build/dependency architecture drift')
manifest=(root/'deploy/install/MANIFEST').read_text()
for p in (d['desktop_entry_path'], d['desktop_exec'], '/usr/share/icons/hicolor/scalable/apps/synveil.svg'):
    if p not in manifest: raise SystemExit('manifest lacks '+p)
integration=json.loads((root/'deploy/linux/package-integration-v1.json').read_text())
deb=next(x for x in integration['formats'] if x['format']=='DEB')
if deb['package_identity'] != d['package_name'] or d['native_package_architecture'] not in deb['architectures']:
    raise SystemExit('P012 integration drift')
if integration['autostart'] is not False: raise SystemExit('P012 autostart drift')
for hook in ('postinst','prerm','postrm'):
    h=(root/'deploy/packages/debian'/hook).read_text().lower()
    for bad in ('synveil-desktop','systemctl --user','enable-linger','curl ','wget ','zenity','kdialog','whiptail','dialog '):
        if bad in h: raise SystemExit(f'{hook} contains forbidden token {bad}')
print('Prompt013 static contract: PASS')
PY

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$ROOT/deploy/applications/synveil.desktop"
  echo 'desktop-file-validate: PASS'
else
  echo 'SKIPPED — desktop-file-validate unavailable'
fi

if [[ -z "$DEB" ]]; then
  echo 'SKIPPED — artifact absent'
  exit 0
fi
[[ -f "$DEB" ]] || { echo "DEB artifact not found: $DEB" >&2; exit 1; }
expected_depends="$(sed -n 's/^DEB_DEPENDS="\(.*\)"/\1/p' "$ROOT/deploy/packages/build.sh")"
[[ "$(dpkg-deb --field "$DEB" Package)" == synveil ]]
[[ "$(dpkg-deb --field "$DEB" Version)" == 0.1.0 ]]
[[ "$(dpkg-deb --field "$DEB" Architecture)" == amd64 ]]
[[ "$(dpkg-deb --field "$DEB" Depends)" == "$expected_depends" ]]
dpkg-deb --info "$DEB" >/dev/null
contents="$(dpkg-deb --contents "$DEB")"
for path in usr/bin/synveil-client usr/bin/synveil-desktop usr/bin/synveil-scheduled-maintenance-once usr/lib/systemd/user/synveil-client.service usr/share/applications/synveil.desktop usr/share/icons/hicolor/scalable/apps/synveil.svg usr/share/doc/synveil/LICENSE usr/share/doc/synveil/NOTICE; do
  grep -Eq "^-[rwx-]{9}[[:space:]]+root/root[[:space:]]+.*\\./$path$" <<<"$contents" || { echo "missing/non-root-owned $path" >&2; exit 1; }
done
! grep -q './usr/lib/systemd/system/synveil-client.service' <<<"$contents"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
dpkg-deb --extract "$DEB" "$tmp"
cmp "$ROOT/deploy/applications/synveil.desktop" "$tmp/usr/share/applications/synveil.desktop"
[[ -x "$tmp/usr/bin/synveil-desktop" && ! -x "$tmp/usr/share/applications/synveil.desktop" && ! -x "$tmp/usr/share/icons/hicolor/scalable/apps/synveil.svg" ]]
if command -v desktop-file-validate >/dev/null 2>&1; then desktop-file-validate "$tmp/usr/share/applications/synveil.desktop"; fi
echo 'Prompt013 actual DEB artifact: PASS'
