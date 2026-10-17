#!/usr/bin/env bash
# tests/infra/test_plan_capture_lib.sh — unit tests for tests/infra/plan_capture_lib.sh
#
# Validates fork-free plan capture/match helpers introduced for task #4708:
# hardening test_verify_scope.sh B9-default against nondeterministic --print-plan
# output under concurrent load (esc-4574-42 class: pipe-to-grep EINTR).
#
# Covers:
#   plan_match        — fork-free ERE matcher ([[ =~ ]])
#   plan_capture_complete — completeness check via structural markers
#   plan_narrow_active    — extract NARROW_ACTIVE value from dump
#   capture_print_plan    — retry-on-incomplete-capture wrapper
#   plan_count_noncomment_lines — fork-free non-comment line counter
#   plan_is_narrowing_axis_line — narrowing-axis line classification (#6391)
#   plan_narrowing_axis_match / plan_offaxis_match / plan_narrowing_axis_count
#                         — dump-level axis predicates (#6391)
#   plan_strip_comments   — fork-free comment stripper (#6247)
#   plan_header_field     — typed, order-independent plan header field reader

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

[ -f "$SCRIPT_DIR/plan_capture_lib.sh" ] || { echo "ERROR: plan_capture_lib.sh not found at $SCRIPT_DIR/plan_capture_lib.sh"; exit 1; }
source "$SCRIPT_DIR/plan_capture_lib.sh"

# Negative assertion helper (assert() only checks for success rc).
refute() { ! "$@"; }

echo "=== plan_capture_lib unit tests ==="

# ---------------------------------------------------------------------------
# Section 1: plan_match — fork-free ERE matcher
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_match: fork-free ERE matching ---"

# Sample plan dump used across multiple assertions.
# Includes a literal-asterisk line for the escaped-star test (b4).
_SAMPLE_PLAN="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets --message-format=json 2>&1 | tee /tmp/clippy.json
cargo nextest run --workspace --profile debug --exclude reify-occt-gated
tests/infra/run_all.sh
tests/infra/test_verify_scope.sh
tests/infra/test_verify_*.sh
cargo-test-occt-gated.sh foo"

# (a) Matches a literal substring present in the sample plan dump.
assert "plan_match: literal 'cargo clippy' matches" \
    plan_match "$_SAMPLE_PLAN" "cargo clippy"

# (b1) Matches alternation pattern used by the suite.
assert "plan_match: alternation 'cargo (test|nextest run) --workspace'" \
    plan_match "$_SAMPLE_PLAN" "cargo (test|nextest run) --workspace"

# (b2) Matches .* same-line pattern used by the suite.
assert "plan_match: '.*' same-line 'cargo nextest run --workspace.*--exclude'" \
    plan_match "$_SAMPLE_PLAN" "cargo nextest run --workspace.*--exclude"

# (b3) Matches escaped-dot pattern used by the suite.
assert "plan_match: escaped-dot 'cargo-test-occt-gated\\.sh'" \
    plan_match "$_SAMPLE_PLAN" "cargo-test-occt-gated\\.sh"

# (b4) Matches escaped-star glob pattern used by the suite.
assert "plan_match: escaped-star 'tests/infra/test_verify_\\*\\.sh'" \
    plan_match "$_SAMPLE_PLAN" "tests/infra/test_verify_\\*\\.sh"

# (c) Returns non-zero when pattern is absent.
assert "plan_match: absent pattern returns non-zero" \
    refute plan_match "$_SAMPLE_PLAN" "cargo build --release"

# (d) .* does NOT cross a newline — grep-equivalent per-line semantics.
# plan_match iterates lines with `read` and matches each individually,
# so . never crosses a line boundary (REG_NEWLINE behaviour, same as grep -qE).
_MULTILINE_DUMP="line one content
line two content"
assert "plan_match: '.*' same-line match works in multiline dump (line one present)" \
    plan_match "$_MULTILINE_DUMP" "line one.*content"
assert "plan_match: absent same-line pattern fails in multiline dump" \
    refute plan_match "$_MULTILINE_DUMP" "line one.*ABSENT"
# Cross-line pattern must NOT match (grep -qE parity: . never crosses newline).
assert "plan_match: '.*' does not cross newline (cross-line pattern refuted)" \
    refute plan_match "$_MULTILINE_DUMP" "line one.*line two"

# (e) Empty pattern matches (grep -qE "" parity).
assert "plan_match: empty pattern matches any non-empty dump" \
    plan_match "$_SAMPLE_PLAN" ""

# ---------------------------------------------------------------------------
# Section 2: plan_capture_complete — structural completeness check
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_capture_complete: structural completeness ---"

_COMPLETE_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace"

_TRUNCATED_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL"

_EMPTY_PLAN_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=0 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
# (no commands — docs/yaml-only scope)"

# (a) Complete dump with both markers returns 0.
assert "plan_capture_complete: complete dump returns 0" \
    plan_capture_complete "$_COMPLETE_DUMP"

# (b) Truncated dump (header only, no commands marker) returns non-zero.
assert "plan_capture_complete: truncated dump returns non-zero" \
    refute plan_capture_complete "$_TRUNCATED_DUMP"

# (c) Empty string returns non-zero.
assert "plan_capture_complete: empty string returns non-zero" \
    refute plan_capture_complete ""

# (d) Empty-PLAN dump (both markers present, but no actual commands) returns 0.
# Completeness is structural — independent of whether commands exist.
assert "plan_capture_complete: docs-only (no commands) dump still returns 0" \
    plan_capture_complete "$_EMPTY_PLAN_DUMP"

# THE MARKER PAIR IS NOT ENOUGH ON ITS OWN. Both markers sit in the plan HEADER:
# in a real merge-role plan they land on lines 1 and 11 of 39, and every line the
# T-series asserts on — the release pre-builds, the nextest passes — comes after
# them. A capture truncated anywhere past the commands marker therefore carries
# both markers and is certified complete, so capture_print_plan returns without
# retrying and the missing line surfaces as a spurious assertion FAIL instead.
# Cases (e)/(f) pin the boundary that closes that class without closing the
# legitimate docs-only case: at least one line must follow the commands marker.
_MARKER_ONLY_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands (executed in order; '&&' semantics — stop on first failure) ---"

