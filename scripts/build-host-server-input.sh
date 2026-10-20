#!/usr/bin/env bash
# Build genuine source bytes and an UNSIGNED input manifest. This does not sign,
# qualify, publish a release, install a helper, or manufacture dependency bytes.
set -euo pipefail
export LC_ALL=C
host_input_repo=$(cd "$(dirname "$0")/.." && pwd -P)
host_input_out=${1:-"$host_input_repo/target/host-server-input"}
[[ $# -le 1 && $(uname -s) == Linux && $(uname -m) == x86_64 ]] || exit 2
[[ -z $(git -C "$host_input_repo" status --porcelain) ]] || { printf 'A clean committed source tree is required for an input identity.\n' >&2; exit 1; }
source "$host_input_repo/deploy/packages/common/reproducible.sh"
synveil_prepare_reproducible_rust_build "$host_input_repo"
host_input_revision=$(git -C "$host_input_repo" rev-parse HEAD)
host_input_os=$(python3 - <<'PY'
from pathlib import Path
values = dict(line.split('=', 1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
pair = (values.get('ID', '').strip('"'), values.get('VERSION_ID', '').strip('"'))
if pair not in [('ubuntu', '24.04'), ('fedora', '42')]:
    raise SystemExit('Build inputs on the declared Ubuntu/Fedora x86_64 target; generic Linux is not qualified')
print('-'.join(pair))
PY
)
host_input_out=$(python3 - "$host_input_out" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1]).absolute()
if '..' in path.parts or path == Path('/'):
    raise SystemExit('unsafe input publication path')
if any(parent.is_symlink() for parent in (path, *path.parents)):
    raise SystemExit('redirected input publication path')
print(path)
PY
)
mkdir -p "$host_input_out"
host_input_stage=$(mktemp -d "$host_input_out/.server-input.XXXXXXXX")
trap 'rm -rf -- "$host_input_stage"' EXIT
mkdir -p "$host_input_stage/payload/bin"
(cd "$host_input_repo" && cargo build --locked --release -p synveil-api -p synveil-server-host --bin synveil-api --bin synveil-worker --bin synveil-server-migrate --bin synveil-host-helper)
for host_input_binary in synveil-api synveil-worker synveil-server-migrate synveil-host-helper; do
    install -m 0755 "$host_input_repo/target/release/$host_input_binary" "$host_input_stage/payload/bin/$host_input_binary"
done
install -m 0644 "$host_input_repo/LICENSE" "$host_input_repo/deploy/server/NOTICE" "$host_input_stage/payload/"
python3 - "$host_input_repo" "$host_input_stage" "$host_input_revision" "$host_input_os" <<'PY'
from pathlib import Path
import hashlib, json, sys, tomllib
repo, package = map(Path, sys.argv[1:3]); stage = package / "payload"; revision, os_target = sys.argv[3:]
sha = lambda data: hashlib.sha256(data).hexdigest()
version = tomllib.loads((repo / 'Cargo.toml').read_text())['workspace']['package']['version']
provenance = {'schema_version': 1, 'component': 'server', 'version': version,
              'source_revision': revision,
              'source_url': 'https://github.com/nghianguyen150612/Synveil/tree/' + revision,
              'platform': os_target, 'architecture': 'x86_64'}
(stage / 'PROVENANCE').write_text(json.dumps(provenance, sort_keys=True, indent=2) + '\n')
files = [{'path': path.relative_to(stage).as_posix(), 'sha256': sha(path.read_bytes()),
          'bytes': path.stat().st_size, 'executable': bool(path.stat().st_mode & 0o111)}
         for path in sorted(stage.rglob('*')) if path.is_file()]
manifest = {'schema_version': 2, 'kind': 'server', 'version': version, 'packaging_revision': revision,
            'platform': 'linux', 'architecture': 'x86_64', 'qualified_os': os_target,
            'compatibility': 'synveil-desktop-server-v0.2', 'usage': 'managed_host_production_input',
            'upstream_url': provenance['source_url'], 'upstream_revision': revision,
            'provenance_sha256': sha((stage / 'PROVENANCE').read_bytes()),
            'license': {'spdx_id': 'MIT', 'license_file': 'LICENSE',
                        'license_sha256': sha((stage / 'LICENSE').read_bytes()),
                        'notice_file': 'NOTICE',
                        'notice_sha256': sha((stage / 'NOTICE').read_bytes())},
            'files': files}
# The signature-verifying producer expects the manifest outside the closed input
# directory. An unsigned build is never registered as a trusted root catalog.
(package / 'input-manifest.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
PY
host_input_destination="$host_input_out/server-$host_input_revision"
[[ ! -e "$host_input_destination" && ! -L "$host_input_destination" ]] || { printf 'Existing input preserved; use a fresh output root.\n' >&2; exit 1; }
mv --no-target-directory "$host_input_stage" "$host_input_destination"
printf 'Unsigned source input: %s\n' "$host_input_destination"
printf 'Supply an independently approved signature and pinned PostgreSQL/Caddy inputs before catalog assembly.\n'
