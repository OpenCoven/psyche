const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');

const { verifyChecksum, resolvePackageName, SUPPORTED } = require('../scripts/verify-checksum.js');
const manifest = require('../package.json');

function tempFile(contents) {
  const p = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'psyche-')), 'bin');
  fs.writeFileSync(p, contents);
  return p;
}

test('accepts a binary whose digest matches', () => {
  const file = tempFile('pretend-binary');
  const digest = crypto.createHash('sha256').update('pretend-binary').digest('hex');
  assert.doesNotThrow(() => verifyChecksum(file, digest));
});

test('rejects a substituted binary', () => {
  const file = tempFile('tampered');
  const wrong = crypto.createHash('sha256').update('original').digest('hex');
  assert.throws(() => verifyChecksum(file, wrong), /checksum mismatch/);
});

test('rejects a missing binary rather than exec-ing nothing', () => {
  assert.throws(() => verifyChecksum('/nonexistent/psyche', 'deadbeef'), /not found/);
});

test('maps platform and arch to the companion package name', () => {
  assert.strictEqual(resolvePackageName('darwin', 'arm64'), '@opencoven/psyche-darwin-arm64');
  assert.strictEqual(resolvePackageName('linux', 'x64'), '@opencoven/psyche-linux-x64');
  assert.throws(() => resolvePackageName('sunos', 'sparc'), /unsupported platform/);
});

// --- the shipped manifest ----------------------------------------------------

const ZERO_DIGEST = '0'.repeat(64);

test('the placeholder digests fail closed rather than disabling the check', () => {
  // No release artifact exists at this gate, so no real digest can be recorded
  // and every entry is all-zero. That is a *closed* door, not an open one: a
  // placeholder is not expected to match any release artifact, so
  // `verifyChecksum` rejects the binaries tested against it. The release job
  // that builds the companion packages is what replaces these. If this test
  // ever has to change, the change is "the digests are real now" — never "the
  // check is off".
  const shipped = Object.entries(manifest.psyche.checksums);
  assert.ok(shipped.length > 0);
  for (const [key, digest] of shipped) {
    assert.strictEqual(digest, ZERO_DIGEST, `${key} should still be a placeholder`);
  }

  // The property the placeholder relies on, asserted rather than assumed.
  const file = tempFile('any real binary at all');
  assert.throws(() => verifyChecksum(file, ZERO_DIGEST), /checksum mismatch/);
  // Empty files included: sha256("") is e3b0c442..., not zeros.
  const empty = tempFile('');
  assert.throws(() => verifyChecksum(empty, ZERO_DIGEST), /checksum mismatch/);
});

test('every supported platform has a manifest entry and a companion dependency', () => {
  // A platform in SUPPORTED but absent from the manifest resolves far enough to
  // pass `resolvePackageName` and then dies on "no recorded checksum" — at the
  // user's terminal, not here. Keep the three lists in lockstep.
  const platforms = [...SUPPORTED].sort();
  assert.deepStrictEqual(Object.keys(manifest.psyche.checksums).sort(), platforms);
  assert.deepStrictEqual(
    Object.keys(manifest.optionalDependencies).sort(),
    platforms.map((p) => `@opencoven/psyche-${p}`)
  );
});

// --- the wrapper's own logic -------------------------------------------------
//
// `bin/psyche.js` holds the entire user-facing path: platform resolution,
// checksum lookup, package resolution, spawn, and exit-code mapping. Left in the
// bin entry point it is reachable only by spawning the whole wrapper, which
// needs the companion packages installed — so in practice it would ship with no
// coverage at all. `resolveBinary` and `exitCodeFor` are therefore exported from
// a module and the bin file is a shim over them. The companion-package lookup is
// injected so these run on a machine where nothing has been published.

const { resolveBinary, exitCodeFor } = require('../scripts/resolve-binary.js');

const MANIFEST = {
  psyche: { checksums: { 'linux-x64': 'abc', 'darwin-arm64': 'def' } },
};

test('resolves the binary inside the companion package', () => {
  const found = resolveBinary('linux', 'x64', MANIFEST, () => '/pkgs/psyche-linux-x64/package.json');
  assert.strictEqual(found.binary, path.join('/pkgs/psyche-linux-x64', 'bin', 'psyche'));
  assert.strictEqual(found.expected, 'abc');
});

test('appends .exe only on Windows', () => {
  const manifest = { psyche: { checksums: { 'win32-x64': 'ghi' } } };
  const found = resolveBinary('win32', 'x64', manifest, () => '/pkgs/psyche-win32-x64/package.json');
  assert.ok(found.binary.endsWith('psyche.exe'));
});

/** A resolver that fails the way Node's really does, carrying `code`. */
function failsWith(code) {
  return () => {
    const err = new Error(`stand-in for ${code}`);
    err.code = code;
    throw err;
  };
}

test('names the missing companion package rather than surfacing MODULE_NOT_FOUND', () => {
  // The bare `require.resolve` failure reads
  // "Cannot find module '@opencoven/psyche-linux-x64/package.json'", which tells
  // an operator nothing about what to do. npm skips optional dependencies whose
  // platform does not match, so this is the *expected* state on an unsupported
  // host, not an exotic one.
  //
  // The stand-in sets `code`, not just a message: that is the field Node
  // populates and the field `resolveBinary` branches on, so a fake that only
  // matched the text would pass while proving nothing.
  assert.throws(
    () => resolveBinary('linux', 'x64', MANIFEST, failsWith('MODULE_NOT_FOUND')),
    /@opencoven\/psyche-linux-x64.*not installed/s
  );
});

test('does not tell an operator to reinstall a package that is already installed', () => {
  // A companion package declaring `exports` without a `"./package.json"` entry
  // is present and resolvable, yet `require.resolve('<pkg>/package.json')` fails
  // with ERR_PACKAGE_PATH_NOT_EXPORTED. Reporting "not installed" there sends
  // the operator round a reinstall loop that can never succeed, so absence and
  // misconfiguration must not collapse into one message.
  assert.throws(
    () => resolveBinary('linux', 'x64', MANIFEST, failsWith('ERR_PACKAGE_PATH_NOT_EXPORTED')),
    (err) =>
      /is installed but its package.json could not be resolved/.test(err.message) &&
      /exports/.test(err.message) &&
      !/not installed\./.test(err.message)
  );
});

test('refuses a platform with no recorded checksum', () => {
  assert.throws(
    () => resolveBinary('darwin', 'x64', MANIFEST, () => '/pkgs/x/package.json'),
    /no recorded checksum/
  );
});

test('reports a signal death as 128 + signal, not as success or a bare 1', () => {
  // `spawnSync` sets status to null when the child is killed by a signal. A
  // wrapper that maps that to 1 tells `psyche start; echo $?` the daemon exited
  // with a generic error when it was actually SIGKILLed — and a wrapper that
  // maps it to 0 is worse. 128+n is what a shell reports for the same death.
  assert.strictEqual(exitCodeFor({ status: 0, signal: null }), 0);
  assert.strictEqual(exitCodeFor({ status: 3, signal: null }), 3);
  assert.strictEqual(exitCodeFor({ status: null, signal: 'SIGKILL' }), 137);
  assert.strictEqual(exitCodeFor({ status: null, signal: 'SIGTERM' }), 143);
  // Neither status nor signal: spawn itself failed.
  assert.strictEqual(exitCodeFor({ status: null, signal: null }), 1);
});
