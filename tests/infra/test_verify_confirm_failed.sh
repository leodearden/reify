#!/usr/bin/env bash
# tests/infra/test_verify_confirm_failed.sh — boundary test for task 7423,
# implementing §9.1 (B1-B11) of docs/prds/verify-confirm-failed-self-discovery.md.
#
# WHAT IS UNDER TEST — the offline lane's failure-confirmation seam:
#   RECORDING run  `run-offline-deep.sh --test-threads=N`
#                  → offline-role nextest with --no-fail-fast + JUnit capture
#                  → a profile-qualified confirm MANIFEST of failing bare test IDs.
#   CONFIRM run    `run-offline-deep.sh --test-threads=N --confirm-failed`
#                  → re-runs exactly that manifest's subset
#                  → prints ONLY the still-failing bare IDs, one per line.
#
# WHY THE ASSERTIONS LOOK THE WAY THEY DO. dark-factory's already-landed
# consumer spawns the confirm run with stdout=PIPE, stderr=STDOUT — the two
# streams are MERGED — and treats every non-blank line of the result as one
# confirmed-still-failing test ID. So the merged capture IS the wire contract,
# and every confirm-path assertion here captures `2>&1` into ONE buffer for
# exactly that reason. A stray diagnostic on either stream is not cosmetic: it
# becomes a bogus "test ID" that DF fingerprints and files a fix task against.
# B9 is the standing regression guard for that invariant.
#
# Every assertion EXECUTES a real entry point (verify.sh, run-offline-deep.sh,
# gen-nextest-config.sh, confirm-failed-manifest.sh). None asserts on source
# text — a grep for a flag in a script proves nothing about the command the
# plan actually emits.
#
# Mirrors tests/infra/test_run_offline_deep.sh for the wrapper/plan idioms and
# tests/infra/test_verify_retry_failed_only.sh for the retry-pipeline ones.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

VERIFY_SH="$REPO_ROOT/scripts/verify.sh"
GEN_NEXTEST_CONFIG="$REPO_ROOT/scripts/gen-nextest-config.sh"

# Run-private scratch root; removed on EXIT. Holds the generated nextest
# configs and the JUnit fixtures, so nothing here touches the lane's target/.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/reify-confirm-failed-test.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

echo "=== verify.sh --confirm-failed boundary tests (task 7423, PRD §9.1) ==="

# ---------------------------------------------------------------------------
# Shared helper: capture one role's `--print-plan` output.
#
# Stderr is discarded so the capture is the pure plan; rc is taken via
# `|| rc=$?` so a RED-phase non-zero exit reports as a clean assertion FAIL
# rather than tripping this suite's own `set -e` inside a command
# substitution.
# ---------------------------------------------------------------------------
plan_for_role() {
    local _role="$1" _rc=0
    DF_VERIFY_ROLE="$_role" bash "$VERIFY_SH" test --print-plan 2>/dev/null || _rc=$?
    return "$_rc"
}

PLAN_TASK="$(plan_for_role task || true)"
PLAN_MERGE="$(plan_for_role merge || true)"
PLAN_OFFLINE="$(plan_for_role offline || true)"
PLAN_BACKGROUND="$(plan_for_role background || true)"

# ===========================================================================
# Section A (leaf α) — offline-role `--no-fail-fast` + unconditional JUnit
# capture in the GENERATED nextest config.
#
# `--no-fail-fast` is the precondition for manifest COMPLETENESS (PRD §5.3):
# without it a recording pass with an early failure never runs the later
# tests at all — they are not even attributed as skipped — so the recorded
# failed-set silently under-captures. It is scoped to the offline role so
# task/merge/background keep their deliberate fail-fast posture.
# ===========================================================================
echo ""
echo "--- Section A (leaf α): offline --no-fail-fast + unconditional JUnit capture ---"

assert "A1: role=offline plan emits --no-fail-fast on its cargo-nextest line" \
    bash -c 'printf "%s\n" "$1" | grep -E "(^| )cargo nextest run " | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_OFFLINE"

# A2-A4: the negative half of the role scoping. Asserted per role rather than
# as one combined check so a regression names the role that leaked.
assert "A2: role=task plan does NOT contain --no-fail-fast (fail-fast posture preserved)" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_TASK"

assert "A3: role=merge plan does NOT contain --no-fail-fast" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_MERGE"

assert "A4: role=background plan does NOT contain --no-fail-fast" \
    bash -c '! printf "%s\n" "$1" | grep -q -- "--no-fail-fast"' \
    _ "$PLAN_BACKGROUND"

# A5-A6: JUnit capture is UNCONDITIONAL — it lives in the GENERATED per-run
# config copy, not in the tracked .config/nextest.toml, so a bare developer
# `cargo nextest` is unaffected while every verify.sh-driven pass records one.
# Executed for real (the generator is run, not grepped) under two roles, to
# pin that it does not become role-gated by accident.
gen_config_for_role() {
    local _role="$1" _out
    _out="$WORK/nextest-$_role.toml"
    local _path
    _path="$(DF_VERIFY_ROLE="$_role" bash "$GEN_NEXTEST_CONFIG")" || return 1
    [ -f "$_path" ] || return 1
    cp "$_path" "$_out"
    rm -f "$_path"
    printf '%s\n' "$_out"
}

CFG_TASK="$(gen_config_for_role task || true)"
CFG_OFFLINE="$(gen_config_for_role offline || true)"

assert "A5: gen-nextest-config.sh (role=task) emits a [profile.default.junit] table with a path= value" \
    bash -c '[ -n "$1" ] && [ -f "$1" ] && grep -q "^\[profile\.default\.junit\]$" "$1" && sed -n "/^\[profile\.default\.junit\]$/,/^\[/p" "$1" | grep -qE "^path = \".+\"$"' \
    _ "$CFG_TASK"

assert "A6: gen-nextest-config.sh (role=offline) emits the same [profile.default.junit] table (capture is unconditional, not role-gated)" \
    bash -c '[ -n "$1" ] && [ -f "$1" ] && grep -q "^\[profile\.default\.junit\]$" "$1" && sed -n "/^\[profile\.default\.junit\]$/,/^\[/p" "$1" | grep -qE "^path = \".+\"$"' \
    _ "$CFG_OFFLINE"

