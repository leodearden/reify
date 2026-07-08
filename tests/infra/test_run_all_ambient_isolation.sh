#!/usr/bin/env bash
# Regression guard for task 4961 (esc-4906-45): test_run_all.sh's knob-UNSET
# sub-cases must behave identically whether an env var ambiently injected by
# the merge-tier verify pipeline is genuinely unset OR inherited as an
# ambient export.
#
# Root cause this guards against: scripts/verify.sh's run_all.sh plan line
# (verify.sh:1430) carries a prefix env (currently REIFY_AUDIT_NO_COLD_BUILD=1
# REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1), and orchestrator.yaml's verify_env
# block (orchestrator.yaml:148-152) exports more vars (REIFY_GATE_EXCLUDE_HEAVY,
# RUSTC_WRAPPER, CARGO_INCREMENTAL, REIFY_RUN_ALL_EXCLUDE_HOST_INFRA) into the
# whole verify.sh process tree by design. Both sets are inherited AMBIENTLY by
# every one of run_all.sh's ~103 pool tests. test_run_all.sh's own
# knob-UNSET sub-cases (T9a/T9b/T13b/T14b/T17c) used to invoke run_all.sh via
# bare prefix-assignment (`RUN_ALL_CLASSIFICATION_MANIFEST=... bash
# "$RUN_ALL" ...`), which sets only the named vars and does NOT clear an
# inherited exported ambient var. Under the orchestrator those "unset"
# sub-cases silently inherited REIFY_RUN_ALL_EXCLUDE_HOST_INFRA=1, dropped
# their fixtures' host-exclusive members from discovery (run_all.sh's H3
# flip-seam, task 4925), and failed their discovered-count / hostx-header
# assertions -- blocking the verify gate for every --include-infra task.
#
# This file cannot live inside test_run_all.sh itself: self-invocation would
# recurse infinitely (test_run_all.sh IS the discovery runner's test
# subject). Instead it drives the REAL test_run_all.sh exactly once per
# ledger var, as a subprocess, under a hostile ambient export -- the same
# shape orchestrator.yaml's verify_env / the run_all.sh plan line's prefix
# env produce -- and asserts the nested suite still exits 0. test_run_all.sh
# never invokes run_all.sh against the real tests/infra/ directory (all of
# its sub-cases use temp-dir fixtures), so these direct invocations do not
# themselves recurse.
#
# Task 5152 (coverage-completeness follow-up): generalized from a single
# hardcoded var (REIFY_RUN_ALL_EXCLUDE_HOST_INFRA) to every var declared in
# the checked-in ledger tests/infra/run-all-ambient-vars.manifest. The
# ledger is cross-checked below against the LIVE injected-var set derived
# from the real injection source(s), so a var newly added there without a
# matching ledger entry fails this guard by construction -- closing the
# exact gap that left REIFY_AUDIT_NO_COLD_BUILD uncovered (its hostile
# sub-case would previously have passed vacuously anyway, since it makes the
# reify-audit freshness guard SKIP rather than fail -- see
# run-all-ambient-vars.manifest's header).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

TARGET="$SCRIPT_DIR/test_run_all.sh"
[ -f "$TARGET" ] || { echo "ERROR: test_run_all.sh not found at $TARGET"; exit 1; }

MANIFEST="$SCRIPT_DIR/run-all-ambient-vars.manifest"
[ -f "$MANIFEST" ] || { echo "ERROR: run-all-ambient-vars.manifest not found at $MANIFEST (task 5152 ledger of every env var ambiently injected into the run_all.sh pool)"; exit 1; }

[ -f "$REPO_ROOT/orchestrator.yaml" ] || { echo "ERROR: orchestrator.yaml not found at $REPO_ROOT/orchestrator.yaml"; exit 1; }

echo "=== run_all.sh ambient-isolation regression guard (task 4961 / esc-4906-45; manifest-driven, task 5152) ==="

# ---------------------------------------------------------------------------
# kv_lookup <kv_multiline> <key>
#
# Looks up <key> in a newline-separated list of KEY=VALUE lines. Prints the
# value to stdout and returns 0 on the first match; returns 1 with no output
# if <key> is absent. Fork-free (bash read loop + case, no pipe/subshell).
# ---------------------------------------------------------------------------
kv_lookup() {
    local _kv="$1" _key="$2" _line
    while IFS= read -r _line; do
        case "$_line" in
            "$_key="*)
                printf '%s' "${_line#*=}"
                return 0
                ;;
        esac
    done <<< "$_kv"
    return 1
}

# ---------------------------------------------------------------------------
# kv_key_member <key> <keys_multiline>
#
# Returns 0 iff <key> appears as a whole line in <keys_multiline>. Fork-free.
# ---------------------------------------------------------------------------
kv_key_member() {
    local _key="$1" _keys="$2" _line
    while IFS= read -r _line; do
        [ "$_line" = "$_key" ] && return 0
    done <<< "$_keys"
    return 1
}

