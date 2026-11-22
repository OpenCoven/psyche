#!/usr/bin/env bash
set -euo pipefail
root=$(git rev-parse --show-toplevel)
head=$(git -C "$root" rev-parse HEAD)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
profile=profiles/receipt-replay
if [[ -n "$(git -C "$root" status --porcelain --untracked-files=all -- "$profile" crates/psyche-coven/tests/fixtures/execution-request-launch.json)" ]]; then
  echo "Commit the receipt profile and golden inputs before qualifying HEAD" >&2
  exit 1
fi
for build in a b; do
  mkdir "$work/$build"
  git -C "$root" -c core.autocrlf=false -c core.eol=lf archive "$head" -- "$profile" crates/psyche-coven/tests/fixtures/execution-request-launch.json | tar -xf - -C "$work/$build"
  python3 -I "$work/$build/$profile/pack.py" "$work/$build/receipt-replay.pyz"
done
cmp "$work/a/receipt-replay.pyz" "$work/b/receipt-replay.pyz"
pin=$(python3 -c 'import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' "$work/a/receipt-replay.pyz")
expected=$(cat "$work/a/$profile/archive.sha256")
if [[ "$pin" != "$expected" ]]; then
  echo "Receipt archive SHA-256 mismatch: expected $expected, built $pin" >&2
  exit 1
fi
mkdir "$work/consumer"
cd "$work/consumer"
cp "$work/a/receipt-replay.pyz" .
cp "$work/b/$profile/consumer/"* .
python=$(command -v python3)
echo "source=$head archive_sha256=$pin"
case "$(uname -s)" in
  Linux) sudo -n unshare --net -- setpriv --reuid="$(id -u)" --regid="$(id -g)" --clear-groups "$python" -I qualify.py "$pin" ;;
  Darwin) sandbox-exec -p '(version 1)(allow default)(deny network*)' "$python" -I qualify.py "$pin" ;;
  *) echo "Offline qualification requires Linux or macOS" >&2; exit 1 ;;
esac
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  echo "digest=$pin" >> "$GITHUB_OUTPUT"
fi
if [[ $# -eq 1 ]]; then cp receipt-replay.pyz "$1"; fi
