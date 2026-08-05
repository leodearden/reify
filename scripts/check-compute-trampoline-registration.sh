#!/usr/bin/env bash
# scripts/check-compute-trampoline-registration.sh
#
# INV-FEA-1 regression guard (PRD docs/prds/compute-fea-hardening.md task A5;
# task 5076).
#
# INV-FEA-1: every engine-construction site registers the compute trampolines
# through the ONE bundler, `Engine::register_production_compute_fns`, so no site
# can drift into registering a partial bundle. The hazards this catches are both
# silent — each compiles clean and passes every type check:
#   (1) a known construction site stops delegating to the bundler;
#   (2) a site keeps delegating but passes the WRONG MorphRegistration variant —
#       specifically, flipping gui/src-tauri/src/engine.rs's
#       `#[cfg(feature = "gui")]` arm from `Enabled(..)` to `Unavailable{..}`
#       silently un-registers the mesh-morph producer (the esc-2962-66 class);
#   (3) a FOURTH site hand-rolls the bundle from its halves instead of calling
#       the bundler, so a later leg added to the bundler never reaches it.
#
# WHAT IS MATCHED — two independent passes, both reading the SAME
# production-code view (see PRODUCTION-CODE VIEW below):
#
#   POSITIVE (delegation).  A literal table of the known engine-construction
#   sites, each pinned to the MorphRegistration variant it must pass. Each site
#   must be tracked, must call `register_production_compute_fns(`, and must
#   carry its required variant token. Matching is on CONTENT, never on line
#   numbers: engine.rs's call moved :1057 -> :1085 between two revisions purely
#   from the file growing.
#
#   NEGATIVE (no fourth bundler).  Any DIRECT call to a bundle HALF —
#   `register_compute_fns(` or `register_shell_extract_compute_fns(` — is a
#   hand-rolled bundle and is flagged as file:line. The halves are the precise
#   signal: a hand-rolled bundler is definitionally a site that calls them
#   itself instead of delegating. `Engine::new` /
#   `Engine::with_registered_kernel` are deliberately NOT matched — they have
#   hundreds of legitimate call sites — and neither is `register_morph_producer(`,
#   which is a legitimate public API rather than a bundler.
#
# PRODUCTION-CODE VIEW (shared by BOTH passes — `_AWK_PRODUCTION_PROLOGUE`).
# Each raw line is reduced to its production code by a single LEFT-TO-RIGHT
# LEXER (`_strip_line`), not by a chain of regex substitutions. Comment text is
# DROPPED; string, char-literal and raw-string CONTENTS are BLANKED with their
# delimiters kept (a blanked string reads exactly `""`). The lexer carries
# `/* … */` (which nests), `"…"` and `r#"…"#` state ACROSS lines, and tells a
# char literal from a lifetime, so `&'static str` and `<'a, 'b>` survive
# untouched. A line is invisible to both passes when it is inside a
# `#[cfg(test)] mod` body, or wholly inside a block comment or a carried-over
# multi-line string.
#
# The brace counts that drive `depth` — and so BOTH depth-driven skippers, the
# `#[cfg(test)] mod` one and the definition-file `in_bundler` one — are taken
# from that LEXED view, never from the raw line. A brace that exists only inside
# a comment, a string, a char literal or a raw string must not move `depth`: a
# stray `{` over-extends the test-module skipper until the fourth-bundler pass
# goes blind (hazard 3), and a stray `}` releases it early until the per-site
# variant pin goes vacuous (hazard 2). Both are live shapes, not hypotheticals —
# four unbalanced brace char literals sit in the scan set today
# (crates/reify-kernel-occt/src/lib.rs:4930,4948,9167 and
# crates/reify-test-support/src/orphan_audit.rs:778) and 277 of its files carry
# raw strings.
#
# A two-regex chain cannot do this in EITHER order, which is why it was replaced
# rather than reordered: strip comments first and a `//` living inside a STRING
# truncates the code after it (11 lines of the live scan set change their brace
# balance under that, e.g. crates/reify-audit/src/ptodo.rs:90
# `if line.trim_start().starts_with("//") {`); blank strings first and a quote
# living inside a COMMENT mis-blanks the code after it.
#
# Still BEST-EFFORT, deliberately not exhaustive: no macro expansion, no `cfg`
# evaluation, and a `*/` appearing inside a string inside a block comment is not
# modelled.
#
# The two passes are therefore symmetric by construction. That symmetry is
# load-bearing on BOTH sides:
#   - negative: crates/reify-mesh-morph/src/lib.rs carries a rustdoc MENTION of
#     a bundle half OUTSIDE any `#[cfg(test)]` module, so only the //-strip
#     keeps it green;
#   - positive: engine.rs already carries three inline `#[cfg(test)]` modules,
#     and crates/reify-cli/src/main.rs:4390 carries the near-miss STRING
#     "…must pass MorphRegistration::Enabled, ". Without test/comment/string
#     removal, one unit test or assertion message naming
#     `MorphRegistration::Enabled(` would make the variant pin VACUOUS — the
#     production `#[cfg(feature = "gui")]` arm could then be flipped to
#     `Unavailable` with the gate still exiting 0, i.e. hazard (2) unguarded.
# Deliberately NOT required: that the variant token and the delegation call sit
# in the same enclosing item. All three sites satisfy that today, but a routine
# refactor that hoists the variant into a helper fn would then be a false RED,
# and a false RED on a clean tree is worse than the residual it closes (a
# SECOND *production* mention of the variant in the SAME file — none exists:
# engine.rs has exactly one `MorphRegistration::Enabled(` in production code).
#
# COVERED SCOPE (negative pass) — tracked Rust production inputs:
#   crates/*/src/*.rs, crates/*/benches/*.rs, crates/*/examples/*.rs,
#   crates/*/build.rs, gui/src-tauri/src/*.rs, gui/src-tauri/build.rs,
#   gui/src-tauri/benches/*.rs.
# NOT COVERED, stated so the gate is not mistaken for exhaustive: `tests/` dirs
# (integration tests are test code by construction — skipped by path), and any
# Rust outside those pathspecs (e.g. a new top-level crate root outside
# `crates/`). Widening is a one-line edit to SCOPE_PATHSPECS.
#
# EXCLUDED (negative pass):
#   - comments and string literals, per the PRODUCTION-CODE VIEW above;
#   - test code: `tests/` dirs (by path) and `#[cfg(test)] mod` bodies
#     (brace-depth tracked, best-effort). Five real in-src callers live in
#     `#[cfg(test)]` modules and are legitimate: compute_persist.rs:529,672;
#     compute_targets/as_printed_material.rs:542; compute_targets/mod.rs:541,583;
#   - inside the DEFINITION files (EXEMPT_DEFINITION_FILES) — and ONLY there —
#     the two `fn register_*_compute_fns(` definition lines and the body of
#     `fn register_production_compute_fns(` (that body IS the bundler). The
#     exemption is scoped to those two constructs rather than granted
#     file-wholesale, so a fourth hand-rolled bundle added ELSEWHERE in
#     compute_targets/mod.rs — the single most likely place for someone to add
#     one — is still flagged;
#   - escaped sites: any line carrying the inline escape
#         // trampoline-registration:allow — <reason>
#     mirroring reify-audit's `// ptodo:allow` convention (§6.8) and
#     check-nan-safe-ordering.sh's `// nan-safe:allow`. The escape is a
#     NEGATIVE-pass concept only (it declares an intentional direct half-call);
#     it never suppresses a positive-pass delegation requirement.
#
# HERMETIC SOURCE SET: `git ls-files` lists only tracked files, so untracked
# build artifacts never enter the scan (mirrors check_event_inventory.sh).
#
# Usage: scripts/check-compute-trampoline-registration.sh [--repo-root <dir>]
# Exit codes:
#   0  clean — every known site delegates with its required variant, and no
#      production source hand-rolls the bundle
#   1  at least one violation (each printed to stderr)
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

