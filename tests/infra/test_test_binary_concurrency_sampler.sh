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
#                              (default, since task 6375: unset — enumerate
#                              <PROC_ROOT>/<pid>/exe directly; B2 pins it)
# No real host process is inspected, so the expectations are exact integers on
# ANY host and under ANY concurrent load — the Tests 13a-15b form the sibling
# nextest suites use.  The two deliberate exceptions are C4a/C4b, which pin
# KERNEL premises no fake tree can (DAC and Landlock behaviour of the real
# /proc); they read only processes they chose or spawned, so their integers stay
# exact, and they SKIP where the premise cannot be exercised.  Compile-free:
# bash/awk/readlink (plus python3 for C4b's Landlock helper) only, no cargo, no
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
# plus the merge role's bypass (test_semaphore_acquire in scripts/lib_test_semaphore.sh) two runs from
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

# ---------------------------------------------------------------------------
# A10 (THE CWD-DEPENDENCE FIX).  `for glob in $DEPS_GLOB` was unquoted, so the
# expansion underwent BOTH word-splitting (intended — DEPS_GLOB is a
# space-separated pattern LIST) and pathname expansion (never intended).  The
# default `*/target/*/deps/*` is itself a live glob, so whenever the sampler's
# cwd happened to contain a matching tree the loop variable bound to REAL
# RELATIVE PATHS instead of the pattern, and `case "$exe" in ($glob)` could
# never match an absolute /proc/<pid>/exe target.
#
# WHY THIS BLOCKED.  Measured on the identical fixture: peak=1 nonzero_samples=1
# from a lane root, peak=0 nonzero_samples=0 from a cwd containing a matching
# target/*/deps tree.  A cwd-induced zero is TEXTUALLY INDISTINGUISHABLE from the
# genuine defect-(b) INCONCLUSIVE reading this instrument exists to expose — a
# silently wrong measurement in the one place the instrument was built to be
# trustworthy.  The sampler is HOST-WIDE and may be launched from any directory,
# so its result must be cwd-independent.
#
# DELIBERATELY NOT ASSERTED: wall-clock cost.  The expansion is also a real
# per-sample performance blowup (it materialises every matching path on every
# pid, every tick), but this suite runs in bucket `pool` — CONCURRENTLY with
# other suites — so any timing assert would be load-flaky.  A10a covers the same
# root cause; the perf regression cannot recur without A10a going red.
# ---------------------------------------------------------------------------
echo ""
echo "--- A10: the match set is cwd-independent (DEPS_GLOB is split, never pathname-expanded) ---"

assert "A10a: a real test binary still counts when the sampler runs from a cwd that the DEFAULT glob matches (peak=1 — the reading must not depend on where it was launched)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 801=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4
        _pids_cmd_file "$d/pids" 801
        # ONE matching path is sufficient to trigger pathname expansion — the
        # 840k-path host directory that made this visible in production is not
        # needed and would make the assert slow and non-hermetic.
        mkdir -p "$d/cwd/lane/target/debug/deps"
        : > "$d/cwd/lane/target/debug/deps/x"
        cd "$d/cwd"
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '

# A10b pins that the fix disables PATHNAME EXPANSION ONLY.  DEPS_GLOB is
# documented as a space-separated pattern list, so the word-splitting half must
# survive: a "fix" that merely quoted "$DEPS_GLOB" would collapse the two
# patterns into one literal string and count nothing.
#
# Measured pre-fix value here is peak=1, not 0: the first pattern is destroyed by
# the cwd match, while the second (`*/build/*/tests/*`) happens to match nothing
# on disk and so survives literally under bash's default (nullglob off).  The
# assert is peak=2 either way — one of the two patterns is silently lost.
assert "A10b: two space-separated --deps-glob patterns BOTH still match from a glob-matching cwd (peak=2 — word-splitting must survive the fix)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" \
            801=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4 \
            802=/home/u/build/v1/tests/t-9999
        _pids_cmd_file "$d/pids" 801 802
        mkdir -p "$d/cwd/lane/target/debug/deps"
        : > "$d/cwd/lane/target/debug/deps/x"
        cd "$d/cwd"
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 \
              --deps-glob "*/target/*/deps/* */build/*/tests/*")
        [ "$(_field "$out" peak)" = "2" ]
    '

