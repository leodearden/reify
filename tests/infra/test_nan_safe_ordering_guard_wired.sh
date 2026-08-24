#!/usr/bin/env bash
# Infrastructure test for task 5093 (INV-FEA-3 regression guard).
# Validates that scripts/verify.sh's lint plan includes a guarded, timeout-
# wrapped invocation of scripts/check-nan-safe-ordering.sh, that the script
# exists/executes and passes clean on the current tree, and — the task's
# user-observable signal — that the gate actually DETECTS a synthetic unguarded
# `partial_cmp(...).unwrap_or(Ordering::Equal)` and CLEARS it once annotated
# with the `// nan-safe:allow` escape. Mirrors test_event_inventory_wired.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== check-nan-safe-ordering.sh wiring tests ==="

# The orchestrator runs scripts/verify.sh, so wiring is asserted against the
# verify.sh lint plan. --include-infra so the lint-side infra leaf appears;
# --scope all for the full plan; env/comment lines stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan | grep -v '^#')"
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --include-infra --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS TEST_PLAN_SEGS

# -- (a): script is referenced in the lint plan --------------------------------
echo ""
echo "--- (a): scripts/check-nan-safe-ordering.sh is in the lint plan ---"
assert "lint plan contains 'scripts/check-nan-safe-ordering.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'scripts/check-nan-safe-ordering.sh'"

# -- (b): if-test-f guard is used ---------------------------------------------
echo ""
echo "--- (b): if test -f guard is used in the lint plan ---"
assert "lint plan contains 'if test -f scripts/check-nan-safe-ordering.sh'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'if test -f scripts/check-nan-safe-ordering.sh'"

# -- (c): WARNING echo is present for guard-skip branch -----------------------
echo ""
echo "--- (c): WARNING echo for guard-skip branch in the lint plan ---"
assert "lint plan has WARNING echo for check-nan-safe-ordering.sh skip" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'WARNING.*check-nan-safe-ordering'"

# -- (d): invocation is wrapped with timeout --kill-after=60 ------------------
echo ""
echo "--- (d): timeout --kill-after=60 wraps the invocation in the lint plan ---"
# Exact-shape pattern: it cannot span &&-separated clauses because every segment
# between 'timeout --kill-after=60' and the script name is a short anchored class
# with no greedy '.*'.
TIMEOUT_PATTERN='timeout --kill-after=60 [0-9]+m bash scripts/check-nan-safe-ordering\.sh'
assert "lint plan wraps check-nan-safe-ordering.sh with 'timeout --kill-after=60'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE '$TIMEOUT_PATTERN'"

# Synthetic-negative: the tight pattern must reject a 'timeout' on a different
# clause than the script (a greedy '.*' regex would silently pass this).
assert "TIMEOUT_PATTERN rejects: timeout on a different clause than the script" \
    bash -c "! echo 'lint_command: timeout --kill-after=60 30m cargo clippy && bash scripts/check-nan-safe-ordering.sh' | grep -qE '$TIMEOUT_PATTERN'"

# -- (e): script exists and is executable on disk ------------------------------
echo ""
echo "--- (e): scripts/check-nan-safe-ordering.sh exists and is executable ---"
assert "scripts/check-nan-safe-ordering.sh exists" \
    test -f "$REPO_ROOT/scripts/check-nan-safe-ordering.sh"
assert "scripts/check-nan-safe-ordering.sh is executable" \
    test -x "$REPO_ROOT/scripts/check-nan-safe-ordering.sh"

# -- (f): script is NOT in the test plan (placement in lint only) --------------
echo ""
echo "--- (f): scripts/check-nan-safe-ordering.sh is NOT in the test plan ---"
assert "test plan does NOT reference scripts/check-nan-safe-ordering.sh" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'scripts/check-nan-safe-ordering.sh'"

# -- (g): script runs clean (exit 0) against the current tree ------------------
echo ""
echo "--- (g): check-nan-safe-ordering.sh exits 0 on the current tree ---"
assert "bash scripts/check-nan-safe-ordering.sh --repo-root REPO_ROOT exits 0" \
    bash "$REPO_ROOT/scripts/check-nan-safe-ordering.sh" --repo-root "$REPO_ROOT"
assert "bash scripts/check-nan-safe-ordering.sh exits 0 with CWD=repo root (mirrors lint_command)" \
    bash -c "cd '$REPO_ROOT' && bash scripts/check-nan-safe-ordering.sh"

