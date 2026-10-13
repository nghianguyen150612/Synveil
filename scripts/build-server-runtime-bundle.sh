#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-"$root/target/server-runtime"}
[[ $# -le 1 ]] || { printf 'Usage: build-server-runtime-bundle.sh [OUTPUT_DIRECTORY]\n' >&2; exit 2; }
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || {
  printf '[synveil-server-bundle] Linux x86_64 build host required\n' >&2; exit 1;
}
# Resolve relative output paths from the caller, reject redirected ancestors,
# and never permit a source-tree or filesystem-root publication target.
out=$(python3 - "$out" "$root" <<'PY'
import pathlib, sys
output = pathlib.Path(sys.argv[1]).absolute()
source = pathlib.Path(sys.argv[2])
if any(part == '..' for part in output.parts):
    raise SystemExit('output path contains traversal')
if output == pathlib.Path('/') or output == source or output in source.parents:
    raise SystemExit('output path overlaps the source or filesystem root')
if source in output.parents and source / 'target' not in (output, *output.parents):
    raise SystemExit('output inside the repository must be below target/')
for candidate in (output, *output.parents):
    if candidate.is_symlink():
        raise SystemExit('output path contains a symlink')
    if candidate.exists() and not candidate.is_dir():
        raise SystemExit('output ancestor is not a directory')
print(output)
PY
)
revision=$(git -C "$root" rev-parse HEAD)
runtime_id="synveil-${revision}"
stage="$out/$runtime_id"
mkdir -p -- "$out"
stage_tmp=$(mktemp -d "$out/.synveil-server-runtime.XXXXXXXX")
# Only this invocation's private temporary directory is ever removed.
trap 'rm -rf -- "$stage_tmp"' EXIT
export CARGO_TARGET_DIR="$root/target"
(cd "$root" && cargo build --locked --release -p synveil-api --bin synveil-api --bin synveil-worker --bin synveil-server-migrate)
for component in synveil-api synveil-worker synveil-server-migrate; do
  install -m 0755 "$root/target/release/$component" "$stage_tmp/$component"
done
install -m 0644 "$root/LICENSE" "$root/NOTICE" "$stage_tmp/"
python3 - "$stage_tmp" "$revision" "$root/Cargo.toml" <<'PY'
import hashlib, json, pathlib, sys, tomllib
root, revision = pathlib.Path(sys.argv[1]), sys.argv[2]
version = tomllib.loads(pathlib.Path(sys.argv[3]).read_text())['workspace']['package']['version']
files=[]
for path in sorted(root.iterdir()):
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    files.append({"component":path.name,"path":path.name,"size":path.stat().st_size,"sha256":digest,"mode":format(path.stat().st_mode & 0o777, '04o')})
manifest={"schema_version":1,"runtime_id":"synveil-"+revision,"product_version":version,"source_revision":revision,"platform":"linux","architecture":"x86_64","runtime_compatibility":"synveil-server-v1","files":files}
(root/"runtime-manifest.json").write_text(json.dumps(manifest,sort_keys=True,indent=2)+"\n")
PY
# Publish atomically without overwriting an existing identity, even an empty
# foreign directory. A rerun may reuse only an identical, closed inventory.
mv --no-clobber --no-target-directory -- "$stage_tmp" "$stage"
if [[ -d "$stage_tmp" ]]; then
  python3 - "$stage_tmp" "$stage" <<'PY'
import hashlib, pathlib, sys
expected, existing = map(pathlib.Path, sys.argv[1:])
if existing.is_symlink() or not existing.is_dir():
    raise SystemExit('runtime publication identity is not an owned directory')
if sorted(p.name for p in expected.iterdir()) != sorted(p.name for p in existing.iterdir()):
    raise SystemExit('existing runtime inventory differs; preserved without overwrite')
for source in expected.iterdir():
    target = existing / source.name
    if target.is_symlink() or not target.is_file() or (source.stat().st_mode & 0o777) != (target.stat().st_mode & 0o777):
        raise SystemExit('existing runtime file identity differs; preserved without overwrite')
    with source.open('rb') as first, target.open('rb') as second:
        if hashlib.file_digest(first, 'sha256').digest() != hashlib.file_digest(second, 'sha256').digest():
            raise SystemExit('existing runtime bytes differ; preserved without overwrite')
PY
fi
printf '%s\n' "$stage"
