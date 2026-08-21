#!/usr/bin/env bash
# scripts/with-jcodemunch-serve.sh
#
# The SINGLE transient-serve lifecycle wrapper: spawn a pinned
# `jcodemunch-mcp serve`, readiness-poll it, run the wrapped command against it,
# and tear the serve down on EVERY exit path.
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4 (δ), D5
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/δ
#
# D5 retires the persistent `deploy/systemd/jcodemunch-serve.service` unit. Port
# 8901 has exactly one consumer in the world — `reify-audit` — so a machine-wide
# always-on daemon buys nothing and silently rots (PRD §2.1). This script is the
# replacement: every consumer brings its own serve up for exactly as long as it
# needs one. That only works if teardown is unconditional, which is why the
# refusal markers below include a LEAK marker and why the teardown path is the
# most heavily-tested part of this file.
#
#   E_JC_SERVE_PORT_BUSY     the target port is already accepting — REFUSED,
#                            never adopted and never killed
#   E_JC_SERVE_SPAWN_FAILED  the serve child died before becoming ready
#   E_JC_SERVE_NOT_READY     the endpoint never answered as jcodemunch-mcp
#   E_JC_SERVE_LEAKED        teardown ran and the port is STILL accepting
#
# Usage:
#   scripts/with-jcodemunch-serve.sh <command> [args...]
#   scripts/with-jcodemunch-serve.sh --port 8917 -- <command> [args...]
#   scripts/with-jcodemunch-serve.sh --dry-run           # print the serve argv
#
# ── `/mcp` HAS NO TRAILING SLASH ────────────────────────────────────────────
#
# The readiness probe POSTs to `http://127.0.0.1:$PORT/mcp`, and `JCODEMUNCH_URL`
# is exported with the same spelling. `/mcp/` is NOT equivalent: a real serve
# 307-redirects it and the redirect DROPS the `mcp-session-id` header, so the
# session contract silently breaks downstream instead of failing here. Already
# pinned at `crates/reify-audit/src/bin/reify-audit.rs:185-188` and reused by
# `scripts/smoke-jcodemunch-serve.sh`; do not "tidy" a slash onto either.
#
# ── READINESS IS AN IDENTITY CHECK, NOT A LIVENESS CHECK ────────────────────
#
# A bare TCP connect is answered happily by ANY port squatter, so the probe
# requires `result.serverInfo.name == "jcodemunch-mcp"` before declaring ready —
# `crates/reify-audit/tests/jcodemunch_session_live.rs:210-264` (α) makes the
# same demand for the same reason. α picks an ephemeral port; this script
# DEFAULTS to a fixed 8901, so the squatter risk here is strictly higher, not
# lower.
#
# ── THE IDENTITY LEVER IS PART OF THE INVOCATION ────────────────────────────
#
# The serve is spawned under an explicit `env JCODEMUNCH_GIT_ROOT_IDENTITY=0`
# prefix, exactly as `scripts/jcodemunch-index-reify.sh` (β) does for the
# indexer. Without it, jcodemunch resolves a GIT identity — at the pinned
# 1.108.54 `config.py:384` ships `"git_root_identity": True` — and answers for
# `leodearden/reify` (the empty husk) rather than for the per-path
# `local/reify-4ae45bbd` that β actually indexes. Every invocation site carries
# this obligation (PRD §4.2 / the capability manifest's note at §2: "β does; δ
# and ζ carry the same obligation in their task records").
#
# PIN-BUMP CHECKLIST — this env var is accepted but DEPRECATED upstream; the
# package logs "will be removed in v2.0. Use config.jsonc instead." A bump past
# v2.0 must re-establish the lever in config.jsonc BEFORE landing, or the
# identity silently reverts:
#   * THE KEY IS `"git_root_identity": false`, NOT `"identity_mode": "local"`.
#     `config.py:384` is the shipped default that has to be flipped; `:474` is
#     its CONFIG_TYPES entry — the map a key must appear in to survive the load
#     at all.
#   * `"identity_mode"` is a TRAP: the shipped config template advertises it
#     (config.py:1872-1896, even presenting it as the preferred spelling) yet at
#     1.108.54 it is in neither DEFAULTS nor CONFIG_TYPES, so it is discarded
#     silently on the load path (config.py:708, "Ignore unknown keys silently").
#   * Run `jcodemunch-mcp config --check` (server.py:6042) against any
#     config.jsonc a bump introduces: `validate_config` DOES name an
#     unrecognised key (config.py:1194). It is the only signal upstream gives.
# Carried as an explicit `env` prefix rather than an `export` so `--dry-run`
# prints a command that actually reproduces the behaviour when pasted.
#
# ── STDERR DISCIPLINE IS A CORRECTNESS CONSTRAINT, NOT COSMETICS ────────────
#
# `reify-audit` writes its JSON findings array to STDERR
# (`crates/reify-audit/src/bin/reify-audit.rs:689-695`) and every consumer
# extracts it as the TRAILING block — `scripts/smoke-predone-hook.sh:233` pipes
# through `awk 'BEGIN{p=0} /^\[/{p=1} p{print}' | jq -e 'type=="array"'`, and
# `crates/reify-audit/tests/cli.rs` uses `rfind("\n[")`. So:
#
#   * stdout belongs ENTIRELY to the wrapped command. This script says nothing
#     there, ever, which is also what keeps it generic rather than
#     reify-audit-specific.
#   * every message this script emits goes to stderr and is emitted BEFORE the
#     wrapped command runs.
#   * teardown is SILENT on success and loud only on a leak. One line of
#     teardown chatter appended after the wrapped command would break that
#     extraction and take the whole δ signal with it.
#
# Prerequisites: uvx (https://docs.astral.sh/uv/), curl, jq.

