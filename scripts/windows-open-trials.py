"""Run the eight-thread migration test on the original Windows source."""
import hashlib
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

TEST = "concurrent_first_open_applies_migration_once"
MIGRATIONS = Path("crates/psyche-store/tests/migrations.rs")
CONNECTION = Path("crates/psyche-store/src/connection.rs")
BASE = "ad22ada23473d120578985f26f27f23e1fde633c"
TRIALS = 120


def output(*cmd):
    return subprocess.check_output(cmd, text=True).strip()


def binary():
    build = output("cargo", "test", "--locked", "-p", "psyche-store", "--test",
                   "migrations", "--no-run", "--message-format=json")
    return next(event["executable"] for line in build.splitlines()
                if (event := json.loads(line)).get("executable")
                and event["target"]["name"] == "migrations")


def instrument_errors():
    source = CONNECTION.read_text(encoding="utf-8")
    lines = [line.replace("StoreError::file_operation", f"trace_file_error({number})")
             for number, line in enumerate(source.splitlines(keepends=True), start=1)]
    assert source.count("StoreError::file_operation") == 20
    assert "fs::symlink_metadata(sidecar)" in lines[280]
    lines[280] = lines[280].replace("symlink_metadata(sidecar)", "symlink_metadata(&sidecar)")
    assert "trace_file_error(284)(error)" in lines[283]
    lines[283] = lines[283].replace("trace_file_error(284)(error)", "observe_sidecar(&sidecar, error)")
    lines.append("""
fn observe_sidecar(path: &Path, error: std::io::Error) -> StoreError {
    let role = ["-journal", "-wal", "-shm"].into_iter()
        .find(|suffix| path.as_os_str().to_string_lossy().ends_with(suffix)).unwrap_or("unknown");
    let first_code = error.raw_os_error();
    for delay in [0, 2, 10] {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        match std::fs::symlink_metadata(path) {
            Ok(_) => eprintln!("sidecar={role} first={first_code:?} delay_ms={delay} next=present"),
            Err(next) => eprintln!("sidecar={role} first={first_code:?} delay_ms={delay} next={:?}", next.raw_os_error()),
        }
    }
    trace_file_error(284)(error)
}
""")
    lines.append("""
fn trace_file_error(line: u32) -> impl FnOnce(std::io::Error) -> StoreError {
    move |error| {
        eprintln!("file_operation line={line} kind={:?} code={:?}", error.kind(), error.raw_os_error());
        StoreError::file_operation(error)
    }
}
""")
    CONNECTION.write_text("".join(lines), encoding="utf-8")


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
baseline = Counter({0: 0})
instrument_errors()
try:
    diagnosed = trials("diagnostic") + trials("diagnostic-suite", suite=True)
finally:
    subprocess.run(["git", "restore", "--", str(CONNECTION)], check=True)
assert hashlib.sha256(MIGRATIONS.read_bytes()).hexdigest() == digest
assert not output("git", "status", "--porcelain", "--untracked-files=all")
print("trial batches complete; assertions preserved; checkout clean", flush=True)
sys.exit(1 if set(baseline) != {0} or set(diagnosed) != {0} else 0)