# (e) Both markers present but NOTHING after the commands marker returns non-zero.
assert "plan_capture_complete: markers present with nothing after the commands marker returns non-zero" \
    refute plan_capture_complete "$_MARKER_ONLY_DUMP"

# (f) Regression pin for (e)'s fix, stated adjacent to it: the docs-only dump is
# NOT the truncation in (e) — verify.sh:3663 unconditionally emits either real
# command lines or the comment "# (no commands — nothing to verify for this
# action/scope)" after the marker, so a COMMENT line after the marker is a
# complete plan. A fix that demanded a non-comment command line would red this.
assert "plan_capture_complete: a comment-only line after the commands marker still returns 0" \
    plan_capture_complete "$_EMPTY_PLAN_DUMP"

# ---------------------------------------------------------------------------
# Section 3: plan_narrow_active — extract NARROW_ACTIVE value
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_narrow_active: NARROW_ACTIVE extraction ---"

_NARROW0_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands ---"

_NARROW1_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# narrowing — NARROW_ACTIVE=1 affected=reify-doc
# --- commands ---"

_NO_NARROW_DUMP="# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task
# --- commands ---"

# (a) NARROW_ACTIVE=0 -> echoes "0".
assert "plan_narrow_active: NARROW_ACTIVE=0 echoes '0'" \
    test "$(plan_narrow_active "$_NARROW0_DUMP")" = "0"

# (b) NARROW_ACTIVE=1 -> echoes "1".
assert "plan_narrow_active: NARROW_ACTIVE=1 echoes '1'" \
    test "$(plan_narrow_active "$_NARROW1_DUMP")" = "1"

# (c) Dump lacking narrowing line -> echoes empty.
assert "plan_narrow_active: no narrowing line echoes empty" \
    test "$(plan_narrow_active "$_NO_NARROW_DUMP")" = ""

# ---------------------------------------------------------------------------
# Section 4: capture_print_plan — retry-on-incomplete-capture wrapper
# ---------------------------------------------------------------------------
echo ""
echo "--- capture_print_plan: retry-on-incomplete-capture ---"

# Use a counter FILE (survives the command-substitution subshell) for tracking
# how many times the fixture function is called.
_COUNTER_FILE="$(mktemp)"
trap 'rm -f "$_COUNTER_FILE"' EXIT

# Fixture: emits TRUNCATED on attempt 1, COMPLETE on attempt >= 2.
_fake_emit_succeed_on_second() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    if [ "$cnt" -ge 2 ]; then
        printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
        printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
        printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
        printf '%s\n' "cargo clippy --workspace"
    else
        # Truncated: header only, no '# --- commands' marker.
        printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
        printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    fi
}

# (a) Returns 0, OUT holds complete dump, counter == 2 (retried exactly once).
printf '0' > "$_COUNTER_FILE"
_OUT_A=""
assert "capture_print_plan (a): returns 0 when second attempt succeeds" \
    capture_print_plan _OUT_A 3 _fake_emit_succeed_on_second

assert "capture_print_plan (a): OUT holds complete dump" \
    plan_capture_complete "$_OUT_A"

_cnt_a=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (a): retried exactly once (counter == 2)" \
    test "$_cnt_a" = "2"

# Fixture: always emits truncated dump.
_fake_emit_always_truncated() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    # Header only — no '# --- commands' marker.
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
}

# (b) Returns non-zero after exactly max_attempts; OUT holds last (truncated) capture.
printf '0' > "$_COUNTER_FILE"
_OUT_B=""
assert "capture_print_plan (b): returns non-zero after exhausting max_attempts" \
    refute capture_print_plan _OUT_B 3 _fake_emit_always_truncated

_cnt_b=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (b): called exactly max_attempts times (counter == 3)" \
    test "$_cnt_b" = "3"

assert "capture_print_plan (b): OUT holds last (truncated) capture (non-empty)" \
    test -n "$_OUT_B"

# Fixture: always emits complete dump on first call.
_fake_emit_always_complete() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    printf '%s\n' "cargo clippy --workspace"
}

# (c) Returns 0 with counter == 1 (no superfluous retries).
printf '0' > "$_COUNTER_FILE"
_OUT_C=""
assert "capture_print_plan (c): returns 0 on first complete dump" \
    capture_print_plan _OUT_C 3 _fake_emit_always_complete

_cnt_c=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (c): no superfluous retries (counter == 1)" \
    test "$_cnt_c" = "1"

# THE KILLED-MID-WRITE CLASS. Structural markers certify what the child WROTE;
# they say nothing about whether it finished. A producer SIGKILLed after the
# header has emitted both markers, so a marker-only oracle accepts its partial
# output as a whole plan. The child's exit status is the sound oracle for this:
# command substitution reads to EOF, so a producer that exited 0 necessarily
# wrote its whole plan, and truncation requires the producer to die — which is
# exactly what a non-zero rc reports.
#
# Fixture: emits a MARKER-COMPLETE dump every time, then exits 137 (SIGKILL).
_fake_emit_complete_but_killed() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    printf '%s\n' "cargo clippy --workspace"
    return 137
}

# (d) A marker-complete dump from a child that DIED is an incomplete capture:
#     retry, then return non-zero on exhaustion.
printf '0' > "$_COUNTER_FILE"
_OUT_D=""
assert "capture_print_plan (d): marker-complete dump from a non-zero-exit child returns non-zero" \
    refute capture_print_plan _OUT_D 3 _fake_emit_complete_but_killed

_cnt_d=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (d): a dead child is RETRIED to max_attempts (counter == 3)" \
    test "$_cnt_d" = "3"

assert "capture_print_plan (d): OUT is still assigned on exhaustion (caller's assertion stays the failure surface)" \
    test -n "$_OUT_D"

# Fixture: complete dump AND an explicit clean exit — the healthy producer.
_fake_emit_complete_and_exits_zero() {
    local cnt
    cnt=$(cat "$_COUNTER_FILE" 2>/dev/null || echo 0)
    cnt=$((cnt + 1))
    printf '%s' "$cnt" > "$_COUNTER_FILE"
    printf '%s\n' "# verify.sh plan — action=all profile=debug scope=staged include_infra=1 nextest=cargo-nextest role=task"
    printf '%s\n' "# narrowing — NARROW_ACTIVE=0 affected=ALL"
    printf '%s\n' "# --- commands (executed in order; '&&' semantics — stop on first failure) ---"
    printf '%s\n' "cargo clippy --workspace"
    return 0
}

