#!/usr/bin/env bash
# tests/infra/test_agent_cargo_shim.sh — integration tests for scripts/agent-bin/cargo PSI shim.
#
# Drives the cargo shim in isolation with injected PSI fixtures and a hermetic
# stub 'real' cargo, verifying the β-layer shim contract (C-S1 transparent,
# C-S2 semantics-preserving).  Modeled on tests/infra/test_cpu_admit.sh.
#
# Skip guard: exits 0 (skip) on hosts without /proc/pressure/cpu.
# Fail-open (missing PSI source) is still exercised via PROC_PATH override.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SHIM="$REPO_ROOT/scripts/agent-bin/cargo"

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $REPO_ROOT/tests/infra/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$REPO_ROOT/tests/infra/test_helpers.sh"

if [ ! -r /proc/pressure/cpu ]; then
    echo "SKIP: kernel lacks /proc/pressure/cpu (PSI gate is Linux-only)"
    exit 0
fi

WORKDIR="$(mktemp -d)"
STUB_DIR="$WORKDIR/stub-cargo-bin"
mkdir -p "$STUB_DIR"
trap 'rm -rf "$WORKDIR"' EXIT

# ---------------------------------------------------------------------------
# Hermeticity: neutralize default-ON memory gating (task 4911) for shim
# invocations that do not set REIFY_CPU_ADMIT_MEM_PROC_PATH themselves.
# scripts/agent-bin/cargo never sets REIFY_CPU_ADMIT_MEM_*; its memory-pressure
# behavior is 100% inherited from cpu-admit.sh's direct-exec default, which as
# of task 4911 defaults memfull threshold to 10 (memory dimension default-ON
# on the CLI/agent axis).  Pre-existing cycles (B/D/E/F/G/H/I/J/K/L) do not
# exercise memory and must stay deterministic regardless of host memory load —
# export a quiet memory fixture (memfull=0) so they inherit a deterministic
# memory-ok state.  Cycle M overrides REIFY_CPU_ADMIT_MEM_PROC_PATH per-case to
# exercise the memory dimension explicitly.  Mirrors the neutralization in
# tests/infra/test_cpu_admit.sh.
# ---------------------------------------------------------------------------
_MEM_PSI_QUIET="$(mktemp -p "$WORKDIR" mem-psi-quiet.XXXXXX)"
printf 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
    > "$_MEM_PSI_QUIET"
export REIFY_CPU_ADMIT_MEM_PROC_PATH="$_MEM_PSI_QUIET"
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# Harness helpers
# ---------------------------------------------------------------------------

# make_psi_fixture <avg10>
# Writes a /proc/pressure/cpu-formatted fixture to a temp file and echoes its path.
make_psi_fixture() {
    local avg10="$1"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n' \
        "$avg10" > "$fixture"
    echo "$fixture"
}

# make_stub_cargo
# Writes an executable stub cargo into STUB_DIR that echoes "STUB_CARGO <args>"
# to stdout and exits ${STUB_EXIT_CODE:-0}.  Pass STUB_EXIT_CODE=N as a run_shim
# env arg to verify the shim's exec preserves a non-zero exit code (C-S2).
# The shim resolves this as the 'real' cargo because the hermetic PATH excludes
# ~/.cargo/bin and places STUB_DIR before /usr/bin.
make_stub_cargo() {
    cat > "$STUB_DIR/cargo" <<'STUBEOF'
#!/usr/bin/env bash
echo "STUB_CARGO $*"
exit "${STUB_EXIT_CODE:-0}"
STUBEOF
    chmod +x "$STUB_DIR/cargo"
}

# run_shim <proc_path> [VAR=val ...] -- <cargo-args ...>
# Invokes the cargo shim under a HERMETIC PATH with the given PSI proc path plus
# any extra env overrides.  Use -- to separate env overrides from cargo args.
# After returning, sets globals:
#   SHIM_RC      — exit code
#   SHIM_STDOUT  — captured stdout
#   SHIM_STDERR  — captured stderr
#   SHIM_ELAPSED — elapsed seconds (integer)
SHIM_RC=0
SHIM_STDOUT=""
SHIM_STDERR=""
SHIM_ELAPSED=0

