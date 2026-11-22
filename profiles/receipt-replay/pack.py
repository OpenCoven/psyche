"""Build a byte-reproducible, uncompressed Python archive from a source export."""

import hashlib
import json
import sys
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PROFILE = "psyche.experimental.receipt-replay.v1"
GOLDEN_DIGEST = "75d651c5eb7f6e3ccd65631fce08afdcb8ac2a800bc0d8db55eaf9cf43519d04"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")


def build(output):
    request = (ROOT / "crates/psyche-coven/tests/fixtures/execution-request-launch.json").read_bytes()
    if hashlib.sha256(request).hexdigest() != GOLDEN_DIGEST:
        raise ValueError("Rust launch golden digest changed")
    observation = canonical({"schema_version": PROFILE, "request_id": json.loads(request)["request_id"],
                             "request_digest": "sha256:" + GOLDEN_DIGEST, "receipt_hex": "01", "executions": 1})
    # This profile exercises a synthetic launch with the Rust golden's fixed authority and payload.
    properties = {key: {"const": value} for key, value in json.loads(request).items()}
    properties["request_id"] = {"type": "string", "pattern": "^req_[0-7][0-9A-HJKMNP-TV-Z]{25}$"}
    for key in ("cwd", "project_root"):
        properties[key] = {"type": "string", "pattern": "^/[a-zA-Z0-9_-]{1,63}(?:/[a-zA-Z0-9_-]{1,63}){0,3}$"}
    properties["principal_id"] = {"type": "string", "pattern": "^principal:[a-zA-Z0-9_-]{1,64}$"}
    launch_schema = {"$schema": "https://json-schema.org/draft/2020-12/schema",
                     "type": "object", "additionalProperties": False,
                     "required": sorted(properties), "properties": properties}
    files = {"launch.schema.json": canonical(launch_schema),"__main__.py": (HERE / "run.py").read_bytes(),
             "observation.schema.json": (HERE / "observation.schema.json").read_bytes(),
             "golden/request.json": request, "golden/observation.json": observation}
    manifest = {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}
    files["manifest.json"] = canonical({"profile": PROFILE, "files": manifest})
    with ZipFile(output, "w", compression=ZIP_STORED) as archive:
        for name, data in sorted(files.items()):
            item = ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            item.create_system = 3
            item.external_attr = 0o100644 << 16
            archive.writestr(item, data)
    print(hashlib.sha256(output.read_bytes()).hexdigest(), output.name)


if __name__ == "__main__":
    build(Path(sys.argv[1]))
