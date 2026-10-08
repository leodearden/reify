# shellcheck shell=bash
# scripts/lib_rust_production_view.sh — the ONE Rust literal/comment lexer
# shared by this repo's grep-style Rust source gates and audits (task 6202).
# Sourced only. Sourcing defines two awk program texts and nothing else: it
# runs no awk and has no other side effect.
#
# PRODUCTION-CODE VIEW. `_strip_line` reduces each raw line to its production
# code with a single LEFT-TO-RIGHT lexer:
#   - comment text is DROPPED; string, char-literal and raw-string CONTENTS are
#     BLANKED with their delimiters kept, so a blanked string reads exactly `""`;
#   - `/* … */` (which nests), `"…"` and `r#"…"#` state is carried ACROSS lines,
#     byte and C-string prefixes included (`b"…"`, `br#"…"#`, `c"…"`, `cr#"…"#`);
#   - a char literal (`'{'`, `'\''`, `'\x7b'`, `'\u{7b}'`) is told apart from a
#     lifetime or a label, so `&'static str`, `<'a, 'b>` and `'outer:` survive.
# It is one lexer, not a chain of two regex substitutions, because such a chain
# is wrong in EITHER order: strip comments first and a `//` inside a STRING
# truncates the code after it; blank strings first and a quote inside a
# COMMENT mis-blanks the code after it.
#
# The production layer counts braces on that LEXED view, never on the raw line,
# and drives `depth` and the test-module skipper from those counts: a brace
# that exists only inside a comment, a string, a char literal or a raw string
# must not move `depth`, or the skipper silently over-extends (swallowing
# production code below it) or releases early. The skipper arms on a cfg
# attribute that compiles its item into test builds ONLY: `#[cfg(test)]`;
# `#[cfg(all(test, …))]`, whatever follows `test`; or `#[cfg(any(test, …))]`
# when every other predicate is a feature named `test…` (`test-support`,
# `test-instrumentation`). Anything not shown to be test-only reads as
# production, which is the loud direction (a false red, with the consumer's
# inline escape as relief): `any(test, feature = "gui")`,
# `any(test, debug_assertions)`, negated spellings (`not(test)`,
# `any(not(test), …)`), `cfg_attr(test, …)`, a `test` that is not the first
# predicate, and a cfg that rustfmt has split across lines. Whitespace between
# tokens is tolerated; a line break is not. It arms only when a `mod IDENT`
# declaration is seen before the block's opening brace — on the same line, or
# (`pending_mod`) on a brace-less `mod tests` line whose `{` comes later, legal
# Rust that no rustfmt gate rules out here. A bare `#[cfg(test)] fn` or
# `#[cfg(any(test, …))] use …;` therefore does NOT arm it, and a
# self-terminating `mod tests;` leaves nothing armed. A line wholly inside a
# block comment or a carried-over string is skipped before any bookkeeping, so
# a lexer desync would fail silently toward green; the END block therefore
# WARNs on any file whose lexer state is unbalanced at EOF.
#
# BEST-EFFORT, deliberately not exhaustive: no macro expansion, no `cfg`
# evaluation, and a `*/` inside a string inside a block comment is not modelled.
#
# POSIX awk only (substr / index / length / match, extra parameters as locals,
# no gensub), so no consumer is silently gawk-only. Pinned by the mawk-agreement
# cases in tests/infra/test_rust_production_view_lib.py.
#
# CONSUMER CONTRACT. Two exports, each a complete awk program text to which a
# consumer appends its own rules:
#   RUST_LEXER_AWK            the lexer functions only:
#                               _strip_line(line)    the production-code view
#                               _lexer_open_state()  "block_comment",
#                                                    "raw_string", "string", or
#                                                    "" when balanced
#                               _lexer_reset()       forget all carried state
#   RUST_PRODUCTION_VIEW_AWK  RUST_LEXER_AWK, then a per-line rule that `next`s
#                             test-module lines and wholly-carried lines, then
#                             the END lexer-balance WARN on stderr. Pass
#                             `-v quiet_eof=1` to silence that WARN when the
#                             consumer `exit`s mid-file by design.
# An appended rule may read `code` (the lexed line), `comment_tail` (the dropped
# `//…` text, for escape-token checks), `carried_in`, `depth`, `n_open` and
# `n_close`. It must not write the lexer's own state; use the accessors.
#
# Consumers, each sourcing this file from its OWN script directory and treating
# a failed load as "could not run":
#   scripts/check-nan-safe-ordering.sh               RUST_PRODUCTION_VIEW_AWK
#   scripts/check-compute-trampoline-registration.sh RUST_PRODUCTION_VIEW_AWK
#   scripts/audit-orphan-producers.sh                RUST_LEXER_AWK

