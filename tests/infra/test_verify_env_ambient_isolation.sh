#!/usr/bin/env bash
# Regression guard for task 4966: knob-sensitive tests/infra self-suites must
# behave identically under the production orchestrator.yaml verify_env
# ambient, not just in a clean default shell.
#
# Root cause this guards against: orchestrator.yaml's verify_env block
# exports REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1 and REIFY_GATE_EXCLUDE_HEAVY=1
# (plus sccache/incremental/semaphore/PSI knobs) into the whole verify.sh
# process tree by design. Deploy 65b8412206 flipped both knobs to "1" and
# broke two infra self-suites in one night -- each caught only at L2 after a
# multi-hour debugger loop, because both suites were GREEN standalone (a bare
# invocation never sets the ambient) and only RED in-pipeline (where the
# ambient is genuinely present). Task 4961 guarded test_run_all.sh
# (test_run_all_ambient_isolation.sh). Task 4965 fixed the resulting
# shell-quoting bug in test_occt_flock_gate.sh's T1/T3-T7 asserts
# (5af93e53c0). This file is the general drift-guard: it extracts the FULL
# verify_env export set directly from orchestrator.yaml -- the single source
# the orchestrator itself injects from, so there is no second manifest to
# drift out of sync -- and re-runs a knob-sensitive suite once under that
# exact ambient, so a FUTURE knob flip that breaks ANY suite is caught here
# first instead of at L2.
#
# This file cannot live inside test_occt_flock_gate.sh itself: it drives the
# REAL suite exactly once, as a subprocess, under a hostile ambient export --
# the same shape orchestrator.yaml's verify_env produces -- and asserts the
# nested suite still exits 0 with 0 failed. Mirrors the run-the-real-suite-
# once idiom from test_run_all_ambient_isolation.sh (task 4961).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$REPO_ROOT/orchestrator.yaml" ] || { echo "ERROR: orchestrator.yaml not found at $REPO_ROOT/orchestrator.yaml"; exit 1; }

echo "=== verify_env ambient-isolation drift-guard (task 4966) ==="

