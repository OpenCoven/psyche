"""Generate the first Build profile's denial and canonicalization cases.

Every input is derived deterministically from the canonical vectors in
crates/psyche-core/tests/fixtures/vectors/. The expected result of each case is
the reviewed pin in EXPECT below; `cargo test --locked -p psyche-test-support
--test first_build_profile` checks every case against the Rust decoder, and the
kit runner checks them again from an extracted archive.

    python3 -I profiles/first-build-profile/make_denials.py          # write
    python3 -I profiles/first-build-profile/make_denials.py --check  # fail on drift

Do not edit denials.json by hand.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
VECTORS = ROOT / "crates/psyche-core/tests/fixtures/vectors"
OUTPUT = HERE / "denials.json"
FORMAT = "psyche.experimental.first-build-profile.denials.v1"

# The publication surface selection (docs/protocol/PUBLICATION-SURFACE.md).
STABLE_TARGETS = ["identity_snapshot", "intent", "surface_event"]
EXPERIMENTAL = ["graph", "graph_node", "execution_binding", "delegation", "budget", "approval",
                "evidence", "verdict", "recovery", "addon", "error"]
EXCLUDED = {"surface_effect": "row 13 held", "delivery": "row 13 held"}
SELECTED = STABLE_TARGETS + EXPERIMENTAL


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def reversed_keys(value):
    if isinstance(value, dict):
        return {key: reversed_keys(value[key]) for key in reversed(list(value))}
    if isinstance(value, list):
        return [reversed_keys(item) for item in value]
    return value


def vector(kind):
    raw = (VECTORS / f"psyche.{kind}.v1.json").read_bytes()
    return raw.decode("utf-8"), json.loads(raw)


def with_schema(document, schema):
    return compact({**document, "schema_version": schema})


def rejected(code, **fields):
    return {"result": "rejected", "code": code, **fields}


def unsupported(found):
    return rejected("unsupported_major", found=found, supported=1)


def shape(schema, field):
    return rejected("invalid_shape", schema=schema, field=field)


def cases():
    out = []
    for kind in SELECTED:
        raw, document = vector(kind)
        out.append((f"{kind}/unknown_major_v2", with_schema(document, f"psyche.{kind}.v2"), unsupported(2)))
        out.append((f"{kind}/downgrade_major_v0", with_schema(document, f"psyche.{kind}.v0"), unsupported(0)))
        out.append((f"{kind}/unknown_field", compact({**document, "zz_unknown_field": True}),
                    EXPECT_UNKNOWN_FIELD[kind]))
        out.append((f"{kind}/reordered_keys", compact(reversed_keys(document)),
                    {"result": "canonicalizes", "vector": kind}))
        out.append((f"{kind}/whitespace", json.dumps(document, ensure_ascii=False, indent=2) + "\n",
                    {"result": "canonicalizes", "vector": kind}))
    raw, intent = vector("intent")
    _, graph = vector("graph")
    _, identity = vector("identity_snapshot")
    without_schema = {key: value for key, value in intent.items() if key != "schema_version"}
    out += [
        ("global/unknown_schema_kind", with_schema(intent, "psyche.unregistered_kind.v1"),
         rejected("unknown_schema")),
        ("global/malformed_major", with_schema(intent, "psyche.intent.vx"), unsupported(0)),
        ("global/excluded_kind_surface_effect_v2", with_schema(vector("surface_effect")[1],
                                                               "psyche.surface_effect.v2"), unsupported(2)),
        ("global/missing_schema_version", compact(without_schema), shape("error", "schema_version")),
        ("global/duplicate_key", raw[:-1] + ',"schema_version":"psyche.intent.v1"}', shape("error", "json")),
        ("global/trailing_data", raw + "{}", shape("error", "json")),
        ("global/truncated", raw[:-1], shape("error", "json")),
        ("global/not_an_object", compact([intent]), shape("error", "schema_version")),
        ("global/unknown_enum_value", compact({**graph, "state": "widened_state"}),
         rejected("unknown_enum_value", schema="graph", field="state")),
        ("global/unsafe_integer", compact({**identity, "revision": 9007199254740993}),
         EXPECT_UNSAFE_INTEGER),
    ]
    return out


# Reviewed pins for the decoder's classification of an added top-level field.
EXPECT_UNKNOWN_FIELD = {kind: shape(kind, "document") for kind in SELECTED}
EXPECT_UNSAFE_INTEGER = shape("error", "document")


def render():
    payload = {"format": FORMAT,
               "cases": [{"name": name, "input": data, "expect": expect} for name, data, expect in cases()]}
    return (json.dumps(payload, ensure_ascii=False, indent=1, sort_keys=True) + "\n").encode("utf-8")


def main():
    data = render()
    if sys.argv[1:] == ["--check"]:
        if not OUTPUT.is_file() or OUTPUT.read_bytes() != data:
            raise SystemExit(f"{OUTPUT.relative_to(ROOT)} is stale; run make_denials.py")
        return
    if sys.argv[1:]:
        raise SystemExit("usage: make_denials.py [--check]")
    OUTPUT.write_bytes(data)


if __name__ == "__main__":
    main()
