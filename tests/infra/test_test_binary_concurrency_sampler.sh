#!/usr/bin/env bash
# Contract guard for scripts/sample-test-binary-concurrency.sh (task 6018).
#
# WHAT THE SAMPLER IS FOR.  Task 6018 un-narrowed the global nextest pool
# ([profile.default] test-threads: a fixed 16 -> the host CPU count).  Its
# ACCEPTANCE clause — inherited from task 5984 carry-over 1, esc-5984-2 — is an
# OBSERVED peak count of concurrently-running test binaries, not a computed one.
# This suite pins the instrument that produces that number, so the acceptance
# artifact stays reproducible after the observation itself is filed.
#
# THE TWO MEASURED DEFECTS IT EXISTS TO CLOSE.  Both were found in the earlier
# attempt at this measurement and are recorded in the task description:
#
#   (a) OVERCOUNT.  `pgrep -fc "<lane>/target/debug/deps/"` matches on ARGV, and
#       argv is not identity: reproduced live in this worktree, `pgrep -f
#       "target/.*/deps/"` returns pids whose /proc/<pid>/exe resolves to
#       ~/.cargo/bin/sccache and rustup's rustc — compile-phase processes, not
#       test binaries.  The earlier 48-count snapshot is that artefact.  The fix
#       is to CONFIRM each candidate by readlink /proc/<pid>/exe, the spoof-proof
#       idiom scripts/lib_proc_reaper.sh:197-205 already uses.  A1 pins it.
#
#   (b) UNDERSAMPLING.  A 15-minute window returned a maximum of 7 — BELOW the
#       then-configured 16 — purely because it landed between nextest execution
#       phases (compile/link occupies much of a verify; test-threads bounds
#       execution only).  A peak alone cannot distinguish "the bound held" from
#       "I sampled the wrong phase", so a low reading was wrongly readable as
#       evidence the cap was working.  The fix is to report nonzero_samples
#       alongside peak, making a never-saw-the-execution-phase window mechanically
#       INCONCLUSIVE rather than a matter of the reader's judgement.  A7 pins it.
#
# HERMETICITY.  Every assert builds a fake proc tree of <root>/<pid>/exe symlinks
# under its own mktemp -d and drives the sampler through two injection seams:
#   REIFY_SAMPLER_PROC_ROOT  — the /proc to read (default /proc)
#   REIFY_SAMPLER_PIDS_CMD   — a command printing candidate pids, one per line
#                              (default: the `pgrep -f` prefilter)
# No real host process is inspected, so the expectations are exact integers on
# ANY host and under ANY concurrent load — the Tests 13a-15b form the sibling
# nextest suites use.  Compile-free: bash/awk/readlink only, no cargo, no
# nextest, no workspace compile (task 4613).
#
# Bucket `pool` in tests/infra/run-all-classification.manifest: hermetic, safe to
# run concurrently.  That row and this file MUST land in the same commit —
# tests/infra/test_run_all_classification.sh fails on a discovered-but-undeclared
# file AND on a declared row with no file.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

SAMPLER="$REPO_ROOT/scripts/sample-test-binary-concurrency.sh"

echo "=== test_test_binary_concurrency_sampler.sh (task 6018) ==="
echo "sampler: $SAMPLER"

# ---------------------------------------------------------------------------
# Fixture helpers.
#
# _mk_proc <root> <pid>=<exe-target> ...
#   Builds <root>/<pid>/exe as a symlink to <exe-target>, the shape the sampler
#   readlinks.  The target need not exist: readlink reports the link's TEXT, and
#   that is precisely what /proc/<pid>/exe gives for a deleted binary too.
#
# A pid passed to REIFY_SAMPLER_PIDS_CMD with NO <root>/<pid> directory models
# the real race the sampler must survive: the process exited between the argv
# prefilter and the exe confirmation (A5).
# ---------------------------------------------------------------------------
_mk_proc() {
    local root="$1"; shift
    local spec pid target
    for spec in "$@"; do
        pid="${spec%%=*}"
        target="${spec#*=}"
        mkdir -p "$root/$pid"
        ln -sfn "$target" "$root/$pid/exe"
    done
}

# _pids_cmd_file <path> <pid> ...  — a fixed candidate list, one pid per line.
_pids_cmd_file() {
    local path="$1"; shift
    {
        echo '#!/usr/bin/env bash'
        local p
        for p in "$@"; do echo "echo $p"; done
    } > "$path"
    chmod +x "$path"
}

# _peak_of <summary-line>  — extract the peak= field.
_field() {
    # usage: _field <line> <key>
    printf '%s\n' "$1" | tr ' ' '\n' | awk -F= -v k="$2" '$1==k{print $2; exit}'
}