# ── Known engine-construction sites: <path>|<required MorphRegistration variant>
# The variant pin is what makes hazard (2) statically RED. reify-eval's test
# runner requires Unavailable rather than Enabled because reify-mesh-morph is
# not one of its dependencies.
KNOWN_SITES=(
    'crates/reify-cli/src/main.rs|MorphRegistration::Enabled[(]'
    'gui/src-tauri/src/engine.rs|MorphRegistration::Enabled[(]'
    'crates/reify-eval/src/test_runner.rs|MorphRegistration::Unavailable'
)

# ── Negative-pass scope. Single-star git pathspecs are NOT path-boundary-aware,
# so 'crates/*/src/*.rs' matches every tracked .rs at any depth beneath a crate's
# src/. benches/examples/build.rs are included because they are real build
# inputs a hand-rolled bundle could hide in (see COVERED SCOPE above).
SCOPE_PATHSPECS=(
    'crates/*/src/*.rs'
    'crates/*/benches/*.rs'
    'crates/*/examples/*.rs'
    'crates/*/build.rs'
    'gui/src-tauri/src/*.rs'
    'gui/src-tauri/build.rs'
    'gui/src-tauri/benches/*.rs'
)

# ── The two files that DEFINE the bundle halves. They are NOT skipped
# wholesale: they are scanned with `allow_defs=1`, which permits exactly two
# constructs (the `fn register_*_compute_fns(` definition lines, and the body of
# `fn register_production_compute_fns(` — the bundler itself) and flags every
# other direct half-call in them like anywhere else.
EXEMPT_DEFINITION_FILES=(
    'crates/reify-eval/src/compute_targets/mod.rs'
    'crates/reify-eval/src/shell_extract_compute.rs'
)

