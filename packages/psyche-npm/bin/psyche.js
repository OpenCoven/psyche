#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');
const { verifyChecksum } = require('../scripts/verify-checksum.js');
const { resolveBinary, exitCodeFor, FORWARDED_SIGNALS } = require('../scripts/resolve-binary.js');

// The wrapper resolves and execs a Rust binary. It holds no daemon, storage,
// identity, policy, or transport logic — that boundary is fixed by PLAN.md W2.
function main() {
  const { binary, expected } = resolveBinary(
    process.platform,
    process.arch,
    require('../package.json')
  );

  // Verified immediately before exec. This is a check, not a guarantee: nothing
  // stops the file being replaced between the digest and the spawn. On Linux
  // `open()` plus exec of `/proc/self/fd/N` would close the window; there is no
  // portable equivalent, so the window is documented rather than claimed shut.
  //
  // Note also what this check is *for*: the expected digest ships inside this
  // package, so it authenticates the companion binary against a claim this
  // wrapper makes — not against a compromised wrapper. It is a substitution
  // check, not a signature.
  verifyChecksum(binary, expected);

  // `spawn`, never `spawnSync`. A synchronous spawn blocks the event loop inside
  // waitpid, so a `process.on('SIGTERM')` handler is JavaScript that cannot run
  // until the child has already exited — the wrapper dies, the daemon is
  // reparented to init, and it never drains. Verified: under `spawnSync` a
  // `kill -TERM` of the wrapper leaves the child alive with PPID 1 and its TERM
  // trap unfired.
  //
  // That failure is invisible interactively, which is what makes it dangerous:
  // Ctrl-C appears to work because the tty signals the whole foreground process
  // group and the child hears it directly, with the wrapper playing no part.
  // Every non-interactive supervisor — `docker stop`, `systemctl stop`,
  // supervisord, a Kubernetes preStop hook — signals the wrapper PID alone, and
  // psyched's SIGTERM handling is unreachable through a synchronous wrapper.
  //
  // `stdio: 'inherit'` so the daemon's stderr reaches the operator's terminal
  // unbuffered, and so `psyche status --json | jq` still works through here.
  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });

  // Forwarded rather than relied upon: the process-group delivery that makes
  // Ctrl-C work does not happen for a directed kill.
  for (const signal of FORWARDED_SIGNALS) {
    process.on(signal, () => {
      child.kill(signal);
    });
  }

  // `spawn` reports exec failure through this event rather than by throwing, so
  // without it the user gets a bare exit 1 and no output. Reachable past a
  // passing checksum: a companion package shipping a wrong-architecture binary
  // matches its recorded digest and then fails at exec.
  child.on('error', (err) => {
    console.error(err.message);
    process.exit(1);
  });

  // `close`, not `exit`: it fires after the inherited stdio streams are done, so
  // the daemon's final lines cannot be truncated by this process exiting first.
  child.on('close', (status, signal) => {
    process.exit(exitCodeFor({ status, signal }));
  });
}

try {
  main();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}
