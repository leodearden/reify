#!/usr/bin/env bash
# scripts/lib_jcodemunch_pin.sh — the SINGLE definition site for the jcodemunch
# INVOCATION TRIPLE: the wheel pin, the interpreter, and the identity lever.
#
# Designed to be SOURCED, never executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_jcodemunch_pin.sh"
#   or:   source "$REPO_ROOT/scripts/lib_jcodemunch_pin.sh"
#
# Task #6454. Before this file existed the three values were COPIED across
# every consumer, and a bump had to touch all of them in one change or drift
# silently — a serve running an OLDER wheel than the indexer that WROTE the
# index it is being asked to query is silent at the call site: the session
# opens, the query answers, and the answer is merely wrong.
#
# ── THE CONSUMERS, AND WHY TWO OF THEM STAY LITERAL ─────────────────────────
#
# THREE PRODUCTION CONSUMERS source or read this file:
#   * β  scripts/jcodemunch-index-reify.sh    — the `watch --once` indexer;
#   * δ  scripts/with-jcodemunch-serve.sh     — the transient-serve wrapper;
#   * α  crates/reify-audit/tests/jcodemunch_session_live.rs — the live session
#        test, which cannot source a shell lib and therefore carries
#        `const JCODEMUNCH_PIN` / `const JCODEMUNCH_PYTHON` instead. Both are
#        cross-checked against THIS file by the guard suites below, so they are
#        mirrors, not independent owners.
#
# TWO GUARD NEEDLES DELIBERATELY KEEP THE LITERAL VALUES, and must be bumped by
# hand alongside this file:
#   * tests/infra/test_with_jcodemunch_serve.sh — b2_pin_and_shape;
#   * tests/infra/test_jcodemunch_index_reify.sh — `argv pins jcodemunch-mcp==…`.
# That is NOT an oversight. Once β and δ source this lib their constructed argv
# is DERIVED from it, so a guard that read the expected value from here and
# compared it against an argv built from here would be comparing the lib to
# itself — tautologically green, and silently so. The literal needles are the
# only assertions that fail when the value in THIS FILE changes, which is
# exactly the event a pin bump is.
#
# The two guard suites cross-check every consumer against this file:
# tests/infra/test_with_jcodemunch_serve.sh (lib contract, δ's constructed
# argv, α's consts) and tests/infra/test_jcodemunch_index_reify.sh (β's
# constructed argv). Both are hermetic — no uvx, no PyPI, no network.
#
# THE STATE IS NOW UNIFIED, for the interpreter as well as the wheel (#6548).
# Both values have ONE definition site — this file — and all three production
# consumers are gate-cross-checked against it: β and δ through their CONSTRUCTED
# --dry-run argv, α through `const JCODEMUNCH_PIN` and `const JCODEMUNCH_PYTHON`.
# α previously owned its interpreter independently, hardcoding `--python 3.12`
# inline while β and δ ran 3.13; that divergence is closed and the const is a
# MIRROR of JC_PYTHON, not an independent owner. Nothing here is
# hand-reconciled any more — a one-sided change reds the gate.
#
# ── PIN-BUMP CHECKLIST ──────────────────────────────────────────────────────
#
# Consolidated here ONCE, from what were near-duplicate copies in β's and δ's
# headers. A bump touches this file, α's two consts, and the two literal guard
# needles named above — and must work through the following:
#
# 1. THE IDENTITY LEVER IS DEPRECATED UPSTREAM. `JCODEMUNCH_GIT_ROOT_IDENTITY`
#    is accepted at the pinned 1.108.54, but the package logs "will be removed
#    in v2.0. Use config.jsonc instead." A bump past v2.0 must re-establish the
#    lever in config.jsonc BEFORE landing, or the identity silently reverts to
#    `leodearden/reify` (the empty husk) and every downstream gate starts
#    interrogating a path nothing writes.
#
# 2. THE SUCCESSOR KEY IS `"git_root_identity": false` — NOT
#    `"identity_mode": "local"`. Two line numbers, because a bumper needs both:
#      * `config.py:384` is the shipped DEFAULT that actually has to be flipped
#        (`"git_root_identity": True`);
#      * `config.py:474` is its CONFIG_TYPES entry (`"git_root_identity": bool`)
#        — the map a key must appear in to survive the load at all.
#    Following :474 alone lands a reader on a type table, not on the lever.
#
# 3. `"identity_mode"` IS A TRAP worth naming rather than merely omitting. The
#    shipped config template ADVERTISES it (config.py:1872-1896, even presenting
#    it as the PREFERRED spelling and `git_root_identity` as its deprecated
#    alias), yet at 1.108.54 it appears in neither DEFAULTS nor CONFIG_TYPES. A
#    bump that reached for the advertised key would look configured while the
#    identity had already reverted.
#
# 4. IT FAILS CLOSED, BUT NOT INVISIBLY — and one half of that is a cheap check:
#      * on the LOAD path an unknown key is dropped with no error and no log
#        line (config.py:708, `# Ignore unknown keys silently`);
#      * `validate_config` DOES flag it: "Config key 'identity_mode' is not
#        recognized (unknown key)" (config.py:1194), reachable from the CLI as
#        `jcodemunch-mcp config --check` (server.py:6042).
#    So RUN `jcodemunch-mcp config --check` against any config.jsonc a bump
#    introduces. It is the only signal upstream gives here.
#
# 5. RE-MEASURE THE INTERPRETER against the new wheel, and measure it against
#    the SUBCOMMAND you care about. See JC_PYTHON below for what "the bare form
#    does not run at all" means on this host, and for the two standing
#    measurements (`serve` and `watch`) that authorise the current value.
#
# 6. BUMP THE TWO LITERAL GUARD NEEDLES named above in the same change. They do
#    not read this file by design, so they are what fails when the value HERE
#    moves -- which is the whole point of them, and the one step a bumper who
#    only greps for the old value will still get right.
#
# All of the above was re-verified first-hand against the PINNED 1.108.54 wheel,
# not a neighbouring release.
#
# ── THIS FILE MUST BE INERT AT LOAD ─────────────────────────────────────────
#
# NO `set -euo pipefail` and NO side effects — in particular nothing on stdout,
# ever. This lib is sourced INTO scripts whose `--dry-run` stdout IS their
# contract (both guard suites parse it), so one stray `echo` here would corrupt
# the output every one of those assertions reads. Setting shell options here
# would likewise silently impose them on whatever sourced us.

