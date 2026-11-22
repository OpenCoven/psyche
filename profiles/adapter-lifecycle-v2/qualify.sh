#!/usr/bin/env bash
# Both builds use clean exports. Retain the compared artifact at the caller's path.
set -euo pipefail
output=${1:?supply an output archive path}
source_head=$(git rev-parse HEAD)
receipt=bada1b6a854e72e0cc20cfe8e54ca2d11c6ca7a9
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for build in a b; do
  tree="$work/$build"
  mkdir -p "$tree/producer" "$tree/receipt"
  git -c core.autocrlf=false archive "$source_head" profiles/adapter-lifecycle-v2 | tar -x -C "$tree/producer"
  git -c core.autocrlf=false archive "$receipt" profiles/receipt-replay crates/psyche-coven/tests/fixtures/execution-request-launch.json | tar -x -C "$tree/receipt"
  python3 -I "$tree/receipt/profiles/receipt-replay/pack.py" "$tree/inner.pyz"
  python3 -I "$tree/producer/profiles/adapter-lifecycle-v2/pack.py" "$tree/inner.pyz" "$tree/kit.pyz"
done
python3 -I - "$work" <<'PY'
import hashlib, json, sys
from pathlib import Path
from zipfile import ZipFile
work = Path(sys.argv[1])
def inventory(path):
    with ZipFile(path) as bundle:
        members = {name: hashlib.sha256(bundle.read(name)).hexdigest() for name in bundle.namelist()}
    return {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "members": members}
a, b = (inventory(work / build / "kit.pyz") for build in ("a", "b"))
pin = (work / "a/producer/profiles/adapter-lifecycle-v2/archive.sha256").read_text().strip()
if a != b or a["sha256"] != pin:
    raise SystemExit("kit failed reproducibility or pin check")
print(json.dumps(a, sort_keys=True))
PY
profile="$work/a/producer/profiles/adapter-lifecycle-v2"
(cd "$profile" && python3 -B -m unittest discover -s tests -v)
python3 -I "$profile/tests/packaged_controls.py" "$work/a/kit.pyz" "$(cat "$profile/archive.sha256")"
python3 -I "$profile/tests/comparator_result.py" "$work/a/kit.pyz" "$(cat "$profile/archive.sha256")"
cp "$work/a/kit.pyz" "$output"
