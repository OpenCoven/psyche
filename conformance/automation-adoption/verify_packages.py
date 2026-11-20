"""Inspect the two exact Coven contract tarballs using Python 3.9+."""

import argparse
import copy
from hashlib import sha256
from io import BytesIO
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_package(path, profile, files, pins):
    with path.open("rb") as stream:
        raw = stream.read(1024 * 1024)
    require(sha256(raw).hexdigest() == pins["bundles"][profile], f"wrong bundle digest for {profile}")
    with tarfile.open(fileobj=BytesIO(raw), mode="r:gz") as package:
        def read_json(name):
            member = package.extractfile(name)
            require(member is not None, f"missing {name}")
            return json.load(member)

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path)
    parser.add_argument("authority", type=Path)
    args = parser.parse_args()
    pins = json.loads((HERE / "source-pins.json").read_bytes())
    base = read_package(args.base, "coven.automations.v1",
                        ["automation-receipt.schema", "automation-run.schema"], pins)
    authority = read_package(args.authority, "coven.automations.authority.v1",
                             ["test-vectors", "capabilities", "upstream-artifacts"], pins)
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


if __name__ == "__main__":
    main()
