"""Check the archived driver's response to an unsuccessful comparator process."""
import json
from pathlib import Path
import runpy
import sys
from tempfile import TemporaryDirectory
from zipfile import ZipFile


def check(archive, pin):
    sys.path.insert(0, str(archive))
    driver = runpy.run_path(str(archive))
    request, _ = driver["load_bundle"](archive, pin)
    with TemporaryDirectory() as temp, ZipFile(archive) as package:
        root = Path(temp)
        fixture = root / "reference_fixture.py"
        fixture.write_bytes(package.read(fixture.name))
        golden = root / "request.json"
        golden.write_bytes(request)
        for result, additional in (("denied", 0), ("passed", 1)):
            scenario = root / result
            scenario.mkdir()
            commands = [
                [sys.executable, "-I", str(fixture), role, "conforming", str(golden), str(scenario / "audit.jsonl")]
                for role in ("adapter", "observer")
            ]
            reply = {"result": result, "additional_executions": additional}
            # Exercise the inherited comparator descriptor and normal child-output parsing.
            comparator = f"print({json.dumps(reply)!r})\n".encode()
            try:
                driver["qualify"](scenario, request, comparator, *commands, [])
            except ValueError as error:
                if str(error) != "comparator_failed":
                    raise
            else:
                raise AssertionError(f"qualification accepted comparator response {reply}")
            print(json.dumps({"comparator_response": reply, "expected_refusal": "comparator_failed"}))


if __name__ == "__main__":
    check(Path(sys.argv[1]).resolve(), sys.argv[2])
