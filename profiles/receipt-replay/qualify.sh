#!/usr/bin/env bash
set -euo pipefail
root=$(git rev-parse --show-toplevel)
head=$(git -C "$root" rev-parse HEAD)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
profile=profiles/receipt-replay
for build in a b; do
  mkdir "$work/$build"
  git -C "$root" archive "$head" | tar -xf - -C "$work/$build"
  python3 -I "$work/$build/$profile/pack.py" "$work/$build/receipt-replay.pyz"
done
cmp "$work/a/receipt-replay.pyz" "$work/b/receipt-replay.pyz"
pin=$(python3 -c 'import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' "$work/a/receipt-replay.pyz")
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