# _run_one <proc-root> <pids-cmd> [extra sampler args...]
#   One sample (`--duration 0`), stdout captured.
_run_one() {
    local root="$1" pidscmd="$2"; shift 2
    REIFY_SAMPLER_PROC_ROOT="$root" \
    REIFY_SAMPLER_PIDS_CMD="$pidscmd" \
        bash "$SAMPLER" --duration 0 --interval 0 "$@"
}

# ---------------------------------------------------------------------------
# A1 (THE OVERCOUNT FIX — the whole point of the instrument).
#
# A pid whose candidate-list membership is REAL (its argv genuinely contains a
# target/.../deps/ path, so the pgrep prefilter returns it) but whose
# /proc/<pid>/exe resolves to ~/.cargo/bin/sccache must be EXCLUDED.  This is
# the exact false positive reproduced live in this worktree; without the exe
# confirmation the sampler reports compile-phase processes as test binaries and
# the acceptance number is meaningless.
# ---------------------------------------------------------------------------
echo ""
echo "--- A1 (overcount fix): argv-matched compile-phase processes are excluded by the exe confirmation ---"

assert "A1: a pid whose /proc/<pid>/exe resolves to ~/.cargo/bin/sccache is NOT counted (argv match alone would have counted it)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 101=/home/u/.cargo/bin/sccache
        _pids_cmd_file "$d/pids" 101
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "0" ]
    '

assert "A1b: a pid whose /proc/<pid>/exe resolves to rustup's rustc is NOT counted (the second live false positive)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 102=/home/u/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc
        _pids_cmd_file "$d/pids" 102
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "0" ]
    '

# ---------------------------------------------------------------------------
# A2/A3: what a test binary actually looks like.  BOTH cargo profiles count —
# verify.sh runs debug and release passes, and the pool bound applies to each.
# ---------------------------------------------------------------------------
echo ""
echo "--- A2-A3: real test binaries under target/{debug,release}/deps are counted ---"

assert "A2: a pid whose exe resolves under <lane>/target/debug/deps/<bin>-<hash> IS counted" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 201=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4
        _pids_cmd_file "$d/pids" 201
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '

assert "A3: target/release/deps/ counts too (both cargo profiles run under verify)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 301=/home/u/lanes/_lane-9/target/release/deps/reify_syntax-77aa01be
        _pids_cmd_file "$d/pids" 301
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '

# ---------------------------------------------------------------------------
# A4 (HOST-WIDE, not lane-scoped).  The task description asks for "the host-wide
# aggregate": test-threads bounds ONE nextest run, and with the semaphore at 1
# plus the merge role's bypass (scripts/lib_test_semaphore.sh:91) two runs from
# DIFFERENT lanes are reachable at once.  A lane-scoped counter cannot see that
# and would under-report the host by construction.
# ---------------------------------------------------------------------------
echo ""
echo "--- A4: the count is HOST-WIDE — test binaries under different lane roots aggregate ---"

assert "A4: two pids under two DIFFERENT lane roots both count (peak=2, not 1 — the sampler is host-wide, not lane-scoped)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" \
            401=/home/u/lanes/_lane-3/target/debug/deps/reify_compiler-1111aaaa \
            402=/home/u/lanes/_lane-7/target/debug/deps/reify_eval-2222bbbb
        _pids_cmd_file "$d/pids" 401 402
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "2" ]
    '

# ---------------------------------------------------------------------------
# A5: the prefilter/confirm RACE is normal, not an error.  A process can exit
# between the argv prefilter and the exe readlink; in real /proc its whole
# directory vanishes.  The sampler must skip it silently and keep counting the
# survivors — an instrument that aborts mid-window destroys the observation.
# ---------------------------------------------------------------------------
echo ""
echo "--- A5: a pid that vanished between prefilter and confirmation is skipped, not fatal ---"

assert "A5: a candidate pid with no <root>/<pid> directory is skipped without error, and the surviving real test binary still counts (exit 0, peak=1)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 501=/home/u/lanes/_lane-1/target/debug/deps/reify_cli-3333cccc
        _pids_cmd_file "$d/pids" 500 501
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '

# ---------------------------------------------------------------------------
# A6: PEAK, not first and not last.  The reported number must be the MAXIMUM
# over the window.  Driven by a pids command whose SECOND invocation returns a
# 3-pid burst and every other invocation returns 1, so a sampler that reported
# the first sample (1), the last sample (1), or the mean would all disagree
# with 3.  `--duration 3 --interval 1` guarantees at least two invocations on
# any host; the assert tolerates any sample count >= 2 rather than pinning an
# exact one, so a loaded host cannot false-RED it.
# ---------------------------------------------------------------------------
echo ""
echo "--- A6: the reported peak is the MAXIMUM over the window, not the first or last sample ---"

