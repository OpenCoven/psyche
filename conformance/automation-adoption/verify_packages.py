"""Inspect the two exact Coven contract tarballs using Python 3.9+."""

import argparse
import copy
from hashlib import sha256
from io import BytesIO
import json
from pathlib import Path
import re
import tarfile

HERE = Path(__file__).resolve().parent
BUNDLE_LIMIT = 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_package(path, profile, files, pins):
    with path.open("rb") as stream:
        raw = stream.read(BUNDLE_LIMIT + 1)
    require(len(raw) <= BUNDLE_LIMIT, f"bundle for {profile} exceeds {BUNDLE_LIMIT} bytes")
    require(sha256(raw).hexdigest() == pins["bundles"][profile], f"wrong bundle digest for {profile}")
    with tarfile.open(fileobj=BytesIO(raw), mode="r:gz") as package:
        def read_json(name):
            require(name in package.getnames(), f"missing {name}")
            return json.load(package.extractfile(name))

        manifest = read_json("manifest.json")
        require(manifest["sourceCommit"] == pins["coven"], "unrecognized source revision")
        require(manifest["contractProfile"] == profile, "wrong profile")
        prefix = profile.replace(".", "-")
        return {name: read_json(f"{prefix}/{name}.json") for name in files}


def correlate(binding, evidence, trusted):
    base = binding["base"]
    for key in ("automationId", "automationRevision", "occurrenceId", "occurrenceFenceGeneration",
                "runId", "attemptId", "attemptNumber"):
        require(base[key] == evidence[key] == trusted[key], f"{key} mismatch")
    require(base["occurrenceKey"] == trusted["occurrenceKey"], "occurrence key mismatch")
    require(base["adoptionKey"] == trusted["adoptionKey"], "attempt adoption key mismatch")
    require(base["definitionDigest"] == evidence["definitionDigest"], "definition digest mismatch")
    require(base["definitionDigest"]["value"] == trusted["definitionDigest"], "unknown definition")
    require(binding["bindingId"] == evidence["bindingId"] == trusted["bindingId"], "unknown binding")
    require(binding["integrity"] == evidence["bindingDigest"], "receipt names a different binding")
    require(evidence["bindingDigest"]["value"] == trusted["bindingDigest"], "unexpected binding digest")
    require(evidence["receiptId"] == trusted["receiptId"], "receipt identity mismatch")
    require(evidence["baseReceiptDigest"]["value"] == trusted["baseReceiptDigest"], "wrong base receipt")
    require(binding["principal"]["principalId"] == evidence["principalId"] == trusted["principalId"],
            "principal mismatch")
    require(binding["threads"]["decisionDigest"] == evidence["authorization"]["decisionDigest"],
            "Threads decision mismatch")
    familiar = binding["familiar"]
    for field in ("familiarRootId", "identityRevisionId", "declarationDigest"):
        require(familiar[field] == evidence["familiar"][field], f"familiar {field} mismatch")
    require(familiar["embodimentDigest"]["value"] == trusted["familiarEmbodimentDigest"],
            "unexpected embodiment digest")
    grants = binding["capabilities"]["granted"]
    receipt_grants = evidence["capabilities"]["granted"]
    require(len(grants) == len(set(grants)) and len(receipt_grants) == len(set(receipt_grants))
            and set(grants) == set(receipt_grants) == set(trusted["grantedCapabilities"]),
            "granted capabilities differ")


