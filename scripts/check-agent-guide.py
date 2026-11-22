"""Check the contributor entrypoints, code map and crate diagram against the code."""

import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HELP = "cargo run --locked -p psyche-cli --bin psyche -- --help"
ARCHITECTURE = "docs/ARCHITECTURE.md"
CODEMAP = "docs/CODEMAP.md"
CONTRACTS = "crates/psyche-core/src/contracts/mod.rs"
POISON = ("GIT_DIR", "GIT_WORK_TREE")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def child_env():
    # An inherited GIT_DIR or GIT_WORK_TREE redirects Git away from the
    # directory a command names. No child process may see any GIT_* value.
    return {
        key: value
        for key, value in os.environ.items()
        if not key.upper().startswith("GIT_")
    }


def run(command, cwd, capture=False):
    stdout = subprocess.PIPE if capture else None
    done = subprocess.run(
        command, cwd=cwd, env=child_env(), check=True, stdout=stdout
    )
    return done.stdout.decode("utf-8") if capture else None


def read(root, path):
    return (root / path).read_text(encoding="utf-8")


def declares(source, suffix, symbol):
    """Whether a file's text declares `symbol`; `Owner::member` checks both."""

    def word(name):
        return re.search(
            rf"(?<![A-Za-z0-9_]){re.escape(name)}(?![A-Za-z0-9_])", source
        )

    parts = symbol.split("::")
    last = parts[-1]
    if not all(word(part) for part in parts):
        return False
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", last):
        return True
    lower = re.fullmatch(r"[a-z_][a-z0-9_]*", last)
    if suffix == ".py" and lower:
        return bool(re.search(rf"\bdef\s+{last}\b", source))
    if suffix != ".rs":
        return True
    if lower:
        # A function, test, module or macro; `Owner::member` may be a field.
        field = rf"|\b{last}:(?!:)" if len(parts) > 1 else ""
        pattern = rf"\b(?:fn|mod)\s+{last}\b|\bmacro_rules!\s+{last}\b{field}"
    elif re.fullmatch(r"[A-Z][A-Z0-9_]*", last):
        pattern = rf"\b(?:const|static)\s+{last}\b"
    elif len(parts) == 1:
        pattern = rf"\b(?:struct|enum|trait|type|union)\s+{last}\b"
    else:
        return True  # `Enum::Variant`: both names occur as whole words.
    return bool(re.search(pattern, source))


def codemap_references(text):
    """Yield (line, token, symbols) for every backticked span in the map.

    A reference is `path` or `path` :: `A`, `B`. Spans pair left to right, so
    the symbols are exactly the spans joined to a path by ` :: ` and `, `.
    """
    for number, line in enumerate(text.splitlines(), 1):
        spans = list(re.finditer(r"`([^`]*)`", line))
        index = 0
        while index < len(spans):
            token = spans[index][1]
            index += 1
            symbols = []
            joiner = " :: "
            while (
                index < len(spans)
                and line[spans[index - 1].end() : spans[index].start()] == joiner
            ):
                symbols.append(spans[index][1])
                index += 1
                joiner = ", "
            yield number, token, symbols


def validate_codemap_references(root, text):
    sources = {}
    for number, token, symbols in codemap_references(text):
        row = f"{CODEMAP}:{number}:"
        if "/" not in token:
            require(not symbols, f"{row} `::` must follow a repository path: {token}")
            continue
        path = PurePosixPath(token)
        require(
            not path.is_absolute() and ".." not in path.parts,
            f"{row} path leaves the repository: {token}",
        )
        target = root / token
        require(target.exists(), f"{row} path does not exist: {token}")
        if not symbols:
            continue
        require(target.is_file(), f"{row} symbols need a file: {token}")
        if token not in sources:
            sources[token] = target.read_text(encoding="utf-8")
        for symbol in symbols:
            require(
                declares(sources[token], path.suffix, symbol),
                f"{row} symbol {symbol} is not declared in {token}",
            )


