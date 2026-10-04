#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-"$root/target/server-runtime"}
revision=$(git -C "$root" rev-parse HEAD)
runtime_id="synveil-${revision}"
stage="$out/$runtime_id"
rm -rf -- "$stage.tmp"
mkdir -p -- "$stage.tmp"
cargo build --locked --release -p synveil-api --bin synveil-api --bin synveil-worker --bin synveil-server-migrate
for component in synveil-api synveil-worker synveil-server-migrate; do
  install -m 0755 "$root/target/release/$component" "$stage.tmp/$component"
done
python3 - "$stage.tmp" "$revision" <<'PY'
import hashlib, json, os, pathlib, sys
root, revision = pathlib.Path(sys.argv[1]), sys.argv[2]
files=[]
for path in sorted(root.iterdir()):
    data=path.read_bytes()
    files.append({"component":path.name,"path":path.name,"size":len(data),"sha256":hashlib.sha256(data).hexdigest(),"mode":"0755"})
manifest={"schema_version":1,"runtime_id":"synveil-"+revision,"product_version":"0.1.0","source_revision":revision,"platform":"linux","architecture":"x86_64","runtime_compatibility":"synveil-server-v1","files":files}
(root/"runtime-manifest.json").write_text(json.dumps(manifest,sort_keys=True,indent=2)+"\n")
PY
mv -- "$stage.tmp" "$stage"
printf '%s\n' "$stage"
