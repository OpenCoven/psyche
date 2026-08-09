#!/usr/bin/env bash
set -euo pipefail

metadata=$(cargo metadata --no-deps --format-version 1)

for crate in psyche-store psyche-coven psyche-surfaces psyche-test-support; do
  jq -e --arg crate "$crate" '.packages[] | select(.name == $crate)' \
    <<<"$metadata" >/dev/null
done

# Assert psyche-test-support has publish == []  (i.e. not publishable).
jq -e '
  .packages[]
  | select(.name == "psyche-test-support")
  | .publish == []
' <<<"$metadata" >/dev/null || {
  echo "FAIL: psyche-test-support must have publish = false (publish == [] in metadata)" >&2
  exit 1
}

# Verify no manifest combines publish.workspace and a bare publish = in the
# same [package] table — that is a TOML conflict and signals a copy-paste error.
for crate in psyche-store psyche-coven psyche-surfaces psyche-test-support; do
  manifest="crates/$crate/Cargo.toml"
  if grep -qE '^\s*publish\.workspace' "$manifest" && \
     grep -qE '^\s*publish\s*=' "$manifest"; then
    echo "FAIL: $manifest declares both publish.workspace and publish = in [package]" >&2
    exit 1
  fi
done

echo "OK: all G2 workspace crates present and publication metadata is correct."
