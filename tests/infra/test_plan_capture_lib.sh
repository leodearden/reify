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

test_summary