# A10c: silent-zero hardening.  `[ -n "$DEPS_GLOB" ]` passes on whitespace, and
# splitting whitespace yields ZERO patterns — after which every sample counts 0
# forever and the summary line looks exactly like an honest empty host.  Same
# silent-wrong-measurement class as A10a, so it must be rejected loudly at parse
# time rather than reported as data.
assert "A10c: a whitespace-only --deps-glob is REJECTED with a diagnostic (non-zero exit), not silently accepted as zero patterns" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        _mk_proc "$root" 801=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4
        _pids_cmd_file "$d/pids" 801
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 --deps-glob "   " 2>"$d/err") \
            && rc=0 || rc=$?
        # Non-zero exit, no summary line on stdout, and a diagnostic naming the flag.
        [ "$rc" -ne 0 ] || exit 1
        [ -z "$out" ] || exit 1
        grep -q "deps-glob" "$d/err"
    '

# A10d: the fix must not OVERREACH.  Disabling pathname expansion for the whole
# script would silently break any REIFY_SAMPLER_PIDS_CMD that legitimately relies
# on globbing — and it would fail by returning ZERO candidates, i.e. as another
# silent zero.  Globbing must be suppressed around the DEPS_GLOB split ALONE.
#
# NOTE FOR THE READER: this assert is GREEN BOTH BEFORE AND AFTER the fix by
# design — it is a regression guard on the fix, not a demonstration of the bug.
# Do not go hunting for a missing RED here.
assert "A10d: a REIFY_SAMPLER_PIDS_CMD that itself relies on globbing still works (peak=1 — globbing is suppressed only around the pattern split)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _field)"'
        _mk_proc "$root" 801=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4
        echo 801 > "$d/pidlist-a"
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" REIFY_SAMPLER_PIDS_CMD="cat $d/pidlist-*" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '


# ---------------------------------------------------------------------------
# B1 (THE PREFILTER->CONFIRM RACE — defect (d), measured).
#
# A1's exe confirmation is exact, but the pre-fix implementation paid for it with
# ONE `readlink` FORK PER CANDIDATE, issued after the prefilter had already
# returned its list.  Every fork widens the gap between "this pid was listed" and
# "this pid's exe was read", and nextest test binaries routinely live 0.2-0.8 s —
# so candidates that were genuinely running when the sample began were counted as
# vanished (A5's benign skip) by the time their turn came.
#
# MEASURED, not hypothesised: the pre-fix sampler undercounts a batched
# whole-/proc snapshot by 35-65%.  The A/B series and method live in
# docs/notes/nextest-global-pool-concurrency-observation.md -> "Prefilter->confirm
# race", which owns those numbers; this assert pins the BEHAVIOUR they justify,
# in integers that are exact on any host under any load.
#
# WHAT THIS ASSERT PINS: ONE CALL COVERS ALL CANDIDATES OF A SAMPLE.  The PATH
# shim delegates to the real readlink and only THEN destroys the remaining
# fixture pids, so it models the real failure semantics exactly — a candidate
# that exits after being observed but before its own confirmation would have run.
# An implementation that confirmed in batches of two would report peak=2 and stay
# RED; only whole-sample batching reports 5.  Measured pre-fix value: peak=1.
#
# DELIBERATELY NOT ASSERTED: wall-clock cost or fork count.  This suite runs in
# bucket `pool`, CONCURRENTLY with others, and A10's note already rejects timing
# asserts here as load-flaky; a fork-count assert would pass vacuously whenever
# the shim is not picked up.  The deletion tripwire yields exact integers on any
# host under any load instead.
# ---------------------------------------------------------------------------
echo ""
echo "--- B1 (race fix): every candidate of one sample is confirmed by a SINGLE invocation ---"

