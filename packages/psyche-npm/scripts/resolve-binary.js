'use strict';

const path = require('node:path');
const { resolvePackageName } = require('./verify-checksum.js');

// `os.constants.signals` rather than a hand-written table: the numbers differ
// across platforms and a wrong constant here produces a plausible but incorrect
// exit code, which is the hardest kind of bug to notice.
const { signals } = require('node:os').constants;

/**
 * Locates the platform binary and the digest it must match.
 *
 * `resolvePkgJson` is injected — defaulting to `require.resolve` — so tests can
 * exercise this on a machine where no companion package has been published.
 */
function resolveBinary(platform, arch, manifest, resolvePkgJson = (id) => require.resolve(id)) {
  const pkg = resolvePackageName(platform, arch);
  const key = `${platform}-${arch}`;

  const expected = manifest.psyche && manifest.psyche.checksums[key];
  if (!expected) {
    throw new Error(`no recorded checksum for ${key}`);
  }

  let pkgJson;
  try {
    pkgJson = resolvePkgJson(`${pkg}/package.json`);
  } catch (cause) {
    // Only absence means "not installed". A package that declares `exports`
    // without a `"./package.json"` entry is present and resolvable yet fails
    // here with ERR_PACKAGE_PATH_NOT_EXPORTED, and telling that operator to
    // reinstall sends them round a loop that cannot terminate. Companion
    // packages must therefore omit `exports` or export `./package.json`; this
    // branch is what makes a violation legible instead of misleading.
    if (cause && cause.code !== 'MODULE_NOT_FOUND') {
      throw new Error(
        `${pkg} is installed but its package.json could not be resolved ` +
          `(${cause.code ?? 'unknown error'}). A companion package must not ` +
          `hide package.json behind an "exports" map.`,
        { cause }
      );
    }
    // npm silently skips an optional dependency whose platform does not match,
    // so this is the ordinary state on an unsupported host. Say which package is
    // missing and that reinstalling is the fix; `MODULE_NOT_FOUND` says neither.
    throw new Error(
      `${pkg} is not installed. It ships the ${key} binary and is an optional ` +
        `dependency of @opencoven/psyche; reinstall without --no-optional.`,
      { cause }
    );
  }

  const binaryName = platform === 'win32' ? 'psyche.exe' : 'psyche';
  return { binary: path.join(path.dirname(pkgJson), 'bin', binaryName), expected };
}

/**
 * Signals this wrapper forwards to the daemon.
 *
 * Ctrl-C already reaches the child without any of this — a tty signals the whole
 * foreground process group — which is exactly why the gap was easy to miss. A
 * directed `kill` from a supervisor hits the wrapper alone, so forwarding is the
 * only thing that lets `psyched`'s SIGTERM handling run at all.
 *
 * Windows has no real signals; Node emulates SIGINT, SIGBREAK, SIGHUP and
 * SIGTERM on top of console events, and listening for a signal it does not
 * emulate throws. SIGUSR1 and SIGUSR2 are deliberately absent everywhere: Node
 * reserves SIGUSR1 to start its inspector, and taking it over would silently
 * disable that for anyone debugging the wrapper.
 */
const FORWARDED_SIGNALS =
  process.platform === 'win32'
    ? ['SIGINT', 'SIGTERM', 'SIGBREAK']
    : ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT'];

/**
 * Maps a child's `(status, signal)` outcome to the exit code this process
 * should report. Shaped for `child.on('close')`, and identical to what
 * `spawnSync` returns, so the contract did not change when the wrapper moved off
 * the synchronous spawn.
 *
 * A child killed by a signal has `status === null`; reporting 1 for that claims
 * the daemon chose to fail when it was actually killed, and reporting 0 would
 * claim it succeeded. 128+n is what a shell reports for the same death.
 */
function exitCodeFor(result) {
  if (result.status !== null && result.status !== undefined) {
    return result.status;
  }
  if (result.signal) {
    return 128 + (signals[result.signal] ?? 0);
  }
  return 1; // spawn itself failed
}

module.exports = { resolveBinary, exitCodeFor, FORWARDED_SIGNALS };
