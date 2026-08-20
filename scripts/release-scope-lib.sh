#!/usr/bin/env bash
# scripts/release-scope-lib.sh — shared release-sensitive crate-set logic.
#
# This library is the SINGLE implementation of "which workspace crates have tests
# that behave differently between debug and release builds". It is sourced by both:
#   - scripts/verify.sh                          (decides the release-scoped test pass)
#   - tests/infra/test_release_scoped_scope.sh   (drift catcher)
# so the declared set and the grep-derived set each have exactly one definition —
# divergence between the verify entrypoint and the drift test becomes impossible
# by construction.
#
# Designed to be sourced, not executed directly:
#   source "$(dirname "${BASH_SOURCE[0]}")/release-scope-lib.sh"
#
# Provides:
#   release_declared_set    prints the declared release-sensitive crates (one per
#                           line), reading scripts/release-sensitive-crates.txt with
#                           comments/blank lines stripped and whitespace trimmed.
#   release_sensitive_set   prints the grep-derived release-sensitive workspace crate
#                           names (sorted-unique, one per line).
#
# Environment:
#   RELEASE_SENSITIVE_CRATES_FILE  Override the declared-list path. Defaults to
#                                  release-sensitive-crates.txt next to this library.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_RELEASE_SCOPE_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_RELEASE_SCOPE_LIB_SOURCED=1

_RELEASE_SCOPE_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Repo root is one level up from scripts/.
_RELEASE_SCOPE_LIB_REPO_ROOT="$(cd "$_RELEASE_SCOPE_LIB_DIR/.." && pwd)"
RELEASE_SENSITIVE_CRATES_FILE="${RELEASE_SENSITIVE_CRATES_FILE:-$_RELEASE_SCOPE_LIB_DIR/release-sensitive-crates.txt}"

# Shared file→crate mapping + reverse-dependency closure (affected_crates →
# _reverse_closure → _reify_compile_closure). Sourced here so
# release_delta_requires_pass can reuse the SINGLE compile-closure implementation
# rather than carry a second copy (K8 — no lock-step duplicate; drift caught by
# the test_affected_crates_lib.sh family). Mirrors affected-crates-lib.sh's own
# existence-guarded source of occt-scope-lib.sh; every library on this chain has a
# source guard, so this is an idempotent no-op under verify.sh's dual sourcing and
# under test_release_scoped_scope.sh (which also sources occt-scope-lib.sh directly).
[ -f "$_RELEASE_SCOPE_LIB_DIR/affected-crates-lib.sh" ] || { echo "release-scope-lib.sh: ERROR — scripts/affected-crates-lib.sh not found next to release-scope-lib.sh" >&2; return 1; }
# shellcheck source=scripts/affected-crates-lib.sh
source "$_RELEASE_SCOPE_LIB_DIR/affected-crates-lib.sh"

# release_declared_set — print the declared release-sensitive crate list, one crate
# per line, with comment lines (^\s*#) and blank lines removed and surrounding
# whitespace trimmed. Mirrors occt_declared_set in scripts/occt-scope-lib.sh.
release_declared_set() {
    if [ ! -f "$RELEASE_SENSITIVE_CRATES_FILE" ]; then
        echo "ERROR: release-sensitive-crates.txt not found at $RELEASE_SENSITIVE_CRATES_FILE" >&2
        return 1
    fi
    grep -v '^\s*#' "$RELEASE_SENSITIVE_CRATES_FILE" \
        | grep -v '^\s*$' \
        | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

# release_sensitive_set — derive the ACTUAL release-sensitive set by grepping for
# the three release-sensitivity mechanisms over crates/ and gui/src-tauri/.
#
# Mechanism A: cfg_attr(debug_assertions, ignore ...) — tests ignored in debug,
#   exercised only in release.  The ignore token may be bare or followed by
#   "= reason", so the pattern stops at "ignore" without requiring a closing paren.
#   Heavy tests in reify-solver-elastic and reify-eval use this mechanism.
#
# Mechanism B: cfg(not(debug_assertions)) — code only compiled in release
#   (debug_assert! calls are elided).  Crates using this mechanism include
#   reify-eval, reify-core, reify-expr, reify-runtime, reify-stdlib, reify-gui.
#
# Mechanism C: runtime cfg!(debug_assertions) — tests that assert different outcomes
#   in debug vs release via an inline macro expression (not a compile-time attribute).
#   Example: diagnostics.rs:511 in reify-mesh-morph asserts outcome.is_err() ==
#   cfg!(debug_assertions), exercising a release-only no-op path only in release.
#   Pattern: '^[^/]*cfg!(debug_assertions)' — the macro must appear before the first
#   '/' on the line, which excludes //, ///, //! comment lines while still catching
#   mid-line uses like 'if cfg!(debug_assertions)' and 'cfg!(debug_assertions),'.
#
# Mechanisms A+B are ANCHORED at line start (optional whitespace then '#[cfg...') to
# exclude doc-comment false positives (e.g. //! lines describing these attributes).
# A line beginning with whitespace then '#[cfg...' is an attribute; a line
# beginning with '//' is a comment and is never matched.
#
# File-to-crate mapping:
#   crates/<dir>/...  → <dir>  (package name equals directory name for all crates/)
#   gui/src-tauri/... → reify-gui
release_sensitive_set() {
    local pat_a='^\s*#\[cfg_attr\(debug_assertions,\s*ignore'
    local pat_b='^\s*#\[cfg\(not\(debug_assertions\)\)\]'
    local pat_c_pos='^[^/]*cfg!\(debug_assertions\)'
    local pat_c_neg='^[^/]*cfg!\(not\(debug_assertions\)\)'
    local repo_root="$_RELEASE_SCOPE_LIB_REPO_ROOT"

    {
        # Mechanism A: heavy tests gated behind cfg_attr(debug_assertions, ignore ...)
        grep -rlE "$pat_a" --include='*.rs' \
            "$repo_root/crates" "$repo_root/gui/src-tauri" 2>/dev/null || true
        # Mechanism B: cfg(not(debug_assertions)) — release-only fallback code
        grep -rlE "$pat_b" --include='*.rs' \
            "$repo_root/crates" "$repo_root/gui/src-tauri" 2>/dev/null || true
        # Mechanism C: runtime cfg!(debug_assertions) / cfg!(not(debug_assertions))
        # profile branching — macro must appear before any '/' on the line (excludes
        # comment lines).
        grep -rlE "$pat_c_pos" --include='*.rs' \
            "$repo_root/crates" "$repo_root/gui/src-tauri" 2>/dev/null || true
        grep -rlE "$pat_c_neg" --include='*.rs' \
            "$repo_root/crates" "$repo_root/gui/src-tauri" 2>/dev/null || true
    } | while IFS= read -r file; do
        case "$file" in
            "$repo_root/crates/"*)
                local crate="${file#"$repo_root/crates/"}"
                crate="${crate%%/*}"
                echo "$crate"
                ;;
            "$repo_root/gui/src-tauri/"*)
                echo "reify-gui"
                ;;
        esac
    done | sort -u
}

