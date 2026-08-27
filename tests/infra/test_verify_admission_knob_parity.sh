#!/usr/bin/env bash
# Infrastructure test for task 6393.
#
# Validates ADMISSION-KNOB PARITY: for the host-global admission knobs, the
# value in dark-factory-orchestrator.yaml's `verify_env` block MUST equal the
# owning script's `:-` fallback default.
#
# WHY THIS INVARIANT IS EQUALITY, NOT "yaml may override" (the incident this
# closes -- 2026-08-19 13:08:47, /deb session deb-reify-4189698):
#   verify_env is injected ONLY by the orchestrator, into the verify
#   subprocesses IT spawns.  A verify.sh run started any other way -- the
#   /unblock manual full gate (`verify.sh all --scope all --profile both`),
#   a hand-run gate, a fixture -- sees NONE of it and falls back to the
#   in-repo defaults.  So a knob whose yaml value diverges from its script
#   default silently splits the fleet into two populations with different
#   admission behaviour, on ONE host-global lock:
#     * REIFY_TEST_SEMAPHORE_WAIT diverged (yaml `unlimited`, script 1800)
#       for ~7 weeks after the clock-stop seam deployed 2026-06-27 (task
#       4838).  Orchestrator verifies waited gracefully; manual ones burned a
#       finite 1800s deadline and exited 75 -- discarding ~55min of completed
#       compile.  The @@REIFY_CLOCK_*@@ markers do NOT help: they pause
#       dark-factory's EXTERNAL verify_command_timeout_secs, never the local
#       `_deadline=$(( _start + _wait ))` in lib_slot_acquire.sh.
#     * REIFY_TEST_SEMAPHORE_CONCURRENCY diverged the same day (yaml 2,
#       script 1).  N is the slot-file COUNT: a participant at N=1 polls only
#       `<lock>.slot-1` and can never take `.slot-2`, so it starves next to a
#       free slot.  Mutual exclusion on a shared lock base is only coherent
#       while every participant agrees on N.
#   Hence: to retune one of these, change BOTH -- the script default is the
#   contract every non-orchestrator participant obeys.
#
# SCOPED DELIBERATELY to the admission knobs.  Most verify_env rows are
# ACTIVATIONS that are *supposed* to differ from the script default (e.g.
# REIFY_RELEASE_DELTA_SKIP, CARGO_HOME) -- a blanket parity rule over the
# whole block would be wrong.  KNOB_TABLE below is the closed set, and
# section (C) guards its COMPLETENESS so a future admission knob cannot be
# added to the yaml without a parity row here.
#
# NO SKIP PATH BY CONSTRUCTION: pure bash + awk + sed, no python/PyYAML.  A
# guard that can silently degrade to SKIP is a guard that reads green while
# broken (cf. the PyYAML SKIP arms in test_cpu_governance_config.sh, whose
# (A)/(A2)/(C) sections this file otherwise mirrors in shape).
#
# Related: scripts/lib_test_semaphore.sh (WAIT/CONCURRENCY defaults),
# scripts/verify.sh psi_gate() (MAX_WAIT default),
# docs/prds/verify-admission-wait-clock-stop.md (the clock-stop seam),
# docs/notes/verify-pipeline-knobs.md (operational digest).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== admission-knob verify_env/default parity tests (task 6393) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# verify_env_exports — parse KEY=VALUE from the yaml verify_env block.
# Mirrored VERBATIM from tests/infra/test_verify_env_ambient_isolation.sh:59-85
# (task 4966's drift-guard), the established copy convention -- see the same
# reuse-note in tests/infra/test_verify_release_delta_skip.sh:435-441 for why
# no shared lib exists.  This is the 3rd mirror; refresh all copies together
# if the awk logic ever changes.
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

# yaml_value <KEY> -> the verify_env value, or "" when the key has no row.
yaml_value() {
    printf '%s\n' "$VERIFY_ENV_KV" | sed -n "s/^$1=//p" | head -1
}

# script_default_count <file> <KNOB> -> number of `${KNOB:-` sites.
# Fixed-string (-F) so the pattern is identical under GNU grep and ugrep.
script_default_count() { grep -cF "\${$2:-" "$1"; }

# script_default <file> <KNOB> -> the text between `:-` and the closing brace.
script_default() { sed -n "s/.*\${$2:-\([^}]*\)}.*/\1/p" "$1" | head -1; }