# A7: the generator's `test-threads` sed is line-anchored but NOT
# section-anchored (its own header warning; pinned by test_occt_gated_scope.sh
# Test 17k). Adding a SECOND named profile would put a second `test-threads`
# line in reach of that one sed and silently clobber it. Only a sub-table of
# the existing profile.default may be appended — assert exactly that.
assert "A7: the generated config declares NO profile other than 'default' (a second named profile would be clobbered by the line-anchored test-threads sed)" \
    bash -c 'set -e; [ -n "$1" ]; [ -f "$1" ]; [ "$(grep -oE "^\[+profile\.[a-zA-Z0-9_-]+" "$1" | sed -E "s/^\[+profile\.//" | sort -u)" = "default" ]' \
    _ "$CFG_OFFLINE"

# ===========================================================================
# Section B (B10) — byte-identical when inactive.
#
# The confirm machinery is a side channel: with `--confirm-failed` unset, the
# non-offline roles' plans must carry no trace of it. The offline plan DOES
# change (it gains --no-fail-fast, and later the fused manifest write) — that
# is the intended, deliberate change to the offline golden, so B10 is stated
# per role rather than globally.
# ===========================================================================
echo ""
echo "--- Section B (B10): confirm machinery is invisible in every non-offline plan ---"

for _role_plan in "task:$PLAN_TASK" "merge:$PLAN_MERGE" "background:$PLAN_BACKGROUND"; do
    _role="${_role_plan%%:*}"
    _plan="${_role_plan#*:}"
    assert "B10/$_role: plan contains no 'confirm' token (the confirm path emits no plan line for this role)" \
        bash -c '! printf "%s\n" "$1" | grep -qi "confirm"' \
        _ "$_plan"
done

# ===========================================================================
# Section C (leaf β) — the shared JUnit reader, scripts/confirm-failed-manifest.sh.
#
# ONE implementation of "derive the failed set from a nextest JUnit report",
# used by BOTH the recording pass's inline manifest write and the confirm
# run's own derivation (PRD §4.2 step 5 requires one helper, not two).
#
# THE FIXTURES BELOW ARE THE MEASURED 0.9.136 SHAPE, not the PRD's. Probed
# live on 2026-09-18 against cargo-nextest 0.9.136 (1d5bf1ec9), two findings
# that the reader must survive and that these fixtures therefore encode:
#
#   1. A PASSING <testcase> is NOT self-closing — nextest writes an
#      open/close pair with whitespace between. PRD §3 says "passing = empty
#      <testcase/>", which is wrong for this version. So "failed" must be
#      decided by the presence of a <failure>/<error> CHILD, never by
#      self-closing-ness. Fixture (a) carries BOTH spellings of a pass so a
#      reader that got this backwards cannot pass by luck.
#   2. Each failing <testcase> carries <system-out>/<system-err> children
#      whose text repeats the bare test name on its own indented line. A
#      line-oriented grep over the XML therefore yields garbage; the reader
#      must actually parse the document. Fixture (a) reproduces that text
#      verbatim, including the `failures:` block, so a grep-based reader
#      over-reports and fails C1.
#
# testcase/@name is the BARE test id, never package-prefixed — it is exactly
# the string nextest's `test(=<id>)` exact-match filterset takes, so the
# reader applies zero transformation.
# ===========================================================================
echo ""
echo "--- Section C (leaf β): scripts/confirm-failed-manifest.sh JUnit reader ---"

CONFIRM_MANIFEST_SH="$REPO_ROOT/scripts/confirm-failed-manifest.sh"
FIX="$WORK/fixtures"
mkdir -p "$FIX"

# (a) 3 testcases: one pass in each of the two spellings, two failures whose
#     <failure> bodies and <system-out>/<system-err> text repeat the names.
cat > "$FIX/a-two-failures.xml" <<'XML_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="4" failures="2" errors="0" uuid="00000000-0000-0000-0000-000000000001" timestamp="2026-09-18T06:40:07.301+01:00" time="0.022">
    <testsuite name="junitprobe" tests="4" disabled="0" errors="0" failures="2">
        <testcase name="tests::probe_pass_one" classname="junitprobe" timestamp="2026-09-18T06:40:07.302+01:00" time="0.011">
        </testcase>
        <testcase name="tests::probe_pass_selfclosed" classname="junitprobe" timestamp="2026-09-18T06:40:07.302+01:00" time="0.010"/>
        <testcase name="tests::probe_fail_three" classname="junitprobe" timestamp="2026-09-18T06:40:07.302+01:00" time="0.011">
            <failure message="thread &apos;tests::probe_fail_three&apos; (3375278) panicked at src/lib.rs:8:29" type="test failure with exit code 101">thread &apos;tests::probe_fail_three&apos; (3375278) panicked at src/lib.rs:8:29:
assertion `left == right` failed
  left: 1
 right: 2</failure>
            <system-out>
running 1 test
test tests::probe_fail_three ... FAILED

failures:

failures:
    tests::probe_fail_three

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s

</system-out>
            <system-err>
thread &apos;tests::probe_fail_three&apos; (3375278) panicked at src/lib.rs:8:29:
assertion `left == right` failed
</system-err>
        </testcase>
        <testcase name="tests::probe_fail_two" classname="junitprobe" timestamp="2026-09-18T06:40:07.302+01:00" time="0.021">
            <failure message="thread &apos;tests::probe_fail_two&apos; (3375294) panicked at src/lib.rs:6:27" type="test failure with exit code 101">deliberate probe failure</failure>
            <system-out>
running 1 test
test tests::probe_fail_two ... FAILED

failures:
    tests::probe_fail_two

</system-out>
        </testcase>
    </testsuite>
</testsuites>
XML_EOF

# (b) the same shape, but the failing case carries <error> rather than
#     <failure> (nextest's spelling for a non-panic abnormal exit).
cat > "$FIX/b-error-child.xml" <<'XML_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="2" failures="0" errors="1" uuid="00000000-0000-0000-0000-000000000002" timestamp="2026-09-18T06:40:07.301+01:00" time="0.022">
    <testsuite name="junitprobe" tests="2" disabled="0" errors="1" failures="0">
        <testcase name="tests::probe_pass_one" classname="junitprobe" time="0.011">
        </testcase>
        <testcase name="tests::probe_abort_four" classname="junitprobe" time="0.011">
            <error message="Test aborted" type="test abort with signal 6">SIGABRT</error>
        </testcase>
    </testsuite>
</testsuites>
XML_EOF

# (c) malformed / truncated XML — the writer was killed mid-report.
cat > "$FIX/c-malformed.xml" <<'XML_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="2" errors="0">
    <testsuite name="junitprobe" tests="3">
        <testcase name="tests::probe_fail_two" classname="junitprobe">
            <failure message="panicked">thread panic
XML_EOF

# (d) an empty file.
: > "$FIX/d-empty.xml"

