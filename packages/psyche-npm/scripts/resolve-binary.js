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
 * Maps a `spawnSync` result to the exit code this process should report.
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

module.exports = { resolveBinary, exitCodeFor };