set -euo pipefail

# The one port D5 names. `--port` moves it; the guard needs that because it is a
# `pool` member and must never bind a host-global fixed port.
DEFAULT_PORT=8901

usage() {
    cat <<'USAGE'
Usage: scripts/with-jcodemunch-serve.sh [--port N] [--dry-run] [--] <command> [args...]

Spawns a pinned transient `jcodemunch-mcp serve` on 127.0.0.1, waits until it
answers a real MCP `initialize` AS jcodemunch, runs <command> against it with
JCODEMUNCH_URL exported, then tears the serve down on every exit path.

  --port N    Serve on port N instead of the default 8901. Also the value
              exported in JCODEMUNCH_URL.
  --dry-run   Print the exact serve argv that would be spawned, exit 0. Spawns
              nothing and does NOT run <command>.
  --          End the wrapper's own flags; everything after begins <command>.
              Only needed when <command> itself starts with a dash.
  -h, --help  Show this help and exit.

<command> runs as a foreground child with stdin/stdout/stderr inherited
untouched and its exit status propagated verbatim. This script writes NOTHING
to stdout and writes its own messages to stderr only BEFORE <command> starts —
so a trailing JSON block on <command>'s stderr (reify-audit's findings array)
is still the last thing on stderr when the wrapper exits.

Refusal markers (stderr, always non-zero exit):
  E_JC_SERVE_PORT_BUSY     the target port is already accepting; this script
                           never adopts and never kills a serve it did not spawn
  E_JC_SERVE_SPAWN_FAILED  the serve child died before becoming ready
  E_JC_SERVE_NOT_READY     the endpoint never answered as jcodemunch-mcp within
                           the readiness deadline
  E_JC_SERVE_LEAKED        teardown ran and the port is STILL accepting — a
                           serve was leaked and must be reclaimed by hand
USAGE
}

# ── Messages: stderr only, and only before the wrapped command runs ──────────
say()    { printf 'with-jcodemunch-serve: %s\n' "$*" >&2; }
die()    { printf 'with-jcodemunch-serve: %s\n' "$*" >&2; exit 1; }
# refuse <MARKER> <message…> — a greppable refusal. The marker leads the line so
# a consumer can `grep -q E_JC_SERVE_…` without matching prose that merely
# mentions it.
refuse() {
    local marker="$1"; shift
    printf 'with-jcodemunch-serve: %s: %s\n' "$marker" "$*" >&2
    exit 1
}

# ── Argv split ───────────────────────────────────────────────────────────────
#
# Wrapper flags first; the FIRST non-flag token (or an explicit `--`) begins the
# wrapped command, and everything from there is passed through untouched — no
# re-splitting, no globbing, no re-quoting. An unknown leading dash is an error
# rather than the start of the command: a command name beginning with a dash is
# vanishingly rare and `--` already covers it, whereas silently treating
# `--prot 8917` as a command would spawn a serve on the wrong port and then fail
# with a confusing 127.
PORT="$DEFAULT_PORT"
DRY_RUN=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --port)
            [ "$#" -ge 2 ] || { echo "with-jcodemunch-serve.sh: --port requires an N argument" >&2; usage >&2; exit 64; }
            PORT="$2"; shift 2 ;;
        --port=*) PORT="${1#*=}"; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        --) shift; break ;;
        -*)
            echo "with-jcodemunch-serve.sh: unknown wrapper flag '$1'" >&2
            echo "with-jcodemunch-serve.sh: use '--' to end the wrapper's flags if this is meant to be the wrapped command" >&2
            usage >&2
            exit 64 ;;
        *) break ;;
    esac
done

# Validated here rather than left to fail inside curl or the listener probe: a
# non-numeric port would otherwise surface as an unrelated connection error many
# seconds into a spawn.
case "$PORT" in
    ''|*[!0-9]*) echo "with-jcodemunch-serve.sh: --port must be a number, got '$PORT'" >&2; exit 64 ;;