# (e) the SAME bare name appearing in two testsuites — a genuine nextest
#     shape (one testsuite per binary) and the reason the reader dedupes.
cat > "$FIX/e-duplicate-names.xml" <<'XML_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="3" errors="0">
    <testsuite name="bin_one" tests="2" failures="2">
        <testcase name="tests::shared_name" classname="bin_one">
            <failure message="panicked">boom</failure>
        </testcase>
        <testcase name="tests::only_in_one" classname="bin_one">
            <failure message="panicked">boom</failure>
        </testcase>
    </testsuite>
    <testsuite name="bin_two" tests="1" failures="1">
        <testcase name="tests::shared_name" classname="bin_two">
            <failure message="panicked">boom</failure>
        </testcase>
    </testsuite>
</testsuites>
XML_EOF

# (f) a clean report — every testcase passed.
cat > "$FIX/f-all-pass.xml" <<'XML_EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="2" failures="0" errors="0">
    <testsuite name="junitprobe" tests="2" failures="0">
        <testcase name="tests::probe_pass_one" classname="junitprobe">
        </testcase>
        <testcase name="tests::probe_pass_two" classname="junitprobe"/>
    </testsuite>
</testsuites>
XML_EOF

# extract_ids <fixture> -> writes stdout to $EX_OUT, stderr to $EX_ERR, rc to $EX_RC.
# rc is taken via `|| EX_RC=$?` so the RED phase (a missing script exits 127)
# reports as clean assertion FAILs rather than tripping this suite's set -e.
extract_ids() {
    EX_RC=0
    bash "$CONFIRM_MANIFEST_SH" extract "$1" >"$WORK/ex.out" 2>"$WORK/ex.err" || EX_RC=$?
    EX_OUT="$(cat "$WORK/ex.out")"
    EX_ERR="$(cat "$WORK/ex.err")"
}

extract_ids "$FIX/a-two-failures.xml"
assert "C1: fixture (a): extract prints EXACTLY the 2 failing bare IDs, one per line, and nothing else" \
    bash -c '[ "$1" = "tests::probe_fail_three
tests::probe_fail_two" ]' \
    _ "$EX_OUT"

assert "C1b: fixture (a): extract exits 0 on a well-formed report" \
    test "$EX_RC" -eq 0

assert "C1c: fixture (a): neither passing testcase is emitted (open/close pair AND self-closed spellings)" \
    bash -c '! printf "%s\n" "$1" | grep -q "probe_pass"' \
    _ "$EX_OUT"

extract_ids "$FIX/b-error-child.xml"
assert "C2: fixture (b): an <error> child counts as failed identically to <failure>" \
    bash -c '[ "$1" = "tests::probe_abort_four" ]' \
    _ "$EX_OUT"

extract_ids "$FIX/e-duplicate-names.xml"
assert "C3: fixture (e): a bare name appearing in two testsuites is emitted ONCE (deduplicated)" \
    bash -c '[ "$1" = "tests::only_in_one
tests::shared_name" ]' \
    _ "$EX_OUT"

extract_ids "$FIX/f-all-pass.xml"
assert "C4: fixture (f): an all-passing report prints NOTHING and exits 0" \
    bash -c '[ -z "$1" ] && [ "$2" -eq 0 ]' \
    _ "$EX_OUT" "$EX_RC"

extract_ids "$FIX/c-malformed.xml"
assert "C5: fixture (c): malformed XML prints NOTHING on stdout" \
    bash -c '[ -z "$1" ]' \
    _ "$EX_OUT"

assert "C5b: fixture (c): malformed XML signals the problem via exit status only (non-zero)" \
    bash -c '[ "$1" -ne 0 ]' \
    _ "$EX_RC"

extract_ids "$FIX/d-empty.xml"
assert "C6: fixture (d): an empty file prints NOTHING on stdout" \
    bash -c '[ -z "$1" ]' \
    _ "$EX_OUT"

assert "C6b: fixture (d): an empty file signals the problem via exit status only (non-zero)" \
    bash -c '[ "$1" -ne 0 ]' \
    _ "$EX_RC"

extract_ids "$FIX/does-not-exist.xml"
assert "C7: a missing path prints NOTHING on stdout and exits non-zero" \
    bash -c '[ -z "$1" ] && [ "$2" -ne 0 ]' \
    _ "$EX_OUT" "$EX_RC"

# C8: stdout is a WIRE CONTRACT for the confirm caller, so every diagnostic
# must be on stderr. Proven on the one path that has a diagnostic to emit.
extract_ids "$FIX/c-malformed.xml"
assert "C8: diagnostics go to stderr, never stdout (malformed-XML case says something on stderr)" \
    bash -c '[ -n "$1" ]' \
    _ "$EX_ERR"

# ===========================================================================
# Section D (leaf β) — the recording pass's inline manifest write.
#   B1  a complete recording writes exactly the failing set + a tree-pinned sidecar
#   B1b a zero-failure recording writes an EMPTY manifest, not an absent one
#   B8  an UNCLEAN recording (killed / timed out) writes nothing
#   B11 a FAIL-FAST recording writes nothing, at the same exit code 100
#
# HOW THESE STAY HERMETIC. Running the whole offline plan would build the
# workspace and run npm. Instead each case takes the REAL nextest command
# string verify.sh emits — byte for byte, from `--print-plan`, the faithful
# oracle several suites already pin — and executes THAT with a stub `cargo`
# first on PATH. The stub plants a chosen JUnit report and exits a chosen
# code, so the fused write logic runs for real against controlled inputs.
#
# B11 IS THE HEADLINE. A fail-fast pass that stops after its first failure
# ALSO exits 100, so the exit code alone cannot tell a truncated failed-set
# from a complete one. B11 drives role=task (no --no-fail-fast) with the
# IDENTICAL fixture and the IDENTICAL exit code 100 and asserts nothing is
# written — proving the gate discriminates on fail-fast activity, not on the
# exit code.
# ===========================================================================
echo ""
echo "--- Section D (leaf β): recording-run manifest write-gate (B1/B1b/B8/B11) ---"

STUB_BIN="$WORK/stub-bin"
mkdir -p "$STUB_BIN"
cat > "$STUB_BIN/cargo" <<'STUB_EOF'
#!/usr/bin/env bash
# Stub `cargo` for tests/infra/test_verify_confirm_failed.sh. On a `nextest
# run` it plants a chosen JUnit report where nextest would have written one and
# exits a chosen code; every other invocation succeeds silently. Never invokes
# the real cargo.
#
# THE CATCH-ALL ARM IS LOAD-BEARING, not politeness. verify.sh probes runner
# availability with `cargo nextest --version` and REFUSES to run (rather than
# silently fall back to the -E-less cargo-test plan) when that probe fails, so
# a stub that answered every invocation with the chosen failure code would make
# the confirm path refuse before any subset pass ran.
set -u
case "$*" in
    *"nextest run"*)
        mkdir -p "$(dirname "$REIFY_TEST_STUB_JUNIT_DEST")"
        if [ -n "${REIFY_TEST_STUB_JUNIT_SRC:-}" ]; then
            cp "$REIFY_TEST_STUB_JUNIT_SRC" "$REIFY_TEST_STUB_JUNIT_DEST"
        fi
        exit "${REIFY_TEST_STUB_RC:-0}"
        ;;