def record_kinds(root):
    """Map each RecordKind to its prefix and schema strings, failing closed."""
    source = read(root, CONTRACTS)

    def body(name, signature):
        found = re.search(signature + r" \{\n(.*?)\n\s*\}", source, re.S)
        require(found, f"{CONTRACTS}: cannot parse {name}")
        return found[1]

    arms = r" \{\s*match self"
    variants = re.findall(
        r"^\s+([A-Z][A-Za-z0-9]*),$",
        body("RecordKind", r"pub enum RecordKind"),
        re.M,
    )
    prefixes = dict(
        re.findall(
            r'RecordKind::(\w+) => "([^"]*)"',
            body(
                "RecordKind::prefix",
                r"pub const fn prefix\(self\) -> &'static str" + arms,
            ),
        )
    )
    names = dict(
        re.findall(
            r'SchemaKind::(\w+) => "([^"]*)"',
            body("SchemaKind::name", r"const fn name\(self\) -> &'static str" + arms),
        )
    )
    stored = re.findall(
        r"SchemaKind::(\w+) => Some\(RecordKind::(\w+)\)",
        body(
            "SchemaKind::record_kind",
            r"pub const fn record_kind\(self\) -> Option<RecordKind>" + arms,
        ),
    )
    major = re.search(r"^const SUPPORTED_MAJOR: u16 = (\d+);$", source, re.M)
    require(major, f"{CONTRACTS}: cannot parse SUPPORTED_MAJOR")
    require(
        variants
        and len(set(variants)) == len(variants)
        and set(variants) == set(prefixes) == {kind for _, kind in stored}
        and all(schema in names for schema, _ in stored),
        f"{CONTRACTS}: RecordKind, prefix and schema registry disagree",
    )
    return {
        kind: (
            prefixes[kind],
            sorted(
                f"psyche.{names[schema]}.v{major[1]}"
                for schema, stored_as in stored
                if stored_as == kind
            ),
        )
        for kind in variants
    }


def validate_codemap_kinds(root, text):
    where = f"{CODEMAP} record kinds:"
    section = re.search(r"^## Record kinds\n(.*?)(?=^## |\Z)", text, re.S | re.M)
    require(section, f"{where} section not found")
    rows = {}
    for kind, prefix, schemas in re.findall(
        r"^\| `([A-Z][A-Za-z0-9]*)` \| `([^`]*)` \| (`[^|]*`) \|",
        section[1],
        re.M,
    ):
        require(kind not in rows, f"{where} duplicate row {kind}")
        rows[kind] = (prefix, sorted(re.findall(r"`([^`]*)`", schemas)))
    require(rows, f"{where} table not found")
    expected = record_kinds(root)
    missing = sorted(set(expected) - set(rows))
    extra = sorted(set(rows) - set(expected))
    require(not missing, f"{where} missing {', '.join(missing)}")
    require(not extra, f"{where} extra {', '.join(extra)}")
    for kind, (prefix, schemas) in expected.items():
        require(
            rows[kind][0] == prefix,
            f"{where} {kind} prefix is {rows[kind][0]}, RecordKind::prefix says {prefix}",
        )
        require(
            rows[kind][1] == schemas,
            f"{where} {kind} schema is {', '.join(rows[kind][1])},"
            f" the registry says {', '.join(schemas)}",
        )


def validate_architecture(root, crates, edges):
    """Compare the dependency diagram with Cargo's intra-workspace edges.

    `A + B <- C <- D` reads: C depends on A and B, and D depends on C. A line
    holding one crate name declares a crate without workspace dependencies.
    """
    where = f"{ARCHITECTURE} diagram"
    block = re.search(r"^```text\n(.*?)^```$", read(root, ARCHITECTURE), re.S | re.M)
    require(block and block[1].strip(), f"{where} not found")
    drawn = set()
    named = set()
    for line in block[1].splitlines():
        groups = [
            [name.strip() for name in group.split("+")] for group in line.split("<-")
        ]
        names = [name for group in groups for name in group]
        require(
            all(re.fullmatch(r"[a-z][a-z0-9-]*", name) for name in names),
            f"{where} has an unreadable line: {line}",
        )
        named.update(names)
        for dependencies, consumers in zip(groups, groups[1:]):
            drawn.update((c, d) for c in consumers for d in dependencies)
    unknown = sorted(named - crates)
    require(not unknown, f"{where} names unknown crate {', '.join(unknown)}")
    absent = sorted(crates - named)
    require(not absent, f"{where} omits workspace crate {', '.join(absent)}")

    def listed(pairs):
        return ", ".join(f"{c} -> {d}" for c, d in sorted(pairs)) or "none"

    require(
        drawn == edges,
        f"{where} differs from Cargo: missing {listed(edges - drawn)};"
        f" extra {listed(drawn - edges)}",
    )


