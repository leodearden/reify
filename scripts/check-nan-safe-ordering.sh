#!/usr/bin/env bash
# scripts/check-nan-safe-ordering.sh
#
# INV-FEA-3 regression guard (PRD docs/prds/compute-fea-hardening.md task F1,
# §Sketch §4, §Resolved design decision 6; task 5093).
#
# Rejects NEW NaN-unsafe ordering in the FEA numeric crates/modules. The hazard:
#     x.partial_cmp(y).unwrap_or(Ordering::Equal)
# `partial_cmp` returns None for a NaN operand; `.unwrap_or(Ordering::Equal)`
# then silently treats NaN as equal-to-everything, yielding an incorrect /
# unstable sort. The sanctioned fix is `f64::total_cmp` (see interpolation.rs,
# E3 / task 5090), which never produces this fragment.
#
# WHAT IS MATCHED: the silent-fallback FRAGMENT `unwrap_or( … Ordering::Equal … )`
# on a single, comment-stripped line. Matching the fragment — rather than the
# full `partial_cmp(...).unwrap_or(...)` chain on one line — is deliberate: it
# catches BOTH the single-line form AND the multi-line form where `.partial_cmp`
# and `.unwrap_or` sit on separate lines (e.g. modal_ops.rs
# `frequency_ascending_order`). In practice `Ordering::Equal` as an `unwrap_or`
# fallback is always a comparator fallback; a legitimately-guarded site opts out
# via the escape below.
#
# ACCEPTED OVER-FLAG (measured, task 5159). This header used to claim that
# "`f64::total_cmp` produces no `unwrap_or`, so the sanctioned fix is never
# flagged". THAT IS FALSE: the fragment match also flags a comparator built
# the sanctioned way, when the `Option<Ordering>` being defaulted comes from
# `find`/`max_by`/`min_by` instead of from `partial_cmp`:
#     .map(|(x, y)| x.total_cmp(y)).find(|o| !o.is_eq()).unwrap_or(Equal)
# Live instances: `impl Ord for SampledField` in crates/reify-ir/src/value.rs
# (two — the nested `cmp_floats` helper and the `axis_grids` leg), both
# OUTSIDE the covered scope below, so the gate is green on the real tree.
#
# The over-flag is RETAINED DELIBERATELY — do not "tidy" the matcher. The
# reasoning is recorded once, canonically, in
# docs/prds/compute-fea-hardening.md "Resolved design decision 9" §G; it is
# NOT restated here, because three copies of one argument is the drift the
# same decision exists to stop. Behaviour you need at this file: the site is
# flagged, and `// nan-safe:allow — <reason>` is the sanctioned response.
# Pinned by block (hJ) of tests/infra/test_nan_safe_ordering_guard_wired.sh.
#
# COVERED SCOPE (exactly): the FEA numeric crates reify-solver-elastic,
# reify-kernel-gmsh, reify-fdm, reify-shell-extract, reify-mesh-morph, plus
# reify-eval's compute_targets/ and modal_ops.rs — the task 5093 spec — AND
# reify-stdlib, added by task #6376 once its four class-A sites were hardened
# and its three class-B sites annotated, AND reify-constraints, added by task
# #6377 once `eval_objective_set` was made to fail closed on a non-finite
# accumulator. Those two tasks are the two halves of decision 9's widening
# trigger; with #6377 landed the trigger is FULLY fired.
#
# THE RULE behind that list (so a new crate can be judged, not guessed): the
# covered scope is the PHYSICAL/GEOMETRIC NUMERIC SOLVE PATH. Cache-eviction
# scores, warm-pool cost ordering, version/event ordering and IR `Value`
# ordering are deliberately OUT — a mis-sorted eviction candidate costs a
# cache miss, a mis-sorted principal stress is a wrong engineering answer.
#
# The excluded set, the per-site census behind the call, and the condition
# under which the scope widens are recorded ONCE, canonically, in
# docs/prds/compute-fea-hardening.md "Resolved design decision 9" (task
# 5159) — read it before touching SCOPE_PATHSPECS. Do NOT add or remove an
# entry without updating decision 9 AND the (hK) exclusion pins in
# tests/infra/test_nan_safe_ordering_guard_wired.sh — the three are meant to
# fail together.
#
# WARNING: this scope is NARROWER than INV-FEA-3's registry wording used to
# suggest ("numeric crates"), and decision 9's 2026-08-20 census found
# genuinely UNGUARDED sites outside it. Only reify-eval's engine_build.rs now
# remains there, owned by filed follow-up hardening and NOT by this gate — do
# not read this gate's green as evidence that it is safe. (reify-stdlib was on
# that list until task #6376 hardened it, and reify-constraints until task
# #6377 did; both are now in scope above.)
#
# PRODUCTION-CODE VIEW: each raw line is matched in its production-code view —
# comments dropped, string/char/raw-string contents blanked, braces counted on
# that lexed view, test-gated `mod` bodies skipped — produced by the shared
# lexer in scripts/lib_rust_production_view.sh, whose header documents the
# mechanism. A file whose lexer state is unbalanced at EOF gets a
# verdict-neutral `WARN: … lexer state unbalanced at EOF` on stderr; promoting
# it to a hard failure once that is trusted tree-wide is a follow-up.
#
# Measured WHEN THE LEXED VIEW LANDED, on the 121-file scan set of the day (task
# 5093/5159 era — the scan set has grown twice since; do NOT read 121 as a
# current figure, and do not inline a fresh one here either, because it goes
# stale on every widening. The authoritative count and its measurement history
# live in docs/prds/compute-fea-hardening.md decision 9, "Measured widening
# cost"): no file's brace-depth bookkeeping ended the file mid-drift (both
# views returned depth to 0 by EOF), but the per-line view diverged from the
# old raw-$0 view on 508 lines across 40 of those 121 files (worst:
# reify-fdm/src/toolpath.rs, 187 lines) — so the bookkeeping was drifting
# mid-file even though it happened to re-balance by EOF. The fix was
# verdict-neutral on the tree of the day: both the old and the lexed view
# flagged 0 sites across all 121 files. This closes a latent hazard, not a
# live bug. The old raw-$0 view no longer exists in this gate, so the
# divergence half of that measurement is historical and not re-measurable;
# the balance half IS, and was re-measured on 2026-09-01 against the current
# (post-#6376/#6377) scan set — every file still ends balanced, no WARN.
#
# EXCLUDED:
#   - comments and string literals, per the PRODUCTION-CODE VIEW above;
#   - test code: `tests/` dirs (by path) and test-gated `mod` BODIES —
#     `#[cfg(test)]`, `#[cfg(any(test, …))]`, `#[cfg(all(test, …))]`; the
#     arming rule and its limits live in scripts/lib_rust_production_view.sh
#     (brace-depth tracked, best-effort; a `mod IDENT` declaration must be
#     seen before the block's opening brace — not necessarily on the same
#     line, e.g. `mod tests` then `{` on the next line is honored too — so a
#     bare `#[cfg(test)] fn` or `#[cfg(test)] use …;` is NOT exempt) — a
#     synthetic negative-example fixture asserting the old panic-prone
#     behavior is thus permitted inside a real test module. A legitimate
#     test-only helper that is not in a `mod` needs the `// nan-safe:allow`
#     escape below instead;
#   - escaped sites: any line carrying the inline escape
#         // nan-safe:allow — <reason>
#     mirroring reify-audit's `// ptodo:allow` convention (§6.8). Annotate a
#     guarded happy-path sort (finiteness pre-checked upstream, so partial_cmp
#     never returns None) with the escape and a one-line rationale.
#
# HERMETIC SOURCE SET: `git ls-files` lists only tracked files, so untracked
# build artifacts never enter the scan (mirrors check_event_inventory.sh).
#
# Usage: scripts/check-nan-safe-ordering.sh [--repo-root <dir>]
# Exit codes:
#   0  clean — no unguarded matches
#   1  at least one unguarded match (each printed as file:line: <source>)
#   2  usage / not-a-git-work-tree error, an empty scan set (SCOPE_PATHSPECS
#      matched nothing), the shared lexer lib could not be loaded, or an awk
#      failure while scanning