violations=""
note() { violations+="$1"$'\n'; }

# ── Shared awk prologue: lex each line down to its PRODUCTION code in `code`,
# count the braces that drive `depth` from THAT view, and `next` the line
# entirely when it is test-only or wholly inside a block comment / carried-over
# string. Both passes prepend this, so neither can see what the other cannot.
#
# The `#[cfg(test)]` skipper additionally requires the opening line to declare a
# `mod` (check-nan-safe-ordering.sh:106-120 does not). Without that requirement
# a bare `#[cfg(test)] use …;` — gui/src-tauri/src/engine.rs:21, :102, :104 all
# have one — leaves the skipper armed until the NEXT brace-opening line, which
# can be an unrelated production item; that misfire is a silent under-flag on
# the negative pass but a FALSE RED on the positive pass, so it is fixed here
# rather than inherited.
#
# The prologue is assembled through a quoted heredoc rather than a single-quoted
# assignment so the lexer can contain apostrophes (it must reason about `'` to
# tell a char literal from a lifetime).
_AWK_PRODUCTION_PROLOGUE="$(cat <<'AWK_PROLOGUE'
# _strip_line(line) -> the PRODUCTION-code view of one raw line.
#
# Comment text is DROPPED. String, char-literal and raw-string CONTENTS are
# BLANKED with their delimiters KEPT, so a blanked string reads exactly "" —
# the shape the _code_has patterns and the h2c fixture are written against.
#
# Cross-line state lives in globals: in_block (a NESTING depth, because Rust
# block comments nest), in_str, and in_raw + raw_hashes. carried_in records
# whether the line STARTED inside one of them.
#
# POSIX constructs only (substr / index / length / match, extra parameters as
# locals, no gensub), so the gate is not silently gawk-only.
function _strip_line(line,   out, i, n, ch, h, j, k, pfx, rest) {
    carried_in = (in_block > 0 || in_str || in_raw)
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
            if (substr(line, i, 2) == "//") break   # //… tail: drop it entirely
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

    # A line wholly inside a block comment or a carried-over multi-line string
    # contributes no code braces at all — drop it before any depth bookkeeping.
    if (carried_in && code == "") next

    # Brace counts on the LEXED view, never on $0 (throwaway copies so `code`
    # stays intact). Counting the raw line lets a brace that exists only inside
    # a comment, a string, a char literal or a raw string move `depth`, which
    # silently mis-drives both skippers below — see PRODUCTION-CODE VIEW.
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
    } else if (pending_test) {
        # The pending gate survives only across blank lines, further attributes
        # and comments; any other non-mod line disarms it. A comment-only line
        # now lexes to the empty string, so the former `t ~ /^\/\//` alternative
        # is unreachable and is gone: `t == ""` already covers it.
        t = code; sub(/^[ \t]+/, "", t)
        if (!(t == "" || t ~ /^#\[/ || t ~ /(^|[^A-Za-z0-9_])mod[ \t]/)) pending_test = 0
    }
    if (pending_test && n_open > 0) {
        if (code ~ /(^|[^A-Za-z0-9_])mod[ \t]+[A-Za-z_]/) {
            in_test = 1
            test_base = depth        # depth BEFORE this block opened
            depth += n_open - n_close
            pending_test = 0
            next
        }
        pending_test = 0             # cfg(test) on a fn/field/use, not a module
    }
    depth += n_open - n_close
}
AWK_PROLOGUE
)"

# _code_has <file> <awk-regex> — is the pattern present in PRODUCTION code?
_code_has() {
    awk -v pat="$2" "$_AWK_PRODUCTION_PROLOGUE"'
        code ~ pat { found = 1; exit }
        END { exit(found ? 0 : 1) }
    ' "$1"
}

