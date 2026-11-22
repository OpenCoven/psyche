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
CONTRACTS_DIR = "crates/psyche-core/src/contracts"
CONTRACTS = f"{CONTRACTS_DIR}/mod.rs"
RECORDS = "crates/psyche-store/src/records.rs"
BINDINGS = "crates/psyche-store/src/execution_bindings.rs"
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


WORD = re.compile(r"[A-Za-z0-9_]+")
# Each pattern captures, at every position, the whole word that follows a
# declaration keyword, so a lookup in the captured set is exactly a search
# for the keyword followed by that word. The lookahead keeps overlapping
# matches such as `struct type Name`.
DECLARATIONS = {
    "def": re.compile(r"(?=\bdef\s+(\w+))"),
    "fn": re.compile(r"(?=\b(?:fn|mod)\s+(\w+))"),
    "macro": re.compile(r"(?=\bmacro_rules!\s+(\w+))"),
    "field": re.compile(r"(?=\b(\w+):(?!:))"),
    "const": re.compile(r"(?=\b(?:const|static)\s+(\w+))"),
    "type": re.compile(r"(?=\b(?:struct|enum|trait|type|union)\s+(\w+))"),
}
INDEXES = {}


class SourceIndex:
    """A file's whole words and declared names, computed once per text."""

    def __init__(self, source):
        self.source = source
        self.words = set(WORD.findall(source))
        self.found = {}

    def word(self, name):
        if WORD.fullmatch(name):
            return name in self.words
        return bool(
            re.search(
                rf"(?<![A-Za-z0-9_]){re.escape(name)}(?![A-Za-z0-9_])", self.source
            )
        )

    def declared(self, kind, name):
        if kind not in self.found:
            self.found[kind] = set(DECLARATIONS[kind].findall(self.source))
        return name in self.found[kind]


def source_index(source):
    if source not in INDEXES:
        INDEXES[source] = SourceIndex(source)
    return INDEXES[source]


def declares(source, suffix, symbol):
    """Whether a file's text declares `symbol`, by textual match only.

    Every `::` part must occur as a whole word and the last part must follow
    a declaration keyword somewhere in the file, comments and strings
    included. `Owner::member` does not tie the member to the owner.
    """
    index = source_index(source)
    parts = symbol.split("::")
    last = parts[-1]
    if not all(index.word(part) for part in parts):
        return False
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", last):
        return True
    lower = re.fullmatch(r"[a-z_][a-z0-9_]*", last)
    if suffix == ".py" and lower:
        return index.declared("def", last)
    if suffix != ".rs":
        return True
    if lower:
        # A function, test, module or macro; `Owner::member` may be a field.
        return (
            index.declared("fn", last)
            or index.declared("macro", last)
            or (len(parts) > 1 and index.declared("field", last))
        )
    if re.fullmatch(r"[A-Z][A-Z0-9_]*", last):
        return index.declared("const", last)
    if len(parts) == 1:
        return index.declared("type", last)
    return True  # `Enum::Variant`: both names occur as whole words.


SYMBOL = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*")