set -euo pipefail

REPO_ROOT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo-root) REPO_ROOT="${2:-}"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--repo-root <dir>]"
            exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$REPO_ROOT" ]]; then
    REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
fi
if [[ -z "$REPO_ROOT" ]] || ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ERROR: not a git work tree: ${REPO_ROOT:-<cwd>}" >&2
    exit 2
fi

# The shared lexer, resolved beside THIS script (never the CWD or --repo-root:
# the gate scans fixture repos that have no scripts/). A failed load is "could
# not scan", exit 2; a bare `source` failing under set -e would exit 1, which
# reads as "violation found".
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_rust_production_view.sh
if ! source "$SCRIPT_DIR/lib_rust_production_view.sh" || [[ -z "${RUST_PRODUCTION_VIEW_AWK:-}" ]]; then
    echo "ERROR: cannot load the shared Rust lexer lib: $SCRIPT_DIR/lib_rust_production_view.sh" >&2
    exit 2
fi

# Covered-scope pathspecs. Single-star git pathspecs are NOT path-boundary-aware,
# so 'crates/reify-fdm/*.rs' matches every tracked .rs at any depth under it.
#
# Scope rule: the physical/geometric numeric solve path (see COVERED SCOPE in
# the header). Adding or removing an entry here requires updating
# docs/prds/compute-fea-hardening.md "Resolved design decision 9" and the
# (hK) pins in tests/infra/test_nan_safe_ordering_guard_wired.sh in the same
# change — those pins assert the current exclusions and will fail if this
# list moves without them.
SCOPE_PATHSPECS=(
    'crates/reify-solver-elastic/*.rs'
    'crates/reify-kernel-gmsh/*.rs'
    'crates/reify-fdm/*.rs'
    'crates/reify-shell-extract/*.rs'
    'crates/reify-mesh-morph/*.rs'
    'crates/reify-eval/src/compute_targets/*.rs'
    'crates/reify-eval/src/modal_ops.rs'
    'crates/reify-stdlib/*.rs'
    'crates/reify-constraints/*.rs'
)

