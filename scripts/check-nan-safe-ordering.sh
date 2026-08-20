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
# flagged". That is false. The fragment match CAN flag a comparator that
# already uses the sanctioned `f64::total_cmp`, when the `Option<Ordering>`
# being defaulted comes from `find`/`max_by`/`min_by` instead of from
# `partial_cmp`:
#     .map(|(x, y)| x.total_cmp(y)).find(|o| !o.is_eq()).unwrap_or(Equal)
# There the `unwrap_or` only supplies the "compared equal elementwise" answer
# for an EXHAUSTED `find`, and no NaN hazard exists. Two such sites are live
# today — crates/reify-ir/src/value.rs:290 and :309 — both OUTSIDE the covered
# scope below, so the gate is green on the real tree.
#
# The over-flag is RETAINED DELIBERATELY. Narrowing the matcher to require a
# nearby `partial_cmp` would trade a fail-SAFE false positive (cost: one
# annotation) for a possible false NEGATIVE (cost: a silently-landed NaN
# hazard) — the wrong direction for a safety gate — and would also lose the
# multi-line form this fragment match exists to catch. `// nan-safe:allow —
# <reason>` is the sanctioned response; for this shape the reason is true and
# informative ("total_cmp already; unwrap_or defaults an exhausted find, not a
# partial_cmp") rather than a rubber stamp. Pinned by block (hJ) of
# tests/infra/test_nan_safe_ordering_guard_wired.sh; rationale recorded in
# docs/prds/compute-fea-hardening.md "Resolved design decision 9".
#
# COVERED SCOPE (exactly — mirrors the task 5093 spec): the FEA numeric crates
# reify-solver-elastic, reify-kernel-gmsh, reify-fdm, reify-shell-extract,
# reify-mesh-morph, plus reify-eval's compute_targets/ and modal_ops.rs.
#
# PRODUCTION-CODE VIEW: each raw line is reduced to its production code by a
# single LEFT-TO-RIGHT LEXER (`_strip_line`), not a comment-tail regex strip.
# Comment text is DROPPED; string, char-literal and raw-string CONTENTS are
# BLANKED with their delimiters kept (a blanked string reads exactly `""`).
# The lexer carries `/* … */` (which nests), `"…"` and `r#"…"#` state ACROSS
# lines, and tells a char literal from a lifetime, so `&'static str` and
# `<'a, 'b>` survive untouched. The brace counts that drive the #[cfg(test)]
# module skipper's `depth` are taken from that lexed view, never from the raw
# line: a brace that exists only inside a comment, a string, a char literal or
# a raw string must not move `depth`, or it silently drifts the skipper —
# over-extending it (swallowing a production hazard below) or releasing it
# early (false-REDing a legitimate test-module hazard).
#
# Measured on this gate's own 121-file scan set: no file's brace-depth
# bookkeeping ends the file mid-drift (both views return depth to 0 by EOF),
# but the per-line view diverges from the old raw-$0 view on 508 lines across
# 40 of the 121 files (worst: reify-fdm/src/toolpath.rs, 187 lines) — so the
# bookkeeping was drifting mid-file even though it happened to re-balance by
# EOF. The fix is verdict-neutral on the live tree: both the old and the
# lexed view flag 0 sites across all 121 files today. This closes a latent
# hazard, not a live bug.
#
# EXCLUDED:
#   - comments and string literals, per the PRODUCTION-CODE VIEW above;
#   - test code: `tests/` dirs (by path) and `#[cfg(test)] mod` BODIES
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
#      matched nothing), or an awk failure while scanning

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