esac
exit 0
STUB_EOF
chmod +x "$STUB_BIN/cargo"

CONFIRM_MANIFEST="$WORK/confirm-manifest-release.txt"
CONFIRM_SIDECAR="$WORK/confirm-sidecar-release.json"
CONFIRM_JUNIT="$WORK/junit/reify-confirm.xml"

# run_recording <role> <stub-rc> <junit-fixture-or-empty> -> sets REC_RC.
#
# Resolves the plan with the confirm paths pointed at $WORK (so nothing writes
# into the lane's target/), lifts out the --workspace nextest command, and
# executes exactly that string with the stub cargo first on PATH.
run_recording() {
    local _role="$1" _stub_rc="$2" _fixture="$3"
    local _plan _cmd
    _plan="$(
        DF_VERIFY_ROLE="$_role" \
        REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$CONFIRM_MANIFEST" \
        REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$CONFIRM_SIDECAR" \
        REIFY_VERIFY_CONFIRM_JUNIT="$CONFIRM_JUNIT" \
        bash "$VERIFY_SH" test --profile release --print-plan 2>/dev/null
    )" || true
    # First plain nextest line (the `if test -f gui/...` guarded gui pass is
    # excluded — it is a different pass, and role=task narrows its release
    # selector to `-p <crate>` rather than `--workspace`, so match on the
    # subcommand rather than on the selector).
    #
    # `--print-plan` emits a PLACEHOLDER config path containing '<' and '>',
    # which is a hermeticity feature of print mode (no temp file is created)
    # but would parse as shell redirections here. Rewrite it to a scratch path;
    # the stub cargo never reads it.
    _cmd="$(printf '%s\n' "$_plan" | grep -E '(^| )cargo nextest run ' | grep -v '^if test ' | head -n1 || true)"
    # `[^ ;]*`, not `[^ ]*`: the placeholder is the LAST token before the `;`
    # that separates the nextest pass from the fused manifest write, and a
    # space-only character class would swallow that separator too.
    _cmd="$(printf '%s\n' "$_cmd" | sed "s#--config-file [^ ;]*#--config-file $WORK/nextest-stub.toml#")"
    if [ -z "$_cmd" ]; then
        REC_RC=127
        return 0
    fi
    rm -f "$CONFIRM_JUNIT"
    REC_RC=0
    (
        cd "$REPO_ROOT"
        PATH="$STUB_BIN:$PATH" \
        REIFY_TEST_STUB_JUNIT_DEST="$CONFIRM_JUNIT" \
        REIFY_TEST_STUB_JUNIT_SRC="$_fixture" \
        REIFY_TEST_STUB_RC="$_stub_rc" \
        bash -c "$_cmd"
    ) >/dev/null 2>&1 || REC_RC=$?
}

# --- B1: a complete recording (rc=100, 2 failures) ---
rm -f "$CONFIRM_MANIFEST" "$CONFIRM_SIDECAR"
run_recording offline 100 "$FIX/a-two-failures.xml"

assert "D1 (B1): a complete offline recording writes a manifest naming exactly the 2 failing bare IDs" \
    bash -c '[ -f "$1" ] && [ "$(cat "$1")" = "tests::probe_fail_three
tests::probe_fail_two" ]' \
    _ "$CONFIRM_MANIFEST"

assert "D2 (B1): the same recording stamps a confirm-owned sidecar whose tree_oid == git rev-parse HEAD:" \
    bash -c '[ -f "$1" ] && [ "$(sed -n "s/.*\"tree_oid\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$1" | head -n1)" = "$2" ]' \
    _ "$CONFIRM_SIDECAR" "$(git -C "$REPO_ROOT" rev-parse HEAD:)"

assert "D3 (B1): the recording pass re-exits with the nextest pass's OWN exit code (100), so a red pass stays red" \
    test "$REC_RC" -eq 100

# --- B1b: a zero-failure recording writes an EMPTY manifest, not an absent one ---
rm -f "$CONFIRM_MANIFEST" "$CONFIRM_SIDECAR"
run_recording offline 0 "$FIX/f-all-pass.xml"

assert "D4 (B1b): a zero-failure recording writes an EMPTY manifest (present, not absent — absent would mean 'never recorded')" \
    bash -c '[ -f "$1" ] && [ ! -s "$1" ]' \
    _ "$CONFIRM_MANIFEST"

assert "D5 (B1b): a green recording still exits 0" \
    test "$REC_RC" -eq 0

# --- B8: an UNCLEAN recording writes nothing and preserves what was there ---
PRIOR_CONTENT='tests::previously_recorded'
for _unclean_rc in 124 137; do
    printf '%s\n' "$PRIOR_CONTENT" > "$CONFIRM_MANIFEST"
    run_recording offline "$_unclean_rc" "$FIX/a-two-failures.xml"

    assert "D6 (B8): an unclean recording (exit $_unclean_rc) leaves the pre-existing manifest byte-unchanged" \
        bash -c '[ "$(cat "$1")" = "$2" ]' \
        _ "$CONFIRM_MANIFEST" "$PRIOR_CONTENT"

    rm -f "$CONFIRM_MANIFEST"
    run_recording offline "$_unclean_rc" "$FIX/a-two-failures.xml"

    assert "D7 (B8): an unclean recording (exit $_unclean_rc) with no prior manifest writes none" \
        bash -c '[ ! -f "$1" ]' \
        _ "$CONFIRM_MANIFEST"
done

# --- B11: the fail-fast write-gate, the headline assertion ---
printf '%s\n' "$PRIOR_CONTENT" > "$CONFIRM_MANIFEST"
run_recording task 100 "$FIX/a-two-failures.xml"

assert "D8 (B11): with --no-fail-fast NOT active, the IDENTICAL fixture at the IDENTICAL exit code 100 leaves the pre-existing manifest byte-unchanged" \
    bash -c '[ "$(cat "$1")" = "$2" ]' \
    _ "$CONFIRM_MANIFEST" "$PRIOR_CONTENT"

rm -f "$CONFIRM_MANIFEST" "$CONFIRM_SIDECAR"
run_recording task 100 "$FIX/a-two-failures.xml"