# ---------------------------------------------------------------------------
# (A) PARSER SANITY — an absence found below must be a real absence, never a
#     parser-shape regression.
# ---------------------------------------------------------------------------
echo ""
echo "--- (A) parser sanity ---"

assert "A1: dark-factory-orchestrator.yaml exists" \
    test -f "$ORCH_YAML"

VERIFY_ENV_KV="$(verify_env_exports "$ORCH_YAML")"

assert "A2: verify_env block parse is non-empty" \
    test -n "$VERIFY_ENV_KV"

# A known-wired sibling row that is NOT one of the knobs under test.
assert "A3: parser sees a known-wired sibling row (REIFY_RUN_ALL_EXCLUDE_HOST_INFRA)" \
    bash -c 'printf "%s\n" "$1" | grep -q "^REIFY_RUN_ALL_EXCLUDE_HOST_INFRA="' _ "$VERIFY_ENV_KV"

# ---------------------------------------------------------------------------
# (B) PARITY — yaml value == owning script's :- fallback default.
# ---------------------------------------------------------------------------
echo ""
echo "--- (B) admission-knob parity (yaml verify_env vs script :- default) ---"

# KNOB_TABLE rows: "<KNOB> <repo-relative owning script>"
KNOB_TABLE="
REIFY_TEST_SEMAPHORE_CONCURRENCY scripts/lib_test_semaphore.sh
REIFY_TEST_SEMAPHORE_WAIT scripts/lib_test_semaphore.sh
REIFY_PSI_GATE_MAX_WAIT scripts/verify.sh
"

while read -r _knob _rel; do
    [ -n "$_knob" ] || continue
    _file="$REPO_ROOT/$_rel"

    assert "B[$_knob]: owning script $_rel exists" \
        test -f "$_file"

    # Exactly ONE `${KNOB:-` site. Zero means the knob was renamed or the
    # default was hard-coded away (the parity check would then silently
    # compare against ""); more than one means the "the default" is
    # ambiguous. Both are drift this guard must surface, not tolerate.
    _n="$(script_default_count "$_file" "$_knob" || true)"
    assert "B[$_knob]: exactly one \${$_knob:-...} default site in $_rel (got ${_n:-0})" \
        test "${_n:-0}" -eq 1

    _script_default="$(script_default "$_file" "$_knob")"
    _yaml_value="$(yaml_value "$_knob")"

    assert "B[$_knob]: has a verify_env row in dark-factory-orchestrator.yaml" \
        test -n "$_yaml_value"

    assert "B[$_knob]: verify_env value '${_yaml_value}' == $_rel default '${_script_default}'" \
        test "$_yaml_value" = "$_script_default"
done <<< "$KNOB_TABLE"

# ---------------------------------------------------------------------------
# (C) COMPLETENESS — every admission-family knob carried by verify_env has a
#     KNOB_TABLE row above.  Without this, adding a 4th admission knob to the
#     yaml would silently escape the parity rule.
#     Families: the test-run semaphore and the PSI gate, i.e. exactly the two
#     gates docs/prds/verify-admission-wait-clock-stop.md §1 names as the
#     verify pipeline's admission waits.
# ---------------------------------------------------------------------------
echo ""
echo "--- (C) KNOB_TABLE completeness over the admission families ---"

_ADMISSION_KEYS_IN_YAML="$(printf '%s\n' "$VERIFY_ENV_KV" \
    | sed -n 's/=.*//p' \
    | grep -E '^REIFY_(TEST_SEMAPHORE|PSI_GATE)_' \
    | sort -u || true)"

_TABLE_KEYS="$(printf '%s\n' "$KNOB_TABLE" | awk 'NF {print $1}' | sort -u)"

_UNCOVERED="$(comm -23 <(printf '%s\n' "$_ADMISSION_KEYS_IN_YAML" | grep -v '^$' || true) \
                       <(printf '%s\n' "$_TABLE_KEYS"))"

assert "C1: every REIFY_{TEST_SEMAPHORE,PSI_GATE}_* verify_env row has a KNOB_TABLE row (uncovered: ${_UNCOVERED:-none})" \
    test -z "$_UNCOVERED"

# Vacuity guard for (C): if the family regex ever stops matching anything, C1
# passes trivially. Pin that the family is non-empty on the live yaml.
assert "C2: the admission families are non-empty in verify_env (vacuity guard for C1)" \
    test -n "$_ADMISSION_KEYS_IN_YAML"

test_summary
