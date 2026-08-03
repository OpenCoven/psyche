#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const { verifyChecksum } = require('../scripts/verify-checksum.js');
const { resolveBinary, exitCodeFor } = require('../scripts/resolve-binary.js');

// The wrapper resolves and execs a Rust binary. It holds no daemon, storage,
// identity, policy, or transport logic — that boundary is fixed by PLAN.md W2.
function main() {
  const { binary, expected } = resolveBinary(
    process.platform,
    process.arch,
    require('../package.json')
  );

  // Verified immediately before exec. This is a check, not a guarantee: nothing
  // stops the file being replaced between the digest and the spawn. Closing that
  // window needs the OS to hold the verified handle open across the exec, which
  // Node cannot express — it is recorded here so the check is not mistaken for
  // more than it is.
  verifyChecksum(binary, expected);

  // `stdio: 'inherit'` so the daemon's stderr reaches the operator's terminal
  // unbuffered, and so `psyche status --json | jq` still works through the
  // wrapper.
  process.exit(exitCodeFor(spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })));
}

try {
  main();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}