assert "D9 (B11): with --no-fail-fast NOT active, no manifest is written at all (a truncated failed-set is never recorded as if complete)" \
    bash -c '[ ! -f "$1" ]' \
    _ "$CONFIRM_MANIFEST"

assert "D10 (B11): the fail-fast recording still propagates its own exit code (100) unchanged" \
    test "$REC_RC" -eq 100

# ===========================================================================
# Section E (leaf γ) — the --confirm-failed flag surface.
#
# THIS IS THE LIVE DEFECT. dark-factory's offline lane already calls
# `run-offline-deep.sh --test-threads=1 --confirm-failed` in production; today
# verify.sh's parser falls through to its unknown-argument arm, dumps usage to
# stderr and exits 64, so every confirmation call the lane makes fails.
#
# E1/E4 run the real entry point with a `timeout` guard. The flag's own code
# path must return promptly on a vacuous manifest (nothing recorded → nothing
# to confirm), so a hang here is itself the failure: without the guard a
# regression that let --confirm-failed fall through to a real test run would
# stall this suite for hours instead of reporting.
# ===========================================================================
echo ""
echo "--- Section E (leaf γ): --confirm-failed flag surface ---"

# run_verify <timeout-secs> <args...> -> sets V_RC and V_OUT (streams MERGED,
# exactly as dark-factory captures them).
run_verify() {
    local _timeout="$1"; shift
    V_RC=0
    V_OUT="$(cd "$REPO_ROOT" && timeout "$_timeout" bash "$VERIFY_SH" "$@" 2>&1)" || V_RC=$?
}

# Point the confirm state at an empty scratch dir so these invocations resolve
# a vacuous (absent) manifest and return immediately without running anything.
export REIFY_VERIFY_CONFIRM_MANIFEST_DEBUG="$WORK/absent-debug.txt"
export REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$WORK/absent-release.txt"
export REIFY_VERIFY_CONFIRM_SIDECAR_DEBUG="$WORK/absent-debug.json"
export REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$WORK/absent-release.json"
export REIFY_VERIFY_CONFIRM_JUNIT="$WORK/absent-junit.xml"

run_verify 120 test --confirm-failed
assert "E1: 'verify.sh test --confirm-failed' is ACCEPTED (does not exit 64 — the live lane defect)" \
    bash -c '[ "$1" -ne 64 ]' \
    _ "$V_RC"

assert "E2: 'test --confirm-failed' output contains no 'unknown argument'" \
    bash -c '! printf "%s\n" "$1" | grep -q "unknown argument"' \
    _ "$V_OUT"

assert "E3: 'test --confirm-failed' output does not dump the usage text (no 'Usage:' / 'Options:')" \
    bash -c '! printf "%s\n" "$1" | grep -qE "^(Usage|Options):"' \
    _ "$V_OUT"

assert "E4: 'test --confirm-failed' returned promptly (not killed by the 120s guard — it must not fall through to a real test run)" \
    bash -c '[ "$1" -ne 124 ] && [ "$1" -ne 137 ]' \
    _ "$V_RC"

run_verify 120 all --confirm-failed
assert "E5: 'verify.sh all --confirm-failed' is ACCEPTED (valid for action in {test, all})" \
    bash -c '[ "$1" -ne 64 ]' \
    _ "$V_RC"

# The flag narrows a previously-recorded TEST failure set, so it is meaningless
# for the single-pass lint/typecheck actions. Rejected in the same strict style
# --profile and --scope use for an invalid value.
for _bad_action in lint typecheck; do
    run_verify 120 "$_bad_action" --confirm-failed
    assert "E6/$_bad_action: 'verify.sh $_bad_action --confirm-failed' exits 64 (valid only for action in {test, all})" \
        bash -c '[ "$1" -eq 64 ]' \
        _ "$V_RC"
done

run_verify 60 --help
assert "E7: --help documents --confirm-failed" \
    bash -c 'printf "%s\n" "$1" | grep -q -- "--confirm-failed"' \
    _ "$V_OUT"

# E8 catches a specific, silent regression: usage() is `sed -n '<start>,<end>p'`
# over this script's own header, so INSERTING header lines without widening the
# range truncates the usage text from the bottom — the new flag would be
# documented while an existing tail line silently disappeared. The frozen
# string below is the header's last usage line as of task 7423; if a future
# header edit is meant to move the window's end, update this constant
# deliberately rather than letting the truncation pass unnoticed.
USAGE_LAST_LINE='    (else cargo uses its own per-process job pool). Role→FIFO selection:'
assert "E8: the usage window is not truncated — its frozen last line is still the LAST line of --help" \
    bash -c '[ "$(printf "%s\n" "$1" | tail -n1)" = "$2" ]' \
    _ "$V_OUT" "$USAGE_LAST_LINE"

# E9 resolves PRD §11's open question. --confirm-failed self-drives the
# REIFY_VERIFY_RETRY_* pipeline, so an externally-set REIFY_VERIFY_RETRY_SCOPE
# is two callers driving one consumption pipeline with different subsets.
# Refuse loudly rather than invent a silent precedence between them.
V_RC=0
V_OUT="$(cd "$REPO_ROOT" && REIFY_VERIFY_RETRY_SCOPE=failed_only timeout 120 bash "$VERIFY_SH" test --confirm-failed 2>&1)" || V_RC=$?

assert "E9: --confirm-failed together with an externally-set REIFY_VERIFY_RETRY_SCOPE exits 64 (ambiguous double-drive, refused loudly)" \
    bash -c '[ "$1" -eq 64 ]' \
    _ "$V_RC"

assert "E9b: that refusal says so on a 'verify.sh: ERROR' line naming the conflicting variable" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^verify\.sh: ERROR\b.*REIFY_VERIFY_RETRY_SCOPE"' \
    _ "$V_OUT"

# ===========================================================================
# Section F (leaf γ) — THE OUTPUT CONTRACT (B2/B3/B4/B5/B6 + B9).
#
# Every capture here MERGES stdout and stderr into one buffer, because that is
# exactly what dark-factory's consumer does (stdout=PIPE, stderr=STDOUT) and
# the merged stream IS the contract. Asserting on stdout alone would pass while
# a stderr diagnostic quietly corrupted the caller's parse.
#
# B9 is the standing guard and the highest-value assertion in the suite: every
# captured line must be either a plausible bare test id or the one sanctioned
# `verify.sh: ERROR` banner. Anything else — an executor echo, a nextest
# progress line, an ionice WARNING, a `retry refused:` diagnostic, the closing
# `all checks passed` tail — is a line dark-factory would file a fix task
# against.
# ===========================================================================
echo ""
echo "--- Section F (leaf γ): the confirm run's output contract (B2-B6, B9) ---"