# Source guard — prevent double-sourcing. Load-bearing rather than hygienic:
# JC_IDENTITY_ENV is an ARRAY, and a re-source that appended to it would double
# the `env` prefix in every consumer's spawned command.
if [ "${_REIFY_LIB_JCODEMUNCH_PIN_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_JCODEMUNCH_PIN_SH_SOURCED=1

# ── The wheel pin ────────────────────────────────────────────────────────────
#
# 1.108.54 because PRD §8 (docs/prds/jcodemunch-substrate-restoration.md)
# records that 1.108.27 is no longer on PyPI. An UNPINNED invocation would
# silently follow upstream into a version whose flags and schema neither β nor δ
# has been verified against.
JC_PIN="jcodemunch-mcp==1.108.54"

# ── The interpreter ──────────────────────────────────────────────────────────
#
# THE INTERPRETER IS PART OF THE PIN (esc-6107-4). `--from jcodemunch-mcp==…`
# alone is only HALF a pin: it fixes the package and leaves the interpreter
# floating, and uvx defaults to the newest interpreter uv manages — on this host
# cpython-3.14.0+freethreaded. Measured 2026-08-13, the bare form fails outright:
#
#   × Failed to download and build `tree-sitter-embedded-template==0.25.0`
#   ╰─▶ The built wheel … is not compatible with the current Python 3.14t
#
# i.e. a transitive dep of the PINNED jcodemunch-mcp publishes no
# 3.14t-compatible wheel, so the primitive could not run AT ALL on the canonical
# checkout. Pinning the minor keeps resolution reproducible as newer
# interpreters land on the host.
#
# 3.13 specifically: it is what `python3` already resolves to here (3.13.9), and
# it resolved and ran clean at this exact package pin. MEASURED 2026-08-22 (task
# 6109 step-15) against δ's `serve`: uvx installed 37 packages in 311 ms from a
# warm cache, the serve answered `initialize` as `jcodemunch-mcp` on 8901, and
# three full wrapped runs completed over it (readiness ~13 s cold, ~5 s warm).
# β separately measured 3.13 against the heavier `watch` closure, a superset of
# what `serve` resolves.
#
# AND MEASURED AGAIN 2026-09-04 (task 6929 / #6548), which is what closed the
# last divergence: α had been carrying 3.12 because 3.12 was what IT had
# measured against `serve`, while β's 3.13 evidence came from `watch`. Running
# α's EXACT argv on host leo-MS-7C35 with uvx 0.11.6, changing only the
# interpreter —
#
#   uvx --python 3.13 --from jcodemunch-mcp==1.108.54 jcodemunch-mcp serve \
#       --transport streamable-http --host 127.0.0.1 --port <ephemeral> --watcher=false
#
# — installed 37 packages in 382 ms and answered `initialize` with
# result.serverInfo.name == "jcodemunch-mcp" after ~38 s on a cold-ish cache. So
# 3.13 is measured against BOTH subcommands directly, not inferred for either.
#
# NOTE for a future bumper who repeats that measurement:
# `result.serverInfo.version` reports upstream's INTERNAL version string
# ("1.29.1" at this pin), NOT the PyPI wheel version. Readiness checks assert on
# serverInfo.NAME for exactly that reason and must not be "tightened" to the
# version.
JC_PYTHON="3.13"

# ── The identity lever ───────────────────────────────────────────────────────
#
# `local/<basename>-<sha1>` is NOT what jcodemunch resolves by default. At the
# pinned 1.108.54 `config.py:384` ships `"git_root_identity": True`, so
# `git_root.py::_configured_identity_mode` answers "git" and
# `resolve_index_identity(mode="config")` takes the git branch for ANY checkout
# with a `.git` and a parseable `origin`. /home/leo/src/reify has
# `origin = https://github.com/leodearden/reify.git`, so it resolved to
# `leodearden/reify` — the per-path branch was never reached. Measured against
# the pinned wheel with a clean store: default config -> `git leodearden/reify`;
# with this env var -> `local/reify-4ae45bbd`.
#
# CARRIED AS AN EXPLICIT `env` ARGV PREFIX rather than an `export`, so that
# `--dry-run` prints a command that actually reproduces this behaviour when
# pasted. That is also why it is an ARRAY and not a string: both consumers
# splice it as `"${JC_IDENTITY_ENV[@]}"` straight into the argv they build, and
# a string-valued copy would word-split differently at the call site.
#
# Consumer-specific rationale deliberately stays OUT of this file: β's two
# "why per-path is worth forcing" legs (incremental survival, worktree
# isolation), its `resolve_index_identity` short-circuit note and its benign
# empty-husk note are β-only reasoning and live in β's header.
JC_IDENTITY_ENV=(env JCODEMUNCH_GIT_ROOT_IDENTITY=0)

# DELIBERATELY NOT EXPORTED. Every consumer sources this file and splices the
# three values into an argv IN-SHELL, so nothing needs them in a child's
# ENVIRONMENT — and bash cannot export an array at all, so exporting the two
# scalars alone would create an asymmetry that reads as an oversight. Keeping
# all three as plain shell variables makes the uniform rule "source this file,
# then build your argv".
