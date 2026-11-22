"""Offline correlation check for two observations of one synthetic execution."""

import hashlib
import json
import re
import sys
import zipfile
from decimal import Decimal, InvalidOperation
from pathlib import Path

VERSION = "psyche.experimental.receipt-replay.v1"
PAYLOADS = {"__main__.py", "observation.schema.json", "launch.schema.json", "golden/request.json", "golden/observation.json"}
LIMIT = 64 * 1024


class Denial(ValueError):
    pass


def unique_object(pairs):
    obj = dict(pairs)
    if len(obj) != len(pairs):
        raise Denial("duplicate_field")
    return obj


def invalid_number(_):
    raise Denial("invalid_json")


def document(data, *, exact_numbers=False):
    try:
        obj = json.loads(data.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=invalid_number,
                         parse_float=Decimal if exact_numbers else float)
    except Denial:
        raise
    except (ValueError, RecursionError, InvalidOperation):
        raise Denial("invalid_json") from None
    if not isinstance(obj, dict):
        raise Denial("invalid_document")
    return obj


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verify_bundle():
    with zipfile.ZipFile(sys.argv[0]) as bundle:
        names = bundle.namelist()
        if (PAYLOADS | {"manifest.json"}) - set(names):
            raise Denial("missing_file")
        if len(names) != len(PAYLOADS) + 1 or set(names) != PAYLOADS | {"manifest.json"}:
            raise Denial("unexpected_file")
        if any(info.file_size > LIMIT for info in bundle.infolist()):
            raise Denial("oversize_file")
        manifest = document(bundle.read("manifest.json"))
        if manifest.get("profile") != VERSION:
            raise Denial("unsupported_version")
        expected = {name: digest(bundle.read(name)) for name in PAYLOADS}
        if manifest.get("files") != expected:
            raise Denial("file_digest_mismatch")
        return document(bundle.read("observation.schema.json")), document(bundle.read("launch.schema.json"))


def read_input(path):
    with Path(path).open("rb") as src:
        content = src.read(LIMIT + 1)
    if len(content) > LIMIT:
        raise Denial("oversize_file")
    return content


def observation(data, schema):
    obj = document(data, exact_numbers=True)
    if obj.get("schema_version") != VERSION:
        raise Denial("unsupported_version")
    if set(obj) != set(schema["required"]):
        raise Denial("invalid_observation")
    for key in ("request_id", "request_digest", "receipt_hex"):
        value = obj[key]
        rule = schema["properties"][key]
        if not isinstance(value, str) or len(value) > rule.get("maxLength", LIMIT):
            raise Denial("invalid_observation")
        if not re.fullmatch(rule["pattern"], value):
            raise Denial("invalid_observation")
    executions = obj["executions"]
    if type(executions) not in (int, Decimal) or executions < 0:
        raise Denial("invalid_observation")
    if isinstance(executions, Decimal) and executions != executions.to_integral_value():
        raise Denial("invalid_observation")
    return obj


def compare(request_bytes, first, replay, launch_schema):
    request = document(request_bytes)
    if request.get("schema_version") != "psyche.execution_request.v1":
        raise Denial("unsupported_version")
    if set(request) != set(launch_schema["required"]):
        raise Denial("unsupported_request_shape")
    for key, rule in launch_schema["properties"].items():
        value = request[key]
        if "const" in rule:
            # JSON booleans and numbers compare equal in Python. Compare their wire types too.
            if json.dumps(value, sort_keys=True) != json.dumps(rule["const"], sort_keys=True):
                raise Denial("unsupported_request_shape")
        elif not isinstance(value, str) or re.fullmatch(rule["pattern"], value) is None:
            raise Denial("unsupported_request_shape")
    root, cwd = request["project_root"], request["cwd"]
    if cwd != root and not cwd.startswith(root + "/"):
        raise Denial("cwd_outside_project_root")
    if json.dumps(request, sort_keys=True, separators=(",", ":")).encode("ascii") != request_bytes:
        raise Denial("noncanonical_request")
    request_digest = "sha256:" + digest(request_bytes)
    for sample in (first, replay):
        if sample["request_id"] != request.get("request_id"):
            raise Denial("request_identity_mismatch")
        if sample["request_digest"] != request_digest:
            raise Denial("request_digest_mismatch")
    if first["receipt_hex"] != replay["receipt_hex"]:
        raise Denial("receipt_mismatch")
    if first["executions"] != 1 or replay["executions"] != 1:
        raise Denial("execution_count_mismatch")
    return {"profile": VERSION, "result": "passed", "request_id": first["request_id"],
            "request_digest": request_digest, "receipt_digest": "sha256:" + digest(bytes.fromhex(first["receipt_hex"])),
            "executions": 1, "additional_executions": 0}


def main():
    if len(sys.argv) != 4:
        raise Denial("expected_request_first_replay")
    schema, launch_schema = verify_bundle()
    request, first, replay = map(read_input, sys.argv[1:])
    return compare(request, observation(first, schema), observation(replay, schema), launch_schema)


if __name__ == "__main__":
    try:
        print(json.dumps(main(), sort_keys=True))
    except (Denial, OSError, zipfile.BadZipFile) as error:
        code = str(error) if isinstance(error, Denial) else "input_unreadable"
        if isinstance(error, FileNotFoundError):
            code = "missing_file"
        print(json.dumps({"result": "denied", "code": code}))
        sys.exit(1)
