"""Build and qualify the experimental native Build receipt verifier kit."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
PROFILE = "profiles/build-receipt-binding"
FORMAT = "psyche.experimental.build-binding-kit.v1"
TARGETS = {
    "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu",
}
GOLDEN = ["allowed_signers", "context.json", "request.json", "receipt.json", "binding.json", "binding.sig"]
SOURCES = {
    "LICENSE": "LICENSE",
    "README.md": f"{PROFILE}/KIT.md",
    "binding.schema.json": f"{PROFILE}/binding.schema.json",
    **{f"golden/{name}": f"{PROFILE}/golden/{name}" for name in GOLDEN},
    "golden/request.json": "crates/psyche-coven/tests/fixtures/execution-request-launch.json",
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def binary(target):
    return "bin/verify-build-binding" + (".exe" if target.endswith("-windows-msvc") else "")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_source(source):
    require(git("rev-parse", "HEAD").decode().strip() == source, "source commit does not match checkout")
    require(not git("status", "--porcelain", "--untracked-files=all"), "build requires a clean checkout")


def build(args):
    verify_source(args.source)
    archive = args.archive.resolve()
    require(ROOT not in archive.parents, "write the archive outside the checkout")
    pin = re.search(r'^channel = "([^"]+)"$', (ROOT / "rust-toolchain.toml").read_text(), re.M)[1]
    def pinned_tool(name):
        path = subprocess.check_output(["rustup", "which", "--toolchain", pin, name], cwd=ROOT, text=True).strip()
        require(Path(path).is_absolute() and Path(path).is_file(), f"missing pinned {name}")
        return path

    compiler = pinned_tool("rustc")
    cargo_bin = pinned_tool("cargo")
    rustc = subprocess.check_output([compiler, "-vV"], cwd=ROOT, text=True)
    require(f"host: {args.target}" in rustc.splitlines(), "native Rust host differs from target")
    require(f"release: {pin}" in rustc.splitlines(), "rustc differs from workspace pin")
    cargo = subprocess.check_output([cargo_bin, "--version"], cwd=ROOT, text=True).strip()
    require(cargo.split()[1] == pin, "cargo differs from workspace pin")
    files = {name: git("show", f"{args.source}:{path}") for name, path in SOURCES.items()}
    exe = binary(args.target)
    # Cargo also accepts compiler and wrapper selections from the environment and config files.
    # Pin the same executable that supplied the manifest version and disable both wrapper scopes.
    build_env = dict(os.environ, RUSTC=compiler, CARGO_BUILD_RUSTC=compiler,
                     RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="",
                     CARGO_BUILD_RUSTC_WRAPPER="", CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER="")
    with tempfile.TemporaryDirectory(prefix="binding-build-") as tmp:
        subprocess.run([cargo_bin, "build", "--locked", "--release", "--target", args.target,
                        "--target-dir", tmp, "-p", "psyche-test-support", "--bin", "verify-build-binding"],
                       cwd=ROOT, check=True, env=build_env)
        files[exe] = (Path(tmp) / args.target / "release" / Path(exe).name).read_bytes()
    verify_source(args.source)
    manifest = {
        "format": FORMAT, "repository": "OpenCoven/psyche", "source_commit": args.source,
        "target": args.target, "rustc": rustc.splitlines()[0], "cargo": cargo,
        "rust_channel": pin, "build_profile": "release", "cargo_locked": True,
        "cargo_lock_sha256": sha(git("show", f"{args.source}:Cargo.lock")),
        "files": {name: sha(data) for name, data in files.items()},
    }
    files["manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    with archive.open("xb") as out, gzip.GzipFile(filename="", fileobj=out, mode="wb", mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tar:
            for name, data in sorted(files.items()):
                info = tarfile.TarInfo(name)
                info.mode = 0o755 if name == exe else 0o644
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
    digest = sha(archive.read_bytes())
    print(f"{digest}  {archive.name}", flush=True)
    return digest


def smoke(args, digest):
    # Authenticate archive bytes against the independent pin before parsing any member.
    size_limit = 32 * 1024 * 1024
    require(args.archive.stat().st_size <= size_limit, "archive exceeds size limit")
    data = args.archive.read_bytes()
    require(sha(data) == digest, "archive SHA-256 mismatch")
    exe = binary(args.target)
    names = {*SOURCES, exe, "manifest.json"}
    with tempfile.TemporaryDirectory(prefix="binding-consumer-") as tmp:
        dest = Path(tmp)
        hashes = {}
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            for member in tar:
                name = member.name
                require(name in names and name not in hashes, "unexpected or duplicate kit member")
                require(member.isfile() and member.size <= (size_limit if name == exe else 65536),
                        "kit member has invalid type or size")
                require(member.mode == (0o755 if name == exe else 0o644), "invalid kit member mode")
                payload = tar.extractfile(member).read()
                hashes[name] = sha(payload)
                path = dest / name
                path.parent.mkdir(exist_ok=True)
                path.write_bytes(payload)
                path.chmod(member.mode)
        require(hashes.keys() == names, "incomplete verifier kit")
        manifest = json.loads((dest / "manifest.json").read_bytes())
        del hashes["manifest.json"]
        require((manifest["format"], manifest["repository"], manifest["source_commit"], manifest["target"],
                 manifest["build_profile"], manifest["cargo_locked"]) ==
                (FORMAT, "OpenCoven/psyche", args.source, args.target, "release", True), "wrong kit provenance")
        require(manifest["files"] == hashes, "member digest mismatch")

        inputs = [f"golden/{name}" for name in GOLDEN]
        env = {k: v for k, v in os.environ.items() if k.upper() in {
            "PATH", "SYSTEMROOT", "WINDIR", "PATHEXT", "COMSPEC", "SYSTEMDRIVE", "TMP", "TEMP", "TMPDIR",
        }}

        def check(arguments, result, code):
            run = subprocess.run([str(dest / exe), *arguments], cwd=dest, env=env, capture_output=True,
                                 timeout=20)
            require(run.returncode == (0 if result == "verified" else 1) and
                    json.loads(run.stdout) == {"result": result, "code": code, "redispatch": False},
                    f"wrong {code} disposition: {run.returncode} {run.stdout!r} {run.stderr!r}")

        check(inputs, "verified", "binding_verified")
        check([*inputs[:-1], "missing.sig"], "unknown", "recovery_required")
        (dest / "changed.bin").write_bytes((dest / inputs[3]).read_bytes() + b"\n")
        check([*inputs[:3], "changed.bin", *inputs[4:]], "quarantine", "conflicting_evidence")
        (dest / "empty-policy").write_bytes(b"")
        check(["empty-policy", *inputs[1:]], "denied", "untrusted_issuer")
    print(f"{args.target} {args.source} verified archive, manifest, and four offline dispositions", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build", "smoke"])
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--source", required=True, help="expected full producer commit")
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--sha256", help="independently pinned digest (required for smoke)")
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