# ---------------------------------------------------------------------------
# Derivation (source 1 of 2): the run_all.sh plan line's prefix env.
#
# Direct-REPO_ROOT `verify.sh ... --print-plan` capture (no git fixture) --
# mirrors tests/infra/test_verify_gate_exclude_heavy.sh:63-64. --print-plan is
# hermetic (never runs cargo/tests). DF_VERIFY_ROLE=merge selects the tier
# whose plan carries the run_all.sh line (scripts/verify.sh:1430);
# REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 keeps the nextest-availability probe
# fast. run_all.sh does not itself export the REIFY_INFRA_SUITE_ACTIVE
# re-entrancy sentinel (only test_verify_semaphore_e2e.sh's Section B spawn
# site does), so this in-pool capture is not suppressed by it -- env -u'ing
# it here is belt-and-braces.
# ---------------------------------------------------------------------------
echo ""
echo "--- Derivation (1/2): run_all.sh plan-line prefix env (scripts/verify.sh --print-plan) ---"

PLAN_DUMP=""
capture_print_plan PLAN_DUMP 3 \
    env -u REIFY_INFRA_SUITE_ACTIVE DF_VERIFY_ROLE=merge REIFY_NEXTEST_PROBE_RETRY_SLEEP=0 \
    bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan || true

RUN_ALL_PLAN_LINE=""
while IFS= read -r _line; do
    case "$_line" in
        *"bash tests/infra/run_all.sh"*)
            RUN_ALL_PLAN_LINE="$_line"
            ;;
    esac
done <<< "$PLAN_DUMP"

assert "merge-role plan contains the run_all.sh command line (non-vacuity: a plan-format change must fail loudly here, not yield an empty extraction below)" \
    test -n "$RUN_ALL_PLAN_LINE"

# Extract the leading KEY=VALUE prefix-env tokens between "then" and whatever
# follows (timeout/bash/...). Fork-free: bash regex + BASH_REMATCH, no
# sed/awk/pipe/subshell. The repeated token group's own shape (identifier=
# value) is what bounds the match on the right -- it stops at the first word
# that isn't KEY=VALUE-shaped (e.g. "timeout"), so this is robust to
# reordering and does not need to name "timeout"/"bash" explicitly (and
# cannot be fooled by later `=`-bearing tokens like --kill-after=60, since
# those never appear directly after "then").
PLAN_LINE_KV=""
if [[ "$RUN_ALL_PLAN_LINE" =~ then[[:space:]]+(([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]+[[:space:]]+)+) ]]; then
    _plan_prefix="${BASH_REMATCH[1]}"
    while [[ "$_plan_prefix" =~ ([A-Za-z_][A-Za-z0-9_]*)=([^[:space:]]+) ]]; do
        PLAN_LINE_KV+="${BASH_REMATCH[1]}=${BASH_REMATCH[2]}"$'\n'
        _plan_prefix="${_plan_prefix#*"${BASH_REMATCH[1]}=${BASH_REMATCH[2]}"}"
    done
fi

assert "run_all.sh plan-line prefix-env extraction is non-empty" \
    test -n "$PLAN_LINE_KV"

assert "run_all.sh plan-line prefix-env contains REIFY_AUDIT_NO_COLD_BUILD (previously-uncovered var, task 5152)" \
    plan_match "$PLAN_LINE_KV" '^REIFY_AUDIT_NO_COLD_BUILD='

PLAN_LINE_KEYS=""
while IFS= read -r _kv; do
    [ -z "$_kv" ] && continue
    PLAN_LINE_KEYS+="${_kv%%=*}"$'\n'
done <<< "$PLAN_LINE_KV"

# ---------------------------------------------------------------------------
# Derivation (source 2 of 2): orchestrator.yaml's verify_env block.
#
# verify_env_exports is mirrored VERBATIM (reuse-note, not extracted to a
# shared lib) from tests/infra/test_verify_env_ambient_isolation.sh:59-85
# (task 4966's drift-guard), to avoid editing that live merge-gate guard
# file -- out of this task's declared scope. Refresh both copies together if
# the awk logic ever changes.
# ---------------------------------------------------------------------------
echo ""
echo "--- Derivation (2/2): orchestrator.yaml verify_env block (verify_env_exports) ---"

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

VERIFY_ENV_KV="$(verify_env_exports "$REPO_ROOT/orchestrator.yaml")"

assert "orchestrator.yaml verify_env block parse is non-empty" \
    test -n "$VERIFY_ENV_KV"

assert "verify_env block contains REIFY_RUN_ALL_EXCLUDE_HOST_INFRA (block-parse sanity)" \
    plan_match "$VERIFY_ENV_KV" '^REIFY_RUN_ALL_EXCLUDE_HOST_INFRA='

assert "verify_env block contains REIFY_GATE_EXCLUDE_HEAVY (block-parse sanity)" \
    plan_match "$VERIFY_ENV_KV" '^REIFY_GATE_EXCLUDE_HEAVY='

