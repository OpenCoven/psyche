# Canonical document vectors (experimental)

One file per registered Psyche v1 schema kind: the fifteen durable record
kinds and the unstored error envelope. `psyche.<kind>.v1.json` holds exactly
the canonical bytes of one valid, synthetic document of that kind, with no
trailing newline.

These are experimental, unpublished vectors for
[#11](https://github.com/OpenCoven/psyche/issues/11). They are not a stable
protocol artifact, no kind is classified stable, and downstream repositories
must not pin them.

## What the vectors are and are not

They are encode/decode vectors: each file decodes with `decode_document`,
passes its kind's validation, and re-encodes to the same bytes. They are not
store-acceptable records and not a consistent data set.

- Every repeated-nibble digest, such as `intent.digest` (`sha256:111…`) or
  `execution_binding.previous_revision_digest` (`sha256:333…`), is a
  placeholder that binds nothing.
- Only `effect_digest` in the `surface_effect` and `delivery` vectors binds
  real content: it is the SHA-256 of that vector's canonical `effect`, and
  the decoder re-checks it.
- IDs do not resolve across vectors. For example, the intent vector's
  `surface_event_id` is not the surface event vector's ID.

## What produced them

- Documents: the typed constructions in `construct` in
  `crates/psyche-core/tests/canonical_vectors.rs`. Every value is synthetic.
- Encoder: `psyche_core::digest::canonical_bytes`, which is RFC 8785 (JSON
  Canonicalization Scheme) through the workspace's `serde_json_canonicalizer`
  dependency at the version locked in `Cargo.lock`.
- Digest: SHA-256 over those bytes, `psyche_core::digest::digest`.

The pin is the digest literal per kind in `pinned_digest` in the same test
file, not this directory. The test fails if a file, a typed construction, the
encoder or a literal changes without the others, if a kind has no file, or if
this directory holds a file no kind claims. Besides the vectors and this
README, the directory test ignores exactly two untracked operating-system
names, `.DS_Store` and `Thumbs.db`; any other entry fails it.

## Regenerating

Do not edit a vector by hand. Change the typed construction, then run:

```sh
PSYCHE_REGENERATE_VECTORS=1 cargo test --locked -p psyche-core --test canonical_vectors -- --nocapture
```

The run encodes all sixteen typed constructions, compares each with the file
on disk, and writes only the files that are missing or differ. It prints each
file's size, digest and action (`unchanged`, `changed` or `created`) and the
`pinned_digest` arms for the current constructions.

- If any file was created or changed, every test fails on purpose with a
  message naming each such file and whether its digest literal is now stale.
  A regenerating run never passes over bytes it wrote. Review the diff,
  re-pin the stale literals, and re-run without the variable.
- If no file changed, nothing was written and the run is the same check as
  one without the variable. It passes only if every literal matches, and
  fails on any stale literal as a normal run does.

Without the variable set to exactly `1`, no test writes anything; any other
value fails the run.

While #11 is experimental and no kind is classified stable, changing a vector
means regenerating it and re-pinning its literal in the same change, under R4
review. Once a kind is classified stable, its vector's bytes are golden bytes,
and changing them needs an explicit compatibility decision under
`CONTRIBUTING.md`.