assert "A6: over a scripted 1,3,1,... sequence the summary reports peak=3 (not the first sample 1, not the last)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _field)"'
        _mk_proc "$root" \
            601=/home/u/lanes/_lane-2/target/debug/deps/a-1111 \
            602=/home/u/lanes/_lane-2/target/debug/deps/b-2222 \
            603=/home/u/lanes/_lane-2/target/debug/deps/c-3333
        cat > "$d/pids" <<PIDS
#!/usr/bin/env bash
n=0
[ -f "$d/n" ] && n=\$(cat "$d/n")
n=\$((n + 1))
echo "\$n" > "$d/n"
if [ "\$n" -eq 2 ]; then
    echo 601; echo 602; echo 603
else
    echo 601
fi
PIDS
        chmod +x "$d/pids"
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 3 --interval 1)
        [ "$(_field "$out" peak)" = "3" ] && [ "$(_field "$out" samples)" -ge 2 ]
    '

# ---------------------------------------------------------------------------
# A7 (THE UNDERSAMPLING FIX).  peak alone is not falsifiable: a window that
# never overlapped an execution phase reports a low peak that reads exactly like
# a bound that held.  samples= and nonzero_samples= make the distinction
# mechanical — nonzero_samples == 0 means the window saw no test binary at all
# and is INCONCLUSIVE, not evidence about the pool.
# ---------------------------------------------------------------------------
echo ""
echo "--- A7 (undersampling fix): samples= and nonzero_samples= are reported alongside peak= ---"

assert "A7a: the summary line carries all three of peak=, samples= and nonzero_samples=" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 701=/home/u/lanes/_lane-4/target/debug/deps/x-4444
        _pids_cmd_file "$d/pids" 701
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ -n "$(_field "$out" peak)" ] &&
        [ -n "$(_field "$out" samples)" ] &&
        [ -n "$(_field "$out" nonzero_samples)" ]
    '

assert "A7b: a window that saw a test binary reports nonzero_samples >= 1 (CONCLUSIVE)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 702=/home/u/lanes/_lane-4/target/debug/deps/y-5555
        _pids_cmd_file "$d/pids" 702
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" nonzero_samples)" -ge 1 ]
    '

assert "A7c: a window that saw ONLY compile-phase processes reports nonzero_samples=0 (INCONCLUSIVE, distinguishable from a bound that held)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 703=/home/u/.cargo/bin/sccache
        _pids_cmd_file "$d/pids" 703
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" nonzero_samples)" = "0" ] && [ "$(_field "$out" peak)" = "0" ]
    '

# ---------------------------------------------------------------------------
# A8: an empty host is DATA, not an error.  The sampler is meant to be launched
# alongside a verify without any coupling to it; if it starts before the run or
# outlives it, exiting non-zero would turn a legitimate empty window into a
# spurious failure in whatever launched it.
# ---------------------------------------------------------------------------
echo ""
echo "--- A8: zero matching processes is data, not an error ---"

assert "A8: no candidate pids at all -> exit 0 with peak=0 (absence of test binaries is data)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"; mkdir -p "$root"
        '"$(declare -f _field)"'
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="true" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        rc=$?
        [ "$rc" -eq 0 ] && [ "$(_field "$out" peak)" = "0" ]
    '

# ---------------------------------------------------------------------------
# A9: the instrument must not perturb what it measures.  It is designed to run
# CONCURRENTLY with a real verify pass, so it must never invoke cargo or nextest
# — that would contend for the very build/test resources whose concurrency it is
# counting, and would take a test-semaphore slot of its own.
#
# The cargo check is comment-stripped (the header legitimately discusses
# cargo-nextest in prose); the nextest check targets INVOCATION forms only, so
# the summary line's "nextest test-binary concurrency" label does not trip it.
# ---------------------------------------------------------------------------
echo ""
echo "--- A9: the sampler invokes neither cargo nor nextest (safe to run alongside a verify) ---"

assert "A9a: no 'cargo' token on any non-comment line of the sampler (it must not contend with the run it measures)" \
    bash -c '
        set -eu
        # [ -f ] FIRST: without it a MISSING sampler makes grep exit non-zero
        # and the negation pass vacuously — the assert would be green before the
        # script is even written.
        [ -f "'"$SAMPLER"'" ] || exit 1
        ! grep -vE "^[[:space:]]*#" "'"$SAMPLER"'" \
          | grep -qE "(^|[^-[:alnum:]_])cargo([^-[:alnum:]_]|$)"
    '

assert "A9b: no nextest INVOCATION (run/list/show-config) anywhere in the sampler" \
    bash -c '
        set -eu
        [ -f "'"$SAMPLER"'" ] || exit 1
        ! grep -qE "nextest[[:space:]]+(run|list|show-config)" "'"$SAMPLER"'"
    '

assert "A9c: the sampler is executable" \
    bash -c '[ -x "'"$SAMPLER"'" ]'

test_summary