# (e) Companion to (d): the rc check must not degrade into "always retry" — a
#     complete dump from a child that exited 0 is accepted on the FIRST attempt.
printf '0' > "$_COUNTER_FILE"
_OUT_E=""
assert "capture_print_plan (e): complete dump from a zero-exit child returns 0" \
    capture_print_plan _OUT_E 3 _fake_emit_complete_and_exits_zero

_cnt_e=$(cat "$_COUNTER_FILE")
assert "capture_print_plan (e): accepted on the first attempt (counter == 1)" \
    test "$_cnt_e" = "1"

assert "capture_print_plan (e): OUT holds the complete dump" \
    plan_capture_complete "$_OUT_E"

# ---------------------------------------------------------------------------
# Section 5: plan_count_noncomment_lines — fork-free non-comment line counter
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_count_noncomment_lines: non-comment line count ---"

# (a) Empty dump -> 0 (grep -cE '^[^#]' on empty input returns 0).
assert "plan_count_noncomment_lines: empty dump -> 0" \
    test "$(plan_count_noncomment_lines "")" = "0"

# (b) All comment lines -> 0 (including '# --- commands' header).
_ALL_COMMENTS="# verify.sh plan — action=all
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands ---"
assert "plan_count_noncomment_lines: all comment lines -> 0" \
    test "$(plan_count_noncomment_lines "$_ALL_COMMENTS")" = "0"

# (c) Mix of comment and command lines -> correct count.
_MIXED_PLAN="# verify.sh plan — action=all
# narrowing — NARROW_ACTIVE=0 affected=ALL
# --- commands ---
cargo clippy --workspace
cargo nextest run --workspace"
assert "plan_count_noncomment_lines: two command lines -> 2" \
    test "$(plan_count_noncomment_lines "$_MIXED_PLAN")" = "2"

# (d) Single command line -> 1.
assert "plan_count_noncomment_lines: single command line -> 1" \
    test "$(plan_count_noncomment_lines "cargo clippy --workspace")" = "1"

# ---------------------------------------------------------------------------
# Section 6: plan_is_narrowing_axis_line — narrowing-axis classification
# ---------------------------------------------------------------------------
# Contract (task #6391): rc 0 iff the line's ` -p <crate>` selector is the one
# substituted from verify.sh's AFFECTED_ALL_FLAGS — i.e. the line sits on the
# axis that REIFY_AFFECTED_CRATES_OVERRIDE / branch-diff narrowing can move.
# Every other ` -p `-bearing plan line is a fixed-crate or independently-scoped
# axis and must classify as OFF-axis.
#
# Every case below is a VERBATIM line shape captured from `verify.sh --print-plan`
# (merge-gate `--profile both --scope all` and narrowing-active `--profile both
# --scope branch` runs of the same fixture) — not invented. The `cargo test`
# twin in (g) is the NEXTEST=0 fallback built at scripts/verify.sh:1944+1950.
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_is_narrowing_axis_line: narrowing-axis classification ---"

# --- ON-AXIS: one case per AFFECTED_ALL_FLAGS consumption site --------------

# (a) clippy site (verify.sh:2563), non-narrowed twin — scope=all -> --workspace.
assert "plan_is_narrowing_axis_line (a): clippy --workspace is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 45m nice -n 5 cargo clippy --workspace --all-targets -- -D warnings"

# (b) clippy site, narrowed — the override's crates substituted in.
assert "plan_is_narrowing_axis_line (b): clippy -p <affected> is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 45m nice -n 15 ionice -c 2 -n 7 cargo clippy -p reify-doc -p reify-ir --all-targets -- -D warnings"

# (c) DEBUG nextest site (verify.sh:2133), non-narrowed twin.
assert "plan_is_narrowing_axis_line (c): nextest --workspace (debug) is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 60m nice -n 5 cargo nextest run --workspace --config-file /tmp/reify-nextest-occt.<print-plan-placeholder> 9<&-"

# (d) DEBUG nextest site, narrowed.
assert "plan_is_narrowing_axis_line (d): nextest -p <affected> (debug) is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 60m nice -n 15 ionice -c 2 -n 7 cargo nextest run -p reify-doc -p reify-ir --config-file /tmp/reify-nextest-occt.<print-plan-placeholder> 9<&-"

# (e) typecheck site (verify.sh:2414), narrowed.
assert "plan_is_narrowing_axis_line (e): cargo check -p <affected> --tests is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 20m cargo check -p reify-doc -p reify-ir --tests"

# (f) typecheck site, non-narrowed twin.
assert "plan_is_narrowing_axis_line (f): cargo check --workspace --tests is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 20m cargo check --workspace --tests"

# (g) DEBUG nextest site, NEXTEST=0 `cargo test` fallback twin (verify.sh:1944).
assert "plan_is_narrowing_axis_line (g): cargo test -p <affected> fallback is ON-axis" \
    plan_is_narrowing_axis_line "timeout --kill-after=60 60m nice -n 15 ionice -c 2 -n 7 cargo test -p reify-doc -p reify-ir -- --test-threads=1 9<&-"

# --- OFF-AXIS: one case per non-narrowed ` -p `-bearing axis ----------------

# (h) THE load-bearing case. The release-sensitivity pass (verify.sh:2098-2128)
# is scoped by scripts/release-sensitive-crates.txt and is deliberately NOT
# narrowed. Its ` -p reify-ir` is exactly what a blanket whole-plan
# `! grep -qE " -p reify-ir"` conflated with a narrowing leak — the conflation
# that stalled task 5166 (2026-07-20 -> 2026-08-20).
assert "plan_is_narrowing_axis_line (h): release-sensitivity nextest pass is OFF-axis" \
    refute plan_is_narrowing_axis_line "timeout --kill-after=60 90m nice -n 5 cargo nextest run -p reify-eval -p reify-eval-fea-tests -p reify-gui -p reify-ir -p reify-solver-elastic --release --config-file /tmp/reify-nextest-occt.<print-plan-placeholder> 9<&-"