# -- (h): DETECTION — the task's user-observable signal ------------------------
# Prove detection (not just presence-of-string-in-script) against a throwaway
# git repo: a synthetic unguarded site in a covered crate is FLAGGED; the same
# site CLEARS once annotated with `// nan-safe:allow`; a doc-comment mention is
# NOT flagged.
echo ""
echo "--- (h): gate detects a synthetic unguarded pattern and honors the escape ---"
GATE="$REPO_ROOT/scripts/check-nan-safe-ordering.sh"
DET_TMP="$(mktemp -d)"
cleanup_det() { rm -rf "$DET_TMP"; }
trap cleanup_det EXIT

# _exits_with <want> <cmd...> — assert an EXACT exit code, not merely non-zero.
# The gate's contract distinguishes exit 1 (violation found) from exit 2
# (usage / not-a-work-tree error); a bare `! cmd` would conflate them, so a
# gate that crashed with a usage error on every invocation would satisfy every
# detection assertion below. Ported from
# task/5076:tests/infra/test_compute_trampoline_registration_wired.sh.
_exits_with() {
    local want="$1"; shift
    local rc=0
    "$@" || rc=$?
    [ "$rc" -eq "$want" ] && return 0
    echo "expected exit $want, got $rc from: $*"
    return 1
}

# Reusable single-file fixture harness, mirroring task/5076's FIX /
# write_baseline / stage idiom. This gate has ONE pass and needs only one
# covered-crate file, so write_fixture/stage are sized down to that.
FIX="$DET_TMP/fixture"
mkdir -p "$FIX"
git -C "$FIX" init -q
git -C "$FIX" config user.email test@invalid.local
git -C "$FIX" config user.name test

# write_fixture — reads the heredoc from stdin into the one covered-crate file
# (crates/reify-fdm is in SCOPE_PATHSPECS).
write_fixture() {
    mkdir -p "$FIX/crates/reify-fdm/src"
    cat > "$FIX/crates/reify-fdm/src/lib.rs"
}

# The gate's source set is `git ls-files`-hermetic: every fixture mutation
# MUST be re-staged via stage() or it is invisible to the gate. This is the #1
# way these fixtures silently pass vacuously.
stage() { git -C "$FIX" add -A; }