assert "B1: five real test binaries, four of which vanish the instant the first confirmation returns, still all count (peak=5 — one batched call covers every candidate; pre-fix peak=1)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _pids_cmd_file _field)"'
        # Captured BEFORE PATH is shimmed, so the shim delegates to the real tool.
        real_rl=$(command -v readlink)
        _mk_proc "$root" \
            601=/home/u/lanes/_lane-1/target/debug/deps/reify_lsp-2f1c9ab4 \
            602=/home/u/lanes/_lane-2/target/debug/deps/reify_core-1111aaaa \
            603=/home/u/lanes/_lane-3/target/debug/deps/reify_eval-2222bbbb \
            604=/home/u/lanes/_lane-4/target/debug/deps/reify_kernel-3333cccc \
            605=/home/u/lanes/_lane-5/target/debug/deps/reify_gui-4444dddd
        _pids_cmd_file "$d/pids" 601 602 603 604 605
        mkdir -p "$d/bin"
        cat > "$d/bin/readlink" <<SHIM
#!/usr/bin/env bash
# Delegate first, destroy second: the remaining candidates disappear only AFTER
# an invocation has completed, which is the race, not a simulation of it.
"$real_rl" "\$@"; rc=\$?
rm -rf "$root/602" "$root/603" "$root/604" "$root/605"
exit \$rc
SHIM
        chmod +x "$d/bin/readlink"
        out=$(PATH="$d/bin:$PATH" REIFY_SAMPLER_PROC_ROOT="$root" \
              REIFY_SAMPLER_PIDS_CMD="bash $d/pids" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "5" ]
    '


# ---------------------------------------------------------------------------
# B2 (THE OTHER HALF OF THE RACE — the prefilter IS the stale list).
#
# B1's batching is necessary but NOT sufficient.  The `pgrep -f` prefilter walks
# argv across all of /proc and costs 0.15-0.24 s on this host (2.6 s per pass on
# the loaded host that produced window 1), so the list it hands back has already
# decayed before confirmation starts.  Batching the confirmation of a stale list
# still confirms a stale list, however fast the batch is.  Candidate discovery
# and confirmation therefore have to be ONE pass over the same snapshot — which
# is exactly what the observation doc asks for.
#
# WHAT THIS ASSERT PINS: the DEFAULT path reads PROC_ROOT itself.  The fixture
# pid is deliberately ABOVE the host's /proc/sys/kernel/pid_max (4194304), so it
# can never collide with a live process and the assert can only pass by reading
# the injected root.  Measured pre-fix value: peak=0 — the default `pgrep -f`
# prefilters the REAL host, and no real pid has a directory under the fake root.
#
# THE SEAM SURVIVES, AS AN OVERRIDE.  REIFY_SAMPLER_PIDS_CMD is still honoured
# when set (A1-A10d all drive it); unset or empty now means "enumerate
# PROC_ROOT".  Both branches must feed the SAME batched confirmation, so this
# suite keeps pinning the code that actually runs beside a gate rather than a
# test-only branch.
# ---------------------------------------------------------------------------
echo ""
echo "--- B2 (race fix): candidate discovery reads PROC_ROOT — no host-wide prefilter on the default path ---"

assert "B2: with REIFY_SAMPLER_PIDS_CMD UNSET, a test binary under the injected PROC_ROOT still counts (peak=1 — discovery must not go via a host-wide pgrep; pre-fix peak=0)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _field)"'
        # 4194305 > /proc/sys/kernel/pid_max (4194304): unreachable as a live pid,
        # so a pass cannot be an accident of the real host.
        _mk_proc "$root" 4194305=/home/u/lanes/_lane-9/target/debug/deps/reify_lsp-2f1c9ab4
        # The parent environment must not leak a PIDS_CMD in — the whole point is
        # to exercise the branch a real run takes.
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0)
        [ "$(_field "$out" peak)" = "1" ]
    '