def check_source_names(source, binding_schema, common, attempt_schema):
    """Compare the fixture's `source` member names and values with Coven's binding base."""
    base = binding_schema["properties"]["base"]
    require(base["additionalProperties"] is False and set(base["required"]) == set(base["properties"]),
            "binding base is not a closed, fully required object")
    fence = attempt_schema["properties"]["dispatchFence"]["properties"]["dispatchGeneration"]
    rules = dict(base["properties"], dispatchGeneration=fence)
    require(set(source) == set(rules), f"source members differ: {sorted(set(source) ^ set(rules))}")
    for name, rule in rules.items():
        if "$ref" in rule:
            file, _, pointer = rule["$ref"].partition("#/$defs/")
            require(file == "common.schema.json" and pointer in common["$defs"], f"unresolved {name}")
            rule = common["$defs"][pointer]
        value = source[name]
        if rule["type"] == "integer":
            require(type(value) is int and rule["minimum"] <= value, f"{name} is not a valid counter")
        elif rule["type"] == "string":
            require(type(value) is str and rule["minLength"] <= len(value) <= rule["maxLength"]
                    and re.search(rule["pattern"], value), f"{name} does not match the producer pattern")
        else:
            require(rule["type"] == "object" and set(value) == set(rule["required"]) == set(rule["properties"]),
                    f"{name} members differ")
            for member, inner in rule["properties"].items():
                require(value[member] == inner["const"] if "const" in inner
                        else re.search(inner["pattern"], value[member]), f"{name}.{member} is not valid")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("authority", type=Path)
    args = parser.parse_args()
    pins = json.loads((HERE / "source-pins.json").read_bytes())
    base = read_package(args.base, "coven.automations.v1",
                        ["automation-receipt.schema", "automation-run.schema", "automation-attempt.schema"], pins)
    authority = read_package(args.authority, "coven.automations.authority.v1",
                             ["test-vectors", "capabilities", "upstream-artifacts",
                              "automation-execution-binding.schema", "common.schema"], pins)
    require("extensions" in base["automation-run.schema"]["properties"], "run cannot carry sidecar")
    receipt = base["automation-receipt.schema"]
    require(receipt["additionalProperties"] is False and "extensions" not in receipt["properties"],
            "unexpected base receipt extension surface")
    advertised = authority["capabilities"]["advertisement"]
    require(advertised["requiredCapability"] == "automations.runtime-authority.v1", "wrong capability")
    expected = {"OpenCoven/familiar-contract": pins["familiarContract"], "OpenCoven/coven-threads": pins["threads"]}
    declared = authority["upstream-artifacts"]["artifacts"]
    require({entry["repository"] for entry in declared} == expected.keys(), "upstream owner mismatch")
    require(all(entry["commit"] == expected[entry["repository"]] for entry in declared), "upstream pin mismatch")

    vectors = authority["test-vectors"]
    binding = vectors["fixtures"]["binding"]
    evidence = vectors["fixtures"]["receiptEvidence"]
    correlate(binding, evidence, vectors["trusted"])
    for field, value in (("attemptId", "different-attempt"), ("occurrenceFenceGeneration", 99),
                         ("bindingId", "different-binding"), ("principalId", "different-principal"),
                         ("baseReceiptDigest", {"value": "0" * 64})):
        changed = copy.deepcopy(evidence)
        changed[field] = value
        try:
            correlate(binding, changed, vectors["trusted"])
        except ValueError:
            continue
        raise ValueError(f"accepted a changed {field}")
    print("Verified both package digests, producer pins and receipt placement.")
    print("Verified one binding/receipt correlation and rejected five changed sidecars.")

    schemas = (authority["automation-execution-binding.schema"], authority["common.schema"],
               base["automation-attempt.schema"])
    sources = [json.loads((HERE / name).read_bytes())["source"] for name in ("request.json", "verified-input.json")]
    require(sources[0] == sources[1], "request and trusted projection name different sources")
    check_source_names(sources[0], *schemas)
    renamed = dict(sources[0], attemptAdoptionKey=sources[0]["adoptionKey"])
    del renamed["adoptionKey"]
    for changed in (renamed, {k: v for k, v in sources[0].items() if k != "attemptNumber"},
                    dict(sources[0], schedule="hourly"), dict(sources[0], attemptNumber=0),
                    dict(sources[0], occurrenceKey="daily report"), dict(sources[0], adoptionKey="short")):
        try:
            check_source_names(changed, *schemas)
        except ValueError:
            continue
        raise ValueError("accepted a source that the producer schema does not describe")
    print("Verified the fixture source members against the binding base and rejected six changes.")


if __name__ == "__main__":
    main()
