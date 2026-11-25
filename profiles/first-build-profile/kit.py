"""Build and qualify the experimental first Build profile prerelease kit.

    python3 profiles/first-build-profile/kit.py build --source SHA --target TRIPLE --archive OUT.tar.gz
    python3 profiles/first-build-profile/kit.py smoke --source SHA --target TRIPLE --archive OUT.tar.gz --sha256 PIN

`build` packages one native target from a clean checkout at SHA and then runs
`smoke`. `smoke` authenticates the archive against the pin before reading any
member, extracts it into a fresh directory and runs the consumer checks.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
PROFILE = "profiles/first-build-profile"
FORMAT = "psyche.experimental.first-build-profile.v1"
STATUS = "experimental-prerelease"
TARGETS = {
    "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu",
}
VECTORS = "crates/psyche-core/tests/fixtures/vectors"
LAUNCH = "crates/psyche-coven/tests/fixtures/execution-request-launch.json"
BINDING = "profiles/build-receipt-binding"
REPLAY = "profiles/receipt-replay"
BINDING_GOLDEN = ["allowed_signers", "context.json", "receipt.json", "binding.json", "binding.sig"]
REPLAY_CONSUMER = ["action.json", "check_inputs.py", "check_witness.py", "qualify.py", "recovery.py",
                   "request.json", "submit.py"]
BINARIES = ["verify-profile", "verify-build-binding"]
SIZE_LIMIT = 64 * 1024 * 1024
MEMBER_PATTERN = re.compile(r"^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+){0,2}$")

sys.path.insert(0, str(ROOT / PROFILE))
import make_denials  # noqa: E402  (same directory; supplies the reviewed selection)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def exe(name, target):
    return f"bin/{name}" + (".exe" if target.endswith("-windows-msvc") else "")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_source(source):
    require(git("rev-parse", "HEAD").decode().strip() == source, "source commit does not match checkout")
    require(not git("status", "--porcelain", "--untracked-files=all"), "build requires a clean checkout")


def show(source, path):
    return git("show", f"{source}:{path}")


def receipt_replay_archive(source):
    """Rebuild the pinned receipt-replay archive from an export of SOURCE."""
    with tempfile.TemporaryDirectory(prefix="first-profile-replay-") as tmp:
        export = subprocess.run(["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "archive", source,
                                 "--", REPLAY, LAUNCH], cwd=ROOT, check=True, capture_output=True).stdout
        with tarfile.open(fileobj=io.BytesIO(export)) as tar:
            tar.extractall(tmp, filter="data") if hasattr(tarfile, "data_filter") else tar.extractall(tmp)
        out = Path(tmp) / "receipt-replay.pyz"
        subprocess.run([sys.executable, "-I", str(Path(tmp) / REPLAY / "pack.py"), str(out)], check=True)
        data = out.read_bytes()
    pin = show(source, f"{REPLAY}/archive.sha256").decode().strip()
    require(sha(data) == pin, f"receipt-replay archive {sha(data)} differs from its pin {pin}")
    return data, pin


def build(args):
    verify_source(args.source)
    archive = args.archive.resolve()
    require(ROOT not in archive.parents, "write the archive outside the checkout")
    require(show(args.source, f"{PROFILE}/denials.json") == make_denials.render(),
            "denials.json is stale; run make_denials.py")
    pin = re.search(r'^channel = "([^"]+)"$', (ROOT / "rust-toolchain.toml").read_text(), re.M)[1]

    def pinned_tool(name):
        path = subprocess.check_output(["rustup", "which", "--toolchain", pin, name], cwd=ROOT, text=True).strip()
        require(Path(path).is_absolute() and Path(path).is_file(), f"missing pinned {name}")
        return path

    compiler, cargo_bin = pinned_tool("rustc"), pinned_tool("cargo")
    rustc = subprocess.check_output([compiler, "-vV"], cwd=ROOT, text=True)
    require(f"host: {args.target}" in rustc.splitlines(), "native Rust host differs from target")
    require(f"release: {pin}" in rustc.splitlines(), "rustc differs from workspace pin")
    cargo = subprocess.check_output([cargo_bin, "--version"], cwd=ROOT, text=True).strip()
    require(cargo.split()[1] == pin, "cargo differs from workspace pin")

    files = {"LICENSE": show(args.source, "LICENSE"),
             "README.md": show(args.source, f"{PROFILE}/CONSUMER.md"),
             "denials.json": show(args.source, f"{PROFILE}/denials.json"),
             "binding/binding.schema.json": show(args.source, f"{BINDING}/binding.schema.json"),
             "binding/golden/request.json": show(args.source, LAUNCH)}
    for name in BINDING_GOLDEN:
        files[f"binding/golden/{name}"] = show(args.source, f"{BINDING}/golden/{name}")
    replay, replay_pin = receipt_replay_archive(args.source)
    files["receipt-replay/receipt-replay.pyz"] = replay
    for name in REPLAY_CONSUMER:
        files[f"receipt-replay/consumer/{name}"] = show(args.source, f"{REPLAY}/consumer/{name}")
    kinds = {}
    for kind in make_denials.SELECTED:
        name = f"vectors/psyche.{kind}.v1.json"
        files[name] = show(args.source, f"{VECTORS}/psyche.{kind}.v1.json")
        kinds[kind] = {"proposed_class": "stable-v1-target" if kind in make_denials.STABLE_TARGETS
                       else "experimental", "vector": name, "sha256": sha(files[name])}

    build_env = dict(os.environ, RUSTC=compiler, CARGO_BUILD_RUSTC=compiler,
                     RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="",
                     CARGO_BUILD_RUSTC_WRAPPER="", CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER="")
    with tempfile.TemporaryDirectory(prefix="first-profile-build-") as tmp:
        command = [cargo_bin, "build", "--locked", "--release", "--target", args.target, "--target-dir", tmp,
                   "-p", "psyche-test-support"]
        for name in BINARIES:
            command += ["--bin", name]
        subprocess.run(command, cwd=ROOT, check=True, env=build_env)
        for name in BINARIES:
            path = exe(name, args.target)
            files[path] = (Path(tmp) / args.target / "release" / Path(path).name).read_bytes()
    verify_source(args.source)

    manifest = {
        "format": FORMAT, "status": STATUS, "repository": "OpenCoven/psyche",
        "source_commit": args.source, "target": args.target, "rustc": rustc.splitlines()[0],
        "cargo": cargo, "rust_channel": pin, "build_profile": "release", "cargo_locked": True,
        "cargo_lock_sha256": sha(show(args.source, "Cargo.lock")),
        "kinds": kinds, "excluded_kinds": make_denials.EXCLUDED,
        "components": {
            "build-receipt-binding": {
                "schema_version": "psyche.build_receipt_binding.v1",
                "request_sha256": sha(files["binding/golden/request.json"]),
                "receipt_sha256": sha(files["binding/golden/receipt.json"]),
                "binding_sha256": sha(files["binding/golden/binding.json"]),
            },
            "receipt-replay": {"profile": "psyche.experimental.receipt-replay.v1",
                               "archive_sha256": replay_pin},
        },
        "files": {name: sha(data) for name, data in files.items()},
    }
    files["manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    binaries = {exe(name, args.target) for name in BINARIES}
    with archive.open("xb") as out, gzip.GzipFile(filename="", fileobj=out, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for name, data in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.mode = 0o755 if name in binaries else 0o644
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    digest = sha(archive.read_bytes())
    print(f"{digest}  {archive.name}", flush=True)
    return digest


def extract(data, dest, binaries):
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar:
            name = member.name
            require(MEMBER_PATTERN.match(name) and ".." not in name.split("/") and name not in seen,
                    f"unexpected or duplicate kit member {name!r}")
            require(member.isfile() and member.size <= SIZE_LIMIT, f"kit member {name!r} has invalid type or size")
            require(member.mode == (0o755 if name in binaries else 0o644), f"invalid mode for {name!r}")
            seen.add(name)
            path = dest / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(tar.extractfile(member).read())
            path.chmod(member.mode)
    require(binaries <= seen and "manifest.json" in seen, "incomplete kit")


def offline(command, cwd):
    """Run COMMAND with networking denied, as the receipt-replay qualification does."""
    system = os.uname().sysname if hasattr(os, "uname") else "Windows"
    if system == "Linux":
        wrapped = ["sudo", "-n", "unshare", "--net", "--", "setpriv", f"--reuid={os.getuid()}",
                   f"--regid={os.getgid()}", "--clear-groups", *command]
    elif system == "Darwin":
        wrapped = ["sandbox-exec", "-p", "(version 1)(allow default)(deny network*)", *command]
    else:
        return None
    return subprocess.run(wrapped, cwd=cwd, capture_output=True, timeout=300)


def smoke(args, digest):
    data = args.archive.read_bytes()
    require(len(data) <= SIZE_LIMIT, "archive exceeds size limit")
    # Authenticate archive bytes against the independent pin before parsing any member.
    require(sha(data) == digest, "archive SHA-256 mismatch")
    binaries = {exe(name, args.target) for name in BINARIES}
    env = {k: v for k, v in os.environ.items() if k.upper() in {
        "PATH", "SYSTEMROOT", "WINDIR", "PATHEXT", "COMSPEC", "SYSTEMDRIVE", "TMP", "TEMP", "TMPDIR",
    }}
    with tempfile.TemporaryDirectory(prefix="first-profile-consumer-") as tmp:
        kit = Path(tmp) / "kit"
        extract(data, kit, binaries)
        verifier = str(kit / exe("verify-profile", args.target))

        def profile(root, expected, *extra):
            run = subprocess.run([verifier, str(root), *extra], env=env, capture_output=True, timeout=60)
            report = json.loads(run.stdout)
            require(report["code"] == expected and report["redispatch"] is False and
                    (run.returncode == 0) == (expected == "profile_verified"),
                    f"profile: expected {expected}, got {run.returncode} {report}")
            print(json.dumps(report, sort_keys=True), flush=True)
            return report

        report = profile(kit, "profile_verified", "--source", args.source, "--target", args.target)
        require(report["vectors_verified"] == len(make_denials.SELECTED), "wrong vector count")

        # Fail-closed controls on copies of the extracted kit.
        def control(name, mutate, expected):
            copy = Path(tmp) / name
            shutil.copytree(kit, copy)
            mutate(copy)
            profile(copy, expected)

        def set_format(root, value):
            manifest = json.loads((root / "manifest.json").read_bytes())
            manifest["format"] = value
            (root / "manifest.json").write_text(json.dumps(manifest))

        def append(root, name, extra):
            path = root / name
            path.write_bytes(path.read_bytes() + extra)

        control("tampered-vector", lambda r: append(r, "vectors/psyche.intent.v1.json", b" "),
                "member_digest_mismatch")
        control("downgrade", lambda r: set_format(r, FORMAT[:-1] + "0"), "unsupported_profile_version")
        control("newer-major", lambda r: set_format(r, FORMAT[:-1] + "2"), "unsupported_profile_version")
        profile(kit, "provenance_mismatch", "--source", "0" * 40)

        # ADR 4 binding: both digests and the issuer signature, four dispositions.
        binding = str(kit / exe("verify-build-binding", args.target))
        golden = [f"binding/golden/{name}" for name in
                  ["allowed_signers", "context.json", "request.json", "receipt.json", "binding.json", "binding.sig"]]

        def check(arguments, result, code):
            run = subprocess.run([binding, *arguments], cwd=kit, env=env, capture_output=True, timeout=20)
            outcome = json.loads(run.stdout)
            require(run.returncode == (0 if result == "verified" else 1) and
                    outcome == {"result": result, "code": code, "redispatch": False},
                    f"wrong {code} disposition: {run.returncode} {run.stdout!r} {run.stderr!r}")
            print(json.dumps(outcome, sort_keys=True), flush=True)

        scratch = Path(tmp) / "binding-controls"
        scratch.mkdir()
        (scratch / "changed.bin").write_bytes((kit / golden[3]).read_bytes() + b"\n")
        (scratch / "empty-policy").write_bytes(b"")
        check(golden, "verified", "binding_verified")
        check([*golden[:-1], str(scratch / "missing.sig")], "unknown", "recovery_required")
        check([*golden[:3], str(scratch / "changed.bin"), *golden[4:]], "quarantine", "conflicting_evidence")
        check([str(scratch / "empty-policy"), *golden[1:]], "denied", "untrusted_issuer")

        # Restart witness: the pinned receipt-replay consumer restarts a durable store offline.
        manifest = json.loads((kit / "manifest.json").read_bytes())
        replay_pin = manifest["components"]["receipt-replay"]["archive_sha256"]
        replay = Path(tmp) / "receipt-replay"
        shutil.copytree(kit / "receipt-replay/consumer", replay)
        shutil.copy(kit / "receipt-replay/receipt-replay.pyz", replay)
        run = offline([sys.executable, "-I", "qualify.py", replay_pin], replay)
        if run is None:
            require(not args.require_restart, "restart witness requires Linux or macOS network isolation")
            print("restart witness skipped: no supported network isolation on this host", flush=True)
        else:
            sys.stdout.write(run.stdout.decode(errors="replace"))
            require(run.returncode == 0, f"restart witness failed: {run.stderr.decode(errors='replace')[-2000:]}")
            print("restart witness passed", flush=True)

        # Running the controls must not have changed the extracted kit.
        profile(kit, "profile_verified", "--source", args.source, "--target", args.target)
    print(f"{args.target} {args.source} verified archive {digest}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["build", "smoke"])
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--source", required=True, help="expected full producer commit")
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--sha256", help="independently pinned digest (required for smoke)")
    parser.add_argument("--require-restart", action="store_true",
                        help="fail instead of skipping when the restart witness cannot be isolated")
    args = parser.parse_args()
    if not re.fullmatch("[0-9a-f]{40}", args.source):
        parser.error("expected a full source commit")
    if args.command == "smoke":
        if not re.fullmatch("[0-9a-f]{64}", args.sha256 or ""):
            parser.error("smoke requires a SHA-256 pin")
        smoke(args, args.sha256)
    else:
        if args.sha256:
            parser.error("supply --sha256 to smoke a previously built kit")
        smoke(args, build(args))


if __name__ == "__main__":
    main()