# Tracked .rs sources in scope, minus integration-test dirs (tests/ excluded per
# spec; inline #[cfg(test)] handled inside the awk pass below).
_files=()
while IFS= read -r -d '' _f; do
    case "$_f" in
        */tests/*) continue ;;
    esac
    _files+=("$_f")
done < <(git -C "$REPO_ROOT" ls-files -z -- "${SCOPE_PATHSPECS[@]}" 2>/dev/null)

# An empty scan set (a crate rename, a module move, a repo reorg that no
# longer matches SCOPE_PATHSPECS) must fail loudly, not exit 0 vacuously —
# a gate that scans nothing looks identical, from the caller's side, to a
# gate that scanned everything and found it clean.
if [[ ${#_files[@]} -eq 0 ]]; then
    echo "ERROR: no tracked .rs files matched SCOPE_PATHSPECS — scope is stale?" >&2
    exit 2
fi

# Per-file scan, appended to the shared production view. In order:
#   1. honor the same-line `nan-safe:allow` escape (mirrors ptodo:allow §6.8);
#   2. flag the unwrap_or(…Ordering::Equal…) fragment as file:line: <source>.
all=""
for f in "${_files[@]}"; do
    # Checked explicitly (rather than left to `set -e`) so a failing awk gets
    # a diagnostic and the documented exit 2 (usage/internal error) — under
    # plain `set -e` propagation a failing `out="$(awk ...)"` would abort the
    # script with awk's own exit status, which for some awk failure modes is
    # 1, indistinguishable from "found a violation" (verified: PATH-shadowing
    # awk to a stub that always exits 1 makes the pre-fix gate exit 1 on a
    # CLEAN fixture, silently, with nothing printed).
    if ! out="$(awk "$RUST_PRODUCTION_VIEW_AWK"'
        {
            # --- inline escape (same-line), mirrors ptodo:allow §6.8 ---
            # Matched against comment_tail (the dropped `//…` text that
            # _strip_line stashed), NOT `code` and NOT raw $0. `code` is
            # wrong because the escape lives in a `//` comment, which the
            # lexer drops — matching `code` would silently kill every
            # escape. Raw $0 is wrong the OTHER way: it also contains any
            # string/char-literal content on the line, so a token that
            # merely *appears inside a string* — e.g. `let _m =
            # "nan-safe:allow";` — would wrongly suppress a real hazard on
            # that same line.
            if (comment_tail ~ /nan-safe:allow/) next

            if (code ~ /unwrap_or\([^)]*Ordering::Equal/) {
                # RAW $0 on purpose: this is human-readable violation output,
                # so it must show the source line as written, not the lexed
                # view.
                printf "%s:%d: %s\n", FILENAME, FNR, $0
            }
        }
    ' "$REPO_ROOT/$f")"; then
        echo "ERROR: awk failed while scanning $f" >&2
        exit 2
    fi
    [[ -n "$out" ]] && all+="$out"$'\n'
done

if [[ -n "${all//$'\n'/}" ]]; then
    printf '%s' "$all" | grep -v '^$' >&2
    n="$(printf '%s' "$all" | grep -c '.')"
    {
        echo ""
        echo "ERROR: $n NaN-unsafe ordering site(s) found (INV-FEA-3, task 5093)."
        echo "Fix with f64::total_cmp, or — if finiteness is already guaranteed"
        echo "at the sort — annotate the site with:"
        echo "    // nan-safe:allow — <why partial_cmp never returns None here>"
    } >&2
    exit 1
fi

exit 0
