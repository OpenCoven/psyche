# Psyche command line

Two binaries ship from one crate: `psyche`, the operator-facing command, and
`psyched`, the daemon. Everything below describes what this build actually does.
Where a command cannot do something, it says so and this page says so — a
command that reports success it did not achieve is treated as a defect here, not
as a rough edge.

## `psyche` and `psyched`

`psyche start` and `psyched` are the same daemon. They parse arguments
separately and then call one shared run path, so they cannot come to mean
different things; their flag sets are asserted equal by a test. Prefer `psyched`
in a unit file, where the extra subcommand word buys nothing.

Both run in the foreground and shut down gracefully on **SIGTERM** or **SIGINT**:
intake stops, in-flight work drains, then the process exits 0. Handlers are
installed before the runtime starts, so a signal arriving during startup is
honoured rather than killing the process at its default disposition. There is no
forced-exit path — a caller wanting one terminates the process itself.

Neither binary daemonises, writes a pidfile, or guards against a second instance.
A concurrent-daemon guard belongs with the lease work in the runtime and is not
in this build.

## Subcommands

| Command | Does |
|---|---|
| `psyche start` | Runs the daemon in the foreground. Equivalent to `psyched`. |
| `psyche stop` | **Nothing.** There is no daemon IPC in this build. Exits 4. |
| `psyche status` | Reports that it could not observe a state, and why. |
| `psyche doctor` | Runs local, credential-free environment checks. |

`psyche stop` is listed as unimplemented rather than quietly returning 0 because
`psyche stop && deploy` would otherwise read "the old daemon is gone" when
nothing had been asked to stop.

`doctor` never reads a credential or touches the network, so it is usable on a
machine that has never been given a Telegram token. It does not contact the
Coven socket at this gate — it only reports the configured path.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The command did what it was asked. |
| 1 | Unexpected. Deliberately unassigned, so it stays a signal. |
| 2 | Usage error — unknown flag, missing argument, bad subcommand. Emitted by clap. |
| 3 | Configuration is missing, unreadable, too large, or invalid. |
| 4 | A daemon was needed and could not be reached. |
| 5 | A check ran to completion and failed. |

Both binaries use the same space, so a unit file does not have to know which one
it invoked.

`doctor` distinguishes 3 from 5 deliberately: **3 means your configuration file
is wrong; 5 means the file was fine and something it describes is not.** An
operator scripting `doctor` could not previously tell those apart.

## Configuration path resolution

In order, first match wins:

1. `--config <path>`
2. `$PSYCHE_CONFIG`
3. `./psyche.toml`

`--config` is accepted on either side of the subcommand: `psyche --config X
status` and `psyche status --config X` are the same command.

The default is relative to the working directory. A systemd system unit defaults
to `WorkingDirectory=/`, so a service without an explicit path resolves
`/psyche.toml` — set `PSYCHE_CONFIG` or pass `--config` there. The error names
the path that was actually tried.

There is no XDG or `/etc` lookup chain. That is deferred to the packaging work,
not rejected.

## Logging

Logs are JSON, one object per line, on **stderr** — never stdout, so a
`--json` document piped into a parser cannot be corrupted by a log line.

`PSYCHE_LOG` sets the filter, using `tracing-subscriber`'s `EnvFilter` syntax.
Unset means `info`. A filter that does not parse produces a one-line warning on
stderr and falls back to `info`, rather than silently behaving as though nothing
had been set.

One thing that warning cannot catch: a bare word is valid `EnvFilter` syntax for
a *target* filter. `PSYCHE_LOG=trce` — the obvious typo for `trace` — parses
successfully, enables a target named `trce`, and produces no output at all. If
logging goes silent after setting `PSYCHE_LOG`, check the spelling of the level.

## `psyche.status.v1`

`psyche status --json` writes one document to stdout:

    {"schema":"psyche.status.v1","observed":false,"state":null,"reason":"no-ipc"}

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | Always `psyche.status.v1` in this build. |
| `observed` | bool | Whether a daemon was actually reached. |
| `state` | string or null | `running`, `draining`, or `stopped`. Non-null only when `observed` is true. |
| `reason` | string or null | Why nothing was observed. Non-null only when `observed` is false. |

**In this build `observed` is always `false` and `state` is always `null`.**
`status` runs in a different process from the daemon and there is no IPC, so it
cannot see a running `psyched` even when one is running. It does not guess:
reporting `stopped` here would be a false statement on exactly the hosts where
the answer matters.

`reason` is a closed vocabulary. `no-ipc` is the only value this build produces.
The daemon-IPC work is expected to add `socket-absent` (nothing is at the
configured path), `connect-refused` (the path exists but nothing is listening —
a stale socket) and `permission-denied` (a daemon may be running; this caller
cannot ask). Treat an unrecognised `reason` as "not observed" rather than as an
error.

The human rendering carries the same caveat and likewise names no state.

## `psyche.doctor.v1`

`psyche doctor --json` writes one document to stdout:

    {"schema":"psyche.doctor.v1",
     "checks":[{"name":"config","status":"ok","detail":"..."}],
     "failed":0}

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | Always `psyche.doctor.v1` in this build. |
| `checks` | array | One entry per check, in a stable order. |
| `checks[].name` | string | Stable identifier: `config`, `data_dir`, `coven_socket_path`, `extensions`. |
| `checks[].status` | string | `ok`, `warn`, `fail`, `info`, or `skipped`. |
| `checks[].detail` | string | Operator-facing explanation. Not a stable format. |
| `failed` | number | How many checks have status `fail`. |

Statuses:

| Status | Meaning |
|---|---|
| `ok` | Checked, and in the state it needs to be. |
| `warn` | Checked, usable, worth knowing about. Does not fail the command. |
| `fail` | Checked, and wrong. Any one of these fails the command. |
| `info` | Not a check. Reported because it is useful in a bug report; cannot fail. |
| `skipped` | Not run, because something it needed was unavailable. |

The check list has the same four entries whatever happens, including when the
configuration does not load — a shorter list would read as though the missing
checks had passed. The same statuses appear in the non-JSON rendering, one
`name: status (detail)` line per check, using the same words.

`coven_socket_path` and `extensions` are `info`: neither contacts anything or
validates anything, so neither can fail.

### The checks

- **`config`** — whether the configuration loaded, and which schema it declares.
  On failure the detail names the path and the reason. `fail` here means the
  command exits 3.
- **`data_dir`** — creates the directory if absent, then writes and removes a
  probe file inside it. `ok` means it existed and a write succeeded; `warn` means
  this run created it, which usually means the path is not the one you meant;
  `fail` means the write failed, and the command exits 5. The word "writable" is
  earned by an actual write — checking only that the directory exists reports a
  mode-500 directory as writable.
- **`coven_socket_path`** — reports the configured path. Nothing is contacted at
  this gate.
- **`extensions`** — reports how many extension tables are present. Values are
  never printed: extension tables are untyped and a future one may hold a
  credential.

## Output discipline

- Logs go to stderr. `--json` documents go to stdout, one whole document.
- `doctor`'s report is its output and goes to stdout in full, including the
  reason a configuration failed to load.
- A configuration is never rendered with `{:?}`, and an extension value is never
  printed. Both are pinned by tests.