# (i) Fixed gui-feature compile check — always `-p reify-gui`, never narrowed.
assert "plan_is_narrowing_axis_line (i): fixed gui-feature cargo check is OFF-axis" \
    refute plan_is_narrowing_axis_line "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 45m nice -n 5 cargo check -p reify-gui --features gui --tests; fi"

# (j) Fixed gui-feature nextest pass — always `-p reify-gui`, never narrowed.
assert "plan_is_narrowing_axis_line (j): fixed gui-feature nextest pass is OFF-axis" \
    refute plan_is_narrowing_axis_line "if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 45m nice -n 5 cargo nextest run -p reify-gui --features gui --config-file /tmp/reify-nextest-occt.<print-plan-placeholder>; fi 9<&-"

# (k) Fixed release pre-build — always `-p reify-audit`, never narrowed.
assert "plan_is_narrowing_axis_line (k): fixed reify-audit release pre-build is OFF-axis" \
    refute plan_is_narrowing_axis_line "if test -f crates/reify-audit/Cargo.toml; then timeout --kill-after=60 45m nice -n 5 cargo build --release -p reify-audit 2>&1; fi"

# (l) Fixed release pre-build — always `-p reify-cli`, never narrowed.
assert "plan_is_narrowing_axis_line (l): fixed reify-cli release pre-build is OFF-axis" \
    refute plan_is_narrowing_axis_line "if test -f crates/reify-cli/Cargo.toml; then timeout --kill-after=60 45m nice -n 5 cargo build --release -p reify-cli 2>&1; fi"

# (m) Non-cargo tool line — no cargo subcommand at all.
assert "plan_is_narrowing_axis_line (m): non-cargo tool line is OFF-axis" \
    refute plan_is_narrowing_axis_line "export LD_LIBRARY_PATH=\"\${REIFY_AMBIENT_LD_LIBRARY_PATH-}\"; ./scripts/check-manifold-deps.sh"

# (n) Comment line (the narrowing header itself).
assert "plan_is_narrowing_axis_line (n): comment line is OFF-axis" \
    refute plan_is_narrowing_axis_line "# narrowing — NARROW_ACTIVE=0 affected= closure="

# (o) Empty line.
assert "plan_is_narrowing_axis_line (o): empty line is OFF-axis" \
    refute plan_is_narrowing_axis_line ""

# (p) ORDERING CONTRACT — the one KNOWN LIMITATION of the classifier, pinned
# here so it is discoverable rather than implicit. Unlike (a)-(o) this line shape
# is SYNTHETIC: verify.sh emits nothing like it today. The ` --release` exclusion
# runs BEFORE the cargo-subcommand allowlist, so a narrowable subcommand that
# ALSO carries ` --release` classifies OFF-axis. That is correct for verify.sh as
# it stands — $AFFECTED_ALL_FLAGS reaches nextest only in the DEBUG branch
# (verify.sh:2133, rel=""), and the check/clippy sites never take --release — but
# it is an assumption about verify.sh, not a property of the line, and it is
# fragile in exactly one direction: a future --release-bearing narrowing site
# would be silently misclassified as off-axis, quietly emptying the axis subset
# that MG-B5/MG-B6a assert an absence over.
#
# The BEHAVIOURAL backstop is test_verify_scope.sh Scenario MG-B5-control: with
# narrowing active it requires the override's crates ON the axis and ABSENT off
# it, so such a site would RED there. This unit case does not defend the
# ordering; it records that the ordering is deliberate and names what does. (#6391)
assert "plan_is_narrowing_axis_line (p): a --release-bearing clippy classifies OFF-axis (flag exclusion precedes the subcommand allowlist — known limitation, backstopped by MG-B5-control)" \
    refute plan_is_narrowing_axis_line "timeout --kill-after=60 45m nice -n 5 cargo clippy -p reify-doc --release --all-targets -- -D warnings"

# ---------------------------------------------------------------------------
# Section 7: plan_narrowing_axis_match / plan_offaxis_match /
#            plan_narrowing_axis_count — dump-level axis predicates
# ---------------------------------------------------------------------------
# The dump-level helpers built on plan_is_narrowing_axis_line (#6391). They are
# what lets a scenario assert "no ` -p ` reached the NARROWING AXIS" without
# forbidding the ` -p ` selectors that other axes emit legitimately.
#
# _AXIS_SAMPLE_PLAN is assembled from the same verbatim capture as Section 6:
# a narrowing-ACTIVE `--profile both --scope branch` run (narrowed clippy +
# narrowed debug nextest, release-sensitivity pass, fixed gui-feature check,
# non-cargo tool line) plus a merge-gate release pre-build line.
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_narrowing_axis_match / plan_offaxis_match / plan_narrowing_axis_count ---"

_AXIS_SAMPLE_PLAN="# verify.sh plan — action=all profile=both scope=branch include_infra=1 nextest=1 role=task
# narrowing — NARROW_ACTIVE=1 affected=reify-doc reify-ir closure=reify-doc reify-ir
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
export LD_LIBRARY_PATH=\"\${REIFY_AMBIENT_LD_LIBRARY_PATH-}\"; ./scripts/check-manifold-deps.sh
timeout --kill-after=60 45m nice -n 15 ionice -c 2 -n 7 cargo clippy -p reify-doc -p reify-ir --all-targets -- -D warnings
if test -f gui/src-tauri/Cargo.toml; then ./scripts/ensure-gui-sidecar-placeholder.sh && timeout --kill-after=60 45m nice -n 15 ionice -c 2 -n 7 cargo check -p reify-gui --features gui --tests; fi
if test -f crates/reify-audit/Cargo.toml; then timeout --kill-after=60 45m nice -n 5 cargo build --release -p reify-audit 2>&1; fi
timeout --kill-after=60 60m nice -n 15 ionice -c 2 -n 7 cargo nextest run -p reify-doc -p reify-ir --config-file /tmp/reify-nextest-occt.<print-plan-placeholder> 9<&-
timeout --kill-after=60 90m nice -n 15 ionice -c 2 -n 7 cargo nextest run -p reify-eval -p reify-eval-fea-tests -p reify-gui -p reify-ir -p reify-solver-elastic --release --config-file /tmp/reify-nextest-occt.<print-plan-placeholder> 9<&-"

