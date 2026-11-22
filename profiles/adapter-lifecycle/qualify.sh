#!/usr/bin/env bash
# Called from the producer checkout. Both inputs are immutable Git objects.
set -euo pipefail
output=${1:?supply an absolute output archive path}
receipt=7babd21136bc4fdd85f03cf9505337f1099f88ac
head=$(git rev-parse HEAD)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
for build in a b; do
  tree="$scratch/$build"
  mkdir -p "$tree/lifecycle" "$tree/replay"
  git archive "$head" profiles/adapter-lifecycle | tar -x -C "$tree/lifecycle"
  git archive "$receipt" profiles/receipt-replay crates/psyche-coven/tests/fixtures/execution-request-launch.json | tar -x -C "$tree/replay"
  python3 -I "$tree/replay/profiles/receipt-replay/pack.py" "$tree/replay.pyz"
  python3 -I "$tree/lifecycle/profiles/adapter-lifecycle/pack.py" "$tree/replay.pyz" "$tree/kit.pyz"
done
python3 - "$scratch/a/kit.pyz" "$scratch/b/kit.pyz" <<'PY'
import hashlib, json, sys, zipfile
from pathlib import Path
def inventory(path):
    with zipfile.ZipFile(path) as bundle:
        files = {n: hashlib.sha256(bundle.read(n)).hexdigest() for n in bundle.namelist()}
        manifest = json.loads(bundle.read('manifest.json'))
        if manifest['files'] != {n: v for n, v in files.items() if n != 'manifest.json'}:
            raise SystemExit('manifest mismatch')
    return {'archive': hashlib.sha256(Path(path).read_bytes()).hexdigest(), 'members': files}
a, b = map(inventory, sys.argv[1:])
if a != b:
    raise SystemExit('reproducibility mismatch')
print(json.dumps(a, sort_keys=True))
PY
python3 -I "$scratch/a/lifecycle/profiles/adapter-lifecycle/test_wire.py" "$scratch/a/kit.pyz"
cp "$scratch/a/kit.pyz" "$output"