# ---------------------------------------------------------------------------
# verify_env_exports <yaml_file>
#
# Extracts orchestrator.yaml's `verify_env:` block as a stream of
# `KEY=VALUE` lines (one per line). Values are simple tokens
# (0/1/2/sccache/unlimited), so callers can `export "KEY=VALUE"` each
# emitted line directly with no further quoting/escaping.
#
# Single awk pass: enter the block on a top-level `verify_env:` line; a
# later top-level, non-comment line (anything NOT starting with whitespace
# or `#`) ends the block, so trailing top-level `#` comments (e.g. the
# jobserver-wiring note preceding orchestrator.yaml's `jobserver:` block)
# do not end it early. While in-block, blank lines and `#` comment lines
# (indented or not) are skipped. Each `  KEY: VALUE` line emits `KEY=VALUE`:
# a double-quoted value is captured verbatim between the first pair of
# quotes; a bare value is reduced to its first whitespace-delimited token
# (dropping any trailing inline comment).
# ---------------------------------------------------------------------------
verify_env_exports() {
    local yaml_file="$1"
    awk '
        /^verify_env:[[:space:]]*$/ { in_block = 1; next }
        in_block && /^[^[:space:]#]/ { in_block = 0 }
        !in_block { next }
        /^[[:space:]]*#/ { next }
        /^[[:space:]]*$/ { next }
        /^[[:space:]]+[A-Za-z_][A-Za-z0-9_]*:/ {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            colon = index(line, ":")
            key = substr(line, 1, colon - 1)
            rest = substr(line, colon + 1)
            sub(/^[[:space:]]+/, "", rest)
            if (substr(rest, 1, 1) == "\"") {
                tail = substr(rest, 2)
                q = index(tail, "\"")
                val = (q > 0) ? substr(tail, 1, q - 1) : tail
            } else {
                n = split(rest, toks, /[[:space:]]+/)
                val = (n >= 1) ? toks[1] : ""
            }
            print key "=" val
        }
    ' "$yaml_file"
}

# ---------------------------------------------------------------------------
# Extractor correctness (a): synthetic fixture.
#
# Exercises block-entry, block-exit, quoted values, bare values, an in-block
# comment, and a blank line -- independent of whatever orchestrator.yaml
# happens to contain today.
# ---------------------------------------------------------------------------
echo ""
echo "--- Extractor correctness (a): synthetic fixture ---"

_FIXTURE="$(mktemp)"
trap 'rm -f "$_FIXTURE"' EXIT

cat > "$_FIXTURE" <<'FIXTURE_EOF'
pre_block_key: pre_block_value
verify_env:
  ALPHA: "1"
  BETA: bare_value
  # in-block comment line, must be skipped
  GAMMA: "unlimited"
next_block:
  child_key: child_value
FIXTURE_EOF

_FIXTURE_ACTUAL="$(verify_env_exports "$_FIXTURE" | sort)"
_FIXTURE_EXPECTED="$(printf '%s\n' 'ALPHA=1' 'BETA=bare_value' 'GAMMA=unlimited' | sort)"

assert "fixture: verify_env_exports emits exactly ALPHA=1, BETA=bare_value, GAMMA=unlimited (sort-compare)" \
    test "$_FIXTURE_ACTUAL" = "$_FIXTURE_EXPECTED"

assert "fixture: verify_env_exports does not leak the pre-block top-level key" \
    bash -c '! printf "%s\n" "$1" | grep -qF "pre_block"' _ "$_FIXTURE_ACTUAL"

assert "fixture: verify_env_exports does not leak the next_block child key" \
    bash -c '! printf "%s\n" "$1" | grep -qF "child_key"' _ "$_FIXTURE_ACTUAL"

assert "fixture: verify_env_exports does not leak the in-block comment line" \
    bash -c '! printf "%s\n" "$1" | grep -qF "in-block comment"' _ "$_FIXTURE_ACTUAL"

# ---------------------------------------------------------------------------
# Extractor correctness (b): real orchestrator.yaml.
#
# Non-vacuity: proves the emitted set actually carries the two knobs that
# caused the escapes, and that block-exit correctly stops before the
# following `jobserver:` block's `enabled: true` (a real adjacent top-level
# key in orchestrator.yaml today).
# ---------------------------------------------------------------------------
echo ""
echo "--- Extractor correctness (b): real orchestrator.yaml ---"

_REAL_ACTUAL="$(verify_env_exports "$REPO_ROOT/orchestrator.yaml")"

assert "orchestrator.yaml: verify_env_exports output contains REIFY_GATE_EXCLUDE_HEAVY=1" \
    bash -c 'printf "%s\n" "$1" | grep -qxF "REIFY_GATE_EXCLUDE_HEAVY=1"' _ "$_REAL_ACTUAL"

assert "orchestrator.yaml: verify_env_exports output contains REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1" \
    bash -c 'printf "%s\n" "$1" | grep -qxF "REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1"' _ "$_REAL_ACTUAL"

assert "orchestrator.yaml: verify_env_exports output does not leak jobserver's enabled=true (block-end guard)" \
    bash -c '! printf "%s\n" "$1" | grep -qxF "enabled=true"' _ "$_REAL_ACTUAL"

assert "orchestrator.yaml: verify_env_exports output is non-empty and every line is well-formed KEY=... " \
    bash -c '[ -n "$1" ] && ! printf "%s\n" "$1" | grep -vE "^[A-Za-z_][A-Za-z0-9_]*="' _ "$_REAL_ACTUAL"

# ---------------------------------------------------------------------------
# End-to-end: test_occt_flock_gate.sh under the REAL production ambient.
#
# Mirrors test_run_all_ambient_isolation.sh (task 4961)'s run-the-real-
# suite-once idiom, generalized from one hardcoded knob to the FULL
# verify_env export set. The export loop and probe run inside the
# `$( ... )` command-substitution subshell only, so none of the ambient
# exports leak back out to this script; `verify_env_exports` is a shell
# function and is inherited by the subshell like any other.
#
# Non-vacuity: post-4965, test_occt_flock_gate.sh exits 0 under BOTH the
# default env and this ambient, so its exit code alone can't prove the
# ambient was actually applied. The in-subshell probe asserts
# REIFY_GATE_EXCLUDE_HEAVY=1 is genuinely set (exit 99 -> RED) before the
# suite ever runs, so "occt exits 0 under a PROVABLY hostile ambient" is the
# real, non-vacuous claim below.
# ---------------------------------------------------------------------------
echo ""
echo "--- End-to-end: test_occt_flock_gate.sh under the real verify_env ambient ---"

amb_rc=0
amb_out="$(
    while IFS= read -r _kv; do
        export "$_kv"
    done < <(verify_env_exports "$REPO_ROOT/orchestrator.yaml")
    [ "${REIFY_GATE_EXCLUDE_HEAVY:-}" = "1" ] || { echo "AMBIENT-NOT-APPLIED"; exit 99; }
    bash "$SCRIPT_DIR/test_occt_flock_gate.sh" 2>&1
)" || amb_rc=$?

assert "test_occt_flock_gate.sh exits 0 under the real verify_env ambient (got rc=$amb_rc)" \
    test "$amb_rc" -eq 0

# Anchored line match (not a substring grep) so an inner mock's own
# "0 failed"-shaped output could never false-pass this assertion -- only the
# nested test_occt_flock_gate.sh's OWN test_summary line qualifies.
if printf '%s\n' "$amb_out" | grep -qE '^Results: [0-9]+ passed, 0 failed$'; then
    assert "test_occt_flock_gate.sh reports 0 failed under the real verify_env ambient" true
else
    assert "test_occt_flock_gate.sh reports 0 failed under the real verify_env ambient (got: $amb_out)" false
fi

test_summary