_AXIS_COMMENTS_ONLY="# verify.sh plan — action=all profile=both scope=all
# narrowing — NARROW_ACTIVE=0 affected= closure=
# --- commands ---"

# --- plan_narrowing_axis_match ---------------------------------------------

# (a) The override's crates DO reach the narrowing axis (narrowed clippy +
# narrowed debug nextest both carry ` -p reify-doc`).
assert "plan_narrowing_axis_match (a): ' -p reify-doc' matches on the narrowing axis" \
    plan_narrowing_axis_match "$_AXIS_SAMPLE_PLAN" " -p reify-doc"

# (b) THE assertion that encodes the whole point of #6391. reify-solver-elastic
# appears ONLY on the release-sensitivity pass — present in the plan, but never
# via narrowing. A blanket whole-plan grep cannot make this distinction.
assert "plan_narrowing_axis_match (b): ' -p reify-solver-elastic' (release pass only) does NOT match the axis" \
    refute plan_narrowing_axis_match "$_AXIS_SAMPLE_PLAN" " -p reify-solver-elastic"

# (c) reify-audit appears only on the fixed release pre-build.
assert "plan_narrowing_axis_match (c): ' -p reify-audit' (fixed pre-build only) does NOT match the axis" \
    refute plan_narrowing_axis_match "$_AXIS_SAMPLE_PLAN" " -p reify-audit"

# (d) Empty dump has no axis lines, so nothing can match.
assert "plan_narrowing_axis_match (d): empty dump matches nothing" \
    refute plan_narrowing_axis_match "" " -p reify-"

# --- plan_offaxis_match (the exact complement) ------------------------------

# (e) The release-sensitivity pass is off-axis and carries reify-solver-elastic.
assert "plan_offaxis_match (e): ' -p reify-solver-elastic' matches off-axis" \
    plan_offaxis_match "$_AXIS_SAMPLE_PLAN" " -p reify-solver-elastic"

# (f) reify-doc reaches ONLY narrowing-axis lines, so it is absent off-axis.
# (This is the shape MG-B5-control uses as its classifier drift guard.)
assert "plan_offaxis_match (f): ' -p reify-doc' does NOT match off-axis" \
    refute plan_offaxis_match "$_AXIS_SAMPLE_PLAN" " -p reify-doc"

# (g) Complement property, asserted directly on a pattern present on BOTH kinds
# of line: ` -p reify-ir` is on the narrowed clippy AND on the release pass, so
# both matchers must return 0. This is exactly the case that made the pre-#6391
# blanket assertion unusable.
assert "plan_offaxis_match (g1): ' -p reify-ir' matches ON-axis (narrowed clippy)" \
    plan_narrowing_axis_match "$_AXIS_SAMPLE_PLAN" " -p reify-ir"
assert "plan_offaxis_match (g2): ' -p reify-ir' ALSO matches OFF-axis (release pass)" \
    plan_offaxis_match "$_AXIS_SAMPLE_PLAN" " -p reify-ir"

# --- plan_narrowing_axis_count ----------------------------------------------

# (h) The sample has exactly two narrowing-axis lines: the narrowed clippy and
# the narrowed debug nextest. Everything else is comment, non-cargo, gui-feature,
# release pre-build, or the release pass.
assert "plan_narrowing_axis_count (h): sample dump -> 2" \
    test "$(plan_narrowing_axis_count "$_AXIS_SAMPLE_PLAN")" = "2"

# (i) Empty dump -> 0.
assert "plan_narrowing_axis_count (i): empty dump -> 0" \
    test "$(plan_narrowing_axis_count "")" = "0"

# (j) Comment-only dump -> 0.
assert "plan_narrowing_axis_count (j): comment-only dump -> 0" \
    test "$(plan_narrowing_axis_count "$_AXIS_COMMENTS_ONLY")" = "0"

# ---------------------------------------------------------------------------
# Section 9: plan_strip_comments — fork-free comment stripper (#6247)
#
# WHY THIS EXISTS: the occt suite's --print-plan captures ran
# `verify.sh --print-plan | grep -v '^#'` in a single shot, and that post-filter
# strips BOTH structural markers plan_capture_complete looks for
# (`# verify.sh plan`, `# --- commands`).  A truncated capture therefore could
# not be DETECTED — it just read as "pattern absent" and fired a misleading
# assertion failure.  The two operations only compose one way round: certify
# completeness on the RAW dump first, THEN reduce to command lines.  This helper
# is the second half, kept fork-free for the same esc-4574-42 reason as the rest
# of the lib (plan_capture_lib.sh:8-14).
#
# Comment stripping is load-bearing for the migration, not cosmetic: several occt
# assertions (test_occt_flock_gate.sh:131,148,296,299,302) assert a pattern is
# ABSENT, and a retained comment line could satisfy the pattern and flip them.
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_strip_comments: fork-free comment stripping ---"

# (a) Mixed dump -> only the non-comment, non-empty lines survive, in order.
_STRIP_MIXED="# verify.sh plan — action=all profile=debug scope=all
# --- commands (executed in order) ---
cargo clippy --workspace --all-targets
./scripts/check-manifold-deps.sh
# trailing comment"
assert "plan_strip_comments (a): mixed dump keeps only command lines, in order" \
    test "$(plan_strip_comments "$_STRIP_MIXED")" = "cargo clippy --workspace --all-targets
./scripts/check-manifold-deps.sh"

# (b) Comment-only dump -> empty.
_STRIP_ALLCOMMENT="# verify.sh plan — action=all
# scope decision — RUN_RUST=1
# --- commands ---"
assert "plan_strip_comments (b): comment-only dump -> empty" \
    test "$(plan_strip_comments "$_STRIP_ALLCOMMENT")" = ""

# (c) Empty dump -> empty.
assert "plan_strip_comments (c): empty dump -> empty" \
    test "$(plan_strip_comments "")" = ""

