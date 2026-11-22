"""Consumer-side conformance controls for the published receipt comparison command."""

import hashlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from zipfile import ZipFile

INPUT_LIMIT = 64 * 1024
MAX_RECEIPT_BYTES = 4096


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")


def check_requests(command, scratch, request, observations):
    request_file = scratch / "request.json"
    samples = [scratch / "first.json", scratch / "replay.json"]
    cases = [
        ("passed", canonical(request)),
        ("invalid_json", b"{"),
        ("invalid_document", b"[]"),
        ("oversize_file", canonical(request).ljust(INPUT_LIMIT + 1, b" ")),
        ("unsupported_version", canonical({**request, "schema_version": "psyche.execution_request.v2"})),
        ("unsupported_request_shape", canonical({**request, "unexpected": True})),
        ("unsupported_request_shape", canonical({k: v for k, v in request.items() if k != "operation"})),
        ("unsupported_request_shape", canonical({**request, "operation": "different"})),
        ("unsupported_request_shape", canonical({**request, "principal_id": "fixture"})),
        ("unsupported_request_shape", canonical({**request, "cwd": 1})),
        ("noncanonical_request", canonical(request) + b"\n"),
    ]
    for expected, raw in cases:
        request_file.write_bytes(raw)
        digest = "sha256:" + hashlib.sha256(raw).hexdigest()
        for sample, observation in zip(samples, observations):
            sample.write_bytes(canonical({**observation, "request_digest": digest}))
        command(expected, request_file, *samples)


def check_observations(command, scratch, args, observations):
    changed = scratch / "sample.json"
    for position, sample in enumerate(observations, start=1):
        valid = canonical(sample)
        missing = {k: v for k, v in sample.items() if k != "receipt_hex"}
        other_id = "req_01J00000000000000000000000"
        if sample["request_id"] == other_id:
            other_id = "req_01J00000000000000000000001"
        cases = [
            ("passed", canonical({**sample, "executions": 1.0})),
            ("unsupported_version", canonical({**sample, "schema_version": "unknown"})),
            ("invalid_observation", canonical(missing)),
            ("invalid_observation", canonical({**sample, "extra": True})),
            ("invalid_observation", canonical({**sample, "receipt_hex": 7})),
            ("invalid_observation", canonical({**sample, "receipt_hex": "a"})),
            ("invalid_observation", canonical({**sample, "receipt_hex": "00" * (MAX_RECEIPT_BYTES + 1)})),
            ("invalid_observation", canonical({**sample, "executions": -1})),
            ("invalid_observation", canonical({**sample, "executions": True})),
            ("invalid_observation", canonical({**sample, "executions": 1.5})),
            ("duplicate_field", b'{"executions":1,' + valid[1:]),
            ("invalid_json", canonical({**sample, "executions": float("nan")})),
            ("invalid_json", canonical({**sample, "executions": float("inf")})),
            ("invalid_document", b"[]"),
            ("request_identity_mismatch", canonical({**sample, "request_id": other_id})),
            ("request_digest_mismatch", canonical({**sample, "request_digest": "sha256:" + "0" * 64})),
            ("execution_count_mismatch", canonical({**sample, "executions": 2})),
            ("oversize_file", valid.ljust(INPUT_LIMIT + 1, b" ")),
            ("passed", valid.ljust(INPUT_LIMIT, b" ")),
        ]
        pair = list(args)
        pair[position] = changed
        for expected, raw in cases:
            changed.write_bytes(raw)
            command(expected, *pair)

    # Matching observations must still report exactly one execution.
    first, replay = scratch / "first.json", scratch / "replay.json"
    for path, sample in zip((first, replay), observations):
        path.write_bytes(canonical({**sample, "executions": 2}))
    command("execution_count_mismatch", args[0], first, replay)
    for path, sample in zip((first, replay), observations):
        path.write_bytes(canonical({**sample, "receipt_hex": "ab" * MAX_RECEIPT_BYTES}))
    command("passed", args[0], first, replay)


def check_bundle(command, scratch, args):
    with ZipFile("receipt-replay.pyz") as original:
        members = {name: original.read(name) for name in original.namelist()}
    manifest = json.loads(members["manifest.json"])
    raw_manifest = members["manifest.json"]
    cases = [
        ("unexpected_file", {"extra.json": b"{}"}),
        ("unsupported_version", {"manifest.json": canonical({**manifest, "profile": "unsupported"})}),
        ("oversize_file", {"manifest.json": raw_manifest.ljust(INPUT_LIMIT + 1, b" ")}),
        ("passed", {"manifest.json": raw_manifest.ljust(INPUT_LIMIT, b" ")}),
    ]
    for expected, changes in cases:
        path = scratch / "candidate.pyz"
        with ZipFile(path, "w") as archive:
            for name, value in {**members, **changes}.items():
                archive.writestr(name, value)
        command(expected, *args, archive=path)


def run_controls(command, args):
    request, first, replay = [json.loads(Path(arg).read_bytes()) for arg in args]
    with TemporaryDirectory() as directory:
        scratch = Path(directory)
        check_requests(command, scratch, request, [first, replay])
        check_observations(command, scratch, args, [first, replay])
        check_bundle(command, scratch, args)
