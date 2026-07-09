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
# via the escape below. `f64::total_cmp` produces no `unwrap_or`, so the
# sanctioned fix is never flagged.
#
# COVERED SCOPE (exactly — mirrors the task 5093 spec): the FEA numeric crates
# reify-solver-elastic, reify-kernel-gmsh, reify-fdm, reify-shell-extract,
# reify-mesh-morph, plus reify-eval's compute_targets/ and modal_ops.rs.
#
# EXCLUDED:
#   - comments: the `//…` tail is stripped before matching, so doc-comment
#     mentions of the hazard (e.g. interpolation.rs, modal_ops.rs rationale
#     comments) are ignored;
#   - test code: `tests/` dirs (by path) and `#[cfg(test)]` modules (brace-depth
#     tracked, best-effort) — a synthetic negative-example fixture asserting the
#     old panic-prone behavior is thus permitted;
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
#   2  usage / not-a-git-work-tree error

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

# Covered-scope pathspecs. Single-star git pathspecs are NOT path-boundary-aware,
# so 'crates/reify-fdm/*.rs' matches every tracked .rs at any depth under it.
SCOPE_PATHSPECS=(
    'crates/reify-solver-elastic/*.rs'
    'crates/reify-kernel-gmsh/*.rs'
    'crates/reify-fdm/*.rs'
    'crates/reify-shell-extract/*.rs'
    'crates/reify-mesh-morph/*.rs'
    'crates/reify-eval/src/compute_targets/*.rs'
    'crates/reify-eval/src/modal_ops.rs'
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

# Per-file scan. The awk pass, for each line, in order:
#   1. tracks brace depth and skips lines inside a #[cfg(test)] module;
#   2. honors the same-line `nan-safe:allow` escape (mirrors ptodo:allow §6.8);
#   3. strips the //… comment tail, then flags the
#      unwrap_or(…Ordering::Equal…) fragment as file:line: <source>.
all=""
for f in "${_files[@]}"; do
    out="$(awk '
        {
            # Brace counts on this line (throwaway copies so $0 stays intact).
            c = $0; n_open  = gsub(/[{]/, "x", c)
            c = $0; n_close = gsub(/[}]/, "x", c)

            # --- #[cfg(test)] module skipping (best-effort brace tracking) ---
            if (in_test) {
                depth += n_open - n_close
                if (depth <= test_base) in_test = 0
                next
            }
            if ($0 ~ /#\[cfg\(test\)\]/) pending_test = 1
            if (pending_test && n_open > 0) {
                in_test = 1
                test_base = depth        # depth BEFORE this block opened
                depth += n_open - n_close
                pending_test = 0
                next
            }
            depth += n_open - n_close

            # --- inline escape (same-line), mirrors ptodo:allow §6.8 ---
            if ($0 ~ /nan-safe:allow/) next

            # --- strip //… comment tail, then match the hazard fragment ---
            code = $0
            sub(/\/\/.*/, "", code)
            if (code ~ /unwrap_or\([^)]*Ordering::Equal/) {
                printf "%s:%d: %s\n", FILENAME, FNR, $0
            }
        }
    ' "$REPO_ROOT/$f")"
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
