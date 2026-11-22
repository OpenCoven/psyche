#!/usr/bin/env python3
"""Build a native CLI and verify its installed npm launcher using local tarballs."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
WRAPPER = ROOT / "packages" / "psyche-npm"
WORKFLOW = ROOT / ".github" / "workflows" / "native-npm-smoke.yml"
COMPANION_PREFIX = "@opencoven/psyche-"
PLATFORM_ENV = "PSYCHE_EXPECTED_NPM_PLATFORM"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run(args, cwd, env, expected=0):
    command = list(map(str, args))
    if os.name == "nt" and command[0].endswith(".cmd"):
        command = [os.environ["COMSPEC"], "/d", "/c", *command]
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, encoding="utf-8", errors="replace")
    if result.returncode != expected:
        raise RuntimeError(f"{args[0]} {args[1]} exited {result.returncode}, expected {expected}\n"
                           f"{result.stdout[-1200:]}\n{result.stderr[-2400:]}")
    return result


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def check_clean(env):
    state = run(["git", "status", "--porcelain", "--untracked-files=all"], ROOT, env).stdout
    require(not state, "The native npm smoke test requires a clean source checkout.")


def workflow_platforms():
    """Read the matrix platform keys without a YAML dependency. An unexpected layout is refused."""
    lines = []
    for line in WORKFLOW.read_text(encoding="utf-8").splitlines():
        text = line.split(" #", 1)[0].rstrip()
        lines.append("" if text.lstrip().startswith("#") else text)
    starts = [index for index, text in enumerate(lines) if text.strip() == "include:"]
    require(len(starts) == 1, f"{WORKFLOW.name} must hold exactly one block-style matrix include list")
    indent = len(lines[starts[0]]) - len(lines[starts[0]].lstrip())
    entries = []
    for text in lines[starts[0] + 1:]:
        if not text:
            continue
        if len(text) - len(text.lstrip()) <= indent:
            break
        item = text.strip()
        if item.startswith("- "):
            entries.append([])
            item = item[2:].strip()
        require(entries, f"{WORKFLOW.name} matrix include list does not start with an entry")
        name, _, value = item.partition(":")
        if name.strip() == "platform":
            entries[-1].append(value.strip().strip("'\""))
    keys = [entry[0] for entry in entries if len(entry) == 1 and entry[0]]
    require(entries and len(keys) == len(entries) and len(set(keys)) == len(keys),
            f"every {WORKFLOW.name} matrix entry must name one distinct platform key")
    binding = f"{PLATFORM_ENV}: ${{{{ matrix.platform }}}}"
    require([text.strip() for text in lines].count(binding) == 1,
            f"{WORKFLOW.name} must pass matrix.platform to this script as {PLATFORM_ENV}")
    return set(keys)


def check_platform_coverage(manifest, env):
    companions = {name[len(COMPANION_PREFIX):] for name in manifest["optionalDependencies"]
                  if name.startswith(COMPANION_PREFIX)}
    checksums = set(manifest["psyche"]["checksums"])
    launcher = set(json.loads(run(
        ["node", "-p", "JSON.stringify([...require('./scripts/verify-checksum.js').SUPPORTED])"], WRAPPER, env).stdout))
    require(companions == checksums == launcher,
            "the wrapper disagrees with itself about its platform keys: "
            f"companions {sorted(companions)}, checksums {sorted(checksums)}, launcher {sorted(launcher)}")
    listed = workflow_platforms()
    require(listed == checksums,
            f"{WORKFLOW.name} matrix differs from the wrapper's platform keys: "
            f"missing from the matrix {sorted(checksums - listed)}, unknown to the wrapper {sorted(listed - checksums)}")


def pinned_toolchain(env):
    pin = re.search(r'^channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"), re.M)
    require(pin, "rust-toolchain.toml does not pin a channel")
    versions = {tool: run([tool, "--version"], ROOT, env).stdout.strip() for tool in ("rustc", "cargo")}
    for tool, version in versions.items():
        require(version.split()[1:2] == [pin[1]],
                f"{tool} reports {version!r} but rust-toolchain.toml pins {pin[1]}; "
                "put the rustup proxies first on PATH")
    return versions["rustc"]


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def pack(npm, directory, env):
    result = run([npm, "pack", "--json", "--offline", "--ignore-scripts"], directory, env)
    return directory / json.loads(result.stdout)[0]["filename"]


def install(npm, consumer, wrapper, companion, env):
    consumer.mkdir()
    write_json(consumer / "package.json", {"name": "smoke-consumer", "private": True})
    # Use relative local tarball paths even when the user's temp path has spaces.
    run([npm, "install", "--offline", "--ignore-scripts", "--no-audit", "--no-fund", "--omit=optional",
         os.path.relpath(wrapper, consumer), os.path.relpath(companion, consumer)], consumer, env)
    launcher = Path("node_modules") / ".bin" / ("psyche.cmd" if os.name == "nt" else "psyche")
    require((consumer / launcher).is_file(), "npm did not create the native launcher")
    return str(launcher)


def expect_checksum_failure(consumer, launcher, expected_hash, found_hash, env):
    result = run([launcher, "--help"], consumer, env, expected=1)
    require("checksum mismatch" in result.stderr and f"expected {expected_hash}, found {found_hash}" in result.stderr
            and not result.stdout, "the installed launcher failed without the expected digest rejection")


def exercise(work, env):
    platform, arch = json.loads(run(["node", "-p", "JSON.stringify([process.platform,process.arch])"], ROOT, env).stdout)
    key = f"{platform}-{arch}"
    expected_key = env.get(PLATFORM_ENV, key)
    require(key == expected_key, f"expected npm platform {expected_key}, measured {key}")
    npm = "npm.cmd" if os.name == "nt" else "npm"
    source = run(["git", "rev-parse", "HEAD"], ROOT, env).stdout.strip()
    manifest = json.loads((WRAPPER / "package.json").read_text(encoding="utf-8"))
    package = f"{COMPANION_PREFIX}{key}"
    require(package in manifest["optionalDependencies"],
            f"unsupported npm platform {key}: the wrapper declares no {package} companion")
    require(key in manifest["psyche"]["checksums"],
            f"the wrapper's checksum table has no entry for npm platform {key}")
    check_platform_coverage(manifest, env)
    companion_version = manifest["optionalDependencies"][package]
    stock_digest = manifest["psyche"]["checksums"][key]
    require(stock_digest == "0" * 64, "rehearsal expects the pre-release wrapper's zero checksum")
    rustc = pinned_toolchain(env)

    target = work / "target"
    run(["cargo", "build", "--locked", "--release", "-p", "psyche-cli", "--bin", "psyche",
         "--target-dir", target], ROOT, env)
    executable = "psyche.exe" if platform == "win32" else "psyche"
    binary = target / "release" / executable
    measured = sha256(binary)
    require(measured != stock_digest, "native executable has the placeholder checksum")

    companion = work / "companion"
    (companion / "bin").mkdir(parents=True)
    shutil.copy2(binary, companion / "bin" / executable)
    write_json(companion / "package.json", {
        "name": package, "version": companion_version, "private": True,
        "os": [platform], "cpu": [arch], "files": ["bin/"],
    })
    companion_tar = pack(npm, companion, env)
    # Pack two independent copies so the source manifest keeps its shipped digest.
    stock = work / "stock"
    measured_wrapper = work / "measured"
    shutil.copytree(WRAPPER, stock, ignore=shutil.ignore_patterns("node_modules", "*.tgz"))
    shutil.copytree(stock, measured_wrapper)
    stock_tar = pack(npm, stock, env)
    manifest["psyche"]["checksums"][key] = measured
    write_json(measured_wrapper / "package.json", manifest)
    measured_tar = pack(npm, measured_wrapper, env)

    untrusted = work / "zero-consumer"
    launcher = install(npm, untrusted, stock_tar, companion_tar, env)
    expect_checksum_failure(untrusted, launcher, stock_digest, measured, env)

    consumer = work / "measured-consumer"
    launcher = install(npm, consumer, measured_tar, companion_tar, env)
    installed_binary = consumer / "node_modules" / package / "bin" / executable
    require(sha256(installed_binary) == measured, "installed companion differs from the release build")
    help_text = run([launcher, "--help"], consumer, env).stdout
    require(all(name in help_text for name in ["doctor", "status", "stop"]), "unexpected installed help")

    data = consumer / "data"
    data.mkdir(mode=0o700)
    (consumer / "psyche.toml").write_text(
        'schema_version = "psyche.config.v1"\n'
        f'data_dir = {json.dumps(data.as_posix())}\n[coven]\n'
        f'socket = {json.dumps((consumer / "coven.sock").as_posix())}\n'
        'required_api_version = "coven.daemon.v1"\n', encoding="utf-8")
    configured = [launcher, "--config", "psyche.toml"]
    doctor = json.loads(run([*configured, "doctor", "--json"], consumer, env).stdout)
    require(doctor["schema"] == "psyche.doctor.v1" and doctor["failed"] == 0, "unexpected doctor result")
    status = json.loads(run([*configured, "status", "--json"], consumer, env).stdout)
    require(status == {"schema": "psyche.status.v1", "observed": False, "state": None, "reason": "no-ipc"},
            "unexpected status observation")
    stopped = run([*configured, "stop"], consumer, env, expected=4)
    require("no daemon IPC" in stopped.stderr, "unexpected stop disposition")

    # Keep the measured manifest and alter only the installed executable.
    with installed_binary.open("ab") as handle:
        handle.write(b"npm-smoke-mismatch")
    expect_checksum_failure(consumer, launcher, measured, sha256(installed_binary), env)
    return {
        "source": source, "platform": key, "expected_platform": expected_key,
        "node": run(["node", "--version"], ROOT, env).stdout.strip(),
        "npm": run([npm, "--version"], ROOT, env).stdout.strip(),
        "rustc": rustc,
        "binary_sha256": measured, "companion_sha256": sha256(companion_tar),
        "stock_wrapper_sha256": sha256(stock_tar), "measured_wrapper_sha256": sha256(measured_tar),
        "exit_codes": {"zero_digest": 1, "help": 0, "doctor": 0, "status": 0, "stop": 4, "changed_binary": 1},
    }


def main():
    check_clean(os.environ)
    with tempfile.TemporaryDirectory(prefix="psyche-npm-smoke-") as directory:
        work = Path(directory).resolve()
        require(ROOT not in work.parents, "set the temporary directory outside the checkout")
        env = dict(os.environ, npm_config_cache=str(work / "npm-cache"), npm_config_update_notifier="false")
        try:
            report = exercise(work, env)
        finally:
            check_clean(env)
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        # A refusal is one message on stderr and exit 1. Other exceptions keep their traceback.
        sys.exit(f"smoke-installed-npm: {error}")