# (d) A '#' that is not in column 1 does NOT make the line a comment. Real
# --print-plan command lines carry inline '#' (shell comments inside a quoted
# `bash -c`, fragment identifiers in paths), and dropping them would silently
# shrink the plan a caller is asserting over.
_STRIP_INLINE_HASH="# --- commands ---
timeout 45m bash -c 'echo \"#not-a-comment\" && cargo check'"
assert "plan_strip_comments (d): '#' outside column 1 keeps the line" \
    test "$(plan_strip_comments "$_STRIP_INLINE_HASH")" = "timeout 45m bash -c 'echo \"#not-a-comment\" && cargo check'"

# (e) EQUIVALENCE CONTROL — the case that actually licenses the migration.
# On a REAL captured dump the fork-free stripper must be byte-identical to the
# `grep -v '^#'` post-filter the occt call sites use today, so replacing them
# cannot change what their absence assertions see.
_REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
_real_print_plan() { ( cd "$_REPO_ROOT" && bash scripts/verify.sh lint --print-plan 2>/dev/null ); }
_REAL_DUMP=""
capture_print_plan _REAL_DUMP 3 _real_print_plan || true

assert "plan_strip_comments (e0): the real --print-plan capture is complete (precondition)" \
    plan_capture_complete "$_REAL_DUMP"

# `|| true` for the same reason capture_print_plan documents: a failing capture
# must leave the ASSERTIONS below as the visible failure surface, not abort the
# suite under `set -e` and hide every case after this point.
_STRIP_FORKFREE="$(plan_strip_comments "$_REAL_DUMP")" || true
_STRIP_VIA_GREP="$(printf '%s\n' "$_REAL_DUMP" | grep -v '^#')" || true
assert "plan_strip_comments (e): real dump -> byte-identical to the grep -v '^#' post-filter" \
    test "$_STRIP_FORKFREE" = "$_STRIP_VIA_GREP"

# (e2) Non-vacuity for (e): the real dump really does carry both comment and
# command lines, so (e) is not comparing two empty strings.
assert "plan_strip_comments (e2): the real dump has command lines left after stripping" \
    test "$(plan_count_noncomment_lines "$_REAL_DUMP")" -gt 0
assert "plan_strip_comments (e3): stripping the real dump removed something" \
    refute test "$_STRIP_FORKFREE" = "$_REAL_DUMP"

# (f) FORK-FREEDOM. The lib's whole rationale is that a pipe forks a subshell
# and a filter which, under concurrent load, can fail with EINTR even when the
# content matches (esc-4574-42). What LICENSES the migration is the behavioural
# equivalence control (e) above; (f1) and (f2) keep a future "simplification"
# back to `| grep` from silently reintroducing the fork that (e) alone would
# still accept, since a forking implementation can be byte-identical in output.
#
# (f1) is BEHAVIOURAL, not a scan of the rendered source: each external filter
# is shadowed by a marker-writing function, so a real call is recorded however
# it is spelled, while a body that merely mentions `grep` in a local name or a
# comment records nothing. A source-text scan gets both of those backwards.
# The shadows live inside a subshell so they cannot leak into later cases --
# `_body_has` below itself runs `grep`.
_STRIP_FILTER_CALLS="$(mktemp)"
(
    grep() { printf 'grep\n' >> "$_STRIP_FILTER_CALLS"; }
    sed()  { printf 'sed\n'  >> "$_STRIP_FILTER_CALLS"; }
    awk()  { printf 'awk\n'  >> "$_STRIP_FILTER_CALLS"; }
    plan_strip_comments "$_STRIP_MIXED" >/dev/null
)
assert "plan_strip_comments (f1): invokes no external filter (grep/sed/awk shadowed and never called)" \
    test ! -s "$_STRIP_FILTER_CALLS"

# NON-VACUITY for (f1): the shadows really do fire when something calls them,
# so the empty marker above is evidence of absence rather than of a probe that
# never worked.
#
# The shadow DRAINS stdin before returning. It is the read end of the pipeline
# below, and a reader that returns without reading closes the pipe under a
# `printf` that may still be mid-write — which kills it with SIGPIPE (141) and,
# under this file's `set -euo pipefail`, aborts the whole suite with no FAIL
# line to name the case. That is a race on write timing, so it fires only under
# load: reproduced once in two runs at load 117, and it reddened task 5417's
# merge gate on 2026-09-17. Same pipefail+SIGPIPE class as the `| grep -q`
# sites task #7115 sweeps, reached through a non-`-q` spelling.
(
    grep() { while IFS= read -r _; do :; done; printf 'grep\n' >> "$_STRIP_FILTER_CALLS"; }
    printf '%s\n' "$_STRIP_MIXED" | grep -v '^#' >/dev/null
)
assert "plan_strip_comments (f1n): the filter shadows DO record a real call (f1 is not vacuous)" \
    test -s "$_STRIP_FILTER_CALLS"
rm -f "$_STRIP_FILTER_CALLS"

# (f2) The one STRUCTURAL check worth keeping: a command substitution is a
# subshell by definition, so its absence is exactly the property (f1) cannot
# observe (a `$( ... )` that forks but calls no external filter).
_body_has() { declare -f "$1" | grep -qF -- "$2"; }

assert "plan_strip_comments (f2): body contains no command substitution" \
    refute _body_has plan_strip_comments '$('

# ---------------------------------------------------------------------------
# Section 10: plan_header_field — typed plan header field reader
#
# Callers used to classify the header with ordered substrings over the whole
# dump, which a reordering or a newly inserted field silently breaks and which a
# lookalike token on any other line satisfies. plan_header_field locates the one
# header line of a kind by its label and reads a field by exact name, so every
# case below is about that contract: order-independence, exact names, content
# keying, and an absent field that reads differently from an empty one.
#
# Every header fixture line starts its physical line with '#', and none of the
# case labels repeats the ordered scope adjacency: the ordered-scope-grep
# ratchet counts code lines only.
# ---------------------------------------------------------------------------
echo ""
echo "--- plan_header_field: typed header field reader ---"

# _field_is <dump> <kind> <NAME> <want> — the field is present (rc 0) and reads
# exactly <want>.
_field_is() {
    local _got _rc=0
    _got="$(plan_header_field "$1" "$2" "$3")" || _rc=$?
    printf 'got [%s] rc=%s, want [%s] rc=0\n' "$_got" "$_rc" "$4"
    [ "$_rc" -eq 0 ] && [ "$_got" = "$4" ]
}

