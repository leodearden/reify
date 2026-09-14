#!/usr/bin/env bash
# Infrastructure drift-guard for task 5264 — scripts/verify.sh must accept the
# --test-threads=N offline parallelism cap.
#
# Background: the dark-factory offline-deep-test-lane worker (β3) invokes
#   scripts/run-offline-deep.sh --test-threads=N …
# which forwards its args verbatim to `verify.sh test --test-threads=N`
# (run-offline-deep.sh delegates `"$SCRIPT_DIR/verify.sh" test "$@"`). Before
# this task verify.sh's arg parser had no --test-threads case, so the flag hit
# the `*)` catch-all, verify.sh exited 64 ("unknown argument '--test-threads=1'")
# BEFORE any cargo work, and the offline lane went red.
#
# This test locks in the primitive (CLAUDE.md seam: "reify ships the primitive,
# dark-factory wires the invocation"). verify.sh must:
#   - accept --test-threads=N in both the '=N' and space-separated forms,
#   - validate N as a positive integer (reject 0/negative/non-numeric/float),
#   - thread N into the emitted cargo nextest / cargo test plan, while leaving
#     the no-flag DEFAULT plan byte-for-byte unchanged,
#   - document --test-threads in `--help`.
#
# Hermetic: drives ONLY `verify.sh --print-plan` and `run-offline-deep.sh
# --print-plan` (verify.sh builds the plan and exits 0 — no cargo build, no
# tests executed). Nextest-vs-fallback command-shape assertions are guarded on
# a NEXTEST_AVAILABLE probe of the plan header's `nextest=` token, the sibling
# idiom from test_run_offline_deep.sh / test_verify_offline_partition.sh; the
# host-independent invariants (accept/exit-0, validation/exit-64,
# default-has-no-flag) are asserted unconditionally.
#
# Mirrors:
#   - tests/infra/test_run_offline_deep.sh — --print-plan oracle + NEXTEST
#     availability probe idiom, wrapper-drift structure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# For nextest_available_in_plan (the plan-header availability probe below).
# Sourcing the lib installs no trap and builds no environment — only
# nextest_absent_init does that, and this suite deliberately never calls it.
[ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || {
    echo "ERROR: nextest_absent_lib.sh not found at $SCRIPT_DIR/nextest_absent_lib.sh"; exit 1; }
source "$SCRIPT_DIR/nextest_absent_lib.sh"

VERIFY="$REPO_ROOT/scripts/verify.sh"
RUN_OFFLINE_DEEP="$REPO_ROOT/scripts/run-offline-deep.sh"

echo "=== verify.sh --test-threads=N tests (task 5264) ==="

# ---------------------------------------------------------------------------
# Test 1: ACCEPTANCE — verify.sh accepts --test-threads=N in both the '=N' and
# the space-separated forms and still exits 0 (--print-plan is hermetic). rc is
# captured via `|| rc=$?` so a RED-phase exit-64 reports a clean assertion FAIL
# here instead of tripping this script's own `set -e`.
# RED (base): verify.sh has no --test-threads case, so both forms hit the `*)`
# catch-all and exit 64 ("unknown argument …").
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: --test-threads=N is accepted (exit 0) — both '=N' and space forms ---"

EQ_RC=0
bash "$VERIFY" test --scope all --print-plan --test-threads=4 >/dev/null 2>&1 || EQ_RC=$?
assert "verify.sh test --print-plan --test-threads=4 ('=N' form) exits 0" \
    test "$EQ_RC" -eq 0

SPACE_RC=0
bash "$VERIFY" test --scope all --print-plan --test-threads 4 >/dev/null 2>&1 || SPACE_RC=$?
assert "verify.sh test --print-plan --test-threads 4 (space form) exits 0" \
    test "$SPACE_RC" -eq 0

# ---------------------------------------------------------------------------
# Test 2: THREADING + DEFAULT-INVARIANT. --test-threads=N must reach the
# emitted cargo command; the no-flag default plan must stay byte-identical to
# today. The command shape depends on whether nextest is installed on this
# host, so those assertions are guarded on a NEXTEST_AVAILABLE probe of the
# plan header's `nextest=` token (sibling idiom, test_run_offline_deep.sh).
# RED (step-2 only parses): emit_nextest_pass does not yet thread the value,
# so the '=4 reaches the plan' assertion fails; the default-invariant
# assertion is already green (it guards step-4 against regressing the default).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: --test-threads=N threads into the plan; default plan unchanged ---"

PLAN_TT4="$(bash "$VERIFY" test --scope all --print-plan --test-threads=4 2>/dev/null)" || true
PLAN_DEFAULT="$(bash "$VERIFY" test --scope all --print-plan 2>/dev/null)" || true

# nextest availability probe — read back OUT of the PLAN_DEFAULT capture above
# via the shared detector in tests/infra/nextest_absent_lib.sh (task 5644), not
# by running --print-plan a third time. PLAN_DEFAULT stays: the default-invariant
# asserts below read it too.
#
# The empty-plan (RED) guard the local `|| true` used to provide is preserved:
# nextest_available_in_plan routes through _nextest_absent_header_of, which is
# itself `|| true`-guarded and returns non-zero (=> NEXTEST_AVAILABLE=0) on an
# empty plan instead of aborting under pipefail. That CONVERTS the failure mode
# rather than removing it — an abort becomes a quiet "unavailable" — so it is
# not a robustness win on its own.
#
# What makes it safe is the else arm below, which is all POSITIVE asserts on the
# fallback shape ("fallback: --test-threads=4 threads into the cargo test line"
# and the two after it, all grepping `cargo test `). A false "unavailable" on a
# nextest-present host routes into that arm and fails loudly against a plan that
# emits `cargo nextest run` instead. Do not weaken those to absence checks — the
# positive form is what keeps a wrong availability answer from going green.
NEXTEST_AVAILABLE=0
if nextest_available_in_plan "$PLAN_DEFAULT"; then
    NEXTEST_AVAILABLE=1
fi
echo "(nextest available on this host: $NEXTEST_AVAILABLE)"

if [ "$NEXTEST_AVAILABLE" -eq 1 ]; then
    assert "nextest: --test-threads=4 threads into the cargo nextest run line" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -qF -- "--test-threads=4"' \
        _ "$PLAN_TT4"

    assert "nextest: DEFAULT plan (no flag) has NO --test-threads= token on any cargo line (byte-identical default)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "--test-threads="' \
        _ "$PLAN_DEFAULT"
else
    assert "fallback: --test-threads=4 threads into the cargo test line" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "--test-threads=4"' \
        _ "$PLAN_TT4"

    assert "fallback: --test-threads=4 replaces the default (no residual --test-threads=1 on the cargo test line)" \
        bash -c '! printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "--test-threads=1"' \
        _ "$PLAN_TT4"

    assert "fallback: DEFAULT plan (no flag) still carries -- --test-threads=1 (byte-identical default)" \
        bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo test " | grep -qF -- "-- --test-threads=1"' \
        _ "$PLAN_DEFAULT"
fi

# ---------------------------------------------------------------------------
# Test 3: E2E — faithful β3 repro through the run-offline-deep.sh wrapper,
# which forwards args verbatim to `verify.sh test`. The forwarded
# --test-threads=2 must reach the offline plan's cargo line. Host-independent
# (nextest -> `cargo nextest run … --test-threads=2 …`; fallback ->
# `cargo test … -- --test-threads=2`), so asserted unconditionally.
# RED (step-2 only parses): the value is accepted (exit 0) but not threaded,
# so the 'carries --test-threads=2' assertion fails.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: E2E run-offline-deep.sh --test-threads=2 --print-plan (β3 repro) ---"

E2E_RC=0
PLAN_E2E="$(bash "$RUN_OFFLINE_DEEP" --test-threads=2 --print-plan 2>/dev/null)" || E2E_RC=$?

assert "run-offline-deep.sh --test-threads=2 --print-plan exits 0" \
    test "$E2E_RC" -eq 0

assert "run-offline-deep.sh --test-threads=2 plan carries --test-threads=2 on a cargo line" \
    bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo " | grep -qF -- "--test-threads=2"' \
    _ "$PLAN_E2E"

# ---------------------------------------------------------------------------
# Test 4: VALIDATION — N must be a positive integer. Reject zero, leading-zero
# forms ('00'/'007' — cargo/nextest parse '00' as 0 and reject it only at
# runtime, so reject at parse time; net validated set is exactly ^[1-9][0-9]*$),
# negative, non-numeric, float, and the explicit empty-value form '--test-threads=' with
# exit 64 (the same invalid-value convention as --profile / --scope). The
# explicit empty value ('--test-threads=') is an ERROR distinct from an UNSET
# flag (no --test-threads at all, which stays exit 0 / default plan — asserted
# in Test 2); telling them apart requires a "flag-was-seen" sentinel, since
# both leave TEST_THREADS empty after parsing. The bare no-value '--test-threads'
# form is deliberately NOT asserted here: it exits 1 via bash ${2:?}, the same
# as the bare '--profile' / '--scope' forms.
# RED (step-4): values are stored/threaded without validation, so they exit 0.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 4: invalid --test-threads values exit 64 (parse-time validation) ---"

for _v in 0 00 007 -1 abc 2.5 ""; do
    _rc=0
    bash "$VERIFY" test --scope all --print-plan --test-threads="$_v" >/dev/null 2>&1 || _rc=$?
    assert "invalid --test-threads='$_v' exits 64 (want positive integer)" \
        test "$_rc" -eq 64
done

# ---------------------------------------------------------------------------
# Test 5: CLI-surface existence — the flag must be documented in usage().
# Existence grep only (one token, not a prose pin), on STDOUT (the -h|--help
# path prints usage to stdout and exits 0). RED (base usage() header does not
# yet mention --test-threads).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 5: --help / -h document the --test-threads flag ---"

assert "verify.sh --help stdout documents the --test-threads flag" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -qF -- "--test-threads"' \
    _ "$VERIFY"

assert "verify.sh -h stdout documents the --test-threads flag" \
    bash -c 'bash "$1" -h 2>/dev/null | grep -qF -- "--test-threads"' \
    _ "$VERIFY"

# ---------------------------------------------------------------------------
# Test 6: a forwarded CLI --test-threads must not silently defeat the generated
# pool.
#
# THE HAZARD, concretely.  verify.sh forwards TEST_THREADS into the emitted
# nextest line at two sites — emit_nextest_pass (~2278) and the gui-feature
# pass (~2750).  nextest's CLI --test-threads OUTRANKS --config-file, so a
# forwarded value silently REPLACES the [profile.default] test-threads pool that
# scripts/gen-nextest-config.sh derived.  The env route is already closed
# (verify.sh unconditionally clears TEST_THREADS at ~600), so the CLI flag is the
# only way a value reaches the pool — and the EXCEEDS case is unreachable by any
# caller today (dark-factory's offline lane passes 1 or 2).  This is therefore
# inert-by-default hardening: it enforces the invariant rather than merely
# logging it, without redding a single live caller.
#
# THE ASYMMETRY IS DELIBERATE.  Capping BELOW the derived pool is the sanctioned
# offline-lane use and must stay legal — visible, but legal.  RAISING the pool
# above the derivation is the silent defeat, and is refused.
#
# WHERE THE COMPARISON LIVES.  In gen-nextest-config.sh, the single place the
# derived `tt` exists (SPOT); duplicating the derivation inside verify.sh to
# compare against it would create exactly the lockstep pair the derivation was
# consolidated to avoid.  The knob matches that script's existing all-env input
# convention (REIFY_OCCT_NPROC, REIFY_NEXTEST_TEST_THREADS_HARD_CAP, ...).
#
# HERMETIC, and compile-free: 6a-6c drive the generator DIRECTLY — pure bash, no
# cargo, no nextest — with the pool pinned to an exact integer on any host by the
# script's own testability knobs, so the expectations are literal integers.
# "Hermetic" here is enforced, not merely claimed: `_gen` scrubs the two inherited
# pool knobs with `env -u`, and 6e is the tripwire that reds if that scrub is ever
# removed.  It has to be enforced, because this suite is run as a CHILD of
# verify.sh — and verify.sh handles the knob (6d pins the shape: a per-invocation
# prefix at each generation site, never a process-global export).
# ---------------------------------------------------------------------------
GEN="$REPO_ROOT/scripts/gen-nextest-config.sh"

# REIFY_OCCT_NPROC=8 and REIFY_NEXTEST_TEST_THREADS_HARD_CAP=8 make
# min(HARD_CAP, nproc) = 8 regardless of the real host.  Verified against the
# emitted config: `test-threads = 8`.
_TT=8

# _gen <cli-value-or-empty> — run the generator with the pool pinned, leaving the
# result in _GEN_RC / _GEN_OUT (stdout) / _GEN_ERR (a file holding stderr).
#
# `env -u` SCRUBS the two knobs that can move the derived `tt`, in BOTH branches.
# Omitting an assignment prefix does not unset anything, and this suite runs as a
# child of verify.sh (verify-pipeline-infra-tests.txt:44), which an earlier draft
# of the wiring exported REIFY_NEXTEST_CLI_TEST_THREADS process-globally into —
# so an inherited value walked straight in.  That route is closed at source now (a
# per-invocation prefix; 6d), but the scrub is what makes the exact-integer
# expectations true under any OTHER caller too, a developer's interactive shell
# included.  REIFY_NEXTEST_TEST_THREADS is scrubbed for the same
# reason and a quieter one: it REPLACES `tt` wholesale
# (gen-nextest-config.sh:315), which would move the pool off the pinned 8 that 6b
# and 6c assert exact integers against.  6e is the tripwire that holds this.
#
# REIFY_OCCT_NPROC and REIFY_NEXTEST_TEST_THREADS_HARD_CAP need no `-u` because
# they are assigned explicitly.  NOT `env -i`: that would drop PATH and TMPDIR and
# change what is under test.  Same idiom the sibling suite already uses against
# this very generator (tests/infra/test_occt_gated_scope.sh:318/340/376/386/425).
_gen() {
    local cli="$1"
    _GEN_ERR="$(mktemp "${TMPDIR:-/tmp}/reify-gen-err.XXXXXX")"
    _GEN_RC=0
    if [ -n "$cli" ]; then
        _GEN_OUT="$(env -u REIFY_NEXTEST_CLI_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS \
                        REIFY_OCCT_NPROC="$_TT" REIFY_NEXTEST_TEST_THREADS_HARD_CAP="$_TT" \
                        REIFY_NEXTEST_CLI_TEST_THREADS="$cli" \
                        bash "$GEN" 2>"$_GEN_ERR")" || _GEN_RC=$?
    else
        _GEN_OUT="$(env -u REIFY_NEXTEST_CLI_TEST_THREADS -u REIFY_NEXTEST_TEST_THREADS \
                        REIFY_OCCT_NPROC="$_TT" REIFY_NEXTEST_TEST_THREADS_HARD_CAP="$_TT" \
                        bash "$GEN" 2>"$_GEN_ERR")" || _GEN_RC=$?
    fi
}

# The generator mktemps its config; the caller owns cleanup (see its header).
_gen_cleanup() {
    [ -z "${_GEN_ERR:-}" ] || rm -f "$_GEN_ERR"
    { [ -z "${_GEN_OUT:-}" ] || [ ! -f "$_GEN_OUT" ]; } || rm -f "$_GEN_OUT"
    return 0
}

# _names <file> <integer> — the diagnostic must name the number, as a number:
# the boundaries stop '64' from satisfying a search for '4' or for '8'.
_names() { grep -qE "(^|[^0-9])$2([^0-9]|\$)" "$1"; }

# 6a: the default must be byte-identical to today — path on stdout, and NOTHING
# on stderr.
#
# The stderr check is an EMPTY-FILE assertion, and deliberately so.  An earlier
# form here grepped that stderr for the string `REIFY_NEXTEST_CLI_TEST_THREADS`
# and asserted its absence — which could never fail, under any generator
# behaviour: the generator's two diagnostics are spelled
# `gen-nextest-config.sh: ERROR — --test-threads=<N> …` and
# `gen-nextest-config.sh: note — --test-threads=<N> …`, and neither writes the
# identifier.  Measured: 0 occurrences on the note branch.  That made 6a a
# tautology dressed as an assertion — it degenerated to "exit 0 and a file
# exists".
#
# Empty-stderr is also strictly STRONGER than grepping for the two known
# diagnostics: it reds if the default path starts emitting any OTHER spurious
# output too, which is exactly the byte-identical-to-today property 6a exists to
# pin.  Measured as the unset path's actual behaviour: rc 0, 0 bytes of stderr.
_check_6a() {
    local ok=0
    _gen ""
    if [ "$_GEN_RC" -eq 0 ] && [ -f "${_GEN_OUT:-}" ] \
       && [ ! -s "$_GEN_ERR" ]; then ok=1; fi
    _gen_cleanup
    [ "$ok" -eq 1 ]
}

# 6b: capping BELOW the pool is sanctioned — visible on stderr, but exit 0 with
# the path still printed.
_check_6b() {
    local ok=0
    _gen 4
    if [ "$_GEN_RC" -eq 0 ] && [ -f "${_GEN_OUT:-}" ] \
       && _names "$_GEN_ERR" 4 && _names "$_GEN_ERR" "$_TT"; then ok=1; fi
    _gen_cleanup
    [ "$ok" -eq 1 ]
}

# 6c: RAISING the pool above the derivation is the silent defeat — refused, and
# with NOTHING on stdout, because the stdout contract is "prints ONLY the
# resolved temp file path".
#
# THE EXIT CODE IS MATCHED EXACTLY, not merely as "non-zero".  64 is the
# repo-wide usage-error convention (Test 4 above asserts it exactly for the same
# reason), it is what gen-nextest-config.sh's header contracts for this case, and
# it is what verify.sh's _die_nextest_config propagates verbatim via `exit "$1"`
# — so it is a value a caller actually depends on.  A non-zero check would also
# be satisfied by a `set -euo pipefail` abort introduced anywhere earlier in the
# generator (an unbound variable, a failed mktemp), which produces no stdout
# either; the two stderr checks narrow that but cannot distinguish "refused for
# the documented reason" from "died for an unrelated one".
_check_6c() {
    local ok=0
    _gen 64
    if [ "$_GEN_RC" -eq 64 ] && [ -z "${_GEN_OUT:-}" ] \
       && _names "$_GEN_ERR" 64 && _names "$_GEN_ERR" "$_TT"; then ok=1; fi
    _gen_cleanup
    [ "$ok" -eq 1 ]
}

# 6d: EVERY generator invocation in verify.sh carries BOTH halves of the seam —
# the per-invocation CLI knob and the _die_nextest_config handler — and the
# handler is defined exactly once.
#
# WHY THE INVOCATIONS AND NOT THE KNOB'S DELIVERY MECHANISM.  An earlier form
# here asserted a single `export REIFY_NEXTEST_CLI_TEST_THREADS=` line.  That
# pinned the weakest part of the wiring and actively cemented the wrong design:
# an export is process-global for the whole verify.sh run and inherited by every
# child, the infra suites verify.sh itself executes among them (measured:
# `REIFY_NEXTEST_CLI_TEST_THREADS=2 bash tests/infra/test_occt_gated_scope.sh`
# goes from 61/0 to 59 passed / 2 FAILED, because Tests 17g/17j pin a pool of 1
# and an inherited 2 then trips the exceeds branch).  The knob is now a
# per-invocation prefix, which the old assert would have RED.
#
# Meanwhile the half that matters carried no coverage at all: dropping
# `|| _die_nextest_config "$?"` from a call site silently restores the
# unattributed `set -e` abort the handler exists to prevent.  Both halves live on
# the invocation, so one assert over the invocations covers the whole seam.
#
# WHY THIS ONE IS A SOURCE-TEXT ASSERT, not a behavioural one.  The generator is
# invoked only in EXECUTE mode — --print-plan deliberately never spawns it, which
# is what keeps the plan a pure, hermetic oracle — and this whole suite IS a
# --print-plan oracle.  There is therefore no behavioural reach from here to the
# wiring, and the static assert is a considered choice rather than a shortcut.
#
# NOTE ON SHAPE: the file is read ONCE into an array and examined in the shell,
# with no pipeline anywhere.  `... | grep -q` under `set -o pipefail` is a
# load-dependent flake: `grep -q` exits at its first match and SIGPIPEs the
# upstream producer, so the PIPELINE reports 141 even though the match succeeded
# — and whether upstream had already finished writing depends on host load.  It
# passed on an idle host and failed under a concurrent gate.  Reading the array
# also gives the next line, which is where the `||` handler sits.
#
# `invocations >= 1` is the anti-vacuity guard: a rename that stopped matching
# would otherwise make this assert pass by examining nothing.
_check_6d() {
    local -a src=()
    mapfile -t src <"$VERIFY" || return 1
    local i line defs=0 invocations=0
    for ((i = 0; i < ${#src[@]}; i++)); do
        line="${src[i]}"
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        [[ "$line" =~ ^[[:space:]]*_die_nextest_config\(\) ]] && defs=$((defs + 1))
        [[ "$line" == *gen-nextest-config.sh* ]] || continue
        # A non-comment mention of the generator is either the handler's own
        # stderr diagnostic, which names it, or a real invocation — and in this
        # script every invocation is a command substitution.  Any third shape is
        # one this assert cannot reason about, so it reds rather than guesses.
        [[ "$line" == *'>&2'* ]] && continue
        [[ "$line" == *'$('* ]] || return 1
        invocations=$((invocations + 1))
        [[ "$line" =~ REIFY_NEXTEST_CLI_TEST_THREADS=\"?\$\{?TEST_THREADS ]] || return 1
        [[ "$line" == *_die_nextest_config* || "${src[i + 1]-}" == *_die_nextest_config* ]] || return 1
    done
    [ "$defs" -eq 1 ] && [ "$invocations" -ge 1 ]
}

# 6e: THE ENV-LEAK TRIPWIRE — the executable proof of this block's HERMETIC
# claim, which is otherwise an advertisement rather than a guarantee.
#
# THE EXPOSURE WAS REAL, NOT HYPOTHETICAL.  scripts/verify-pipeline-infra-tests.txt
# line 44 maps `scripts/verify.sh -> tests/infra/test_verify_*.sh`, so this suite
# is selected and executed as a CHILD of a verify.sh run — and an earlier draft
# of the wiring `export`ed REIFY_NEXTEST_CLI_TEST_THREADS process-globally
# whenever --test-threads=N was passed.  A `_gen` branch that merely OMITS an
# assignment prefix does not unset anything, so that value walked straight into
# the generator.  Measured against the omit-only form: rc=64 and the ERROR
# diagnostic, i.e. 6a red for a reason that had nothing to do with what 6a tests.
#
# THAT ROUTE IS NOW CLOSED AT SOURCE — the knob is a per-invocation prefix at the
# two generation sites (6d pins that), so no child of verify.sh inherits it.  The
# scrub and this tripwire stay anyway, and are not belt-and-braces: `env -u` is
# what makes 6b and 6c's exact-integer expectations true under ANY caller,
# including a developer with either knob exported in their interactive shell, and
# this assert is what stops a future edit from re-widening the export with
# nothing going red.
#
# BOTH knobs are polluted here, not just the subject one.  REIFY_NEXTEST_TEST_THREADS
# REPLACES the derived `tt` wholesale (gen-nextest-config.sh:315), so an inherited
# value would silently move the pool off the 8 that 6b and 6c assert exact
# integers against — a quieter failure than 6c's, and one that would make those
# asserts wrong rather than red.
#
# This is a tripwire, not padding: without it the hermeticity fix in `_gen` can be
# undone by a future edit with nothing going red.
_check_6e() {
    local ok
    # Subshell: the deliberate pollution must not outlive this assert.
    ok=$(
        export REIFY_NEXTEST_CLI_TEST_THREADS=64
        export REIFY_NEXTEST_TEST_THREADS=1
        _gen ""
        _r=0
        if [ "$_GEN_RC" -eq 0 ] && [ -f "${_GEN_OUT:-}" ] && [ ! -s "$_GEN_ERR" ]; then _r=1; fi
        _gen_cleanup
        printf '%s' "$_r"
    )
    [ "$ok" = "1" ]
}

echo ""
echo "--- Test 6: a forwarded CLI --test-threads must not silently defeat the generated pool ---"

assert "6a: REIFY_NEXTEST_CLI_TEST_THREADS unset -> exit 0, path on stdout, and stderr EMPTY (default byte-identical to today; an earlier grep-for-the-identifier form could never fail)" \
    _check_6a

assert "6b: REIFY_NEXTEST_CLI_TEST_THREADS=4 (<= derived pool 8) -> exit 0, path still printed, stderr names BOTH 4 and 8 (capping below is sanctioned, but visible)" \
    _check_6b

assert "6c: REIFY_NEXTEST_CLI_TEST_THREADS=64 (> derived pool 8) -> exit EXACTLY 64 (the usage-error code verify.sh propagates), NO path on stdout, stderr names BOTH 64 and 8 (a CLI value may not RAISE the pool above the derivation)" \
    _check_6c

assert "6d: every non-comment gen-nextest-config.sh invocation in verify.sh carries BOTH the per-invocation REIFY_NEXTEST_CLI_TEST_THREADS=\$TEST_THREADS prefix and the _die_nextest_config handler, and the handler is defined exactly once" \
    _check_6d

assert "6e: an INHERITED REIFY_NEXTEST_CLI_TEST_THREADS/REIFY_NEXTEST_TEST_THREADS must not reach the generator — the unset path stays rc 0, path on stdout, empty stderr even when both knobs are exported (this suite runs as a child of verify.sh, and an earlier draft of the wiring exported the first process-globally)" \
    _check_6e


test_summary