# ---------------------------------------------------------------------------
# C1-C3 (PTRACE BLINDNESS — a third silent zero, task 7500).  The mechanism and
# the measurements live in docs/notes/nextest-global-pool-concurrency-observation.md
# -> "Ptrace blindness (detected, task #7500)"; they are not restated here.
#
# What these asserts pin: the summary line carries own_uid_links= (candidates
# whose /proc/<pid> the caller owns) and resolved_links= (how many of those the
# caller could resolve), and a stderr WARNING names them when fewer than half
# resolved.  A few own-uid denials must NOT warn.
#
# The fixture is ten own-uid fake processes on the DEFAULT discovery path (as
# B2): one test binary and nine non-test images.  Pids sit above pid_max, as in
# B2.  Partial reach is modelled by a PATH readlink shim that resolves only an
# allow-listed subset of operands, delegating them to the real readlink.
# ---------------------------------------------------------------------------

# _mk_reach_fixture <root> — the ten fake own-uid processes C1-C3, C5 and C6 share.
_mk_reach_fixture() {
    local root="$1" pid
    local -a specs=( 4194311=/home/u/lanes/_lane-1/target/debug/deps/reify_lsp-2f1c9ab4 )
    for pid in 4194312 4194313 4194314 4194315 4194316 4194317 4194318 4194319 4194320; do
        specs+=( "$pid=/usr/bin/bash" )
    done
    _mk_proc "$root" "${specs[@]}"
}

# _readlink_allow_shim <bindir> <real_rl> <allowfile>
#   Writes <bindir>/readlink: a multi-operand readlink that resolves ONLY the
#   operands listed (one per line) in <allowfile>, by delegating them to
#   <real_rl>, and omits the rest — exiting 1 if it omitted any, which is the
#   real tool's behaviour for an unresolvable operand.
_readlink_allow_shim() {
    local bindir="$1" real_rl="$2" allowfile="$3"
    mkdir -p "$bindir"
    cat > "$bindir/readlink" <<SHIM
#!/usr/bin/env bash
declare -A allow=()
while IFS= read -r a; do [ -n "\$a" ] && allow["\$a"]=1; done < "$allowfile"
ops=(); rc=0
for op in "\$@"; do
    if [ -n "\${allow[\$op]:-}" ]; then ops+=( "\$op" ); else rc=1; fi
done
if [ "\${#ops[@]}" -gt 0 ]; then "$real_rl" "\${ops[@]}" || rc=\$?; fi
exit \$rc
SHIM
    chmod +x "$bindir/readlink"
}

echo ""
echo "--- C1-C3 (ptrace blindness): the summary reports own_uid_links/resolved_links and WARNs when most own-uid links are unreadable ---"