# unguarded → flagged (gate exits 1)
write_fixture <<'RS'
fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); }
RS
stage
assert "gate FLAGS a synthetic unguarded partial_cmp().unwrap_or(Ordering::Equal) in a covered crate" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# annotated → cleared (gate exits 0)
write_fixture <<'RS'
fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); } // nan-safe:allow — guarded upstream
RS
stage
assert "gate CLEARS the same site once annotated with // nan-safe:allow" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# doc-comment mention → not flagged
write_fixture <<'RS'
/// avoid partial_cmp(...).unwrap_or(Ordering::Equal) here; use total_cmp
fn s() {}
RS
stage
assert "gate does NOT flag a doc-comment mention of the hazard" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hD — BRACE-DEPTH DRIFT in the production-code view (task 6031, porting the
# lexer from task/5076:scripts/check-compute-trampoline-registration.sh).
#
# `depth` (which drives the #[cfg(test)] skipper) is counted from raw $0
# today, never from a comment/string/char-literal-aware view. A brace that
# exists only inside a comment, a string, a char literal or a raw string
# therefore silently moves depth:
#   POSITIVE drift (a stray `{`) over-extends the skipper, so a PRODUCTION
#   hazard after the #[cfg(test)] module is silently skipped and never
#   flagged (hD1/hD3a/hD4/hD5a/hD7).
#   NEGATIVE drift (a stray `}`) releases the skipper EARLY, so a hazard that
#   is legitimately inside the #[cfg(test)] module becomes visible to the
#   hazard match and is FALSE-RED (hD2/hD3b/hD5b).
#
# What is live in THIS gate's 121-file scan set today (measured), vs. what is
# defensive: braces inside STRING literals are rampant (173 in
# compute_targets/elastic_static.rs alone, 26 in fdm_slice.rs, 16 in
# membrane_load.rs) — hD1/hD2/hD6/hD7 are real shapes. Block comments appear
# in 1 file — hD5a/hD5b are real shapes. Char literals with an unbalanced
# brace (hD3a/hD3b) and raw strings (hD4) do NOT occur anywhere in this scan
# set today (zero of each) — kept anyway because the lexer is a single
# left-to-right state machine that must handle strings, char literals and raw
# strings together or not at all, and because the scan set can grow. Do not
# copy task/5076's live-shape citations: that gate's 562-file scan set is a
# different tree.
#
# Same fixture harness as block (h) above (write_fixture / stage /
# _exits_with): one file overwritten per case, EXACT exit codes only.
# ===========================================================================
echo ""
echo "--- (hD): brace-depth drift from comments, strings and char literals ---"

# hD1 — a stray '{' in a test-assertion STRING over-extends the #[cfg(test)]
# skipper, silently swallowing the PRODUCTION hazard below the test module.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn renders_an_open_brace() {
        assert_eq!(render(), "{ open brace in a string");
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD1: a '{' inside a test-assertion STRING does not hide the production hazard below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hD2 — a stray '}' in a test-assertion STRING releases the skipper one line
# early, so the SECOND test's legitimately-in-module hazard is wrongly seen
# as production code and FALSE-REDs.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn reports_an_unclosed_block() {
        assert!(render_error().contains("expected }"));
    }

    #[test]
    fn sorts_with_fallback() {
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD2: a '}' inside a test-assertion STRING does not release the cfg(test) skipper early" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD3a — '{' CHAR LITERAL, positive drift: string blanking alone does not
# cover char literals, so '{' inflates depth exactly like hD1's string brace.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    const OPEN: char = '{';

    #[test]
    fn notes_the_delimiter() {
        assert_eq!(OPEN, '{');
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD3a: a '{' CHAR LITERAL does not hide the production hazard below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hD3b — '}' CHAR LITERAL, negative drift, in the `rest.find([',', '}'])`
# shape: releases the skipper early exactly like hD2's string brace.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_delimiter() {
        let rest = "a,b";
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        assert!(end > 0);
    }

    #[test]
    fn sorts_with_fallback() {
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD3b: a '}' CHAR LITERAL does not release the cfg(test) skipper early" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD4 — multi-line RAW STRING: r#"..."# spans lines and honours no escapes,
# so its unbalanced '{' must not hide the production hazard below it.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_an_unclosed_structure() {
        let source = r#"pub structure Bolt {
    diameter: 5mm
"#;
        assert!(parse(source).is_err());
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD4: a '{' inside a multi-line RAW STRING does not hide the production hazard below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hD5a — '/* { */' BLOCK COMMENT, positive drift.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn documents_the_open_brace() {
        /* { */
        assert!(true);
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD5a: a '{' inside a /* */ BLOCK COMMENT does not hide the production hazard below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hD5b — '/* } */' BLOCK COMMENT, negative drift, as the first statement of
# the test module; the SECOND test's hazard is legitimately in the same
# module.
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn documents_the_close_brace() {
        /* } */
        assert!(true);
    }

    #[test]
    fn sorts_with_fallback() {
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD5b: a '}' inside a /* */ BLOCK COMMENT does not release the cfg(test) skipper early" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD6 — MUST STAY GREEN: forbids the naive "strip // comments first, THEN
# count braces" reorder. A '//' inside a STRING is not a comment; truncating
# there would lose the '{' that follows it and would release the skipper
# early, false-REDing the SECOND test's legitimately-in-module hazard.
write_fixture <<'RS'
pub fn strip_comments(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn skips_comment_lines() {
        if "// not a comment".trim_start().starts_with("//") {
            assert!(true);
        }
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD6: a '//' inside a STRING does not truncate the '{' after it (no false RED on a legitimate cfg(test) hazard)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD7 — '#[cfg(test)]' and 'mod … {' tokens INSIDE STRINGS must not arm the
# skipper. test_base would be 0, so a wrongly-armed skipper never releases
# and swallows every production line after it, including the hazard.
write_fixture <<'RS'
pub const SCAFFOLD: [&str; 2] = [
    "#[cfg(test)]",
    "mod tests {",
];

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD7: a '#[cfg(test)] mod … {' pair inside STRINGS does not arm the test-module skipper" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hD8 — POSITIVE CONTROL for the whole hD block, so it cannot be satisfied by
# the gate simply becoming unconditionally RED: every hostile shape above, in
# ONE test module, with the hazard left legitimately inside it.
write_fixture <<'RS'
pub fn nothing() {}

#[cfg(test)]
mod tests {
    #[test]
    fn every_hostile_shape_at_once() {
        assert_eq!(render(), "{ open brace in a string");
        let close = '}';
        let source = r#"pub structure Bolt {
    diameter: 5mm
"#;
        /* } */
        /* { */
        if "// not a comment".starts_with("//") {
            assert!(source.is_empty() || close == '}');
        }
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD8: every hostile shape at once, hazard legitimately inside the cfg(test) module — gate stays GREEN" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hD9 — PORTABILITY (a real assertion, not decorative): the lexer must be
# POSIX awk only (substr/index/length/match, extra params as locals, no
# gensub), so the ported gate agrees under mawk as well as under the default
# gawk. Re-runs hD1's and hD4's fixtures with `awk` PATH-shadowed to mawk.
#
# GUARD FORM MATTERS (mirrors tests/infra/test_occt_gated_scope.sh:246-254):
# the skip is expressed OUTSIDE the assert, not as an in-body `exit 0` —
# test_helpers.sh's assert() counts any zero-exit checker as a PASS, so an
# in-body guard would still increment PASS while checking nothing. The
# outside form makes a mawk-less host report an explicit SKIP instead.
if command -v mawk >/dev/null 2>&1; then
    MAWK_SHIM="$DET_TMP/mawk-shim"
    mkdir -p "$MAWK_SHIM"
    ln -s "$(command -v mawk)" "$MAWK_SHIM/awk"

    write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn renders_an_open_brace() {
        assert_eq!(render(), "{ open brace in a string");
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
    stage
    assert "hD9: hD1's fixture agrees under PATH-shadowed mawk (POSIX-awk-only lexer)" \
        _exits_with 1 env PATH="$MAWK_SHIM:$PATH" bash "$GATE" --repo-root "$FIX"

    write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_an_unclosed_structure() {
        let source = r#"pub structure Bolt {
    diameter: 5mm
"#;
        assert!(parse(source).is_err());
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
    stage
    assert "hD9: hD4's fixture agrees under PATH-shadowed mawk (POSIX-awk-only lexer)" \
        _exits_with 1 env PATH="$MAWK_SHIM:$PATH" bash "$GATE" --repo-root "$FIX"
else
    echo "  SKIP: hD9 (mawk portability) — mawk is not on PATH on this host."
fi

# hD10/hD11/hD12 — the char-literal-vs-lifetime branch is the lexer's most
# heuristic code (the escape-length cap and the one-char lookahead). The
# header claims &'static str / <'a, 'b> / '\x41' / '\u{7f}' are all handled,
# but until now only plain '{' / '}' (hD3a/hD3b) exercised it. None of these
# three shapes are live in this gate's scan set today; they pin the claim so
# a future tweak to the cap or the lookahead regresses loudly, not silently
# (a lifetime misparsed as an unterminated char literal is exactly the
# state-desync that fails toward GREEN — see hG above).
write_fixture <<'RS'
#[cfg(test)]
mod tests {
    struct P<'a, 'b> {
        x: &'a str,
        y: &'b str,
    }

    fn h<'a>() -> &'static str {
        "ok"
    }

    #[test]
    fn uses_lifetimes() {
        let _p = P { x: "a", y: "b" };
        assert_eq!(h(), "ok");
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD10: lifetimes (<'a, 'b>, &'static str) are not mistaken for char literals and do not hide the hazard below" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn brace_escape() {
        let brace = '\u{7b}';
        assert!(brace.is_ascii());
    }
}

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hD11: a '\\u{7b}' UNICODE-ESCAPE char literal's own brace does not hide the hazard below the test module" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

write_fixture <<'RS'
#[cfg(test)]
mod tests {
    #[test]
    fn escaped_quote() {
        let q = '\'';
        assert_eq!(q, '\'');
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hD12: an escaped self-quote char literal ('\\'') does not desync the lexer; hazard legitimately inside stays exempt" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hE — the #[cfg(test)] skipper REQUIRES a `mod` (task 6031's second defect,
# deliberately left unfixed by the lexer-only port above).
#
# A bare `#[cfg(test)] use …;` / `#[cfg(test)] fn …` declares no module and
# opens no brace of its own, so `pending_test` stays armed until the NEXT
# brace-opening line — which can be an unrelated PRODUCTION item, silently
# skipped wholesale. Verified live in this gate's own scan set today: exactly
# three files carry a bare non-`mod` `#[cfg(test)]` item —
# crates/reify-eval/src/compute_targets/elastic_static.rs:4243
# (`#[cfg(test)] pub(crate) fn extract_pressure_loads`),
# crates/reify-solver-elastic/src/elements/degenerate_shell.rs:657
# (`#[cfg(test)] fn degenerate_assumed_covariant_shear_b`), and
# crates/reify-solver-elastic/src/shell_assembly.rs:1298
# (`#[cfg(test)] pub(crate) fn tilted_q_for_shell_tests`).
#
# Same fixture harness as blocks (h)/(hD) above.
# ===========================================================================
echo ""
echo "--- (hE): the #[cfg(test)] skipper requires a 'mod' ---"

# hE1 — a bare '#[cfg(test)] use …;' arms the skipper, which then swallows
# the NEXT brace-opening line wholesale — here an unrelated PRODUCTION fn.
write_fixture <<'RS'
#[cfg(test)]
use std::cmp::Ordering;

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hE1: a bare '#[cfg(test)] use …;' does not swallow the next production item" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hE2 — SEMANTIC-NARROWING PIN: after this port only #[cfg(test)] mod BODIES
# are exempt, so a hazard inside a bare #[cfg(test)] fn (no enclosing mod)
# becomes visible. This is a DELIBERATE tightening, not a regression: verified
# safe on the live tree — no such site exists there (the ported gate still
# flags 0 of 121 files). `// nan-safe:allow — <reason>` is the sanctioned
# relief valve should one ever appear legitimately. Pinned explicitly so a
# future reader does not "fix" this back into an over-broad skipper.
write_fixture <<'RS'
pub fn nothing() {}

#[cfg(test)]
fn helper(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hE2: a hazard inside a bare #[cfg(test)] fn (no enclosing mod) is now visible (deliberate narrowing)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hE3 — CONTROL: the identical hazard inside a proper #[cfg(test)] mod stays
# exempt. Without this, hE1/hE2 would be satisfiable by simply deleting the
# test-module skipper outright, which would false-RED every legitimate test
# module in the scan set.
write_fixture <<'RS'
pub fn nothing() {}

#[cfg(test)]
mod tests {
    fn helper(v: &mut Vec<f64>) {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hE3: the identical hazard inside a proper #[cfg(test)] mod stays exempt" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hE4 — REGRESSION PIN (amendment): `mod tests` with the `{` on the NEXT
# line, not the same line. Legal Rust, and this repo has no rustfmt gate
# forcing brace-on-same-line (CLAUDE.md: ~5913 unformatted hunks across 685
# files). Before this amendment the arming check only ever looked at the
# CURRENT line's own text for `mod IDENT`, so a brace-only continuation line
# never re-declared `mod` and was wrongly treated as ordinary production
# code — a false RED. Verified empirically against the pre-amendment gate:
# exit 1 (should be 0).
write_fixture <<'RS'
#[cfg(test)]
mod tests
{
    #[test]
    fn sorts_with_fallback() {
        let mut v = Vec::<f64>::new();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
}
RS
stage
assert "hE4: '#[cfg(test)] / mod tests / {' (brace on its own line) still arms the skipper" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hE5 — REGRESSION PIN (amendment): guards the fix above against its own
# obvious failure mode. `mod tests;` is a COMPLETE, self-terminating
# declaration (an external module in another file) with no local body to
# skip. A naive "remember we saw `mod IDENT` and let some later brace arm
# the skipper" fix — without also recognizing the statement-terminating `;`
# — would leave that memory dangling and wrongly swallow the NEXT unrelated
# production brace as if it were the (nonexistent) local body. Verified this
# fixture would regress to exit 0 (swallowed) under such a naive fix, even
# though the pre-amendment gate already gets this right (exit 1) since it
# never remembers `mod` across lines at all.
write_fixture <<'RS'
#[cfg(test)]
mod tests;

pub fn sort_all(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}
RS
stage
assert "hE5: a self-terminating 'mod tests;' (external file, no local body) does not swallow the next unrelated production item" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hF — an awk failure mid-scan surfaces as the documented exit 2
# (usage/internal error), never conflated with exit 1 (violation found).
#
# Under `set -euo pipefail`, a plain `out="$(awk ...)"` left unchecked lets
# `set -e` abort the script with awk's OWN exit status on failure, which for
# some failure modes (verified: PATH-shadowing awk to a stub that always
# exits 1) is 1 — silently indistinguishable from "found a violation", with
# nothing printed and files after the failing one never scanned.
# ===========================================================================
echo ""
echo "--- (hF): an awk failure mid-scan is exit 2, not silently conflated with exit 1 ---"
write_fixture <<'RS'
pub fn nothing() {}
RS
stage
AWK_FAIL_SHIM="$DET_TMP/awk-fail-shim"
mkdir -p "$AWK_FAIL_SHIM"
printf '#!/bin/sh\nexit 1\n' > "$AWK_FAIL_SHIM/awk"
chmod +x "$AWK_FAIL_SHIM/awk"
assert "hF1: an awk failure while scanning a CLEAN fixture is surfaced as exit 2, not exit 1" \
    _exits_with 2 env PATH="$AWK_FAIL_SHIM:$PATH" bash "$GATE" --repo-root "$FIX"
assert "hF1: the awk-failure path prints a diagnostic naming the file being scanned" \
    bash -c "env PATH='$AWK_FAIL_SHIM:$PATH' bash '$GATE' --repo-root '$FIX' 2>&1 | grep -q 'ERROR: awk failed while scanning'"

# ===========================================================================
# hG — lexer end-of-file self-consistency: an unbalanced state (unterminated
# string/raw-string/block-comment, or a brace depth that never returns to 0)
# fails SILENTLY toward GREEN today — every line past a desync lexes to the
# empty string and is dropped by `carried_in && code == "" next`, so the
# rest of the file goes unscanned while the gate still exits 0. Surfaced as
# a WARN on stderr (verdict-neutral: exit code is unaffected either way, so
# this cannot regress detection or the live tree's exit-0 verdict).
# ===========================================================================
echo ""
echo "--- (hG): lexer state left unbalanced at EOF is a loud WARN, not a silent pass ---"

write_fixture <<'RS'
pub fn nothing() {
RS
stage
assert "hG1: an unbalanced brace at EOF stays exit 0 (verdict-neutral — no hazard in this fixture)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"
assert "hG1: ...and prints a WARN naming the unbalanced state to stderr" \
    bash -c "bash '$GATE' --repo-root '$FIX' 2>&1 1>/dev/null | grep -q 'WARN:.*lexer state unbalanced at EOF'"

# Control: the live tree (all 121 in-scope files) has zero EOF drift
# (measured in the plan) — the WARN must not fire spuriously there.
assert "hG2: the WARN does not fire on the live, balanced tree" \
    bash -c "! bash '$GATE' --repo-root '$REPO_ROOT' 2>&1 1>/dev/null | grep -q 'WARN:.*lexer state unbalanced'"

# ===========================================================================
# hH — the `nan-safe:allow` escape must come from a real `//` comment, not
# merely APPEAR anywhere on the line. Matching raw $0 (as before this
# amendment) also matches inside a string or char literal, so a hazard on
# the same line as an unrelated string containing the token was wrongly
# suppressed — a false-GREEN vector in a gate whose whole value is that it
# cannot be talked out of a RED. Verified empirically against the
# pre-amendment gate: exit 0 (should be 1).
# ===========================================================================
echo ""
echo "--- (hH): the nan-safe:allow escape only counts from a real comment ---"

write_fixture <<'RS'
pub fn sort_all(v: &mut Vec<f64>) { let _m = "nan-safe:allow"; v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); }
RS
stage
assert "hH1: a 'nan-safe:allow' token inside a STRING literal does not suppress a real hazard on the same line" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# Control: the real, documented `//` comment form must keep working exactly
# as block (h) above already proves — re-asserted here for locality with
# the string-literal negative case.
write_fixture <<'RS'
fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); } // nan-safe:allow — guarded upstream
RS
stage
assert "hH2: CONTROL — a real trailing '// nan-safe:allow' comment still clears the site" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hI — an EMPTY scan set (no tracked .rs file matches any SCOPE_PATHSPECS —
# a crate rename, a module move, a repo reorg) must fail loudly, not exit 0
# vacuously: from the caller's side, "scanned nothing" and "scanned
# everything and found it clean" look identical unless the gate says so.
# Pre-existing (not introduced by this task's port), but this amendment
# pass is specifically a hardening sweep on this gate's failure modes.
#
# Uses its OWN throwaway repo, not the write_fixture/FIX harness above:
# write_fixture always targets crates/reify-fdm (a COVERED path), so an
# empty scan set needs a repo with no covered crate at all.
# ===========================================================================
echo ""
echo "--- (hI): an empty scan set is a loud error, not a silent vacuous pass ---"
EMPTY_FIX="$DET_TMP/empty-fixture"
mkdir -p "$EMPTY_FIX/crates/reify-unrelated-crate/src"
git -C "$EMPTY_FIX" init -q
git -C "$EMPTY_FIX" config user.email test@invalid.local
git -C "$EMPTY_FIX" config user.name test
cat > "$EMPTY_FIX/crates/reify-unrelated-crate/src/lib.rs" <<'RS'
pub fn nothing() {}
RS
git -C "$EMPTY_FIX" add -A
assert "hI1: a repo with no tracked .rs file matching SCOPE_PATHSPECS exits 2, not a silent 0" \
    _exits_with 2 bash "$GATE" --repo-root "$EMPTY_FIX"
assert "hI1: ...and prints a diagnostic naming the stale scope" \
    bash -c "bash '$GATE' --repo-root '$EMPTY_FIX' 2>&1 | grep -q 'ERROR: no tracked .rs files matched SCOPE_PATHSPECS'"

# ===========================================================================
# hJ — CHARACTERIZATION pin of an ACCEPTED OVER-FLAG (task 5159).
#
# WHAT IS PINNED: the gate flags the fragment `unwrap_or( … Ordering::Equal
# … )` even when the `Option<Ordering>` being defaulted comes from
# `Iterator::find` on an ALREADY-`total_cmp`-based comparator, where no NaN
# hazard exists. Flagged (hJ1); cleared by the escape (hJ2).
#
# PINNED AS ACCEPTED, NOT AS A DEFECT TO BE TIDIED UP. Why the matcher stays
# fragment-based, and why narrowing it is rejected, is recorded ONCE,
# canonically, in docs/prds/compute-fea-hardening.md "Resolved design
# decision 9" §G — read that before proposing a narrower regex. It is not
# restated here on purpose: three copies of one argument is the drift
# decision 9 exists to stop.
#
# The shape below is read verbatim from `impl Ord for SampledField` in
# crates/reify-ir/src/value.rs, where two live instances sit (the nested
# `cmp_floats` helper and the `axis_grids` leg). BOTH are outside the covered
# scope, so the gate is green on the real tree and this over-flag costs the
# tree nothing today — it would cost exactly two annotations if reify-ir were
# ever brought in scope.
# ===========================================================================
echo ""
echo "--- (hJ): the total_cmp().find().unwrap_or() over-flag is deliberate and escapable ---"

write_fixture <<'RS'
fn cmp_floats(a: &[f64], b: &[f64]) -> std::cmp::Ordering {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.total_cmp(y))
        .find(|o| !o.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}
RS
stage
assert "hJ1: the gate FLAGS a total_cmp-based comparator whose unwrap_or defaults an exhausted find (accepted over-flag, not the NaN hazard)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

write_fixture <<'RS'
fn cmp_floats(a: &[f64], b: &[f64]) -> std::cmp::Ordering {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.total_cmp(y))
        .find(|o| !o.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal) // nan-safe:allow — total_cmp already; unwrap_or defaults an exhausted find, not a partial_cmp
}
RS
stage
assert "hJ2: ...and the sanctioned response is the escape, which clears it" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hK — the KEEP-SCOPED decision, pinned executably (task 5159).
#
# These crates/modules are OUT of SCOPE_PATHSPECS by RECORDED DECISION, not
# by oversight. The scope RULE ("the physical/geometric numeric solve path"),
# the 2026-08-20 per-site census, why widening today is not honestly
# executable, and the widening trigger are recorded ONCE, canonically, in
# docs/prds/compute-fea-hardening.md "Resolved design decision 9" — go there,
# not to a paraphrase here.
#
# EXACTLY WHAT THIS BLOCK PINS — read this before citing it as evidence:
#   1. hK1-hK5: an UNGUARDED hazard planted at each excluded path is NOT
#      flagged. This detects ONE thing — a widening of SCOPE_PATHSPECS that
#      brings that path in scope. That is the intended tripwire: the gate's
#      pathspecs, decision 9 and these pins are meant to fail together, and a
#      reviewer seeing one flip should be sent to decision 9's census table
#      and its four follow-up tickets before approving.
#   2. hK1-hK5 (second assert each): the path still EXISTS as a tracked file
#      in the REAL repo. Every other assertion here runs against the
#      synthetic $FIX repo and would keep passing against a path that had
#      been renamed or deleted — pinning a string, not a site. This leg is
#      what makes a crate split or module move fail loudly instead of
#      silently hollowing the block out.
#
# WHAT IT DOES *NOT* PIN: general census drift. A NEW unguarded site
# appearing in an already-excluded file (which is how the 2026-07-09 census
# went stale) leaves every assertion here green. Nothing in this suite
# re-runs the census; decision 9's file-level record is the authority, and
# re-measuring is a human/task job. Do not read a green hK as "the census is
# still accurate".
#
# ANTI-VACUITY: hK6 writes the BYTE-IDENTICAL hazard text to a COVERED path
# and requires exit 1. Without it, a typo in $HK_HAZARD would make all five
# exclusion assertions pass for the wrong reason and the pin would be
# worthless — the same failure mode block (hI) guards at the scan-set level.
# ===========================================================================
echo ""
echo "--- (hK): deliberately-excluded domains stay OUT of the gate's scan set ---"

# The one hazard line every hK assertion uses, written byte-identically to
# both the excluded paths (must NOT flag) and the covered path (MUST flag).
HK_HAZARD='pub fn s(v: &mut Vec<f64>) { v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); }'

# write_hazard_at <repo-relative-path> — plant $HK_HAZARD at that path.
#
# Two-file writer: the COVERED crates/reify-fdm/src/lib.rs is always
# (re)created first, clean, so the scan set is never empty. Without it the
# gate would exit 2 per the (hI) contract and every "not flagged" assertion
# below would be testing the wrong thing — passing because the gate errored
# out, not because the path is out of scope. When <path> IS the covered file
# (hK8) it simply overwrites that clean content with the hazard.
write_hazard_at() {
    local rel="$1"
    rm -rf "$FIX/crates"
    mkdir -p "$FIX/crates/reify-fdm/src"
    printf 'pub fn covered_but_clean() {}\n' > "$FIX/crates/reify-fdm/src/lib.rs"
    mkdir -p "$FIX/$(dirname "$rel")"
    printf '%s\n' "$HK_HAZARD" > "$FIX/$rel"
    stage
}

# The deliberately-excluded set, as measured on 2026-08-20 and SHRUNK from
# seven to five by task #6376, which hardened reify-stdlib's analysis.rs and
# matrix.rs and brought that crate INTO scope (see block (hL) below).
# reify-constraints stays excluded — it is still class A pending its own
# follow-up ticket, so this was a HALF-fired widening trigger, not a
# discharged one. Line numbers drift (that is the whole point of this block);
# the FILE-level record is what is authoritative here and in decision 9.
HK_EXCLUDED=(
    'crates/reify-constraints/src/solver.rs'
    'crates/reify-eval/src/engine_build.rs'
    'crates/reify-eval/src/persistent_cache.rs'
    'crates/reify-eval/src/warm_pool.rs'
    'crates/reify-ir/src/value.rs'
)

_hk_i=0
for _hk_rel in "${HK_EXCLUDED[@]}"; do
    _hk_i=$((_hk_i + 1))
    write_hazard_at "$_hk_rel"
    assert "hK$_hk_i: $_hk_rel is OUT of scope by decision — an UNGUARDED hazard there is not flagged" \
        _exits_with 0 bash "$GATE" --repo-root "$FIX"
    # REAL-TREE leg: the assertion above is entirely synthetic ($FIX), so it
    # would keep passing against a path that no longer exists. Fail loudly on
    # a rename/move/delete instead, and send the reader to decision 9 to
    # re-site the row rather than to delete the pin.
    assert "hK$_hk_i: ...and $_hk_rel still exists as a tracked file in the real tree (census path not renamed — re-site it in decision 9, do not drop the pin)" \
        git -C "$REPO_ROOT" ls-files --error-unmatch "$_hk_rel"
done

# hK6 — ANTI-VACUITY CONTROL. Same bytes, covered path, must flag.
write_hazard_at 'crates/reify-fdm/src/lib.rs'
assert "hK6: CONTROL — the byte-identical hazard IS flagged at a COVERED path (so hK1-hK5 are not passing vacuously)" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hK7 — the covered scope is still wired end-to-end: covered file, clean, green.
write_fixture <<'RS'
pub fn covered_but_clean() {}
RS
stage
assert "hK7: CONTROL — a clean covered file alone is exit 0 (scan set non-empty and scanned)" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# ===========================================================================
# hL — reify-stdlib is now COVERED (task #6376), the positive of (hK).
#
# (hK) can only ever detect that a path is OUT of scope. Widening the gate
# without this block would move reify-stdlib from "pinned excluded" to
# "pinned by nothing" — a silent narrowing later would go unnoticed. This is
# the other half: the crate is in scope AND the escape still works there.
#
# Why the crate moved, the half-fired widening trigger, and what stays
# excluded are recorded ONCE, canonically, in
# docs/prds/compute-fea-hardening.md "Resolved design decision 9" — go there,
# not to a paraphrase here.
# ===========================================================================
echo ""
echo "--- (hL): reify-stdlib is now IN the gate's scan set (task #6376) ---"

# hL1 — the path is genuinely in scope: an unguarded hazard there IS flagged.
write_hazard_at 'crates/reify-stdlib/src/analysis.rs'
assert "hL1: crates/reify-stdlib/src/analysis.rs is COVERED — an UNGUARDED hazard there IS flagged" \
    _exits_with 1 bash "$GATE" --repo-root "$FIX"

# hL2 — and the escape still works in the newly covered crate. Same
# two-file shape as write_hazard_at (clean covered file first, so the scan
# set is never empty and exit 2 can't fake a pass), with the annotation
# appended. stage() is called explicitly: the gate is `git ls-files`-hermetic,
# so an unstaged fixture would pass vacuously.
rm -rf "$FIX/crates"
mkdir -p "$FIX/crates/reify-fdm/src"
printf 'pub fn covered_but_clean() {}\n' > "$FIX/crates/reify-fdm/src/lib.rs"
mkdir -p "$FIX/crates/reify-stdlib/src"
printf '%s // nan-safe:allow — guarded upstream\n' "$HK_HAZARD" \
    > "$FIX/crates/reify-stdlib/src/analysis.rs"
stage
assert "hL2: ...and // nan-safe:allow still clears a site in the newly covered crate" \
    _exits_with 0 bash "$GATE" --repo-root "$FIX"

# hL3 — REAL-TREE leg, mirroring (hK)'s: the covered path still exists.
assert "hL3: ...and crates/reify-stdlib/src/analysis.rs still exists as a tracked file in the real tree" \
    git -C "$REPO_ROOT" ls-files --error-unmatch 'crates/reify-stdlib/src/analysis.rs'

test_summary
