'use strict';

const fs = require('node:fs');
const crypto = require('node:crypto');

const SUPPORTED = new Set([
  'darwin-arm64',
  'darwin-x64',
  'linux-arm64',
  'linux-x64',
  'win32-x64',
]);

/** Maps process.platform/process.arch to the companion package that ships the binary. */
function resolvePackageName(platform, arch) {
  const key = `${platform}-${arch}`;
  if (!SUPPORTED.has(key)) {
    throw new Error(`unsupported platform: ${key}`);
  }
  return `@opencoven/psyche-${key}`;
}

/**
 * Refuses to hand back a binary that is missing or whose digest does not match
 * the manifest. This is the only integrity check between npm and exec.
 */
function verifyChecksum(binaryPath, expectedSha256) {
  if (!fs.existsSync(binaryPath)) {
    throw new Error(`psyche binary not found at ${binaryPath}`);
  }
  const actual = crypto
    .createHash('sha256')
    .update(fs.readFileSync(binaryPath))
    .digest('hex');
  if (actual !== expectedSha256) {
    throw new Error(
      `psyche binary checksum mismatch at ${binaryPath}: expected ${expectedSha256}, found ${actual}`
    );
  }
  return binaryPath;
}

module.exports = { resolvePackageName, verifyChecksum, SUPPORTED };