assert "C1: a caller that can read only 1 of its 10 own-uid exe links is reported blind (own_uid_links=10 resolved_links=1, WARNING naming own_uid_links), exit 0" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _mk_reach_fixture _readlink_allow_shim _field)"'
        real_rl=$(command -v readlink)
        _mk_reach_fixture "$root"
        printf "%s\n" "$root/4194311/exe" > "$d/allow"
        _readlink_allow_shim "$d/bin" "$real_rl" "$d/allow"
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(PATH="$d/bin:$PATH" REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err") \
            && rc=0 || rc=$?
        [ "$rc" -eq 0 ] || exit 1
        [ "$(_field "$out" own_uid_links)" = "10" ] || exit 1
        [ "$(_field "$out" resolved_links)" = "1" ] || exit 1
        [ "$(_field "$out" peak)" = "1" ] || exit 1
        [ "$(_field "$out" nonzero_samples)" = "1" ] || exit 1
        grep WARNING "$d/err" | grep -qE "own_uid_links=10([^0-9]|$)"
    '

assert "C1b: TOTAL blindness (no exe link readable) still reports the own-uid count (own_uid_links=10 resolved_links=0) and WARNs naming it, exit 0" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _mk_reach_fixture _readlink_allow_shim _field)"'
        real_rl=$(command -v readlink)
        _mk_reach_fixture "$root"
        : > "$d/allow"
        _readlink_allow_shim "$d/bin" "$real_rl" "$d/allow"
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(PATH="$d/bin:$PATH" REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err") \
            && rc=0 || rc=$?
        [ "$rc" -eq 0 ] || exit 1
        [ "$(_field "$out" own_uid_links)" = "10" ] || exit 1
        [ "$(_field "$out" resolved_links)" = "0" ] || exit 1
        grep WARNING "$d/err" | grep -qE "own_uid_links=10([^0-9]|$)"
    '

assert "C2: a few own-uid denials are NOT blindness (8 of 10 resolve: own_uid_links=10 resolved_links=8 peak=1, no WARNING)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _mk_reach_fixture _readlink_allow_shim _field)"'
        real_rl=$(command -v readlink)
        _mk_reach_fixture "$root"
        for p in 4194311 4194312 4194313 4194314 4194315 4194316 4194317 4194318; do
            printf "%s\n" "$root/$p/exe"
        done > "$d/allow"
        _readlink_allow_shim "$d/bin" "$real_rl" "$d/allow"
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(PATH="$d/bin:$PATH" REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err") \
            && rc=0 || rc=$?
        [ "$rc" -eq 0 ] || exit 1
        [ "$(_field "$out" own_uid_links)" = "10" ] || exit 1
        [ "$(_field "$out" resolved_links)" = "8" ] || exit 1
        [ "$(_field "$out" peak)" = "1" ] || exit 1
        if grep -q WARNING "$d/err"; then exit 1; fi
    '

# C3: the new fields are APPENDED.  The seven existing fields keep their order —
# the observation doc quotes the old prefix verbatim, and _field-style consumers
# must keep working.
assert "C3: the summary line is the seven existing fields in order, then own_uid_links= and resolved_links= appended (one anchored ERE)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _mk_reach_fixture)"'
        _mk_reach_fixture "$root"
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 2>/dev/null)
        re="^INFO: nextest test-binary concurrency: peak=[0-9]+ samples=[0-9]+ nonzero_samples=[0-9]+ interval=[0-9]+s duration=[0-9]+s deps_glob=[^ ]+ host_nproc=[^ ]+ own_uid_links=[0-9]+ resolved_links=[0-9]+$"
        grep -qE "$re" <<<"$out"
    '

# ---------------------------------------------------------------------------
# C4a/C4b (LIVE KERNEL PREMISES of the C-series detector).
#
# C1-C3 pin the arithmetic over a shim; these two pin the kernel facts it rests
# on, against the real /proc.  C4a: DAC's denials of other-uid processes do not
# read as blindness.  C4b: under real Landlock, own_uid_links still counts the
# own-uid processes whose exe the caller cannot read.
#
# The blindness WARNING is matched by its own_uid_links= token: these fixtures
# contain no test binary, so the independent nonzero_samples=0 WARNING
# legitimately fires as well.
# ---------------------------------------------------------------------------
echo ""
echo "--- C4a-C4b (live): other-uid DAC denials are not blindness; real Landlock blindness is reported ---"

_c4a_skip=""
_c4a_foreign_pids=()
if [ "$(id -u)" -eq 0 ]; then
    _c4a_skip="C4a needs a non-root caller"
else
    for _p in /proc/[0-9]*; do
        [ -O "$_p" ] && continue
        _c4a_foreign_pids+=( "${_p#/proc/}" )
        [ "${#_c4a_foreign_pids[@]}" -ge 20 ] && break
    done
    unset _p
    [ "${#_c4a_foreign_pids[@]}" -gt 0 ] || \
        _c4a_skip="C4a found no other-uid process in /proc (hidepid?)"
fi

if [ -n "$_c4a_skip" ]; then
    echo "  SKIP: $_c4a_skip"
else
    assert "C4a: real /proc — this shell plus ${#_c4a_foreign_pids[@]} other-uid pids reads own_uid_links=1 resolved_links=1 and does NOT warn of blindness (DAC denials of other uids are not in the denominator)" \
        bash -c '
            set -eu
            d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
            '"$(declare -f _field)"'
            for p in $$ '"${_c4a_foreign_pids[*]}"'; do echo "$p"; done > "$d/pids"
            unset REIFY_SAMPLER_PROC_ROOT
            out=$(REIFY_SAMPLER_PIDS_CMD="cat $d/pids" \
                  bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err") \
                && rc=0 || rc=$?
            [ "$rc" -eq 0 ] || exit 1
            [ "$(_field "$out" own_uid_links)" = "1" ] || exit 1
            [ "$(_field "$out" resolved_links)" = "1" ] || exit 1
            if grep WARNING "$d/err" | grep -q own_uid_links; then exit 1; fi
        '
fi

HELPER="$REPO_ROOT/gui/src-tauri/sandbox/landlock_exec.py"
_c4b_skip=""
if ! command -v python3 >/dev/null 2>&1; then
    _c4b_skip="python3 not on PATH"
elif [ ! -f "$HELPER" ]; then
    _c4b_skip="helper missing: $HELPER"
else
    # Probe the exact invocation C4b depends on: the same --writable set, over a
    # scratch dir made the way the assert makes its own.
    _c4b_probe_dir="$(mktemp -d)"
    if ! python3 "$HELPER" --writable "$_c4b_probe_dir" --writable "${TMPDIR:-/tmp}" \
            -- true >/dev/null 2>&1; then
        _c4b_skip="landlock_exec.py --writable <scratch> --writable ${TMPDIR:-/tmp} -- true failed"
    fi
    rm -rf "$_c4b_probe_dir"
    unset _c4b_probe_dir
fi

if [ -n "$_c4b_skip" ]; then
    echo "  SKIP: C4b: Landlock unavailable on this host ($_c4b_skip)"
else
    assert "C4b: real Landlock — 8 own sleep processes read own_uid_links=8 resolved_links=8 unsandboxed, and own_uid_links=8 resolved_links=0 plus a blindness WARNING from inside a nested Landlock domain, exit 0" \
        bash -c '
            set -eu
            d=$(mktemp -d); sl=""
            trap "kill \$sl 2>/dev/null; rm -rf \"$d\"" EXIT
            '"$(declare -f _field)"'
            for i in 1 2 3 4 5 6 7 8; do
                sleep 300 </dev/null >/dev/null 2>&1 &
                sl="$sl $!"
                echo "$!" >> "$d/pids"
            done
            unset REIFY_SAMPLER_PROC_ROOT
            out=$(REIFY_SAMPLER_PIDS_CMD="cat $d/pids" \
                  bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err.direct") \
                && rc=0 || rc=$?
            [ "$rc" -eq 0 ] || exit 1
            [ "$(_field "$out" own_uid_links)" = "8" ] || exit 1
            [ "$(_field "$out" resolved_links)" = "8" ] || exit 1
            if grep WARNING "$d/err.direct" | grep -q own_uid_links; then exit 1; fi
            out=$(REIFY_SAMPLER_PIDS_CMD="cat $d/pids" \
                  python3 "'"$HELPER"'" --writable "$d" --writable "${TMPDIR:-/tmp}" -- \
                  bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err.sandboxed") \
                && rc=0 || rc=$?
            [ "$rc" -eq 0 ] || exit 1
            [ "$(_field "$out" own_uid_links)" = "8" ] || exit 1
            [ "$(_field "$out" resolved_links)" = "0" ] || exit 1
            grep WARNING "$d/err.sandboxed" | grep -qE "own_uid_links=8([^0-9]|$)"
        '
fi

# ---------------------------------------------------------------------------
# C5-C6 (WHAT THE TWO COUNTS RANGE OVER).  Both fields count the SAME operands:
# the caller's own-uid candidates.
#
# C5: a caller that can read OTHER uids' exe links (root, or CAP_SYS_PTRACE)
# must not have those reads mask blindness among its own processes, and its
# other-uid test binary must still count toward peak.  Modelled by candidate
# directories that are symlinks to the root-owned `/`, plus a readlink shim that
# fabricates their targets: the real readlink cannot stand for reach this
# caller lacks.
#
# C6: the non-numeric PROC_ROOT entries (self, thread-self) alias the sampler's
# own helper processes, so they are not candidates and enter neither count.
# ---------------------------------------------------------------------------

# _readlink_map_shim <bindir> <mapfile>
#   Writes <bindir>/readlink: resolves each operand listed in <mapfile> (one
#   `<operand><TAB><target>` per line) to its target, and omits the rest —
#   exiting 1 if it omitted any, as the real tool does.
_readlink_map_shim() {
    local bindir="$1" mapfile="$2"
    mkdir -p "$bindir"
    cat > "$bindir/readlink" <<SHIM
#!/usr/bin/env bash
declare -A map=()
while IFS=\$'\t' read -r op target; do [ -n "\$op" ] && map["\$op"]="\$target"; done < "$mapfile"
rc=0
for op in "\$@"; do
    if [ -n "\${map[\$op]:-}" ]; then printf '%s\n' "\${map[\$op]}"; else rc=1; fi
done
exit \$rc
SHIM
    chmod +x "$bindir/readlink"
}

echo ""
echo "--- C5-C6: own_uid_links and resolved_links both range over own-uid pid candidates only ---"

if [ -O / ]; then
    echo "  SKIP: C5 needs a caller that does not own / (it models other-uid processes as symlinks to /)"
else
    assert "C5: a caller that reads 20 OTHER-uid exe links but only 1 of its 10 own is still reported blind (own_uid_links=10 resolved_links=1, WARNING), and the other-uid test binary still counts (peak=2), exit 0" \
        bash -c '
            set -eu
            d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
            root="$d/proc"
            '"$(declare -f _mk_proc _mk_reach_fixture _readlink_map_shim _field)"'
            _mk_reach_fixture "$root"
            : > "$d/pids"
            for p in 4194311 4194312 4194313 4194314 4194315 4194316 4194317 4194318 4194319 4194320; do
                echo "$p" >> "$d/pids"
            done
            printf "%s\t%s\n" "$root/4194311/exe" \
                /home/u/lanes/_lane-1/target/debug/deps/reify_lsp-2f1c9ab4 > "$d/map"
            for p in $(seq 4194321 4194340); do
                ln -s / "$root/$p"
                echo "$p" >> "$d/pids"
                printf "%s\t%s\n" "$root/$p/exe" /usr/sbin/cron >> "$d/map"
            done
            printf "%s\t%s\n" "$root/4194321/exe" \
                /home/v/lanes/_lane-2/target/debug/deps/reify_core-1111aaaa >> "$d/map"
            _readlink_map_shim "$d/bin" "$d/map"
            out=$(PATH="$d/bin:$PATH" REIFY_SAMPLER_PROC_ROOT="$root" \
                  REIFY_SAMPLER_PIDS_CMD="cat $d/pids" \
                  bash "'"$SAMPLER"'" --duration 0 --interval 0 2>"$d/err") \
                && rc=0 || rc=$?
            [ "$rc" -eq 0 ] || exit 1
            [ "$(_field "$out" own_uid_links)" = "10" ] || exit 1
            [ "$(_field "$out" resolved_links)" = "1" ] || exit 1
            [ "$(_field "$out" peak)" = "2" ] || exit 1
            grep WARNING "$d/err" | grep -qE "own_uid_links=10([^0-9]|$)"
        '
fi

assert "C6: self and thread-self entries under PROC_ROOT are not candidates (own_uid_links=10 resolved_links=10 beside the ten-process fixture)" \
    bash -c '
        set -eu
        d=$(mktemp -d); trap "rm -rf \"$d\"" EXIT
        root="$d/proc"
        '"$(declare -f _mk_proc _mk_reach_fixture _field)"'
        _mk_reach_fixture "$root"
        _mk_proc "$root" self=/usr/bin/readlink thread-self=/usr/bin/readlink
        unset REIFY_SAMPLER_PIDS_CMD
        out=$(REIFY_SAMPLER_PROC_ROOT="$root" \
              bash "'"$SAMPLER"'" --duration 0 --interval 0 2>/dev/null)
        [ "$(_field "$out" own_uid_links)" = "10" ] || exit 1
        [ "$(_field "$out" resolved_links)" = "10" ]
    '


test_summary