# Both texts are assembled through quoted heredocs rather than single-quoted
# assignments so the lexer can contain apostrophes (it must reason about `'` to
# tell a char literal from a lifetime).
RUST_LEXER_AWK="$(cat <<'AWK_LEXER'
# _strip_line(line) -> the PRODUCTION-code view of one raw line.
#
# Comment text is DROPPED. String, char-literal and raw-string CONTENTS are
# BLANKED with their delimiters KEPT, so a blanked string reads exactly "" —
# the shape consumers' patterns are written against.
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

# _lexer_open_state() -> the construct still open after the lines lexed so far:
# "block_comment", "raw_string", "string", or "" when balanced. The three are
# mutually exclusive by construction (each opens only from ordinary code).
function _lexer_open_state() {
    if (in_block > 0) return "block_comment"
    if (in_raw) return "raw_string"
    if (in_str) return "string"
    return ""
}

# _lexer_reset() -> forget all carried state, e.g. between two files lexed in
# one awk process.
function _lexer_reset() {
    in_block = 0
    in_str = 0
    in_raw = 0
    raw_hashes = 0
}
AWK_LEXER
)"

RUST_PRODUCTION_VIEW_AWK="$RUST_LEXER_AWK
$(cat <<'AWK_VIEW'
# _is_test_only_cfg(code, raw) -> 1 when the line carries a cfg attribute that
# arms the test-module skipper (rule: header). Whitespace is squeezed out
# first. An `any(test, …)` tail is read from the RAW line, because the lexed
# view blanks feature names — but only once the lexed line shows a real
# `any(test` head, so a spelling in a comment or a string cannot arm.
function _is_test_only_cfg(code, raw,   squeezed_code, squeezed_raw) {
    squeezed_code = code; gsub(/[ \t]+/, "", squeezed_code)
    if (squeezed_code ~ /#\[cfg\(test\)\]/) return 1
    if (squeezed_code ~ /#\[cfg\(all\(test[,)]/) return 1
    if (squeezed_code !~ /#\[cfg\(any\(test[,)]/) return 0
    squeezed_raw = raw; gsub(/[ \t]+/, "", squeezed_raw)
    return squeezed_raw ~ /#\[cfg\(any\(test(,feature="test[^"]*")*,?\)\)\]/
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

    # --- test-gated MODULE skipping (best-effort brace tracking) ---
    if (in_test) {
        depth += n_open - n_close
        if (depth <= test_base) in_test = 0
        next
    }
    if (_is_test_only_cfg(code, $0)) {
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

# Self-consistency check: a lexer state left unbalanced at EOF (an unterminated
# string / raw string / block comment, or a brace depth that never returns to 0)
# means every line after the desync point lexed wrong — and, per the
# `carried_in && code == ""` skip above, likely went entirely unscanned. That
# fails SILENTLY toward GREEN (an unscanned line cannot be flagged), so it is
# surfaced here rather than left to discover itself. Verdict-neutral: a
# warning, never a non-zero exit; each consumer records its own measurement of
# why that is safe.
#
# `quiet_eof` is for a consumer that stops mid-file on purpose (an `exit` on
# its first match), which leaves the lexer legitimately unbalanced; without it
# every successful early exit would warn.
END {
    if (!quiet_eof && (in_str || in_raw || in_block > 0 || depth != 0))
        printf "WARN: %s: lexer state unbalanced at EOF (str=%d raw=%d block=%d depth=%d)\n", FILENAME, in_str, in_raw, in_block, depth > "/dev/stderr"
}
AWK_VIEW
)"
