"""Check the contributor entrypoints against Cargo and their documented contract."""

import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HELP = "cargo run --locked -p psyche-cli --bin psyche -- --help"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate(root, members, pin):
    manifest = (root / "agent/manifest.yaml").read_text(encoding="utf-8")
    readme = (root / "README.md").read_text(encoding="utf-8")
    require(
        f"  rust: {pin}" in manifest.splitlines() and f"**{pin}**" in readme,
        "Rust pin mismatch",
    )
    require(HELP in readme.splitlines(), "missing explicit operator binary")
    for line in readme.splitlines():
        if line.startswith("cargo run") and "psyche-cli" in line.split():
            require("--bin" in line.split(), "ambiguous operator binary")
    require("  entrypoint: AGENTS.md" in manifest.splitlines(), "manifest entrypoint")
    for key, command in [
        ("bootstrap", "./scripts/agent-bootstrap"),
        ("fast", "./scripts/agent-check fast"),
        ("full", "./scripts/agent-check full"),
    ]:
        helper = root / command.split()[0]
        require(helper.is_file(), f"missing {helper.name}")
        require(
            any(line.strip() == f"{key}: {command}" for line in manifest.splitlines()),
            f"manifest {key}",
        )
        require(command in readme.splitlines(), f"README {key}")
    for name in ("README.md", "AGENTS.md"):
        doc = (root / name).read_text(encoding="utf-8")
        inventory = set(re.findall(r"^\| `(crates/[^/`]+)(?:/\*\*)?` \|", doc, re.M))
        require(members == inventory, f"{name} workspace inventory")
        for link in re.findall(r"\[[^\]]+\]\(([^\s)]+)\)", doc):
            if "://" not in link and not link.startswith("#"):
                require((root / link.split("#")[0]).exists(), f"broken path {link}")


def main():
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT,
        )
    )
    members = {
        Path(p["manifest_path"]).parent.relative_to(ROOT).as_posix()
        for p in metadata["packages"]
        if p["id"] in metadata["workspace_members"]
    }
    pin = re.search(
        r'^channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.M
    )[1]
    validate(ROOT, members, pin)
    print(
        f"checked {len(members)} workspace members, toolchain, commands and linked paths"
    )
    with tempfile.TemporaryDirectory() as scratch:
        fixture = Path(scratch)
        # Hosted Windows checks out on D: and places temporary files on C:.
        # Copy the Git objects so this fixture also works across volumes.
        subprocess.run(
            ["git", "clone", "-q", "--no-hardlinks", "--no-checkout", str(ROOT), str(fixture)],
            check=True,
        )
        subprocess.run(
            ["git", "checkout", "-q", "--detach", "HEAD"], cwd=fixture, check=True
        )
        cases = [
            ("scripts/agent-bootstrap", None, None, "missing agent-bootstrap"),
            (
                "AGENTS.md",
                "crates/psyche-config/**",
                "crates/psyche-config-old/**",
                "workspace inventory",
            ),
            ("README.md", HELP, "cargo run -p psyche-cli -- --help", "operator binary"),
        ]
        for path, old, replacement, expected in cases:
            target = fixture / path
            if old is None:
                target.unlink()
            else:
                source = target.read_text(encoding="utf-8")
                require(old in source, "mutation precondition")
                target.write_text(source.replace(old, replacement), encoding="utf-8")
            try:
                validate(fixture, members, pin)
            except ValueError as exc:
                require(expected in str(exc), f"unexpected failure: {exc}")
                print(f"rejected {path} mutation: {exc}")
            else:
                raise AssertionError(f"accepted drift in {path}")
            subprocess.run(["git", "restore", "--", path], cwd=fixture, check=True)


if __name__ == "__main__":
    main()