CONFIRM_TREE_OID="$(git -C "$REPO_ROOT" rev-parse HEAD:)"

# write_confirm_state <manifest-body-or-empty> <sidecar-tree-oid-or-empty>
# An empty sidecar OID means "write no sidecar at all".
write_confirm_state() {
    local _body="$1" _oid="$2"
    if [ -n "$_body" ]; then
        printf '%s\n' "$_body" > "$CONFIRM_MANIFEST"
    else
        : > "$CONFIRM_MANIFEST"
    fi
    if [ -n "$_oid" ]; then
        printf '{"tree_oid":"%s","profiles":"release","timestamp":"2026-09-18T00:00:00Z"}\n' "$_oid" > "$CONFIRM_SIDECAR"
    else
        rm -f "$CONFIRM_SIDECAR"
    fi
}

# run_confirm <junit-fixture-the-subset-run-will-produce> -> CF_RC, CF_OUT.
# The stub cargo plants that fixture where the confirm run reads its report,
# so "which of the recorded tests still fail" is under the test's control.
run_confirm() {
    local _fixture="$1"
    rm -f "$CONFIRM_JUNIT"
    CF_RC=0
    CF_OUT="$(
        cd "$REPO_ROOT" && \
        PATH="$STUB_BIN:$PATH" \
        REIFY_TEST_STUB_JUNIT_DEST="$CONFIRM_JUNIT" \
        REIFY_TEST_STUB_JUNIT_SRC="$_fixture" \
        REIFY_TEST_STUB_RC="${2:-100}" \
        REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$CONFIRM_MANIFEST" \
        REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$CONFIRM_SIDECAR" \
        REIFY_VERIFY_CONFIRM_JUNIT="$CONFIRM_JUNIT" \
        REIFY_VERIFY_CONFIRM_LOG="$WORK/confirm.log" \
        timeout 300 bash "$VERIFY_SH" test --profile release --confirm-failed 2>&1
    )" || CF_RC=$?
}

# assert_output_purity <label> — B9, applied to whatever CF_OUT currently holds.
# A bare nextest test id is a `::`-separated Rust path (that is what
# testcase/@name is, and what `test(=<id>)` takes). The alternative is the one
# ERROR banner dark-factory's own guard already rejects to [].
assert_output_purity() {
    assert "B9/$1: every captured line is a bare test id or a 'verify.sh: ERROR' banner (no executor echo, nextest chatter, WARNING or 'all checks passed' tail)" \
        bash -c '
            printf "%s\n" "$1" | while IFS= read -r _line; do
                [ -z "$_line" ] && continue
                case "$_line" in
                    "verify.sh: ERROR"*) continue ;;
                esac
                printf "%s" "$_line" | grep -qE "^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z0-9_]+)+$" || { printf "IMPURE: %s\n" "$_line"; exit 1; }
            done' \
        _ "$CF_OUT"
}

# --- B5: no manifest at all ---
rm -f "$CONFIRM_MANIFEST" "$CONFIRM_SIDECAR"
run_confirm "$FIX/a-two-failures.xml"
assert "F1 (B5): an ABSENT manifest yields ZERO bytes of merged output" \
    bash -c '[ -z "$(printf "%s" "$1" | tr -d "[:space:]")" ]' \
    _ "$CF_OUT"
assert "F2 (B5): an ABSENT manifest exits 0" \
    test "$CF_RC" -eq 0
assert_output_purity "B5"

# --- B6: an empty manifest (a recording that found zero failures) ---
write_confirm_state "" "$CONFIRM_TREE_OID"
run_confirm "$FIX/a-two-failures.xml"
assert "F3 (B6): an EMPTY manifest yields ZERO bytes of merged output (same observable as B5, deliberately)" \
    bash -c '[ -z "$(printf "%s" "$1" | tr -d "[:space:]")" ]' \
    _ "$CF_OUT"
assert "F4 (B6): an EMPTY manifest exits 0" \
    test "$CF_RC" -eq 0
assert_output_purity "B6"

# --- B2: 2 recorded, both still fail ---
write_confirm_state "tests::probe_fail_three
tests::probe_fail_two" "$CONFIRM_TREE_OID"
run_confirm "$FIX/a-two-failures.xml"
assert "F5 (B2): 2 recorded and both still failing yields EXACTLY those 2 bare names, one per line, and nothing else" \
    bash -c '[ "$1" = "tests::probe_fail_three
tests::probe_fail_two" ]' \
    _ "$CF_OUT"
assert "F6 (B2): a non-empty confirmed set exits 100" \
    test "$CF_RC" -eq 100
assert_output_purity "B2"

# --- B3: 2 recorded, 1 now passes ---
write_confirm_state "tests::probe_fail_three
tests::probe_fail_two" "$CONFIRM_TREE_OID"
run_confirm "$FIX/b-error-child.xml"
assert "F7 (B3): a partial reproduction yields EXACTLY the 1 still-failing bare name" \
    bash -c '[ "$1" = "tests::probe_abort_four" ]' \
    _ "$CF_OUT"
assert_output_purity "B3"

# --- B4: 2 recorded, both now pass ---
write_confirm_state "tests::probe_fail_three
tests::probe_fail_two" "$CONFIRM_TREE_OID"
run_confirm "$FIX/f-all-pass.xml" 0
assert "F8 (B4): a full reproduction-clear yields ZERO bytes of merged output" \
    bash -c '[ -z "$(printf "%s" "$1" | tr -d "[:space:]")" ]' \
    _ "$CF_OUT"
assert "F9 (B4): a confirmed-clean run exits 0" \
    test "$CF_RC" -eq 0
assert_output_purity "B4"

# --- The no-false-clean guard: the subset run produced NO report at all ---
# (an earlier plan pole failed, the run was killed, nextest never started).
# "Nothing printed" is the wire encoding of "confirmed clean", so this case
# must NOT take it.
write_confirm_state "tests::probe_fail_three
tests::probe_fail_two" "$CONFIRM_TREE_OID"
CF_RC=0
CF_OUT="$(
    cd "$REPO_ROOT" && \
    PATH="$STUB_BIN:$PATH" \
    REIFY_TEST_STUB_JUNIT_DEST="$CONFIRM_JUNIT" \
    REIFY_TEST_STUB_JUNIT_SRC="" \
    REIFY_TEST_STUB_RC=124 \
    REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$CONFIRM_MANIFEST" \
    REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$CONFIRM_SIDECAR" \
    REIFY_VERIFY_CONFIRM_JUNIT="$CONFIRM_JUNIT" \
    REIFY_VERIFY_CONFIRM_LOG="$WORK/confirm.log" \
    timeout 300 bash "$VERIFY_SH" test --profile release --confirm-failed 2>&1
)" || CF_RC=$?