# _field_absent <dump> <kind> <NAME> — rc is exactly 1 and stdout is empty.
_field_absent() {
    local _got _rc=0
    _got="$(plan_header_field "$1" "$2" "$3")" || _rc=$?
    printf 'got [%s] rc=%s, want [] rc=1\n' "$_got" "$_rc"
    [ "$_rc" -eq 1 ] && [ -z "$_got" ]
}

# _field_is_bit <dump> <kind> <NAME> — the field is present and reads 0 or 1.
_field_is_bit() {
    local _got _rc=0
    _got="$(plan_header_field "$1" "$2" "$3")" || _rc=$?
    printf 'got [%s] rc=%s, want [0|1] rc=0\n' "$_got" "$_rc"
    [ "$_rc" -eq 0 ] && [[ "$_got" == [01] ]]
}

# _refused_naming <needle>... -- <cmd...> — <cmd> returns exactly 2, prints
# nothing on stdout, and its stderr contains every needle.
_refused_naming() {
    local -a _needles=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do _needles+=("$1"); shift; done
    shift
    local _out _err _rc=0 _n
    _err="$("$@" 2>&1 >/dev/null)" || true
    _out="$("$@" 2>/dev/null)" || _rc=$?
    printf 'rc=%s stdout [%s] stderr [%s]\n' "$_rc" "$_out" "$_err"
    [ "$_rc" -eq 2 ] && [ -z "$_out" ] || return 1
    for _n in "${_needles[@]}"; do
        [[ "$_err" == *"$_n"* ]] || { printf 'stderr lacks [%s]\n' "$_n"; return 1; }
    done
}

# _ran_fork_free <fn> <calls-file> — <fn> is defined AND the shadowed filters
# recorded no call, so an undefined function cannot pass by recording nothing.
_ran_fork_free() { declare -F "$1" >/dev/null && [ ! -s "$2" ]; }

# _defined_without <fn> <needle> — <fn> is defined AND its body lacks <needle>.
_defined_without() { declare -F "$1" >/dev/null && refute _body_has "$1" "$2"; }

# A realistic dump in today's field order, with a lookalike on every line that
# is not the header being read: the preamble, the retry marker printed BEFORE
# the header (nextest_debug=), a NOTE carrying role=, and a command line that
# repeats RUN_RUST=, HEAVY= and nextest= with other values. The narrowing header
# spans three physical lines: its affected/closure lists are newline-separated
# today (PRD §2.4 defect 7).
_HDR_DUMP="warning: apply_env — REIFY_TEST_SEMAPHORE_WAIT unset, using the default
@@REIFY_RETRY_SCOPE=failed_only@@ nextest_debug=0 nextest_release=0 run_all=0 gui=0
# verify.sh plan — action=all profile=both scope=branch include_infra=1 nextest=1 role=merge
# NOTE: include_infra=1 under role=task gets the selective per-artifact infra subset only
# scope decision — RUN_RUST=1 RUN_GUI=0 RUN_OCCT_GATE=1 RUN_GUI_VITEST=0
# narrowing — NARROW_ACTIVE=1 affected=reify-doc
reify-ir closure=reify-doc
reify-ir from_diff=1
# heavy partition — HEAVY=excluded
# --- environment (process-level; inherited by every command below) ---
# CARGO_INCREMENTAL=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
echo RUN_RUST=0 HEAVY=only nextest=0"

# (a) plan header: first field, last field, and a name another line shadows.
assert "plan_header_field (a1): plan action -> all" \
    _field_is "$_HDR_DUMP" plan action all
assert "plan_header_field (a2): plan role -> merge (the line's last token; the NOTE's role= is not the header)" \
    _field_is "$_HDR_DUMP" plan role merge
assert "plan_header_field (a3): plan nextest -> 1 (not the retry marker's nextest_debug, not the command line's nextest=0)" \
    _field_is "$_HDR_DUMP" plan nextest 1

# (b) scope decision header.
assert "plan_header_field (b1): scope RUN_RUST -> 1 (not the command line's RUN_RUST=0)" \
    _field_is "$_HDR_DUMP" scope RUN_RUST 1
assert "plan_header_field (b2): scope RUN_GUI -> 0" \
    _field_is "$_HDR_DUMP" scope RUN_GUI 0
assert "plan_header_field (b3): scope RUN_GUI_VITEST -> 0" \
    _field_is "$_HDR_DUMP" scope RUN_GUI_VITEST 0

# (c) heavy partition header.
assert "plan_header_field (c): heavy HEAVY -> excluded (not the command line's HEAVY=only)" \
    _field_is "$_HDR_DUMP" heavy HEAVY excluded

# (d) narrowing header, including its continuation lines.
assert "plan_header_field (d1): narrowing NARROW_ACTIVE -> 1" \
    _field_is "$_HDR_DUMP" narrowing NARROW_ACTIVE 1
assert "plan_header_field (d2): narrowing from_diff -> 1 (the field on the last continuation line)" \
    _field_is "$_HDR_DUMP" narrowing from_diff 1
assert "plan_header_field (d3): narrowing affected -> the whole crate list, single-space joined" \
    _field_is "$_HDR_DUMP" narrowing affected "reify-doc reify-ir"
assert "plan_header_field (d4): narrowing closure -> the whole crate list, single-space joined" \
    _field_is "$_HDR_DUMP" narrowing closure "reify-doc reify-ir"

# (e) PRESENT BUT EMPTY is not ABSENT: verify.sh prints `closure=` with nothing
# after it when no closure was computed, and a caller asserting on "" must not
# pass against a plan that lacks the field altogether.
_HDR_EMPTY_FIELDS="# verify.sh plan — action=lint profile=debug scope=all include_infra=0 nextest=1 role=task
# narrowing — NARROW_ACTIVE=0 affected= closure= from_diff=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets"
assert "plan_header_field (e1): narrowing closure= -> present and empty (rc 0)" \
    _field_is "$_HDR_EMPTY_FIELDS" narrowing closure ""
assert "plan_header_field (e2): narrowing affected= -> present and empty (rc 0)" \
    _field_is "$_HDR_EMPTY_FIELDS" narrowing affected ""
