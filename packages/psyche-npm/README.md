# @opencoven/psyche

Thin wrapper that resolves the platform-specific Psyche binary, verifies its
SHA-256 against the recorded manifest, and execs it.

This package contains no daemon, storage, identity, graph, policy,
verification, or surface transport logic. That boundary is fixed by the Psyche
program plan and is not negotiable per-package.

**Not yet published. Publication is gated at G12.**

## What it does

`psyche <args...>` runs `bin/psyche.js`, which performs four steps and nothing
else:

1. **Resolve the platform key.** `process.platform` and `process.arch` are
   joined into a key such as `darwin-arm64`. A key outside the supported list
   below is refused with `unsupported platform: <key>` — the wrapper does not
   guess a nearest match or fall back to a source build.
2. **Look up the expected digest.** The key indexes `psyche.checksums` in this
   package's own `package.json`. A key with no entry is refused with
   `no recorded checksum for <key>`.
3. **Locate the binary.** The key names a companion package,
   `@opencoven/psyche-<key>`, which is resolved via `require.resolve` and is
   expected to contain `bin/psyche` (`bin/psyche.exe` on Windows). If that
   package is absent — the normal state when npm skipped an optional dependency
   whose platform did not match — the error names the package and says to
   reinstall without `--no-optional`, rather than surfacing a bare
   `MODULE_NOT_FOUND`.
4. **Verify, then spawn.** The file's SHA-256 must equal the recorded digest or
   the wrapper aborts without executing anything. On a match it spawns the
   binary with `stdio: 'inherit'`, forwards `process.argv.slice(2)` unchanged,
   and exits with the child's status — or `128 + signum` if the child was killed
   by a signal, which is what a shell reports for the same death.

Arguments and stdio are passed through verbatim. The wrapper parses no flags of
its own, so `psyche --help` is the Rust binary's help, and
`psyche status --json | jq` works through it.

## What the all-zero digests mean

Every entry in `psyche.checksums` is currently 64 zeros:

```json
"linux-x64": "0000000000000000000000000000000000000000000000000000000000000000"
```

This is a placeholder, and it **fails closed**. No release artifact exists yet,
so no true digest can be recorded; a run of all zero bytes is not the SHA-256 of
any file, so step 4 above rejects every real binary it is compared against. The
placeholder therefore cannot be mistaken for a disabled check — the wrapper is
inert until the digests are real, which is the intended state before G12.

The release job that builds the companion packages is what replaces these
values. A test in `test/verify-checksum.test.js` asserts both that the shipped
digests are still placeholders and that a real file never matches one.

## Scope of the integrity check

The SHA-256 comparison is the only integrity check between npm and exec. It is
a check, not a guarantee, and two limits are worth stating plainly:

- **It is a time-of-check/time-of-use check.** The digest is computed and the
  binary is spawned as two separate operations on a path. Nothing in this
  package prevents the file being replaced in between. Closing that window
  requires executing the verified file handle itself, which Node cannot express
  portably.
- **It authenticates the manifest's claim, not the publisher.** The expected
  digest travels inside this package. Anyone able to alter this package can
  alter the digest with it. The check defends against a corrupted or substituted
  *companion* package, not against a compromised wrapper. Publisher
  authentication is a registry-level concern and is not solved here.

## Supported platforms

| Key            | Companion package                  |
| -------------- | ---------------------------------- |
| `darwin-arm64` | `@opencoven/psyche-darwin-arm64`   |
| `darwin-x64`   | `@opencoven/psyche-darwin-x64`     |
| `linux-arm64`  | `@opencoven/psyche-linux-arm64`    |
| `linux-x64`    | `@opencoven/psyche-linux-x64`      |
| `win32-x64`    | `@opencoven/psyche-win32-x64`      |

Any other platform/arch pair is refused. Notably absent: `win32-arm64`, and any
32-bit target.

## Installation

**`npm install @opencoven/psyche` cannot currently succeed from a registry.**
Neither this package nor any of the five companion packages listed in
`optionalDependencies` has been published; there is nothing to install. The
dependency entries are declared ahead of the artifacts they will point at, and
their `0.0.0` versions are placeholders on the same footing as the digests.

## Development

```
npm --prefix packages/psyche-npm test   # node:test, no dependencies
npm pack ./packages/psyche-npm --dry-run
```

`npm pack` takes the package directory positionally. `npm pack --prefix <dir>`
does **not** work: unlike `npm test`, `pack` reads `package.json` from the
working directory and ignores the prefix.

The published tarball contains `bin/`, `scripts/`, `README.md`, and
`package.json` only. Tests are not shipped.