assert "verify_env block contains RUSTC_WRAPPER (block-parse sanity)" \
    plan_match "$VERIFY_ENV_KV" '^RUSTC_WRAPPER='

assert "verify_env block contains CARGO_INCREMENTAL (block-parse sanity)" \
    plan_match "$VERIFY_ENV_KV" '^CARGO_INCREMENTAL='

VERIFY_ENV_KEYS=""
while IFS= read -r _kv; do
    [ -z "$_kv" ] && continue
    VERIFY_ENV_KEYS+="${_kv%%=*}"$'\n'
done <<< "$VERIFY_ENV_KV"

# ---------------------------------------------------------------------------
# LIVE_KEYS: sort-unique union of both injection sources. LIVE_KV mirrors the
# union at the KEY=VALUE level so the hostile-ambient loop below can look up
# a live value for a verify_env-only ledger var, not just a plan-line one.
# ---------------------------------------------------------------------------
LIVE_KV="${PLAN_LINE_KV}${VERIFY_ENV_KV}"
LIVE_KEYS="$(sort -u <<< "${PLAN_LINE_KEYS}${VERIFY_ENV_KEYS}")"

# ---------------------------------------------------------------------------
# Ledger: tests/infra/run-all-ambient-vars.manifest (keys only, one per
# line). Read defensively -- strip comment/blank lines -- mirroring
# run-all-classification-lib.sh's manifest-reading convention.
# ---------------------------------------------------------------------------
echo ""
echo "--- Ledger: tests/infra/run-all-ambient-vars.manifest ---"

MANIFEST_KEYS=""
while IFS= read -r _line; do
    case "$_line" in
        ''|'#'*) continue ;;
    esac
    MANIFEST_KEYS+="$_line"$'\n'
done < "$MANIFEST"

assert "run-all-ambient-vars.manifest declares at least one var" \
    test -n "$MANIFEST_KEYS"

# Full set-equality guard (upgraded from the step-1 plan-line-only subset
# guard, task 5152): every LIVE var (plan-line UNION verify_env) must be
# declared in the ledger, AND every ledger var must still be part of the
# LIVE set -- together these two directions are full set equality, so a var
# newly injected by either source without a matching ledger entry, or a
# stale ledger entry for a var no longer injected by either source, fails
# this guard by construction (task 5152 design decision: equality, not mere
# subset).
while IFS= read -r _live_key; do
    [ -z "$_live_key" ] && continue
    assert "live var $_live_key is declared in run-all-ambient-vars.manifest (add it there)" \
        kv_key_member "$_live_key" "$MANIFEST_KEYS"
done <<< "$LIVE_KEYS"

while IFS= read -r _manifest_key; do
    [ -z "$_manifest_key" ] && continue
    assert "run-all-ambient-vars.manifest's $_manifest_key is still part of the live injected set (remove if stale)" \
        kv_key_member "$_manifest_key" "$LIVE_KEYS"
done <<< "$MANIFEST_KEYS"

# ---------------------------------------------------------------------------
# Manifest-driven hostile-ambient loop: for each ledger var, run the REAL
# test_run_all.sh once with every OTHER ledger var unset and just that one
# var exported to its live value -- the same shape orchestrator.yaml's
# verify_env / the run_all.sh plan line's prefix env produce. `unset` +
# `export` + the nested `bash "$TARGET"` all run inside the `$( ... )`
# command-substitution subshell, so none of it leaks back out to this
# script. Anchored line match (not a substring grep) so an inner mock's own
# "0 failed"-shaped output could never false-pass this assertion -- only the
# nested test_run_all.sh's OWN test_summary line qualifies.
# ---------------------------------------------------------------------------
echo ""
echo "--- Manifest-driven hostile-ambient loop: test_run_all.sh under each ledger var, ambiently ---"

while IFS= read -r _key; do
    [ -z "$_key" ] && continue

    _val="$(kv_lookup "$LIVE_KV" "$_key")" || {
        echo "ERROR: no live value found for ledger var $_key (checked plan-line + verify_env KV)"
        exit 1
    }

    echo ""
    echo "--- hostile-ambient: $_key=$_val (every other ledger var unset) ---"

    amb_rc=0
    amb_out="$(
        while IFS= read -r _unset_key; do
            [ -z "$_unset_key" ] && continue
            unset "$_unset_key"
        done <<< "$MANIFEST_KEYS"
        export "$_key=$_val"
        bash "$TARGET" 2>&1
    )" || amb_rc=$?

    assert "test_run_all.sh exits 0 under ambient $_key=$_val (got rc=$amb_rc)" \
        test "$amb_rc" -eq 0

    if plan_match "$amb_out" '^Results: [0-9]+ passed, 0 failed$'; then
        assert "test_run_all.sh reports 0 failed under ambient $_key=$_val" true
    else
        assert "test_run_all.sh reports 0 failed under ambient $_key=$_val (got: $amb_out)" false
    fi
done <<< "$MANIFEST_KEYS"

test_summary