assert "F10: a subset run that produced NO report does NOT report 'confirmed clean' — it refuses with one 'verify.sh: ERROR' line" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^verify\.sh: ERROR\b"' \
    _ "$CF_OUT"
assert "F11: that refusal leaks no test id alongside the ERROR line" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c . )" -eq 1 ]' \
    _ "$CF_OUT"
assert_output_purity "no-report"

# ===========================================================================
# Section G (leaf γ) — B7, the tree-drift refusal.
#
# The recorded failed-set belongs to ONE tree. If HEAD's tree has moved, the
# recorded ids may name tests that no longer exist, or miss ones that now fail;
# re-running them would produce an answer about a tree nobody asked about.
#
# The refusal is spelled with the pre-existing `verify.sh: ERROR` banner rather
# than a novel sentinel exit code for a measured reason: the live consumer never
# inspects proc.returncode, so a new exit code would be invisible to it, while
# this exact banner is already on its reject-to-[] list (task 5308's
# _VERIFY_USAGE_MARKER_RE). Reusing it is what makes this case degrade safely
# with ZERO dark-factory change.
# ===========================================================================
echo ""
echo "--- Section G (leaf γ): B7 tree-drift refusal ---"

DRIFTED_OID='0000000000000000000000000000000000000000'
write_confirm_state "tests::probe_fail_three
tests::probe_fail_two" "$DRIFTED_OID"
run_confirm "$FIX/a-two-failures.xml"

assert "G1 (B7): a drifted sidecar yields EXACTLY ONE line" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c .)" -eq 1 ]' \
    _ "$CF_OUT"

assert "G2 (B7): that line matches ^verify\.sh: ERROR\b — the exact pattern dark-factory's live guard rejects to []" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^verify\.sh: ERROR\b"' \
    _ "$CF_OUT"

assert "G3 (B7): the refusal exits 64" \
    test "$CF_RC" -eq 64

assert "G4 (B7): the line names the sidecar's recorded OID, so an operator can diagnose it" \
    bash -c 'printf "%s\n" "$1" | grep -qF "$2"' \
    _ "$CF_OUT" "$DRIFTED_OID"

assert "G5 (B7): the line also names the CURRENT tree OID (a refusal naming only one side is not diagnosable)" \
    bash -c 'printf "%s\n" "$1" | grep -qF "$2"' \
    _ "$CF_OUT" "$CONFIRM_TREE_OID"

assert "G6 (B7): no test id leaks onto the stream alongside the refusal" \
    bash -c '! printf "%s\n" "$1" | grep -q "probe_fail"' \
    _ "$CF_OUT"

assert_output_purity "B7"

# ===========================================================================
# Section H (leaf ε) — the LANE ENTRY POINT, end to end: recording → confirm.
#
# Sections D-G drive verify.sh directly. This one drives what dark-factory
# actually spawns — `scripts/run-offline-deep.sh` — in its exact two-call argv
# shape, so the wrapper's own contributions are under test: the
# DF_VERIFY_ROLE=offline export, the --test-threads=1 threading, the
# --confirm-failed detection, and the outcome-line suppression that keeps the
# merged stream parseable.
#
#   call 1   run-offline-deep.sh --test-threads=1                   (recording)
#   call 2   run-offline-deep.sh --test-threads=1 --confirm-failed  (confirm)
#
# TWO DELIBERATE BOUNDS, both load-bearing:
#
# (1) `cargo` is the Section D stub for both calls, so the "planted red" is a
#     JUnit report this test controls rather than a real failing test. That
#     bounds an infra-pool member to seconds AND keeps it hermetic: the real
#     heavy set is eight atoms under a 13h offline release budget, and even one
#     real atom would need a full release build of the workspace plus the three
#     native deps. WHAT IS STILL REAL: both entry points, the offline plan the
#     wrapper emits, the fused manifest write, the tree pin, the child subset
#     plan, the shared JUnit reader and the whole output contract.
#
# (2) CALL 1 EXECUTES THE WRAPPER'S OWN EMITTED TEST-REGION COMMAND rather than
#     letting the wrapper run its full plan. Running the whole offline plan from
#     inside an infra test would acquire the HOST-GLOBAL test-run semaphore that
#     the outer verify run already holds — a self-deadlock, not merely a slow
#     test — and would additionally run npm ci, the gui vitest suite, the PSI
#     gate and the compile gate. The lifted line is byte-for-byte the string the
#     plan executor evals, so the recording behaviour under test is identical;
#     only the semaphore bracket around it is dropped. CALL 2 has no such
#     hazard — `--confirm-failed` returns before the executor and never touches
#     the semaphore — so it is executed literally, exactly as DF spawns it.
# ===========================================================================
echo ""
echo "--- Section H (leaf ε): run-offline-deep.sh recording → confirm, end to end ---"

RUN_OFFLINE_DEEP="$REPO_ROOT/scripts/run-offline-deep.sh"

E2E="$WORK/e2e"
mkdir -p "$E2E/junit"
E2E_MANIFEST="$E2E/manifest-release.txt"
E2E_SIDECAR="$E2E/sidecar-release.json"
E2E_JUNIT="$E2E/junit/reify-confirm.xml"
E2E_LOG="$E2E/confirm.log"

E2E_PLANTED='tests::e2e_planted_red'
E2E_GHOST='tests::e2e_stale_ghost'

# The recording run's report: ONE planted failure beside a passing sibling, so
# the manifest's content is an assertion about extraction, not about a fixture
# that happens to contain a single case.
cat > "$E2E/red.xml" <<XML_EOF
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="2" failures="1" errors="0">
    <testsuite name="e2e_probe" tests="2" failures="1">
        <testcase name="tests::e2e_passing_sibling" classname="e2e_probe" time="0.010"/>
        <testcase name="${E2E_PLANTED}" classname="e2e_probe" time="0.011">
            <failure message="planted red" type="test failure with exit code 101">deliberate e2e plant</failure>
        </testcase>
    </testsuite>
</testsuites>
XML_EOF

# The confirm run's report once the red is un-planted: the subset ran and every
# member passed.
cat > "$E2E/green.xml" <<XML_EOF
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="1" failures="0" errors="0">
    <testsuite name="e2e_probe" tests="1" failures="0">
        <testcase name="${E2E_PLANTED}" classname="e2e_probe" time="0.011"/>
    </testsuite>