esac
if [ "$PORT" -lt 1 ] || [ "$PORT" -gt 65535 ]; then
    echo "with-jcodemunch-serve.sh: --port must be in 1..65535, got '$PORT'" >&2
    exit 64
fi

WRAPPED=("$@")

# --dry-run prints a command it does not run, so it is the one mode that needs
# no wrapped command. Every other mode does: a wrapper with nothing to wrap
# would spawn a serve, prove it ready and tear it straight back down, which is
# an expensive no-op that reads as success.
if [ "${#WRAPPED[@]}" -eq 0 ] && [ "$DRY_RUN" -eq 0 ]; then
    echo "with-jcodemunch-serve.sh: no wrapped command given" >&2
    usage >&2
    exit 64
fi

# ── The serve command ────────────────────────────────────────────────────────
#
# The BARE transient-serve form and nothing more. Mirrors α's spawn at
# `crates/reify-audit/tests/jcodemunch_session_live.rs:156-173`:
#
#     uvx --python 3.13 --from jcodemunch-mcp==1.108.54 jcodemunch-mcp serve \
#         --transport streamable-http --host 127.0.0.1 --port <PORT> --watcher=false
#
# `--watcher=false` because the file watcher indexes the whole repo on start and
# δ needs only the MCP session seam — indexing belongs to β
# (`scripts/jcodemunch-index-reify.sh`) and to nothing else. NOTHING is ever
# appended to this array: in particular the `index` subcommand is never used
# (it is the only subparser accepting `--paths-from`, which DELETEs every
# previously-indexed file absent from its list — server.py:6505,
# index_folder.py:1505-1511, sqlite_store.py:1698), and neither is `watch`.
JC_PIN="jcodemunch-mcp==1.108.54"

# THE INTERPRETER IS PART OF THE PIN (esc-6107-4). `--from jcodemunch-mcp==…`
# alone is only HALF a pin: it fixes the package and leaves the interpreter
# floating, and uvx defaults to the newest interpreter uv manages — on this host
# cpython-3.14.0+freethreaded, against which a transitive dep publishes no
# compatible wheel ("Failed to download and build
# `tree-sitter-embedded-template==0.25.0` … not compatible with the current
# Python 3.14t"), so the bare form does not run at all.
#
# 3.13 vs 3.12 — the two siblings measured DIFFERENT values against DIFFERENT
# subcommands, and this is the reconciliation: α measured `--python 3.12`
# against `serve` (jcodemunch_session_live.rs:157-159), while β measured 3.13
# against the heavier `watch` path, which resolves the full dependency closure
# (a superset of what `serve` needs). 3.13 is chosen here for sibling-
# consistency with β and because a closure that resolved for `watch` necessarily
# covers `serve`. Task 6109 step-15 is where 3.13-on-`serve` gets its own
# first-hand measurement; if it ever fails there, fall back to 3.12 and amend
# this comment with the measurement rather than deleting it.
JC_PYTHON="3.13"

# ── THE IDENTITY LEVER IS PART OF THE INVOCATION ─────────────────────────────
#
# Carried as an explicit argv PREFIX rather than an `export`, so `--dry-run`
# prints a command that actually reproduces this behaviour when pasted. The
# full rationale and the PIN-BUMP CHECKLIST are in this file's header; the one
# line that matters here is that without it jcodemunch answers for
# `leodearden/reify` (the empty husk) instead of the per-path
# `local/reify-4ae45bbd` that β indexes, and the wrapped command then audits
# nothing while emitting a perfectly well-formed empty findings array.
JC_IDENTITY_ENV=(env JCODEMUNCH_GIT_ROOT_IDENTITY=0)

SERVE_CMD=(uvx --python "$JC_PYTHON" --from "$JC_PIN" jcodemunch-mcp)
if [ -n "${REIFY_JC_SERVE_CMD:-}" ]; then
    # TEST-ONLY SEAM. Replaces the `uvx …` prefix so the guard can drive the
    # whole spawn/readiness/teardown lifecycle against a stdlib-only stub serve,
    # with no uvx, no PyPI and no network. Never set in production use;
    # word-split deliberately, so a caller can pass a multi-word command.
    # shellcheck disable=SC2206
    SERVE_CMD=(${REIFY_JC_SERVE_CMD})
fi
SERVE_ARGV=("${JC_IDENTITY_ENV[@]}" "${SERVE_CMD[@]}" serve
    --transport streamable-http --host 127.0.0.1 --port "$PORT" --watcher=false)

if [ "$DRY_RUN" -eq 1 ]; then
    say "exec  $(printf '%q ' "${SERVE_ARGV[@]}")"
    exit 0
fi

# INTERIM (task 6109, TDD): preflight, spawn, readiness, run and teardown land
# in the following steps. Refusing loudly here keeps this intermediate commit
# from looking like a successful no-op.
say "the serve lifecycle is not wired up yet in this commit"
exit 70