assert "plan_header_field (e3): an empty field does not swallow the next one (from_diff -> 0)" \
    _field_is "$_HDR_EMPTY_FIELDS" narrowing from_diff 0

# (f) ABSENT: rc 1 and nothing on stdout.
assert "plan_header_field (f1): an unknown NAME on a present header line is absent" \
    _field_absent "$_HDR_DUMP" scope RUN_MISSING
assert "plan_header_field (f2): a NAME that is only a prefix of real fields (RUN) is absent" \
    _field_absent "$_HDR_DUMP" scope RUN
assert "plan_header_field (f3): heavy on a dump with no heavy partition line is absent" \
    _field_absent "$_HDR_EMPTY_FIELDS" heavy HEAVY
assert "plan_header_field (f4): any field of an empty dump is absent" \
    _field_absent "" plan action

# (g) REORDERED scope header. The values are chosen so that a positional reader
# and a prefix-matching reader each read a different, wrong value.
_HDR_REORDERED="# verify.sh plan — action=all profile=debug scope=all include_infra=0 nextest=1 role=task
# scope decision — RUN_GUI_VITEST=1 RUN_OCCT_GATE=1 RUN_GUI=0 RUN_RUST=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets"
assert "plan_header_field (g1): reordered scope RUN_RUST -> 0" \
    _field_is "$_HDR_REORDERED" scope RUN_RUST 0
assert "plan_header_field (g2): reordered scope RUN_GUI -> 0 (exact name: not RUN_GUI_VITEST)" \
    _field_is "$_HDR_REORDERED" scope RUN_GUI 0
assert "plan_header_field (g3): reordered scope RUN_GUI_VITEST -> 1" \
    _field_is "$_HDR_REORDERED" scope RUN_GUI_VITEST 1

# (h) THE USER-OBSERVABLE SIGNAL: a field added to the scope decision — at the
# front, or between two existing fields — leaves every existing read unchanged.
_HDR_FRONT_INSERT="# verify.sh plan — action=all profile=debug scope=all include_infra=0 nextest=1 role=task
# scope decision — RUN_NEW_AXIS=1 RUN_RUST=0 RUN_GUI=1 RUN_OCCT_GATE=0 RUN_GUI_VITEST=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets"
_HDR_MID_INSERT="# verify.sh plan — action=all profile=debug scope=all include_infra=0 nextest=1 role=task
# scope decision — RUN_RUST=0 RUN_NEW_AXIS=0 RUN_GUI=1 RUN_OCCT_GATE=0 RUN_GUI_VITEST=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets"
assert "plan_header_field (h1): a field inserted at the front leaves RUN_RUST -> 0" \
    _field_is "$_HDR_FRONT_INSERT" scope RUN_RUST 0
assert "plan_header_field (h2): a field inserted at the front leaves RUN_GUI -> 1" \
    _field_is "$_HDR_FRONT_INSERT" scope RUN_GUI 1
assert "plan_header_field (h3): a field inserted between two fields leaves RUN_RUST -> 0" \
    _field_is "$_HDR_MID_INSERT" scope RUN_RUST 0
assert "plan_header_field (h4): a field inserted between two fields leaves RUN_GUI -> 1" \
    _field_is "$_HDR_MID_INSERT" scope RUN_GUI 1

# (i) CONTENT-KEYED, NOT POSITIONAL: the header is found by its label wherever it
# sits — (a) already reads it from under a preamble and a retry marker — and
# when a kind's label starts two lines, the first one is the header.
_HDR_TWO_SCOPE="# verify.sh plan — action=all profile=debug scope=all include_infra=0 nextest=1 role=task
# scope decision — RUN_RUST=1
# scope decision — RUN_RUST=0
# --- commands (executed in order; '&&' semantics — stop on first failure) ---
cargo clippy --workspace --all-targets"
assert "plan_header_field (i): the first line carrying a kind's label wins" \
    _field_is "$_HDR_TWO_SCOPE" scope RUN_RUST 1

# (j) An unknown line-kind is refused, naming the kind and the accepted set.
assert "plan_header_field (j): unknown line-kind -> rc 2, empty stdout, stderr naming it and the accepted kinds" \
    _refused_naming plan_header_field "'bogus'" plan scope narrowing heavy -- \
        plan_header_field "$_HDR_DUMP" bogus action

# (k) REAL-PRINTER CONTROL on Section 9's certified `verify.sh lint` capture: the
# labels the reader keys on are pinned against verify.sh's actual output, not
# only against fixtures written by the reader's author.
assert "plan_header_field (k1): real plan header action -> lint" \
    _field_is "$_REAL_DUMP" plan action lint
for _scope_field in RUN_RUST RUN_GUI RUN_OCCT_GATE RUN_GUI_VITEST; do
    assert "plan_header_field (k2): real scope decision $_scope_field is present and reads 0 or 1" \
        _field_is_bit "$_REAL_DUMP" scope "$_scope_field"
done
for _narrowing_field in NARROW_ACTIVE from_diff; do
    assert "plan_header_field (k3): real narrowing header $_narrowing_field is present and reads 0 or 1" \
        _field_is_bit "$_REAL_DUMP" narrowing "$_narrowing_field"
done

# (l) FORK-FREEDOM, by Section 9's (f1)/(f2) technique.
assert "plan_header_field (l0): is defined (precondition for l1/l2)" \
    declare -F plan_header_field
_HDR_FILTER_CALLS="$(mktemp)"
(
    grep() { printf 'grep\n' >> "$_HDR_FILTER_CALLS"; }
    sed()  { printf 'sed\n'  >> "$_HDR_FILTER_CALLS"; }
    awk()  { printf 'awk\n'  >> "$_HDR_FILTER_CALLS"; }
    plan_header_field "$_HDR_DUMP" narrowing closure >/dev/null
) || true
assert "plan_header_field (l1): invokes no external filter (grep/sed/awk shadowed and never called)" \
    _ran_fork_free plan_header_field "$_HDR_FILTER_CALLS"
rm -f "$_HDR_FILTER_CALLS"
assert "plan_header_field (l2): body contains no command substitution" \
    _defined_without plan_header_field '$('

test_summary