run_shim() {
    local proc_path="$1"; shift
    # Collect extra env VAR=val pairs until -- separator.
    local env_args=()
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        env_args+=("$1"); shift
    done
    [ $# -gt 0 ] && shift  # consume the --
    # Remaining "$@" are the cargo args forwarded to the shim.

    local _stdout_file _stderr_file
    _stdout_file="$(mktemp -p "$WORKDIR" shim-stdout.XXXXXX)"
    _stderr_file="$(mktemp -p "$WORKDIR" shim-stderr.XXXXXX)"

    SHIM_RC=0
    SHIM_STDOUT=""
    SHIM_STDERR=""

    local _t0 _t1
    _t0=$(date +%s)
    env "${env_args[@]}" \
        REIFY_CPU_ADMIT_PROC_PATH="$proc_path" \
        PATH="$REPO_ROOT/scripts/agent-bin:$STUB_DIR:/usr/bin:/bin" \
        bash "$SHIM" "$@" \
        >"$_stdout_file" \
        2>"$_stderr_file" \
        || SHIM_RC=$?
    _t1=$(date +%s)

    SHIM_STDOUT="$(cat "$_stdout_file")"
    SHIM_STDERR="$(cat "$_stderr_file")"
    SHIM_ELAPSED=$(( _t1 - _t0 ))
    rm -f "$_stdout_file" "$_stderr_file"
}

make_stub_cargo

echo "=== agent-bin/cargo shim tests ==="

# ---------------------------------------------------------------------------
# Cycle A: shim file exists and is executable (C-S1 structural prerequisite).
# RED until step-2 creates scripts/agent-bin/cargo.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle A: shim exists and is executable ---"

assert "A: scripts/agent-bin/cargo exists and is executable" \
    test -x "$SHIM"

# ---------------------------------------------------------------------------
# Cycle B: low PSI + heavy subcommand admits instantly (C-S1 / C-S2).
# avg10=40 < THRESHOLD=50 → exit 0, stdout has STUB sentinel + forwarded args
# (proves: strips shim dir, resolves+execs real cargo, preserves args);
# fairness-floor marker absent from stderr (proves: gate fast-admitted, did not
# block-then-timeout).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: low PSI + heavy subcommand admits instantly ---"

PSI_B="$(make_psi_fixture 40)"
run_shim "$PSI_B" \
    DF_VERIFY_ROLE=task REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test --package foo --release

assert "B: exit 0" \
    test "$SHIM_RC" -eq 0
assert "B: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "B: stdout contains forwarded args (test --package foo --release)" \
    bash -c 'printf "%s\n" "$1" | grep -q "test --package foo --release"' _ "$SHIM_STDOUT"
assert "B: no wrongful gating (fairness-floor marker absent from stderr)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle C: high PSI + heavy subcommand → gated then admitted (C-S1 / C-S2).
# avg10=99, MAX_WAIT=2, POLL=1 → exit 0 (NOT 75), elapsed >= 2s, sentinel present
# (admits-on-timeout: admit mode NEVER exits 75 — the C-A2 invariant).
# Also asserts the fairness-floor stderr marker IS present — this is the
# regex-correctness probe for the absent-marker assertions in B/F/H/K2: if the
# cpu-admit.sh fairness-floor message is reworded so that neither "fairness" nor
# "sustained pressure" appear, C's positive check fails loud instead of silently
# neutering the absent-marker guards in those cycles.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle C: high PSI + heavy subcommand → gated ---"

PSI_C="$(make_psi_fixture 99)"
run_shim "$PSI_C" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "C: exit 0 (admit-on-timeout, NOT exit 75)" \
    test "$SHIM_RC" -eq 0
assert "C: NOT exit 75 (admit mode never requeues)" \
    test "$SHIM_RC" -ne 75
assert "C: elapsed >= MAX_WAIT=2s (was gated before admitting)" \
    test "$SHIM_ELAPSED" -ge 2
assert "C: stdout contains STUB_CARGO sentinel (reached real cargo after wait)" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "C: stderr contains fairness-floor marker (regex-correctness probe: same pattern B/F/H/K2 absent-checks depend on)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle D: fail-open — nonexistent PROC_PATH + heavy subcommand (C-A4).
# → exit 0 fast (< 2s), sentinel present (never blocks on non-PSI hosts).
# MAX_WAIT=5/POLL=1 safety: without fail-open would loop until timeout.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle D: fail-open (nonexistent PSI path) ---"

NONEXISTENT_PSI="$WORKDIR/nope/pressure-cpu"   # guaranteed absent

run_shim "$NONEXISTENT_PSI" \
    DF_VERIFY_ROLE=task REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "D: exit 0 (fail-open)" \
    test "$SHIM_RC" -eq 0
assert "D: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "D: stderr contains fail-open WARNING marker" \
    bash -c 'printf "%s\n" "$1" | grep -q "fail-open"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle E: merge bypass — DF_VERIFY_ROLE=merge + high PSI + heavy subcommand (C-A3).
# → exit 0 fast (< 2s) (shim forwards env to cpu-admit's bypass logic).
# MAX_WAIT=5/POLL=1 safety: without bypass would block on avg10=99.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle E: merge bypass ---"

PSI_E="$(make_psi_fixture 99)"
run_shim "$PSI_E" \
    DF_VERIFY_ROLE=merge REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "E: exit 0 (merge bypass)" \
    test "$SHIM_RC" -eq 0
assert "E: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "E: stderr contains merge-bypass marker" \
    bash -c 'printf "%s\n" "$1" | grep -q "bypass"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle F: non-heavy subcommands UNGATED despite saturated PSI (C-S1).
# Under high PSI (avg10=99), --version / metadata / fmt / add must still reach
# the real cargo and must NOT emit the fairness-floor marker (proves: gate was
# bypassed entirely, not blocked-then-admitted-on-timeout).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle F: non-heavy subcommands ungated under high PSI ---"

PSI_F="$(make_psi_fixture 99)"

for _subcmd in "--version" "metadata" "fmt" "add somecrate"; do
    # shellcheck disable=SC2086  # intentional word-splitting for multi-token subcommands
    run_shim "$PSI_F" \
        DF_VERIFY_ROLE=task REIFY_CPU_ADMIT_MAX_WAIT=6 REIFY_CPU_ADMIT_POLL=1 -- \
        $_subcmd
    assert "F: '$_subcmd' under avg10=99 → exit 0" \
        test "$SHIM_RC" -eq 0
    assert "F: '$_subcmd' still reaches real cargo (STUB_CARGO sentinel present)" \
        bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
    assert "F: '$_subcmd' no wrongful gating (fairness-floor marker absent from stderr)" \
        bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"
done

# ---------------------------------------------------------------------------
# Cycle G: heavy-set completeness regression guard (PRD §4.3 / §11 Q4).
# All 8 heavy subcommands {build,test,nextest,check,clippy,bench,doc,build-std}
# must be GATED under high PSI (elapsed >= 1s with MAX_WAIT=1, POLL=1).
# Passes under both v1 (unconditional gate) and the refined shim (step-4).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle G: heavy-set completeness (all 8 subcommands gated) ---"

PSI_G="$(make_psi_fixture 99)"

for _heavy in build test nextest check clippy bench doc build-std; do
    run_shim "$PSI_G" \
        REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1 -- \
        "$_heavy"
    assert "G: '$_heavy' gated (elapsed >= 1s)" \
        test "$SHIM_ELAPSED" -ge 1
    assert "G: '$_heavy' exit 0 (admit-on-timeout)" \
        test "$SHIM_RC" -eq 0
done

# ---------------------------------------------------------------------------
# Cycle H: REIFY_CPU_ADMIT_AGENT_THRESHOLD raises the ceiling above current PSI
# → admits IMMEDIATELY despite high PSI (resolves PRD §11 Q3).
# REIFY_CPU_ADMIT_AGENT_THRESHOLD=100 + avg10=99: 99 < 100 → fast admit;
# fairness-floor marker absent (proves: threshold knob wired, gate fast-admitted).
# Without knob: default 50 is used → 99 >= 50 → blocks until MAX_WAIT, emits
# fairness-floor marker → absent-assert goes RED.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle H: AGENT_THRESHOLD=100 raises ceiling above PSI 99 → fast admit ---"

PSI_H="$(make_psi_fixture 99)"
run_shim "$PSI_H" \
    DF_VERIFY_ROLE=task REIFY_CPU_ADMIT_AGENT_THRESHOLD=100 \
    REIFY_CPU_ADMIT_MAX_WAIT=6 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "H: exit 0" \
    test "$SHIM_RC" -eq 0
assert "H: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "H: no wrongful gating (fairness-floor marker absent from stderr)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle I: REIFY_CPU_ADMIT_AGENT_THRESHOLD lowers the ceiling below current PSI
# → delays despite PSI that default-50 would admit instantly (PRD §11 Q3).
# REIFY_CPU_ADMIT_AGENT_THRESHOLD=10 + avg10=40 (MAX_WAIT=2, POLL=1):
#   • With knob wired: 40 >= 10 → blocks for MAX_WAIT=2s (elapsed >= 2). GREEN (step-6)
#   • Without knob:    default 50 is used → 40 < 50 → immediate admit (elapsed < 2s). RED.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle I: AGENT_THRESHOLD=10 lowers ceiling below PSI 40 → blocks ---"

PSI_I="$(make_psi_fixture 40)"
run_shim "$PSI_I" \
    REIFY_CPU_ADMIT_AGENT_THRESHOLD=10 \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "I: exit 0 (admit-on-timeout, NOT 75)" \
    test "$SHIM_RC" -eq 0
assert "I: AGENT_THRESHOLD=10 + avg10=40 → delayed (elapsed >= MAX_WAIT=2s)" \
    test "$SHIM_ELAPSED" -ge 2
assert "I: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# ---------------------------------------------------------------------------
# Cycle J: exit-code passthrough (C-S2 semantics-preserving).
# The shim uses `exec` — the real cargo's exit code is passed through verbatim.
# STUB_EXIT_CODE=3 makes the stub exit 3; assert SHIM_RC -eq 3 proves exec
# does not swallow a non-zero exit status.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle J: exit-code passthrough (C-S2) ---"

PSI_J="$(make_psi_fixture 40)"
run_shim "$PSI_J" \
    STUB_EXIT_CODE=3 -- \
    test

assert "J: shim exit code == stub exit code 3 (exec preserves non-zero exit)" \
    test "$SHIM_RC" -eq 3
assert "J: stdout contains STUB_CARGO sentinel (real cargo was reached)" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# ---------------------------------------------------------------------------
# Cycle K: '+toolchain' prefix skipping — `cargo +nightly test` must gate on
# 'test', and `cargo +nightly --version` must be ungated (C-S1).
# Verifies that the subcommand scan correctly reads past the '+nightly' token.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle K: +toolchain prefix — classification reads through '+nightly' ---"

PSI_K="$(make_psi_fixture 99)"

# K1: +nightly test → classified as heavy 'test' → gated
run_shim "$PSI_K" \
    REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1 -- \
    +nightly test

assert "K1: '+nightly test' → gated on 'test' (elapsed >= 1s)" \
    test "$SHIM_ELAPSED" -ge 1
assert "K1: exit 0 (admit-on-timeout)" \
    test "$SHIM_RC" -eq 0

# K2: +nightly --version → classified as non-heavy '--version' → ungated
run_shim "$PSI_K" \
    DF_VERIFY_ROLE=task REIFY_CPU_ADMIT_MAX_WAIT=6 REIFY_CPU_ADMIT_POLL=1 -- \
    +nightly --version

assert "K2: exit 0" \
    test "$SHIM_RC" -eq 0
assert "K2: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "K2: no wrongful gating (fairness-floor marker absent from stderr)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"

# ---------------------------------------------------------------------------
# Cycle L: global option flags before subcommand (suggestion 2 coverage).
# `cargo --offline build` and `cargo -q test` must gate on the SUBCOMMAND
# despite the leading flag token.  Without the `-*) continue` fix in the
# subcommand scanner these would classify the flag as non-heavy and skip the
# gate — this cycle is the regression guard for that fix.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle L: global option flags before subcommand (--offline / -q) gated ---"

PSI_L="$(make_psi_fixture 99)"

# L1: --offline build → flag skipped, 'build' classified heavy → gated
run_shim "$PSI_L" \
    REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1 -- \
    --offline build

assert "L1: '--offline build' → gated on 'build' (elapsed >= 1s)" \
    test "$SHIM_ELAPSED" -ge 1
assert "L1: exit 0 (admit-on-timeout)" \
    test "$SHIM_RC" -eq 0
assert "L1: stdout contains STUB_CARGO sentinel (forwarded args unchanged)" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# L2: -q test → flag skipped, 'test' classified heavy → gated
run_shim "$PSI_L" \
    REIFY_CPU_ADMIT_MAX_WAIT=1 REIFY_CPU_ADMIT_POLL=1 -- \
    -q test

assert "L2: '-q test' → gated on 'test' (elapsed >= 1s)" \
    test "$SHIM_ELAPSED" -ge 1
assert "L2: exit 0 (admit-on-timeout)" \
    test "$SHIM_RC" -eq 0

# ---------------------------------------------------------------------------
# make_mem_psi_fixture <memfull> [memsome]
# Writes a /proc/pressure/memory-formatted fixture (some + full lines) and
# echoes its path.  memsome defaults to 0 if not specified.
# Copied verbatim from tests/infra/test_cpu_admit.sh's make_mem_psi_fixture.
# ---------------------------------------------------------------------------
make_mem_psi_fixture() {
    local memfull="$1"
    local memsome="${2:-0}"
    local fixture
    fixture="$(mktemp -p "$WORKDIR" mem-psi-fixture.XXXXXX)"
    printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\nfull avg10=%s avg60=0.00 avg300=0.00 total=0\n' \
        "$memsome" "$memfull" > "$fixture"
    echo "$fixture"
}

# ---------------------------------------------------------------------------
# Cycle M: shim inherits default-ON memfull (shim sets no mem env).
# scripts/agent-bin/cargo never sets REIFY_CPU_ADMIT_MEM_*; its memory-pressure
# behavior is 100% inherited from cpu-admit.sh's direct-exec default (line 393).
# M1 is a RED driver: today that default is empty (memory dimension OFF), so
# a heavy subcommand under high memfull still admits instantly on CPU alone.
# M2/M3 are guards that must stay green both before and after the flip.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle M: shim inherits default-ON memfull (shim sets no mem env) ---"

PSI_M="$(make_psi_fixture 40)"             # CPU quiet-ish: 40 < default THRESHOLD=50
PSI_M_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50
PSI_M_MEM5="$(make_mem_psi_fixture 5)"     # memfull=5

# M1 (RED driver): heavy `test`, CPU=40 (would admit on CPU alone) + memfull=50,
# NO explicit REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD (the shim never sets it) →
# exit 0 AND SHIM_ELAPSED >= 2 AND stderr matches fairness/sustained-pressure
# AND stdout contains STUB_CARGO sentinel (reached real cargo after the wait).
# RED today: agent-bin/cargo does not set the mem env and cpu-admit's CLI
# default is OFF → shim admits instantly on CPU alone → elapsed < 2 → fails.
run_shim "$PSI_M" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "M1: default-ON memfull=50, shim sets no mem env → exit 0" \
    test "$SHIM_RC" -eq 0
assert "M1: elapsed >= MAX_WAIT=2s (shim backed off on memory BY DEFAULT)" \
    test "$SHIM_ELAPSED" -ge 2
assert "M1: stderr matches fairness/sustained-pressure (default memory backoff confirmed)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"
assert "M1: stdout contains STUB_CARGO sentinel (reached real cargo after wait)" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# M2 (guard): heavy `test`, CPU=40 + memfull=5 (< default threshold=10), no
# explicit mem env → fast admit exit 0, no fairness/sustained-pressure marker,
# STUB_CARGO present.  Must stay green before & after the flip.
run_shim "$PSI_M" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM5" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "M2: memfull=5 < default threshold, no explicit mem env → exit 0" \
    test "$SHIM_RC" -eq 0
assert "M2: no wrongful gating (fairness-floor marker absent from stderr)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"
assert "M2: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# M3 (guard): merge bypass — DF_VERIFY_ROLE=merge + memfull=50, no explicit
# mem env → exit 0 fast, STUB_CARGO present.  Must stay green before & after.
run_shim "$PSI_M" \
    DF_VERIFY_ROLE=merge \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_M_MEM50" \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "M3: merge bypass + memfull=50, no explicit mem env → exit 0" \
    test "$SHIM_RC" -eq 0
assert "M3: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# ---------------------------------------------------------------------------
# Cycle N: shim propagates the explicit-empty escape hatch (agent axis)
# Regression for review finding robustness_contract_violation @ cpu-admit.sh:399,
# exercised through the real operator break-glass path: an operator disabling
# memory backoff during an incident by exporting
# REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD="" into the agent's environment before the
# shim execs cpu-admit.sh.  The shim itself never sets this var — its memory
# behavior is 100% inherited from cpu-admit.sh's direct-exec default (line 399).
# N1 is a RED driver: today line-399's `${REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD:-10}`
# (colon-minus) coerces the explicit-empty value back to 10, so the escape
# hatch doesn't work — the shim still backs off on memfull=50.  GREEN after
# step-7 flips to unset-only `${REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD-10}`.
# (Cycle M already covers the shim merge-bypass and low-mem guards; Cycle N
# adds only the new escape-hatch behavior.)
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle N: shim propagates the explicit-empty escape hatch ---"

PSI_N="$(make_psi_fixture 40)"             # CPU quiet-ish: 40 < default THRESHOLD=50
PSI_N_MEM50="$(make_mem_psi_fixture 50)"   # memfull=50

# N1 (RED driver): heavy `test`, CPU=40 (< default THRESHOLD=50, would admit on
# CPU alone) + memfull=50 fixture + explicit-empty
# REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD= (run_shim forwards env_args via
# `env "${env_args[@]}"`, so the shim's child cpu-admit inherits the var
# SET-BUT-EMPTY) → assert SHIM_RC==0 AND SHIM_ELAPSED < 2 (instant admit) AND
# SHIM_STDERR has NO 'fairness'/'sustained pressure' marker AND SHIM_STDOUT
# contains the STUB_CARGO sentinel (reached real cargo without a memory wait).
# RED today: the shim never sets the mem threshold and cpu-admit's colon-minus
# default coerces the exported empty to 10 → memfull=50 >= 10 → shim backs
# off → SHIM_ELAPSED >= 2 → fails.
run_shim "$PSI_N" \
    REIFY_CPU_ADMIT_MEM_PROC_PATH="$PSI_N_MEM50" \
    REIFY_CPU_ADMIT_MEM_FULL_THRESHOLD= \
    REIFY_CPU_ADMIT_MAX_WAIT=2 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "N1: explicit-empty MEM_FULL_THRESHOLD + memfull=50, shim → exit 0" \
    test "$SHIM_RC" -eq 0
# NOTE: "instant admit" is verified load-independently by the marker + sentinel
# assertions below (an on-by-mistake memory dimension would back off →
# admit-on-timeout → fairness/sustained-pressure marker).  No absolute
# wall-clock `elapsed < 2s` upper bound is used — that is the flaky class
# de-flaked by tasks 4841-4847 and guarded by
# tests/infra/test_no_new_wallclock_upper_bounds.sh.
assert "N1: no fairness/sustained-pressure marker (memory dimension OFF)" \
    bash -c '! printf "%s\n" "$1" | grep -qiE "fairness|sustained pressure"' _ "$SHIM_STDERR"
assert "N1: stdout contains STUB_CARGO sentinel (reached real cargo without a memory wait)" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

test_summary