</testsuites>
XML_EOF

# A STALE report, pre-planted at the read path before each confirm call. It
# names an id that appears in no manifest and in no fresh report, so if it ever
# surfaces on the wire the confirm derived its answer from a leftover file
# instead of from the report its own subset pass produced.
cat > "$E2E/stale.xml" <<XML_EOF
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="1" failures="1" errors="0">
    <testsuite name="e2e_probe" tests="1" failures="1">
        <testcase name="${E2E_GHOST}" classname="e2e_probe" time="0.011">
            <failure message="stale" type="test failure with exit code 101">from a previous run</failure>
        </testcase>
    </testsuite>
</testsuites>
XML_EOF

# lane_recording_run <stub-rc> <junit-fixture> -> sets E2E_REC_RC.
# Takes the plan from the WRAPPER (so the role export and the --test-threads=1
# threading are the wrapper's, not this test's) and executes its test-region
# nextest command. See bound (2) above for why the region is lifted.
lane_recording_run() {
    local _stub_rc="$1" _fixture="$2"
    local _plan _cmd
    _plan="$(
        REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$E2E_MANIFEST" \
        REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$E2E_SIDECAR" \
        REIFY_VERIFY_CONFIRM_JUNIT="$E2E_JUNIT" \
        bash "$RUN_OFFLINE_DEEP" --test-threads=1 --print-plan 2>/dev/null
    )" || true
    _cmd="$(printf '%s\n' "$_plan" | grep -E '(^| )cargo nextest run ' | grep -v '^if test ' | head -n1 || true)"
    _cmd="$(printf '%s\n' "$_cmd" | sed "s#--config-file [^ ;]*#--config-file $WORK/nextest-stub.toml#")"
    if [ -z "$_cmd" ]; then
        E2E_REC_RC=127
        return 0
    fi
    rm -f "$E2E_JUNIT"
    E2E_REC_RC=0
    (
        cd "$REPO_ROOT"
        PATH="$STUB_BIN:$PATH" \
        REIFY_TEST_STUB_JUNIT_DEST="$E2E_JUNIT" \
        REIFY_TEST_STUB_JUNIT_SRC="$_fixture" \
        REIFY_TEST_STUB_RC="$_stub_rc" \
        bash -c "$_cmd"
    ) >/dev/null 2>&1 || E2E_REC_RC=$?
}

# lane_confirm_run <stub-rc> <junit-fixture> -> sets E2E_CF_RC, E2E_CF_OUT.
# The literal DF spawn: the real wrapper, the real argv, stdout and stderr
# MERGED into one buffer because that is the wire.
lane_confirm_run() {
    local _stub_rc="$1" _fixture="$2"
    cp "$E2E/stale.xml" "$E2E_JUNIT"
    E2E_CF_RC=0
    E2E_CF_OUT="$(
        cd "$REPO_ROOT" && \
        PATH="$STUB_BIN:$PATH" \
        REIFY_TEST_STUB_JUNIT_DEST="$E2E_JUNIT" \
        REIFY_TEST_STUB_JUNIT_SRC="$_fixture" \
        REIFY_TEST_STUB_RC="$_stub_rc" \
        REIFY_VERIFY_CONFIRM_MANIFEST_RELEASE="$E2E_MANIFEST" \
        REIFY_VERIFY_CONFIRM_SIDECAR_RELEASE="$E2E_SIDECAR" \
        REIFY_VERIFY_CONFIRM_JUNIT="$E2E_JUNIT" \
        REIFY_VERIFY_CONFIRM_LOG="$E2E_LOG" \
        timeout 300 bash "$RUN_OFFLINE_DEEP" --test-threads=1 --confirm-failed 2>&1
    )" || E2E_CF_RC=$?
}

# --- the red is planted: record it, then confirm it ---
rm -f "$E2E_MANIFEST" "$E2E_SIDECAR"
lane_recording_run 100 "$E2E/red.xml"

assert "H1 (ε): the lane's RECORDING call wrote a manifest naming exactly the planted red" \
    bash -c '[ -f "$1" ] && [ "$(cat "$1")" = "$2" ]' \
    _ "$E2E_MANIFEST" "$E2E_PLANTED"

assert "H2 (ε): the recording stamped a confirm-owned sidecar pinned to the CURRENT tree" \
    bash -c '[ -f "$1" ] && [ "$(sed -n "s/.*\"tree_oid\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$1" | head -n1)" = "$2" ]' \
    _ "$E2E_SIDECAR" "$CONFIRM_TREE_OID"

assert "H3 (ε): the recording call still reports its own red (exit 100) — recording never masks the failure it records" \
    test "$E2E_REC_RC" -eq 100

lane_confirm_run 100 "$E2E/red.xml"

assert "H4 (ε): the lane's CONFIRM call emits EXACTLY the planted red's bare name and nothing else, on the merged stream" \
    bash -c '[ "$1" = "$2" ]' \
    _ "$E2E_CF_OUT" "$E2E_PLANTED"

assert "H5 (ε): a confirmed-still-failing lane run exits 100" \
    test "$E2E_CF_RC" -eq 100

assert "H6 (ε): the confirm derived from its OWN fresh report — the stale report pre-planted at the read path never surfaces" \
    bash -c '! printf "%s\n" "$1" | grep -qF "$2"' \
    _ "$E2E_CF_OUT" "$E2E_GHOST"

assert "H7 (ε): the wrapper's own '==> offline deep-test lane' outcome line is absent from the merged capture (it would parse as a test id)" \
    bash -c '! printf "%s\n" "$1" | grep -qF "offline deep-test lane"' \
    _ "$E2E_CF_OUT"

CF_OUT="$E2E_CF_OUT"
assert_output_purity "ε-confirmed"

# --- un-plant the failure: the SAME manifest, a now-green subset ---
lane_confirm_run 0 "$E2E/green.xml"

assert "H8 (ε): with the red un-planted, the confirm call's merged capture is EMPTY — the wire encoding of 'nothing still fails'" \
    bash -c '[ -z "$(printf "%s" "$1" | tr -d "[:space:]")" ]' \
    _ "$E2E_CF_OUT"

assert "H9 (ε): a confirmed-clean lane run exits 0" \
    test "$E2E_CF_RC" -eq 0

assert "H10 (ε): the manifest is UNCHANGED by confirming — the confirm's own narrowed view never overwrites the recording" \
    bash -c '[ "$(cat "$1")" = "$2" ]' \
    _ "$E2E_MANIFEST" "$E2E_PLANTED"

CF_OUT="$E2E_CF_OUT"
assert_output_purity "ε-clean"

test_summary