def validate(root, members, pin, crates, edges):
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
    require(f"`{CODEMAP}`" in read(root, "AGENTS.md"), "AGENTS.md code map pointer")
    codemap = read(root, CODEMAP)
    validate_codemap_references(root, codemap)
    validate_codemap_kinds(root, codemap)
    validate_architecture(root, crates, edges)


def reject(check, label, expected, exact):
    try:
        check()
    except ValueError as exc:
        matched = str(exc) == expected if exact else expected in str(exc)
        require(matched, f"unexpected failure: {exc}")
        print(f"rejected {label}: {exc}")
    else:
        raise AssertionError(f"accepted drift in {label}")


def main():
    metadata = json.loads(
        run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            ROOT,
            capture=True,
        )
    )
    packages = [
        p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]
    ]
    members = {
        Path(p["manifest_path"]).parent.relative_to(ROOT).as_posix() for p in packages
    }
    crates = {p["name"] for p in packages}
    # Normal, build and dev dependencies all constrain the crate graph.
    edges = {
        (p["name"], d["name"])
        for p in packages
        for d in p["dependencies"]
        if d["name"] in crates and d.get("source") is None
    }
    pin = re.search(
        r'^channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.M
    )[1]
    validate(ROOT, members, pin, crates, edges)
    print(
        f"checked {len(members)} workspace members, toolchain, commands, linked paths,"
        f" {len(edges)} dependency edges and the code map"
    )
    inherited = {key: os.environ.get(key) for key in POISON}
    with tempfile.TemporaryDirectory() as scratch, tempfile.TemporaryDirectory() as decoy:
        fixture = Path(scratch)
        try:
            # Export a hostile GIT_DIR and GIT_WORK_TREE for every command
            # below. A child that inherited them would act on the decoy.
            run(["git", "init", "-q", decoy], ROOT)
            os.environ["GIT_DIR"] = str(Path(decoy, ".git"))
            os.environ["GIT_WORK_TREE"] = decoy
            seen = run(["git", "rev-parse", "--absolute-git-dir"], ROOT, capture=True)
            if Path(seen.strip()).resolve() == Path(decoy, ".git").resolve():
                raise AssertionError("a child process inherited GIT_DIR")
            # Hosted Windows checks out on D: and places temporary files on C:.
            # Copy the Git objects so this fixture also works across volumes.
            clone = ["git", "clone", "-q", "--no-hardlinks", "--no-checkout"]
            run(clone + [str(ROOT), str(fixture)], ROOT)
            run(["git", "checkout", "-q", "--detach", "HEAD"], fixture)
            row = re.compile(r"^\| `Addon` \|.*\n", re.M)
            ghost = r"\g<0>| `Ghost` | `gho_` | `psyche.ghost.v1` | none | none |\n"
            cases = [
                ("scripts/agent-bootstrap", None, None, "missing agent-bootstrap", False),
                (
                    "AGENTS.md",
                    "crates/psyche-config/**",
                    "crates/psyche-config-old/**",
                    "workspace inventory",
                    False,
                ),
                (
                    "README.md",
                    HELP,
                    "cargo run -p psyche-cli -- --help",
                    "operator binary",
                    False,
                ),
                (
                    "AGENTS.md",
                    f"`{CODEMAP}`",
                    "`docs/CODEMAP-old.md`",
                    "AGENTS.md code map pointer",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/id.rs` :: ",
                    "`crates/psyche-core/src/ids.rs` :: ",
                    CODEMAP
                    + ":{line}: path does not exist: crates/psyche-core/src/ids.rs",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/id.rs` :: ",
                    "`../psyche-core/src/id.rs` :: ",
                    CODEMAP
                    + ":{line}: path leaves the repository: ../psyche-core/src/id.rs",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/digest.rs` :: `canonical_bytes`",
                    "`digest.rs` :: `canonical_bytes`",
                    CODEMAP + ":{line}: `::` must follow a repository path: digest.rs",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/digest.rs` :: `canonical_bytes`",
                    "`crates/psyche-core/src/digest.rs` :: `canonical_octets`",
                    CODEMAP
                    + ":{line}: symbol canonical_octets is not declared in"
                    + " crates/psyche-core/src/digest.rs",
                    True,
                ),
                (CODEMAP, row, "", CODEMAP + " record kinds: missing Addon", True),
                (CODEMAP, row, ghost, CODEMAP + " record kinds: extra Ghost", True),
                (
                    CODEMAP,
                    row,
                    r"\g<0>\g<0>",
                    CODEMAP + " record kinds: duplicate row Addon",
                    True,
                ),
                (
                    CODEMAP,
                    "\n| `",
                    "\n|`",
                    CODEMAP + " record kinds: table not found",
                    True,
                ),
                (
                    CODEMAP,
                    "## Record kinds\n",
                    "## Kinds\n",
                    CODEMAP + " record kinds: section not found",
                    True,
                ),
                (
                    CONTRACTS,
                    "pub const fn prefix(self) -> &'static str {",
                    "pub const fn prefix(&self) -> &'static str {",
                    CONTRACTS + ": cannot parse RecordKind::prefix",
                    True,
                ),
                (
                    CONTRACTS,
                    'RecordKind::Addon => "adn_",',
                    "",
                    CONTRACTS + ": RecordKind, prefix and schema registry disagree",
                    True,
                ),
                (
                    CODEMAP,
                    "| `Intent` | `int_` |",
                    "| `Intent` | `itn_` |",
                    CODEMAP
                    + " record kinds: Intent prefix is itn_, RecordKind::prefix says int_",
                    True,
                ),
                (
                    CODEMAP,
                    "| `adn_` | `psyche.addon.v1` |",
                    "| `adn_` | `psyche.addons.v1` |",
                    CODEMAP
                    + " record kinds: Addon schema is psyche.addons.v1,"
                    + " the registry says psyche.addon.v1",
                    True,
                ),
                (
                    ARCHITECTURE,
                    "psyche-core <- psyche-config\n",
                    "psyche-config <- psyche-core\n",
                    ARCHITECTURE
                    + " diagram differs from Cargo: missing psyche-config -> psyche-core;"
                    + " extra psyche-core -> psyche-config",
                    True,
                ),
                (
                    ARCHITECTURE,
                    "psyche-core <- psyche-store\n",
                    "psyche-core <- psyche-store\npsyche-core <- psyche-ghost\n",
                    ARCHITECTURE + " diagram names unknown crate psyche-ghost",
                    True,
                ),
            ]
            validate(fixture, members, pin, crates, edges)
            for path, old, replacement, expected, exact in cases:
                target = fixture / path
                if old is None:
                    target.unlink()
                else:
                    source = target.read_text(encoding="utf-8")
                    if hasattr(old, "search"):
                        found = old.search(source)
                        start = found.start() if found else -1
                        mutated = old.sub(replacement, source)
                    else:
                        start = source.find(old)
                        mutated = source.replace(old, replacement)
                    require(start >= 0, "mutation precondition")
                    expected = expected.replace(
                        "{line}", str(source.count("\n", 0, start) + 1)
                    )
                    target.write_text(mutated, encoding="utf-8")
                reject(
                    lambda: validate(fixture, members, pin, crates, edges),
                    f"{path} mutation",
                    expected,
                    exact,
                )
                run(["git", "restore", "--", path], fixture)
            reject(
                lambda: validate_architecture(fixture, crates | {"psyche-ninth"}, edges),
                "a ninth workspace crate",
                ARCHITECTURE + " diagram omits workspace crate psyche-ninth",
                True,
            )
            require(
                all(os.environ.get(key) for key in POISON)
                and [entry.name for entry in Path(decoy).iterdir()] == [".git"]
                and not Path(decoy, ".git", "index").exists(),
                "inherited GIT_* values reached a fixture command",
            )
            print("rejected inherited GIT_DIR and GIT_WORK_TREE: the decoy is untouched")
        finally:
            for key, value in inherited.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value


if __name__ == "__main__":
    main()