def codemap_references(text):
    """Yield (line, token, symbols, stray) for every backticked span in the map.

    A reference is `path` or `path` :: `A`, `B`. Spans pair left to right, so
    the symbols are exactly the spans joined to a path by ` :: ` and `, `.
    `stray` is (separator, name) when the next span looks like one more
    symbol but is joined by other spaces, colons or commas, which would
    otherwise leave it unchecked.
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
            stray = None
            if index < len(spans) and ("/" in token or symbols):
                separator = line[spans[index - 1].end() : spans[index].start()]
                if re.fullmatch(r"[\s:,]*", separator) and SYMBOL.fullmatch(
                    spans[index][1]
                ):
                    stray = (separator, spans[index][1])
            yield number, token, symbols, stray


def validate_codemap_references(root, text):
    sources = {}
    for number, token, symbols, stray in codemap_references(text):
        row = f"{CODEMAP}:{number}:"
        for span in [token] + symbols:
            require("\\" not in span, f"{row} backslash in a backticked token: {span}")
        if stray:
            separator, name = stray
            raise ValueError(
                f"{row} separator {separator!r} before {name} is neither ' :: '"
                " after a path nor ', ' between symbols"
            )
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


NUMBERS = (
    "zero one two three four five six seven eight nine ten eleven twelve thirteen"
    " fourteen fifteen sixteen seventeen eighteen nineteen twenty"
).split()
ORDINALS = {1: "first", 2: "second", 3: "third", 5: "fifth", 8: "eighth"}
ORDINALS.update({9: "ninth", 12: "twelfth", 20: "twentieth"})


def number(value):
    return NUMBERS[value] if value < len(NUMBERS) else str(value)


def ordinal(value):
    return ORDINALS.get(value, number(value) + "th")


def store_tables(root):
    """Which schema kinds `Store::insert` diverts to the binding revisions."""
    records = read(root, RECORDS)
    found = re.search(r"\n    fn insert_with_status\((.*?)\n    \}\n", records, re.S)
    named = set(re.findall(r"CanonicalDocument::(\w+)", found[1])) if found else set()
    diverted = set(
        re.findall(
            r"if let CanonicalDocument::(\w+)\(\w+\) = document \{"
            r"\s*return execution_bindings::insert\(",
            found[1] if found else "",
        )
    )
    require(
        diverted
        and named == diverted
        and "INSERT INTO canonical_records" in records
        and "INSERT INTO execution_binding_revisions" in read(root, BINDINGS),
        f"{RECORDS}: cannot parse the table routing in insert_with_status",
    )
    return diverted


def record_kinds(root):
    """Map each RecordKind to its table cells and the registry, failing closed."""
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
    listed = re.search(r"const ALL: \[SchemaKind; (\d+)\] = \[(.*?)\];", source, re.S)
    entries = re.findall(r"SchemaKind::(\w+)", listed[2]) if listed else []
    require(
        listed
        and int(listed[1]) == len(entries)
        and sorted(entries) == sorted(names),
        f"{CONTRACTS}: SchemaKind::ALL does not list exactly the SchemaKind::name arms",
    )
    documents = dict(
        re.findall(
            r"^\s+(\w+)\((\w+)\),$",
            body("CanonicalDocument", r"pub enum CanonicalDocument"),
            re.M,
        )
    )
    missing = sorted(set(names) - set(documents))
    require(
        not missing,
        f"{CONTRACTS}: CanonicalDocument has no variant for {', '.join(missing)}",
    )
    modules = {}
    for module, group, single in re.findall(
        r"^pub use (\w+)::(?:\{([^}]*)\}|(\w+));$", source, re.M
    ):
        for name in re.findall(r"\w+", group or single):
            modules[name] = module
    unexported = sorted({t for t in documents.values() if t not in modules})
    require(
        not unexported,
        f"{CONTRACTS}: no `pub use` exports {', '.join(unexported)}",
    )
    diverted = store_tables(root)

    def schema(kind):
        return f"psyche.{names[kind]}.v{major[1]}"

    def rust_type(kind):
        held = documents[kind]
        return f"`{CONTRACTS_DIR}/{modules[held]}.rs` :: `{held}`"

    kinds = {}
    for kind in variants:
        schemas = sorted((s for s, stored_as in stored if stored_as == kind), key=schema)
        kinds[kind] = {
            "prefix": prefixes[kind],
            "schemas": [schema(s) for s in schemas],
            "types": "; ".join(rust_type(s) for s in schemas),
            "tables": ", ".join(
                sorted(
                    {
                        "`execution_binding_revisions`"
                        if s in diverted
                        else "`canonical_records`"
                        for s in schemas
                    }
                )
            ),
        }
    unstored = [s for s in names if s not in {s for s, _ in stored}]
    registry = {
        "variants": len(variants),
        "schemas": len(names),
        "unstored": [(schema(s), documents[s]) for s in unstored],
    }
    return kinds, registry


def cell(text):
    """A table cell as written, without backticks when it is one code span."""
    found = re.fullmatch(r"`([^`]*)`", text)
    return found[1] if found else text


def validate_codemap_kinds(root, text):
    where = f"{CODEMAP} record kinds:"
    section = re.search(r"^## Record kinds\n(.*?)(?=^## |\Z)", text, re.S | re.M)
    require(section, f"{where} section not found")
    rows = {}
    for kind, rest in re.findall(
        r"^\| `([A-Z][A-Za-z0-9]*)` \| (`.*) \|$", section[1], re.M
    ):
        require(kind not in rows, f"{where} duplicate row {kind}")
        cells = rest.split(" | ")
        require(len(cells) == 4, f"{where} {kind} row does not have five columns")
        rows[kind] = cells
    require(rows, f"{where} table not found")
    expected, registry = record_kinds(root)
    missing = sorted(set(expected) - set(rows))
    extra = sorted(set(rows) - set(expected))
    require(not missing, f"{where} missing {', '.join(missing)}")
    require(not extra, f"{where} extra {', '.join(extra)}")
    for kind, want in expected.items():
        prefix, schemas, types, tables = rows[kind]
        require(
            cell(prefix) == want["prefix"],
            f"{where} {kind} prefix is {cell(prefix)},"
            f" RecordKind::prefix says {want['prefix']}",
        )
        schemas = sorted(re.findall(r"`([^`]*)`", schemas))
        require(
            schemas == want["schemas"],
            f"{where} {kind} schema is {', '.join(schemas)},"
            f" the registry says {', '.join(want['schemas'])}",
        )
        require(
            types == want["types"],
            f"{where} {kind} Rust type is {types}, CanonicalDocument holds {want['types']}",
        )
        require(
            tables == want["tables"],
            f"{where} {kind} table is {cell(tables)},"
            f" the store writes it to {cell(want['tables'])}",
        )
    sentence = re.search(
        r"^(\w+) `RecordKind` variants, (\w+) schema strings\."
        r" The (\w+), `([^`]*)`, decodes to `(\w+)` and is never stored\.$",
        section[1],
        re.M,
    )
    require(sentence, f"{where} count sentence not found")
    variants, schemas, nth, string, held = sentence.groups()
    require(
        variants.lower() == number(registry["variants"]),
        f"{where} says {variants} RecordKind variants, RecordKind has"
        f" {registry['variants']}",
    )
    require(
        schemas.lower() == number(registry["schemas"]),
        f"{where} says {schemas} schema strings, SchemaKind::name has"
        f" {registry['schemas']}",
    )
    unstored = [
        f"the {ordinal(registry['schemas'])}, {name}, decodes to {document}"
        for name, document in registry["unstored"]
    ]
    require(
        unstored == [f"the {nth}, {string}, decodes to {held}"],
        f"{where} says the {nth}, {string}, decodes to {held} and is never stored;"
        f" the registry says {'; '.join(unstored) or 'every schema is stored'}",
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
    except Exception as exc:
        # A check removed upstream of a lookup surfaces here as a crash.
        raise AssertionError(
            f"{label} crashed instead of failing with {expected!r}:"
            f" {type(exc).__name__}: {exc}"
        ) from exc
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
                    CODEMAP,
                    "| `crates/psyche-core/src/contracts/foundation.rs` :: `Budget` |",
                    "| `crates/psyche-core/src/contracts/foundation.rs` :: `Approval` |",
                    CODEMAP
                    + " record kinds: Budget Rust type is"
                    + " `crates/psyche-core/src/contracts/foundation.rs` :: `Approval`,"
                    + " CanonicalDocument holds"
                    + " `crates/psyche-core/src/contracts/foundation.rs` :: `Budget`",
                    True,
                ),
                (
                    CODEMAP,
                    "`ExecutionBinding` | `execution_binding_revisions` |",
                    "`ExecutionBinding` | `canonical_records` |",
                    CODEMAP
                    + " record kinds: Attempt table is canonical_records,"
                    + " the store writes it to execution_binding_revisions",
                    True,
                ),
                (
                    CODEMAP,
                    "Fifteen `RecordKind` variants",
                    "Sixteen `RecordKind` variants",
                    CODEMAP
                    + " record kinds: says Sixteen RecordKind variants, RecordKind has 15",
                    True,
                ),
                (
                    CODEMAP,
                    "variants, sixteen schema strings.",
                    "variants, seventeen schema strings.",
                    CODEMAP
                    + " record kinds: says seventeen schema strings,"
                    + " SchemaKind::name has 16",
                    True,
                ),
                (
                    CODEMAP,
                    "decodes to `ErrorEnvelope` and is never stored.",
                    "decodes to `Intent` and is never stored.",
                    CODEMAP
                    + " record kinds: says the sixteenth, psyche.error.v1, decodes to"
                    + " Intent and is never stored; the registry says the sixteenth,"
                    + " psyche.error.v1, decodes to ErrorEnvelope",
                    True,
                ),
                (
                    CODEMAP,
                    "Fifteen `RecordKind` variants, sixteen",
                    "Fifteen RecordKind variants, sixteen",
                    CODEMAP + " record kinds: count sentence not found",
                    True,
                ),
                (
                    CODEMAP,
                    "| `bud_` | `psyche.budget.v1` |",
                    "| `bud_` `psyche.budget.v1` |",
                    CODEMAP + " record kinds: Budget row does not have five columns",
                    True,
                ),
                (
                    CONTRACTS,
                    "        SchemaKind::Addon,\n",
                    "",
                    CONTRACTS
                    + ": SchemaKind::ALL does not list exactly the SchemaKind::name arms",
                    True,
                ),
                (
                    CONTRACTS,
                    '            SchemaKind::Addon => "addon",\n',
                    "",
                    CONTRACTS + ": RecordKind, prefix and schema registry disagree",
                    True,
                ),
                (
                    CONTRACTS,
                    "const SUPPORTED_MAJOR: u16 = 1;",
                    "const SUPPORTED_MAJOR: u16 = ONE;",
                    CONTRACTS + ": cannot parse SUPPORTED_MAJOR",
                    True,
                ),
                (
                    CONTRACTS,
                    "    Addon(Addon),\n",
                    "",
                    CONTRACTS + ": CanonicalDocument has no variant for Addon",
                    True,
                ),
                (
                    CONTRACTS,
                    "pub use foundation::{Addon, ",
                    "pub use foundation::{",
                    CONTRACTS + ": no `pub use` exports Addon",
                    True,
                ),
                (
                    RECORDS,
                    "return execution_bindings::insert(",
                    "return execution_bindings::insert_binding(",
                    RECORDS + ": cannot parse the table routing in insert_with_status",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/digest.rs` :: `canonical_bytes`",
                    "`crates\\psyche-core\\src\\digest.rs` :: `canonical_bytes`",
                    CODEMAP
                    + ":{line}: backslash in a backticked token:"
                    + " crates\\psyche-core\\src\\digest.rs",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/digest.rs` :: `canonical_bytes`",
                    "`crates/psyche-core/src/digest.rs`::`canonical_octets`",
                    CODEMAP
                    + ":{line}: separator '::' before canonical_octets is neither"
                    + " ' :: ' after a path nor ', ' between symbols",
                    True,
                ),
                (
                    CODEMAP,
                    "`canonical_bytes`, `digest`, `Sha256Digest`,",
                    "`canonical_bytes`,`canonical_octets`, `Sha256Digest`,",
                    CODEMAP
                    + ":{line}: separator ',' before canonical_octets is neither"
                    + " ' :: ' after a path nor ', ' between symbols",
                    True,
                ),
                (
                    CODEMAP,
                    "`crates/psyche-core/src/digest.rs` :: `canonical_bytes`",
                    "`crates/psyche-core/src/` :: `canonical_bytes`",
                    CODEMAP + ":{line}: symbols need a file: crates/psyche-core/src/",
                    True,
                ),
                # One control per declaration rule. Each name occurs in the
                # file as a whole word, so only the rule itself rejects it.
                (
                    CODEMAP,
                    "`compare`, `observation` compares",
                    "`compare`, `hashlib` compares",
                    CODEMAP
                    + ":{line}: symbol hashlib is not declared in"
                    + " profiles/receipt-replay/run.py",
                    True,
                ),
                (
                    CODEMAP,
                    "`Sha256Digest::parse`",
                    "`Sha256Digest::serde_json_canonicalizer`",
                    CODEMAP
                    + ":{line}: symbol Sha256Digest::serde_json_canonicalizer is not"
                    + " declared in crates/psyche-core/src/digest.rs",
                    True,
                ),
                (
                    CODEMAP,
                    "Registry and decoder: `crates/psyche-core/src/contracts/mod.rs`"
                    + " :: `RecordKind`, `RecordKind::ALL`,",
                    "Registry and decoder: `crates/psyche-core/src/contracts/mod.rs`"
                    + " :: `RecordKind`, `RecordKind::ULID`,",
                    CODEMAP
                    + ":{line}: symbol RecordKind::ULID is not declared in "
                    + CONTRACTS,
                    True,
                ),
                (
                    CODEMAP,
                    "`digest`, `Sha256Digest`, `Sha256Digest::parse`",
                    "`digest`, `Serialize`, `Sha256Digest::parse`",
                    CODEMAP
                    + ":{line}: symbol Serialize is not declared in"
                    + " crates/psyche-core/src/digest.rs",
                    True,
                ),
                (
                    ARCHITECTURE,
                    "```text\n",
                    "```txt\n",
                    ARCHITECTURE + " diagram not found",
                    True,
                ),
                (
                    ARCHITECTURE,
                    "psyche-core <- psyche-config\n",
                    "psyche-core -> psyche-config\n",
                    ARCHITECTURE
                    + " diagram has an unreadable line: psyche-core -> psyche-config",
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
