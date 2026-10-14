#!/usr/bin/env bash
# tests/infra/test_tree_sitter_parse_isolation.sh
#
# Structural grammar-cache-isolation guard (task #5925).
#
# INVARIANT: no tracked file on the EXECUTABLE or AGENT-INSTRUCTION surface may
# invoke `tree-sitter parse` in command position without an XDG_CACHE_HOME
# override on the same line.
#
# WHY. `tree-sitter parse` compiles the grammar to
# $XDG_CACHE_HOME/tree-sitter/lib/<language-name>.so. That artifact is keyed by
# LANGUAGE NAME — not by grammar path — and invalidated only on source mtime.
# Every one of the ~235 linked worktrees of this store therefore resolves to the
# SAME reify.so, so a patched build made in any other lane silently answers this
# lane's parses. The failure mode is what makes it worth a hard gate: the other
# two grammar-gate preconditions (wrong CWD, ungenerated grammar) fail LOUDLY
# with "No language found" and exit 1, whereas this one fails SILENTLY with a
# plausible exit code and no diagnostic. Recorded precedent, not hypothetical:
# docs/prds/v0_6/angle-units-surface-convergence.capability-manifest.md C2, where
# a mid-decompose reading returned exit 0 while a concurrent HYP-A build held the
# cache.
#
# Note the hazard is not fixed by merely relocating the shared dir. This host
# exports XDG_CACHE_HOME=/tmp/reify-agent-xdg-cache for every agent
# (dark-factory-orchestrator.yaml), which is still ONE artifact shared by every
# lane; isolation has to be per-lane to mean anything.
#
# SCOPE, and why it is a scope decision rather than an exemption list.
# A whole-repo text scan is unsatisfiable: 170+ tracked files mention
# `tree-sitter parse` in prose (PRD bodies, .ri fixture header comments, Rust
# doc comments). The discriminator is COMMAND POSITION. Measured on this tree:
# the Block-B pattern matches 7 lines repo-wide, and exactly 2 inside SCOPE.
#
#   * docs/** is OUT of scope. Its matches live in
#     docs/prds/v0_6/*.capability-manifest.{md,yaml} and are ARCHIVAL EVIDENCE of
#     probes executed on a stated date ("probes 2026-07-25: ... exit 1"). They
#     are records, not instructions; rewriting them would falsify a historical
#     measurement. Excluding a class of file whose content is a record is a
#     scope decision — naming an individual offending file would be an
#     exemption, and this guard has none.
#   * crates/ and tree-sitter-reify/ are IN scope even though their Rust code is
#     structurally immune (it drives the linked tree_sitter::Parser, never the
#     CLI cache). Measured cost of keeping them: zero hits. The benefit is that a
#     future shell-out from a build.rs or a test script is caught.
#
# LIMITATION, stated honestly. Block B anchors on command position, so it cannot
# see an invocation embedded mid-pipeline (`... | xargs tree-sitter parse`) or an
# argv built programmatically — and the ONE real caller in this repo is exactly
# that second shape: scripts/prd-capability-check.py's build_command() emits
# [ts_bin, "parse", "--quiet", fixture] with ts_bin from
# _resolve_tree_sitter_bin(), so the literal string never appears anywhere.
# Text alone would therefore be false confidence about the very caller this task
# exists to fix. Block D is the structural half that covers it, paired with the
# behavioural half in scripts/test_prd_capability_check.py
# (TestGrammarCacheIsolation / TestGrammarCacheHome), which runs a real probe
# against a stub that echoes the XDG_CACHE_HOME the CHILD was handed.
#
# Blocks:
#   A: the enumeration + scan pipeline is live (guard is not vacuously green)
#   B: THE INVARIANT — no unguarded command-position invocation inside SCOPE
#   C: positive control — the pattern matches a bare call and the XDG filter
#      suppresses a guarded one (proves the discriminator discriminates)
#   D: structural cross-check on the one programmatic caller — both launch
#      sites in prd-capability-check.py pass env=

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== tree-sitter parse cache-isolation structural guard (task #5925) ==="

TMPDIR_TSISO=$(mktemp -d "${TMPDIR:-/tmp}/test-ts-parse-isolation-XXXXXX")
trap 'rm -rf "$TMPDIR_TSISO"' EXIT

# The executable + agent-instruction surface. Anything that RUNS or INSTRUCTS.
SCOPE=(scripts tests hooks .claude gui tree-sitter-reify crates)

# ── Pattern ──────────────────────────────────────────────────────────────────
#
# Command position: start of line, optional leading whitespace, an optional `$ `
# shell prompt (the transcript idiom used across the PRD docs), then any number
# of VAR=value environment prefixes, then the invocation.
#
# POSIX ERE only — no `grep -P`. Perl mode is silently unavailable on BSD grep,
# which would make a NEGATED assertion like Block B false-positive into green on
# a host where the pattern never compiles. Same rule as
# scripts/test_tree_sitter_generate.sh Test 11.
#
# Self-avoidance: the `tree-[s]itter` char class means this guard's own source
# can never match its own pattern, following the house precedent at
# scripts/test_tree_sitter_generate.sh:186 (`grep "grep -[P]"`).
PAT_TS_PARSE='^[[:space:]]*(\$ )?([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]+[[:space:]]+)*tree-[s]itter[[:space:]]+parse'

# A line is GUARDED when it carries an XDG_CACHE_HOME override. There is no
# allow-marker and no exemption list: after the grammar-gate.md fix the offender
# set inside SCOPE is empty, so the guard needs no escape hatch to be green.
PAT_GUARDED='XDG_CACHE_HOME'

# The one programmatic caller, checked structurally by Block D instead.
PCC='scripts/prd-capability-check.py'

# _tracked_in_scope — every tracked file under SCOPE, one per line.
_tracked_in_scope() {
    git -C "$REPO_ROOT" ls-files -- "${SCOPE[@]}"
}

# _scan_unguarded — print `path:line:text` for every command-position
# `tree-sitter parse` inside SCOPE that carries no XDG_CACHE_HOME on its line.
# Exits 0 regardless of whether anything matched; callers inspect the output.
_scan_unguarded() {
    local f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        [ -f "$REPO_ROOT/$f" ] || continue
        grep -HnE "$PAT_TS_PARSE" "$REPO_ROOT/$f" 2>/dev/null \
            | grep -vE "$PAT_GUARDED" \
            | sed "s|^$REPO_ROOT/||" || true
    done < <(_tracked_in_scope)
}

# ==============================================================================
# Block A: the enumeration and the scan pipeline are live
# ==============================================================================
echo ""
echo "--- Block A: enumeration is non-empty and the scan pipeline runs ---"

_enumeration_non_empty() {
    local n
    n="$(_tracked_in_scope | wc -l)"
    echo "enumerated $n tracked files under: ${SCOPE[*]}"
    [ "$n" -gt 0 ]
}

assert "git ls-files over the in-scope pathspec yields at least one file" \
    _enumeration_non_empty

# A mistyped pathspec, a broken pipe or an uncompilable ERE must not read as a
# pass. Running the scan and requiring a clean exit separates "found nothing"
# from "never looked".
_scan_pipeline_runs() {
    _scan_unguarded >/dev/null
}

assert "the unguarded-invocation scan completes cleanly over the in-scope files" \
    _scan_pipeline_runs

# ==============================================================================
# Block B: THE INVARIANT
# ==============================================================================
echo ""
echo "--- Block B: no unguarded command-position \`tree-sitter parse\` in scope ---"

_no_unguarded_invocation() {
    local hits
    hits="$(_scan_unguarded)"
    [ -z "$hits" ] || {
        echo "VIOLATION: command-position \`tree-sitter parse\` with no XDG_CACHE_HOME on the line:"
        printf '%s\n' "$hits" | sed 's/^/  /'
        echo ""
        echo "WHY THIS IS A BUG: the compiled grammar is cached at"
        echo "  \$XDG_CACHE_HOME/tree-sitter/lib/<language-name>.so"
        echo "keyed by LANGUAGE NAME and invalidated only on source mtime, so every"
        echo "linked worktree of this store shares ONE reify.so. A patched build in any"
        echo "other lane silently answers your parses — exit code plausible, no warning."
        echo ""
        echo "REMEDY: mint one session-scoped cache dir and prefix the invocation:"
        echo "  TS_CACHE=\"\${TS_CACHE:-\$(mktemp -d /tmp/prd-gate-ts-cache-XXXXXX)}\""
        echo "  XDG_CACHE_HOME=\"\$TS_CACHE\" tree-sitter parse --quiet <fixture>"
        echo ""
        echo "The PRD gate itself needs no manual step: $PCC isolates grammar probes"
        echo "automatically (per-repo-root, grammar-fingerprinted, under \$TMPDIR)."
        return 1
    }
}

assert "no tracked file under ${SCOPE[*]} invokes \`tree-sitter parse\` without XDG_CACHE_HOME" \
    _no_unguarded_invocation

# ==============================================================================
# Block C: positive control — the discriminator actually discriminates
# ==============================================================================
echo ""
echo "--- Block C: positive control on synthetic bare / guarded invocations ---"

CONTROL="$TMPDIR_TSISO/control.txt"
{
    printf 'tree-sitter parse --quiet /tmp/x.ri\n'
    printf 'XDG_CACHE_HOME=/tmp/c tree-sitter parse --quiet /tmp/x.ri\n'
} > "$CONTROL"

_pattern_matches_bare_invocation() {
    local n
    n="$(grep -cE "$PAT_TS_PARSE" "$CONTROL" || true)"
    echo "pattern matched $n/2 synthetic lines (both are command-position calls)"
    [ "$n" -eq 2 ]
}

assert "the command-position pattern matches BOTH synthetic invocations" \
    _pattern_matches_bare_invocation

_guard_filter_suppresses_isolated_invocation() {
    local hits n
    hits="$(grep -HnE "$PAT_TS_PARSE" "$CONTROL" | grep -vE "$PAT_GUARDED" || true)"
    n="$(printf '%s' "$hits" | grep -c . || true)"
    echo "after the XDG_CACHE_HOME filter, $n/2 synthetic lines remain:"
    printf '%s\n' "$hits" | sed 's/^/  /'
    # Exactly the bare one survives: 1 means the filter suppresses the guarded
    # line without suppressing everything (which would make Block B vacuous).
    [ "$n" -eq 1 ] && printf '%s' "$hits" | grep -qv 'XDG_CACHE_HOME'
}

assert "the XDG_CACHE_HOME filter suppresses the guarded line and only that one" \
    _guard_filter_suppresses_isolated_invocation

# ==============================================================================
# Block D: structural cross-check on the one programmatic caller
#
# Block B is blind to this file BY CONSTRUCTION: build_command() emits
# [ts_bin, "parse", "--quiet", fixture], so the literal string never appears.
# run_probe() has TWO launch arms and the non-obvious one is load-bearing —
# grammar_substrate_usable() calls run_probe(probe, timeout=...), which goes
# through _run_bounded()'s Popen, so the FIRST real `tree-sitter parse` any gate
# runs takes the arm a naive fix would miss. Both are asserted here so a later
# refactor that drops env= from one, or adds a third launch site, reds at the
# merge gate rather than as a silent false PASS.
# ==============================================================================
echo ""
echo "--- Block D: both launch sites in $PCC pass env= ---"

_pcc_exists() {
    [ -f "$REPO_ROOT/$PCC" ]
}

assert "$PCC exists (Block D is not vacuous)" _pcc_exists

_pcc_mentions_xdg_cache_home() {
    grep -q 'XDG_CACHE_HOME' "$REPO_ROOT/$PCC"
}

assert "$PCC sets XDG_CACHE_HOME for its grammar probes" \
    _pcc_mentions_xdg_cache_home

# Both launch calls are multi-line, so locate each call site and require an
# `env=` argument before the block's closing paren. `grep -Pzo` would be the
# direct multi-line spelling and is banned here (POSIX ERE only), so this walks
# the block with sed.
#
# ONLY REAL CALL SITES COUNT. The docstrings mention `subprocess.run()` in prose
# twice, and an earlier draft of this check keyed on the bare string: the prose
# mention opened a block that ran on until it found the OTHER arm's `env=`, so
# the check reported PASS with env= stripped from the arm under test. That
# false-negative is precisely the failure mode this block exists to prevent, so
# it is worth naming. The discriminator is the empty-paren form: prose is always
# `subprocess.run()`, a real call never is.
#
# Every real site must pass env= — not merely one of them — and finding zero
# sites is itself a failure, so the check cannot go vacuously green if the calls
# are renamed or restructured.
_launch_site_passes_env() {
    local opener_re="$1" label="$2"
    local lines n ln bad=0
    lines="$(grep -nE "${opener_re}\(" "$REPO_ROOT/$PCC" \
        | grep -vE "${opener_re}\(\)" | cut -d: -f1)"
    n="$(printf '%s' "$lines" | grep -c . || true)"
    echo "found $n real $label call site(s) in $PCC"
    [ "$n" -gt 0 ] || {
        echo "VIOLATION: no real $label call site found — this check would be vacuous."
        echo "The launch was renamed or restructured; re-point this guard at it."
        return 1
    }
    while IFS= read -r ln; do
        [ -n "$ln" ] || continue
        if ! sed -n "${ln},/^[[:space:]]*)/p" "$REPO_ROOT/$PCC" | grep -q 'env='; then
            echo "VIOLATION: $label call site at $PCC:$ln does not pass env="
            bad=1
        fi
    done <<< "$lines"
    [ "$bad" -eq 0 ]
}

_run_arm_passes_env() {
    _launch_site_passes_env 'subprocess\.run' 'subprocess.run' \
        || { echo "The unbounded arm (run_probe) must hand grammar probes their private cache."; \
             return 1; }
}

_popen_arm_passes_env() {
    _launch_site_passes_env 'subprocess\.Popen' 'subprocess.Popen' \
        || { echo "This is the arm grammar_substrate_usable() takes — the FIRST real parse a gate runs."; \
             return 1; }
}

assert "the UNBOUNDED launch (subprocess.run in run_probe) passes env=" \
    _run_arm_passes_env

assert "the BOUNDED launch (subprocess.Popen in _run_bounded) passes env=" \
    _popen_arm_passes_env

test_summary
