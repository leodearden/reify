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
# Stub `cargo` for tests/infra/test_verify_confirm_failed.sh Section D only.
# Plants a chosen JUnit report where nextest would have written one, then
# exits a chosen code. Never invokes the real cargo.
set -u
mkdir -p "$(dirname "$REIFY_TEST_STUB_JUNIT_DEST")"
if [ -n "${REIFY_TEST_STUB_JUNIT_SRC:-}" ]; then
    cp "$REIFY_TEST_STUB_JUNIT_SRC" "$REIFY_TEST_STUB_JUNIT_DEST"
fi
exit "${REIFY_TEST_STUB_RC:-0}"
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

test_summary
