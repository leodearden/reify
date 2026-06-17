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
# to stdout and exits 0.  The shim resolves this as the 'real' cargo because the
# hermetic PATH excludes ~/.cargo/bin and places STUB_DIR before /usr/bin.
make_stub_cargo() {
    cat > "$STUB_DIR/cargo" <<'STUBEOF'
#!/usr/bin/env bash
echo "STUB_CARGO $*"
exit 0
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
# avg10=40 < THRESHOLD=50 → exit 0, elapsed < 2s, stdout has STUB sentinel +
# forwarded args (proves: strips shim dir, resolves+execs real cargo, preserves args).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle B: low PSI + heavy subcommand admits instantly ---"

PSI_B="$(make_psi_fixture 40)"
run_shim "$PSI_B" -- test --package foo --release

assert "B: exit 0" \
    test "$SHIM_RC" -eq 0
assert "B: returned fast (< 2s)" \
    test "$SHIM_ELAPSED" -lt 2
assert "B: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
assert "B: stdout contains forwarded args (test --package foo --release)" \
    bash -c 'printf "%s\n" "$1" | grep -q "test --package foo --release"' _ "$SHIM_STDOUT"

# ---------------------------------------------------------------------------
# Cycle C: high PSI + heavy subcommand → gated then admitted (C-S1 / C-S2).
# avg10=99, MAX_WAIT=2, POLL=1 → exit 0 (NOT 75), elapsed >= 2s, sentinel present
# (admits-on-timeout: admit mode NEVER exits 75 — the C-A2 invariant).
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

# ---------------------------------------------------------------------------
# Cycle D: fail-open — nonexistent PROC_PATH + heavy subcommand (C-A4).
# → exit 0 fast (< 2s), sentinel present (never blocks on non-PSI hosts).
# MAX_WAIT=5/POLL=1 safety: without fail-open would loop until timeout.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle D: fail-open (nonexistent PSI path) ---"

NONEXISTENT_PSI="$WORKDIR/nope/pressure-cpu"   # guaranteed absent

run_shim "$NONEXISTENT_PSI" \
    REIFY_CPU_ADMIT_MAX_WAIT=5 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "D: exit 0 (fail-open)" \
    test "$SHIM_RC" -eq 0
assert "D: returned fast < 2s (fail-open, no blocking)" \
    test "$SHIM_ELAPSED" -lt 2
assert "D: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

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
assert "E: returned fast < 2s (merge bypasses PSI wait)" \
    test "$SHIM_ELAPSED" -lt 2
assert "E: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

# ---------------------------------------------------------------------------
# Cycle F: non-heavy subcommands UNGATED despite saturated PSI (C-S1).
# Under high PSI (avg10=99, MAX_WAIT=3, POLL=1), --version / metadata / fmt /
# add must return FAST (elapsed < 2s) and still reach the real cargo.
# RED until step-4 adds subcommand classification (v1 gates everything and
# blocks for MAX_WAIT=3s, failing the elapsed < 2s guard).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle F: non-heavy subcommands ungated under high PSI ---"

PSI_F="$(make_psi_fixture 99)"

for _subcmd in "--version" "metadata" "fmt" "add somecrate"; do
    # shellcheck disable=SC2086  # intentional word-splitting for multi-token subcommands
    run_shim "$PSI_F" \
        REIFY_CPU_ADMIT_MAX_WAIT=3 REIFY_CPU_ADMIT_POLL=1 -- \
        $_subcmd
    assert "F: '$_subcmd' under avg10=99 → exit 0" \
        test "$SHIM_RC" -eq 0
    assert "F: '$_subcmd' returns fast < 2s (ungated — C-S1)" \
        test "$SHIM_ELAPSED" -lt 2
    assert "F: '$_subcmd' still reaches real cargo (STUB_CARGO sentinel present)" \
        bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"
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
# REIFY_CPU_ADMIT_AGENT_THRESHOLD=100 + avg10=99 (MAX_WAIT=3, POLL=1):
#   • With knob wired: 99 < 100 → immediate admit (elapsed < 2s). GREEN (step-6)
#   • Without knob:    default 50 is used → 99 >= 50 → blocks for MAX_WAIT=3s.  RED.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle H: AGENT_THRESHOLD=100 raises ceiling above PSI 99 → fast admit ---"

PSI_H="$(make_psi_fixture 99)"
run_shim "$PSI_H" \
    REIFY_CPU_ADMIT_AGENT_THRESHOLD=100 \
    REIFY_CPU_ADMIT_MAX_WAIT=3 REIFY_CPU_ADMIT_POLL=1 -- \
    test

assert "H: exit 0" \
    test "$SHIM_RC" -eq 0
assert "H: AGENT_THRESHOLD=100 + avg10=99 → immediate admit (elapsed < 2s)" \
    test "$SHIM_ELAPSED" -lt 2
assert "H: stdout contains STUB_CARGO sentinel" \
    bash -c 'printf "%s\n" "$1" | grep -q "STUB_CARGO"' _ "$SHIM_STDOUT"

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

test_summary