# ── POSITIVE PASS ─────────────────────────────────────────────────────────────
for entry in "${KNOWN_SITES[@]}"; do
    site="${entry%%|*}"
    variant="${entry##*|}"

    if [[ -z "$(git -C "$REPO_ROOT" ls-files -- "$site")" ]] || [[ ! -f "$REPO_ROOT/$site" ]]; then
        note "$site: known engine-construction site is missing (expected a tracked file)"
        continue
    fi
    if ! _code_has "$REPO_ROOT/$site" 'register_production_compute_fns[(]'; then
        note "$site: no longer calls Engine::register_production_compute_fns(...)"
    fi
    if ! _code_has "$REPO_ROOT/$site" "$variant"; then
        note "$site: does not pass the required ${variant//\[(\]/(} to register_production_compute_fns"
    fi
done

# ── NEGATIVE PASS ─────────────────────────────────────────────────────────────
# Scan set: tracked production Rust minus `tests/` dirs. The two definition
# files stay IN the set and are scanned with allow_defs=1 (scoped exemption).
_files=()
while IFS= read -r -d '' _f; do
    case "$_f" in
        */tests/*) continue ;;
    esac
    _files+=("$_f")
done < <(git -C "$REPO_ROOT" ls-files -z -- "${SCOPE_PATHSPECS[@]}" 2>/dev/null)

# Per-file action block, appended to the shared prologue. In order:
#   1. honor the same-line `trampoline-registration:allow` escape;
#   2. in a DEFINITION file only, skip the bundler body and the two definition
#      lines (everything else in those files is still matched);
#   3. flag a direct call to a bundle half.
for f in "${_files[@]}"; do
    _allow_defs=0
    for _x in "${EXEMPT_DEFINITION_FILES[@]}"; do
        [[ "$f" == "$_x" ]] && { _allow_defs=1; break; }
    done
    out="$(awk -v rel="$f" -v allow_defs="$_allow_defs" "$_AWK_PRODUCTION_PROLOGUE"'
        {
            # --- inline escape (same-line), mirrors ptodo:allow §6.8 ---
            # RAW $0 on purpose: the escape lives in a `//` comment, which the
            # lexer drops, so matching `code` here would silently kill it.
            if ($0 ~ /trampoline-registration:allow/) next

            if (allow_defs) {
                # Body of fn register_production_compute_fns — THE bundler.
                # depth here is the depth AFTER this line, so subtracting this
                # line net brace delta recovers the depth before it.
                if (in_bundler) {
                    if (depth <= bundler_base) in_bundler = 0
                    else next
                }
                if (!in_bundler) {
                    if (code ~ /fn[ \t]+register_production_compute_fns[(]/) pending_bundler = 1
                    if (pending_bundler && n_open > 0) {
                        in_bundler = 1
                        bundler_base = depth - (n_open - n_close)
                        pending_bundler = 0
                        next
                    }
                }
                # The definition lines themselves are declarations, not calls.
                if (code ~ /fn[ \t]+register_compute_fns[(]/) next
                if (code ~ /fn[ \t]+register_shell_extract_compute_fns[(]/) next
            }

            if (code ~ /register_compute_fns[(]/ || code ~ /register_shell_extract_compute_fns[(]/) {
                # RAW $0 on purpose: this is human-readable violation output,
                # so it must show the source line as written, not the lexed view.
                printf "%s:%d: %s\n", rel, FNR, $0
            }
        }
    ' "$REPO_ROOT/$f")"
    [[ -n "$out" ]] && note "$out"
done

if [[ -n "${violations//$'\n'/}" ]]; then
    printf '%s' "$violations" | grep -v '^$' >&2
    n="$(printf '%s' "$violations" | grep -c '.')"
    {
        echo ""
        echo "ERROR: $n INV-FEA-1 violation(s) found (task 5076)."
        echo "Every engine-construction site must register the compute trampolines"
        echo "through the single bundler:"
        echo "    engine.register_production_compute_fns(<MorphRegistration variant>);"
        echo "rather than calling register_compute_fns / register_shell_extract_compute_fns"
        echo "itself — see docs/prds/compute-fea-hardening.md task A5 (INV-FEA-1)."
        echo "If a site genuinely must call a half directly, annotate it with:"
        echo "    // trampoline-registration:allow — <why the bundle is wrong here>"
    } >&2
    exit 1
fi

exit 0
