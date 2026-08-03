// The wrapper must not swallow a shutdown signal.
//
// `psyched` handles SIGTERM and SIGINT and drains before exiting; that work is
// worth nothing if the process an operator actually signals — the npm wrapper —
// dies without passing the signal on. These tests run the real `bin/psyche.js`
// against a stand-in binary and assert on what the child observed, because the
// defect they guard is invisible from the wrapper's own exit status.
//
// Shown to fail: with `bin/psyche.js` reverted to `spawnSync`, `forwards SIGTERM`
// reports the child still alive with its trap unfired. Ctrl-C keeps working even
// then — a tty signals the whole foreground process group, so the child hears it
// directly and the wrapper plays no part. That is precisely why this needs a
// directed `kill` rather than a terminal to reproduce.

const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn, spawnSync } = require('node:child_process');

const WRAPPER = path.join(__dirname, '..', 'bin', 'psyche.js');

/** Poll until `predicate` holds, or fail the test rather than hang. */
async function waitFor(predicate, description, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail(`timed out after ${timeoutMs}ms waiting for ${description}`);
}

/**
 * Builds a package tree the wrapper can resolve: a fake companion package
 * holding `script` as its binary, and a manifest recording that file's real
 * digest so `verifyChecksum` passes.
 */
function stageWrapper(script) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'psyche-sig-'));
  const key = `${process.platform}-${process.arch}`;
  const binDir = path.join(root, 'node_modules', '@opencoven', `psyche-${key}`, 'bin');
  fs.mkdirSync(binDir, { recursive: true });

  const binary = path.join(binDir, process.platform === 'win32' ? 'psyche.exe' : 'psyche');
  fs.writeFileSync(binary, script, { mode: 0o755 });
  fs.writeFileSync(
    path.join(binDir, '..', 'package.json'),
    JSON.stringify({ name: `@opencoven/psyche-${key}`, version: '0.0.0' })
  );

  // The wrapper reads `../package.json` relative to `bin/psyche.js`, so the real
  // manifest is patched in place and restored by the caller.
  const digest = crypto.createHash('sha256').update(script).digest('hex');
  return { root, key, digest, log: path.join(root, 'child.log') };
}

/**
 * Runs `body` with the shipped manifest's digest for this platform overridden,
 * then restores the file byte-for-byte.
 *
 * `await body()`, not `return body()`: with a synchronous `finally` an async
 * body only gets as far as returning its promise before the manifest is put
 * back, so the wrapper — which reads the manifest from disk in another process —
 * sees the all-zero placeholder, fails the checksum, and exits before the child
 * ever runs. That produced a "timed out waiting for the stand-in binary" failure
 * that looked like a defect in the wrapper rather than in this helper.
 */
async function withDigest(key, digest, body) {
  const manifestPath = path.join(__dirname, '..', 'package.json');
  const original = fs.readFileSync(manifestPath, 'utf8');
  const manifest = JSON.parse(original);
  manifest.psyche.checksums[key] = digest;
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  try {
    return await body();
  } finally {
    fs.writeFileSync(manifestPath, original);
  }
}

// `SIGQUIT` rather than `SIGTERM` for the second case: both are forwarded, and
// using two different signals proves the handler passes the signal it received
// instead of hardcoding one.
for (const signal of ['SIGTERM', 'SIGQUIT']) {
  test(`forwards ${signal} to the daemon instead of orphaning it`, { skip: process.platform === 'win32' && 'POSIX signals' }, async () => {
    const script = `#!/bin/sh
trap 'echo GOT-${signal} >> "$PSYCHE_TEST_LOG"; exit 0' ${signal.slice(3)}
echo UP >> "$PSYCHE_TEST_LOG"
while true; do sleep 0.1; done
`;
    const staged = stageWrapper(script);

    await withDigest(staged.key, staged.digest, async () => {
      const child = spawn(process.execPath, [WRAPPER], {
        env: { ...process.env, NODE_PATH: path.join(staged.root, 'node_modules'), PSYCHE_TEST_LOG: staged.log },
        stdio: 'ignore',
      });

      const logged = () => (fs.existsSync(staged.log) ? fs.readFileSync(staged.log, 'utf8') : '');
      await waitFor(() => logged().includes('UP'), 'the stand-in binary to start');

      child.kill(signal);
      await waitFor(() => logged().includes(`GOT-${signal}`), `the child to receive ${signal}`);

      // The wrapper must also not outlive the child it was waiting on.
      await waitFor(() => child.exitCode !== null || child.signalCode !== null, 'the wrapper to exit');
    });
  });
}

test('reports a spawn failure instead of exiting 1 in silence', { skip: process.platform === 'win32' && 'shebang-less exec' }, async () => {
  // Reachable *past* a passing checksum: a companion package shipping a
  // wrong-architecture binary matches its recorded digest and dies at exec. The
  // user must not be left with a bare exit code and no explanation.
  const script = '\x7fELF not really an executable';
  const staged = stageWrapper(script);

  const result = await withDigest(staged.key, staged.digest, () =>
    spawnSync(process.execPath, [WRAPPER], {
      env: { ...process.env, NODE_PATH: path.join(staged.root, 'node_modules') },
      encoding: 'utf8',
    })
  );

  assert.notStrictEqual(result.status, 0, 'a binary that cannot exec must not report success');
  assert.notStrictEqual(result.stderr.trim(), '', `exit ${result.status} with no explanation`);
});
