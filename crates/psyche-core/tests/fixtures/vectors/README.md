# Canonical document vectors (experimental)

One file per registered Psyche v1 schema kind: the fifteen durable record
kinds and the unstored error envelope. `psyche.<kind>.v1.json` holds exactly
the canonical bytes of one valid, synthetic document of that kind, with no
trailing newline.

These are experimental, unpublished vectors for
[#11](https://github.com/OpenCoven/psyche/issues/11). They are not a stable
protocol artifact, no kind is classified stable, and downstream repositories
must not pin them.

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
this directory holds a file no kind claims.

## Regenerating

Do not edit a vector by hand. Change the typed construction, then run:

```sh
PSYCHE_REGENERATE_VECTORS=1 cargo test --locked -p psyche-core --test canonical_vectors -- --nocapture
```

That rewrites all sixteen files from the typed constructions and prints each
file's size and digest. The run then fails on every digest literal that no
longer matches. Changing a literal is a change to golden bytes and needs an
explicit compatibility decision under `CONTRIBUTING.md`. Without the variable
set to exactly `1`, no test writes anything.