# An empty scan set (a crate rename, a module move, a repo reorg that no
# longer matches SCOPE_PATHSPECS) must fail loudly, not exit 0 vacuously —
# a gate that scans nothing looks identical, from the caller's side, to a
# gate that scanned everything and found it clean.
if [[ ${#_files[@]} -eq 0 ]]; then
    echo "ERROR: no tracked .rs files matched SCOPE_PATHSPECS — scope is stale?" >&2
    exit 2
fi

# ── Shared awk prologue: lex each line down to its PRODUCTION code in `code`,
# and count the braces that drive `depth` — and so the #[cfg(test)] module
# skipper — from THAT view, never from the raw line. See PRODUCTION-CODE VIEW
# above.
#
# The `#[cfg(test)]` skipper additionally requires the opening line to declare
# a `mod`. Without that requirement a bare `#[cfg(test)] use …;` or
# `#[cfg(test)] fn` leaves the skipper armed until the NEXT brace-opening
# line, which can be an unrelated production item, silently swallowing it.
# This guard's lexer is shared with the INV-FEA-1 trampoline-registration
# guard (scripts/check-compute-trampoline-registration.sh — lands as part of
# task/5076, branch tip 627badfa04 as of this writing; NOT yet on main, so
# this path does not resolve in this tree today), kept in sync BY HAND until
# the two can be factored into a shared lib (follow-up filed: see task
# tracker for "extract scripts/lib_rust_production_view.awk").
#
# The prologue is assembled through a quoted heredoc rather than a
# single-quoted assignment so the lexer can contain apostrophes (it must
# reason about `'` to tell a char literal from a lifetime).
_AWK_PRODUCTION_PROLOGUE="$(cat <<'AWK_PROLOGUE'
# _strip_line(line) -> the PRODUCTION-code view of one raw line.
#
# Comment text is DROPPED. String, char-literal and raw-string CONTENTS are
# BLANKED with their delimiters KEPT, so a blanked string reads exactly "" —
# the shape the hazard-fragment match is written against.
#
# Cross-line state lives in globals: in_block (a NESTING depth, because Rust
# block comments nest), in_str, and in_raw + raw_hashes. carried_in records
# whether the line STARTED inside one of them.
#
# POSIX constructs only (substr / index / length / match, extra parameters as
# locals, no gensub), so the gate is not silently gawk-only.
function _strip_line(line,   out, i, n, ch, h, j, k, pfx, rest) {
    carried_in = (in_block > 0 || in_str || in_raw)
    # The dropped `//…` tail (if any) is stashed here, so a caller can match
    # an escape token against the REAL comment text rather than raw $0 —
    # $0 also contains any string/char-literal content on the line, so a
    # token that merely APPEARS inside a string must not count as the
    # escape. Reset unconditionally (including on the fast path below) so a
    # PRIOR line's tail never leaks onto a line with no comment at all.
    comment_tail = ""
    n = length(line)
    # Fast path: nothing here can open a comment, a string or a char literal.
    # (A bare `/` is division or a path; only `//` and `/*` matter.)
    if (!carried_in && line !~ /["']|\/\/|\/\*/) return line
    out = ""
    i = 1
    while (i <= n) {
        # --- state carried in from a previous line, or opened earlier here ---
        if (in_block > 0) {
            if (substr(line, i, 2) == "*/") { in_block--; i += 2; continue }
            if (substr(line, i, 2) == "/*") { in_block++; i += 2; continue }
            i++
            continue
        }
        if (in_raw) {
            # A raw string closes on a quote followed by exactly raw_hashes
            # hashes, and honours NO escapes.
            if (substr(line, i, 1) == "\"") {
                h = 0
                while (h < raw_hashes && substr(line, i + 1 + h, 1) == "#") h++
                if (h == raw_hashes) {
                    out = out substr(line, i, 1 + h)
                    in_raw = 0
                    i += 1 + h
                    continue
                }
            }
            i++
            continue
        }
        if (in_str) {
            if (substr(line, i, 1) == "\\") { i += 2; continue }
            if (substr(line, i, 1) == "\"") { out = out "\""; in_str = 0; i++; continue }
            i++
            continue
        }

        # --- ordinary code: emit the run up to the next interesting token ---
        rest = substr(line, i)
        if (!match(rest, /["']|\/\/|\/\*|(r|b|c|br|cr)#*"/)) { out = out rest; break }
        if (RSTART > 1) { out = out substr(rest, 1, RSTART - 1); i += RSTART - 1; continue }

        ch = substr(line, i, 1)
        if (ch == "/") {
            if (substr(line, i, 2) == "//") {       # //… tail: drop it, but keep
                comment_tail = substr(line, i)      # it for the escape check
                break
            }
            in_block++                              # /*: enter (nesting) comment
            i += 2
            continue
        }
        if (ch == "'") {
            # A CHAR LITERAL closes within one character — two with a backslash
            # escape, more for \x41 / \u{7f}, whose own braces must not count.
            # Anything else is a LIFETIME and is emitted verbatim, so
            # &'static str and <'a, 'b> are untouched.
            if (substr(line, i + 1, 1) == "\\") {
                j = i + 3
                while (j <= n && substr(line, j, 1) != "'") j++
                if (j <= n && j - i <= 12) { out = out "''"; i = j + 1; continue }
            } else if (substr(line, i + 2, 1) == "'") {
                out = out "''"
                i += 3
                continue
            }
            out = out "'"
            i++
            continue
        }
        if (ch == "\"") { out = out "\""; in_str = 1; i++; continue }

        # r"…" / r#"…"# / b"…" / br#"…"# / c"…" / cr#"…"# — the match above
        # guarantees a prefix of at most two letters, then #*, then a quote.
        pfx = ""
        k = i
        while (k <= n && length(pfx) < 2 && index("rbc", substr(line, k, 1)) > 0) {
            pfx = pfx substr(line, k, 1)
            k++
        }
        h = 0
        while (substr(line, k + h, 1) == "#") h++
        if (substr(line, k + h, 1) == "\"") {
            out = out substr(line, i, (k - i) + h + 1)   # prefix + hashes + quote
            if (index(pfx, "r") > 0) { in_raw = 1; raw_hashes = h } else { in_str = 1 }
            i = k + h + 1
            continue
        }
        out = out ch                                     # not a prefix after all
        i++
    }
    return out
}

{
    code = _strip_line($0)

    # A line wholly inside a block comment or a carried-over multi-line
    # string contributes no code braces at all — drop it before any depth
    # bookkeeping.
    if (carried_in && code == "") next

    # Brace counts on the LEXED view, never on $0 (throwaway copies so `code`
    # stays intact). Counting the raw line would let a brace that exists only
    # inside a comment, a string, a char literal or a raw string move `depth`,
    # which silently mis-drives the skipper below — see PRODUCTION-CODE VIEW
    # above.
    c = code; n_open  = gsub(/[{]/, "x", c)
    c = code; n_close = gsub(/[}]/, "x", c)

    # --- #[cfg(test)] MODULE skipping (best-effort brace tracking) ---
    if (in_test) {
        depth += n_open - n_close
        if (depth <= test_base) in_test = 0
        next
    }
    if (code ~ /#\[cfg\(test\)\]/) {
        pending_test = 1
        pending_mod = 0
    } else if (pending_test) {
        # The pending gate survives only across blank lines, further attributes
        # and comments; any other non-mod line disarms it. A comment-only line
        # lexes to the empty string, so `t == ""` already covers it.
        #
        # A `mod IDENT` line that opens no brace of its own — e.g. `mod tests`
        # with the `{` on the NEXT line, legal Rust and this repo has no
        # rustfmt gate forcing brace-on-same-line (CLAUDE.md) — sets
        # `pending_mod` so that later brace-opening line can still arm the
        # skipper below, even though *that* line's own text says nothing
        # about `mod`. Guarded to n_open==0 and no same-line `;` so a
        # self-terminating `mod tests;` (external file, no local body) does
        # NOT leave `pending_mod` armed for some unrelated later brace.
        t = code; sub(/^[ \t]+/, "", t)
        if (t ~ /(^|[^A-Za-z0-9_])mod[ \t]+[A-Za-z_]/ && n_open == 0 && t !~ /;/) pending_mod = 1
        if (!(t == "" || t ~ /^#\[/ || t ~ /(^|[^A-Za-z0-9_])mod[ \t]/ || (pending_mod && n_open > 0))) {
            pending_test = 0
            pending_mod = 0
        }
    }
    if (pending_test && n_open > 0) {
        if (pending_mod || code ~ /(^|[^A-Za-z0-9_])mod[ \t]+[A-Za-z_]/) {
            in_test = 1
            test_base = depth        # depth BEFORE this block opened
            depth += n_open - n_close
            pending_test = 0
            pending_mod = 0
            next
        }
        pending_test = 0             # cfg(test) on a fn/field/use, not a module
        pending_mod = 0
    }
    depth += n_open - n_close
}

# Self-consistency check: a lexer state left unbalanced at EOF (an
# unterminated string/raw-string/block-comment, or a brace depth that never
# returns to 0) means every line after the desync point lexed wrong — and,
# per the `carried_in && code == ""` skip above, likely went entirely
# unscanned. That fails SILENTLY toward GREEN (an unscanned line cannot be
# flagged), so it is surfaced here rather than left to discover itself.
# Verdict-neutral (a warning, not a failure): this gate's own 121-file scan
# set measurably ends every file balanced today (see PRODUCTION-CODE VIEW
# above), so promoting this to a hard failure once that is trusted tree-wide
# is a follow-up, not a day-one behavior change.
END {
    if (in_str || in_raw || in_block > 0 || depth != 0)
        printf "WARN: %s: lexer state unbalanced at EOF (str=%d raw=%d block=%d depth=%d)\n", FILENAME, in_str, in_raw, in_block, depth > "/dev/stderr"
}
AWK_PROLOGUE
)"

# Per-file scan, appended to the shared prologue. In order:
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
    if ! out="$(awk "$_AWK_PRODUCTION_PROLOGUE"'
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
