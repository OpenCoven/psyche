"""Run the eight-thread migration test on the original Windows source."""
import hashlib
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

TEST = "concurrent_first_open_applies_migration_once"
MIGRATIONS = Path("crates/psyche-store/tests/migrations.rs")
BASE = "aa08cd789f78bbece91b5f702b8e81a3fe9b45e8"
TRIALS = 160


def output(*cmd):
    return subprocess.check_output(cmd, text=True).strip()


def binary():
    build = output("cargo", "test", "--locked", "-p", "psyche-store", "--test",
                   "migrations", "--no-run", "--message-format=json")
    return next(event["executable"] for line in build.splitlines()
                if (event := json.loads(line)).get("executable")
                and event["target"]["name"] == "migrations")


def trials(label, *, suite=False):
    executable = binary()
    counts = Counter()
    for i in range(1, (16 if suite else TRIALS) + 1):
        args = [] if suite else ["--exact", TEST, "--nocapture"]
        result = subprocess.run([executable, *args],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, timeout=60)
        log = result.stdout
        assert f"test {TEST}" in log, log
        assert suite or "running 1 test" in log, log
        counts[result.returncode] += 1
        print(f"{label} trial={i} exit={result.returncode}", flush=True)
        if result.returncode:
            print(log, flush=True)
    print(f"{label} exit_counts={dict(counts)}", flush=True)
    return counts


assert sys.platform == "win32"
assert output("git", "rev-parse", "HEAD") == BASE
assert not output("git", "status", "--porcelain", "--untracked-files=all")
digest = hashlib.sha256(MIGRATIONS.read_bytes()).hexdigest()
print(f"source={BASE} windows={sys.getwindowsversion().build} migrations_sha256={digest}", flush=True)
results = trials("fixed") + trials("fixed-suite", suite=True)
assert hashlib.sha256(MIGRATIONS.read_bytes()).hexdigest() == digest
assert not output("git", "status", "--porcelain", "--untracked-files=all")
print("trial batches complete; assertions preserved; checkout clean", flush=True)
sys.exit(0 if set(results) == {0} else 1)