# release_delta_requires_pass <changed-file>... — decide whether the merge-gate
# RELEASE nextest pass must RUN for a given merge delta, or may be SKIPPED because
# the delta touches no release-sensitive crate.
#
#   rc 0  REQUIRED   — the affected crate set intersects the declared
#                      release-sensitive set, OR is the fail-wide ALL sentinel,
#                      OR the declared set could not be read. Emit the release
#                      pass exactly as today.
#   rc 1  SKIPPABLE  — delta-clean: the affected crate set is non-ALL and disjoint
#                      from the release-sensitive set (or empty). The caller may
#                      omit the release pass and emit the frozen plan marker
#                      `RELEASE-PASS: skipped (delta-clean)` in its place.
#
# Affected-crate-set resolution (reuse chain — NO second closure, K8):
#   - REIFY_AFFECTED_CRATES_OVERRIDE set & non-empty => used verbatim. This is the
#     exact hermetic/operator idiom verify.sh's narrowing path uses
#     (scripts/verify.sh:986), letting the --print-plan fixture inject a
#     post-closure crate set with no git and no cargo.
#   - otherwise => affected_crates "$@" (scripts/affected-crates-lib.sh): file→crate
#     mapping (C4 global→ALL, C5 unmappable→ALL, docs/gui-src/tests-infra→skip) plus
#     the reverse-dependency closure via the single shared _reify_compile_closure
#     primitive. Called with no args it returns the empty set (no cargo), so an
#     empty override / empty delta is hermetically delta-clean.
#
# Fail-wide/fail-open ladder (§5.1): the ALL sentinel (C4 global / C5 unmappable)
# and an unreadable declared set both resolve to REQUIRED; only a provably
# delta-clean affected set returns SKIPPABLE. Always safe under `set -euo pipefail`
# (affected_crates always returns 0; grep non-matches are consumed by `if`).
release_delta_requires_pass() {
    local affected
    if [ -n "${REIFY_AFFECTED_CRATES_OVERRIDE:-}" ]; then
        # Operator/testability override: verbatim whitespace/newline-separated set.
        affected="${REIFY_AFFECTED_CRATES_OVERRIDE}"
    else
        # Real run: map the derived merge-delta files to the reverse-closed crate set.
        affected="$(affected_crates "$@")"
    fi

    # Fail-wide: an ALL sentinel anywhere in the (word-split) affected set forces RUN.
    # shellcheck disable=SC2086
    if printf '%s\n' $affected | grep -qxF ALL; then
        return 0
    fi

    # Read the declared release-sensitive set once (single source of truth,
    # scripts/release-sensitive-crates.txt); fail-wide to RUN on any read failure.
    local declared
    declared="$(release_declared_set)" || return 0

    # Intersect: the first affected crate that is release-sensitive forces RUN.
    local _c
    # shellcheck disable=SC2086
    for _c in $affected; do
        if printf '%s\n' "$declared" | grep -qxF "$_c"; then
            return 0
        fi
    done

    # No release-sensitive crate affected (or empty affected set) => delta-clean => SKIP.
    return 1
}
