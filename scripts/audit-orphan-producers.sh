#!/usr/bin/env bash
# scripts/audit-orphan-producers.sh
#
# Portfolio approach G — corpus-wide reviewer aid for "Type A" orphan
# producers: public functions in `crates/reify-*/src/` whose only
# callers are tests, the defining file itself, or `pub use` re-exports.
#
# Design: docs/architecture-audit/g-reviewer-tool-session-prompt.md
# Baseline: docs/architecture-audit/g-tool-baseline-report.md
#
# Anti-gaming: corpus-level only. Per-task worktree invocation would see
# a sliver of the code and is gameable by an implementer adding a fake
# caller in the same task. Reviewers run it at `/review` cadence or on
# demand against a fresh clone of main.
#
# Allow-list: inline `// G-allow: <reason>` on the line immediately
# preceding a `pub fn` declaration marks that fn as intentional library
# API surface. The reason is mandatory.
#
# Exits 0 unless --strict is passed (then exits 1 when orphans without
# `// G-allow:` markers are found); 2 is a usage error. Exit 3 means the audit
# could not run: python3, git or awk is missing, or the shared Rust lexer that
# the `#[cfg(test)]` masking reads (scripts/lib_rust_production_view.sh,
# sourced from this script's own directory) could not be loaded or failed.
# stdout is then EMPTY, so a caller parsing --format json gets nothing rather
# than a partial envelope.
#
# A `#[cfg(test)]` item whose mask never closes (brace-counting runs to
# EOF without the block balancing back to zero) is reported as a WARNING
# on stderr naming the file and the attribute's line -- any `pub fn`
# below such a mask is invisible to this audit. Not gated on --quiet: see
# usage() below.

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/audit-orphan-producers.sh [options]

Options:
  --format FMT   Output format: markdown (default) or json.
  --scope GLOB   Restrict to files matching GLOB (default: crates/reify-*/src).
                 Multiple --scope flags accumulate.
  --quiet        Suppress progress messages on stderr (warnings are still printed).
  --strict       Exit 1 if any non-allow-listed orphans are found.
  -h, --help     Show this message.
USAGE
}

FORMAT="markdown"
SCOPES=()
QUIET=0
STRICT=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --format) FORMAT="$2"; shift 2 ;;
        --scope) SCOPES+=("$2"); shift 2 ;;
        --quiet) QUIET=1; shift ;;
        --strict) STRICT=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown arg: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ ${#SCOPES[@]} -eq 0 ]]; then
    SCOPES=("crates/reify-*/src")
fi

for tool in python3 git awk; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "audit-orphan-producers.sh: $tool not on PATH" >&2
        exit 3
    fi
done

# The shared Rust lexer, resolved beside THIS script (never the CWD or the
# audited repo) and before the cd below, because BASH_SOURCE may be relative to
# the caller's CWD. A failed load is exit 3; nothing has reached stdout yet.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_rust_production_view.sh
if ! source "$SCRIPT_DIR/lib_rust_production_view.sh" || [[ -z "${RUST_LEXER_AWK:-}" ]]; then
    echo "audit-orphan-producers.sh: cannot load the shared Rust lexer lib: $SCRIPT_DIR/lib_rust_production_view.sh" >&2
    exit 3
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

[[ $QUIET == 0 ]] && echo "audit-orphan-producers.sh: scanning ${SCOPES[*]}" >&2

SCOPE_ARGS=()
for s in "${SCOPES[@]}"; do
    SCOPE_ARGS+=("$s")
done

# Command-prefix scope: the lexer text lives only for the python3 process.
RUST_LEXER_AWK="$RUST_LEXER_AWK" python3 - "$FORMAT" "$STRICT" "$QUIET" "${SCOPE_ARGS[@]}" <<'PYTHON_SCRIPT'
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

format_ = sys.argv[1]
strict = sys.argv[2] == "1"
quiet = sys.argv[3] == "1"
scopes = sys.argv[4:]

EXCLUDE_SEGMENTS = {".worktrees", "target", "tests", "benches", "examples"}
# Test-support crates are intentionally only called from test files
# (which we exclude from the caller search), so their publics would
# all look like orphans. Skip them at the crate level.
#
# Caveat, recorded deliberately: that premise no longer holds for ALL of
# reify-test-support. Some of its publics are reached from reify-audit's
# PRODUCTION path, so this sweep cannot see them. The exclusion still earns its
# keep for the crate's bulk and stays; it is an accepted blind spot, not an
# assumed-safe one. WHICH publics, what justifies each `pub` in place of a
# sweep, and the open question about the shape of the arrangement are argued
# once in `sanitize`'s doc comment (crates/reify-test-support/src/git_env.rs);
# this comment points there rather than keeping its own copy of the list.
EXCLUDE_CRATES = {"reify-test-support"}
# Source-level files included only via `#[cfg(test)] mod NAME;`. Reify's
# conventions: `test_*.rs`, `*_test_support.rs`, and — for the god-file test
# evictions (task #5026 α geometry_ops, #5027 β engine_build, …) — the sibling
# test module `<file>/tests.rs` and `<file>/<NAME>_tests.rs`. The eviction moves
# a `#[cfg(test)] mod tests { … }` block to a sibling file whose `#[cfg(test)]`
# gate lives on the *declaration*, so the sibling carries no inner `#[cfg(test)]`
# for `mask_cfg_test` to catch; without a name-based exclusion its test bodies
# would be miscounted as production callers, masking real orphans (and un-
# orphaning same-file-test-caller pins). Detecting the cfg(test)-mod declaration
# precisely would require parsing the module tree; the filename heuristic is
# adequate for v1.
EXCLUDE_FILE_PATTERNS = (
    re.compile(r'(?:^|/)test_[^/]+\.rs$'),
    re.compile(r'_test_support\.rs$'),
    # god-file eviction test siblings: `X/tests.rs`, `X/<NAME>_tests.rs`
    re.compile(r'(?:^|/|_)tests\.rs$'),
)


def discover_sources(scope_globs):
    """Expand each scope (a directory glob) and return sorted unique .rs
    files under it, skipping per-crate tests/benches/examples,
    target/.worktrees trees, dedicated test-support crates, and
    test-only source modules (`src/test_*.rs`).
    """
    found = set()
    root = Path(".")
    for scope in scope_globs:
        # Each scope is a directory pattern like `crates/reify-*/src`.
        for matched_dir in sorted(root.glob(scope)):
            if not matched_dir.is_dir():
                continue
            parts = matched_dir.parts
            if any(p in EXCLUDE_CRATES for p in parts):
                continue
            for rs in matched_dir.rglob("*.rs"):
                rs_parts = set(rs.parts)
                if rs_parts & EXCLUDE_SEGMENTS:
                    continue
                if rs_parts & EXCLUDE_CRATES:
                    continue
                rs_str = rs.as_posix()
                if any(p.search(rs_str) for p in EXCLUDE_FILE_PATTERNS):
                    continue
                found.add(rs)
    return sorted(found)


src_files = discover_sources(scopes)
if not src_files:
    print("audit-orphan-producers.sh: no source files matched", file=sys.stderr)
    sys.exit(0)
if not quiet:
    print(f"audit-orphan-producers.sh: {len(src_files)} source files", file=sys.stderr)

PUB_FN_RE = re.compile(
    r'^\s*pub(\([^)]*\))?\s+'
    r'(?:async\s+)?'
    r'(?:const\s+)?'
    r'(?:unsafe\s+)?'
    r'(?:extern\s+"[A-Za-z]+"\s+)?'
    r'fn\s+([A-Za-z_][A-Za-z0-9_]*)'
)
USE_RE = re.compile(r'^\s*(pub\s+)?use\b')
CFG_TEST_RE = re.compile(r'#\[cfg\(test\)\]')
G_ALLOW_RE = re.compile(r'//\s*G-allow:\s*(.+)')
LINE_COMMENT_RE = re.compile(r'//.*$')


BLOCK_KW_RE = re.compile(r'\b(?:fn|mod|impl|struct|enum|trait|union)\b')


# --- code view: the shared Rust lexer ----------------------------------------
#
# mask_cfg_test reads a "code view" of each file, so braces, keywords and
# `;`/`,` suffixes inside string, char and raw-string literals or comments do
# not perturb it. That view is the shared lexer's `_strip_line` view
# (scripts/lib_rust_production_view.sh, passed in as $RUST_LEXER_AWK): comment
# text dropped, literal contents blanked with the delimiters kept, so a blanked
# string reads exactly `""`. mask_cfg_test needs no column alignment, only
# brace counts, keyword hits and line suffixes, all of which that view keeps.
_FRAME_END = "\x1c"  # ASCII FS: str.splitlines() splits on it, so no line holds it
_EMITTER = (
    '$0 == frame_end { print frame_end _lexer_open_state(); _lexer_reset(); next }\n'
    '{ print _strip_line($0) }\n'
)
_OPEN_STATES = frozenset({"", "block_comment", "raw_string", "string"})


def _cannot_lex(message):
    """Exit 3 ("could not run") before anything reaches stdout, so a caller
    parsing the JSON gets nothing rather than a partial envelope."""
    print(f"audit-orphan-producers.sh: {message}", file=sys.stderr)
    sys.exit(3)


class _FrameError(Exception):
    """The lexer's framed output does not match the files framed into it."""


def _framed_stream(sources):
    """awk's input: Python's OWN splitlines() lines of each `(path, lines)`,
    each file followed by a line holding only _FRAME_END. Feeding Python's
    lines means awk's row numbering cannot drift from Python's (awk alone
    splits on newline only)."""
    return "".join(
        "".join(line + "\n" for line in lines) + _FRAME_END + "\n"
        for _, lines in sources
    )


def _parse_frames(rows, sources):
    """Split the lexer's output `rows` back into one `(code_view, open_state)`
    per `(path, lines)` in `sources`, in order.

    awk prints one code row per input line and, at each separator, _FRAME_END
    followed by the open state ("" when the file ends cleanly). The frame
    count and every file's row count must match the input; a mismatch or an
    unknown state raises _FrameError naming the file and both counts.
    """
    views, code = [], []
    for row in rows:
        if not row.startswith(_FRAME_END):
            code.append(row)
            continue
        state = row[len(_FRAME_END):]
        if state not in _OPEN_STATES:
            raise _FrameError(f"shared Rust lexer reported an unknown open state {state!r}")
        views.append((code, state or None))
        code = []
    for (path, lines), (view, _) in zip(sources, views):
        if len(view) != len(lines):
            raise _FrameError(f"shared Rust lexer returned {len(view)} code rows for "
                              f"the {len(lines)} lines of {path}")
    if code or len(views) != len(sources):
        unframed = sources[len(views)][0] if len(views) < len(sources) else "none"
        raise _FrameError(f"shared Rust lexer returned {len(views)} file frames for "
                          f"{len(sources)} files (first unframed: {unframed})")
    return views


def _run_lexer(stream):
    """Run $RUST_LEXER_AWK plus _EMITTER over `stream` in ONE awk process and
    return its output rows; one awk per file was measured at ~20s wall. An
    empty lexer, a failed start or a non-zero exit is exit 3.

    Deliberately not pinned to LC_ALL=C, though awk lexes faster there: byte
    mode splits a non-ASCII char literal such as '—', so its quotes read as
    lifetimes, and in `['é','{']` the `{` then reaches brace counting."""
    program = os.environ.get("RUST_LEXER_AWK", "")
    if not program:
        _cannot_lex("RUST_LEXER_AWK (the shared Rust lexer) is empty")
    try:
        proc = subprocess.run(
            ["awk", "-v", f"frame_end={_FRAME_END}", program + "\n" + _EMITTER],
            input=stream, stdout=subprocess.PIPE,
            text=True, encoding="utf-8", errors="replace",
        )
    except OSError as e:
        _cannot_lex(f"awk (shared Rust lexer) failed to start: {e}")
    if proc.returncode != 0:
        _cannot_lex(f"awk (shared Rust lexer) failed with exit {proc.returncode}")
    rows = proc.stdout.split("\n")
    if rows[-1] == "":
        rows.pop()
    return rows


def lex_code_views(sources):
    """Lex every `(path, lines)` in `sources` through the shared lexer; return
    `[(code_view, open_state), ...]` in that order.

    `code_view` has one row per line. `open_state` is None when the file ends
    cleanly, else "block_comment" / "raw_string" / "string": a construct left
    open at EOF blanks every row after its open point, which can hide a later
    `#[cfg(test)]` item's header shape SILENTLY (no brace count goes
    non-zero), so the caller warns on it separately. The lexer is reset at
    each file boundary, so no state leaks into the next file. Output that
    does not frame back onto `sources` exits 3.
    """
    if not sources:
        return []
    rows = _run_lexer(_framed_stream(sources))
    try:
        return _parse_frames(rows, sources)
    except _FrameError as e:
        _cannot_lex(str(e))


def mask_cfg_test(lines, code):
    """Mark lines belonging to `#[cfg(test)]`-attributed items.

    Three item shapes:
      1. Block item (`fn`, `mod`, `impl`, `struct`, `enum`, `trait`, `union`):
         mask via brace counting until depth returns to zero.
      2. Single-statement item (`use ...;`, `const ...;`): mask the line.
      3. Struct field / enum variant / match arm with `#[cfg(test)]`
         (line ends with `,`): mask the field line only.

    Brace counts, and the item-shape test that decides whether an item is
    a block (`fn`/`mod`/`impl`/`struct`/`enum`/`trait`/`union`) or a
    single statement/field (its `;`/`,` suffix), are computed from the
    caller-supplied literal/comment-stripped "code view" `code` (see
    `lex_code_views`), so `{`/`}` and keyword/suffix text
    inside string, raw string, char, or byte-string literals and
    line/block comments do not perturb either decision. Locating the item
    header itself -- skipping blank lines, line comments, and stacked
    attributes between the `#[cfg(test)]` attribute and the header -- also
    consults the code view, so a BLOCK comment alone on its own line
    (`/* ... */`, which the raw-text `//`/`#[` prefix checks do not
    recognise) is not mistaken for the header itself. The mask-START
    decision (`CFG_TEST_RE.search` below) reads that code view too, so an
    attribute merely MENTIONED -- in a line or block comment, in a
    `// G-allow:` marker's own reason text, or in a string literal -- no
    longer opens a mask over whatever item happens to follow it. One
    approximation remains: no full Rust lexing is performed beyond what
    these checks need.

    Corpus sweep for the literal-aware mask START (base main 32f4a7b098,
    scope `crates/reify-*/src`). Emphatically NOT neutral, unlike the
    literal-aware brace counting that preceded it: 2827 -> 2845 pub fns
    scanned, 637 -> 645 orphans, 124 -> 128 allow-listed. 13 pub fns become
    newly VISIBLE as orphans and 6 as allow-listed -- each had been
    swallowed whole by a mask that a comment or a literal opened over it.
    Five `persistent_cache.rs` fns LEAVE orphans[], because an over-masked
    region now contributes the real callers it always had. Two fns leave
    allowed[] -- `reify-ir` `capability_kind` and `reify-audit`
    `is_symbol_suppressed` -- by GAINING a real caller (0 -> 1), not by
    being hidden. The 13 newly-visible orphan rows are not silent corpus
    churn: they are enumerated and owned by #6429 for triage, and the
    baseline report they drift against is owned by #7634. Neither list is
    copied here, so neither can go stale against its owner.

    Returns (masked, unclosed): a per-line mask flag, and the 1-based
    attribute line of every block mask that runs to EOF without closing.
    """
    masked = [False] * len(lines)
    unclosed = []
    n = len(lines)
    i = 0
    while i < n:
        if not CFG_TEST_RE.search(code[i]):
            i += 1
            continue
        masked[i] = True
        # Skip blank lines, line comments, block comments, and stacked
        # attributes to find the actual item header. `code[j].strip() ==
        # ""` catches a line that is a block comment in its entirety (a
        # bare `//`/`#[` prefix test on RAW text cannot: the comment's own
        # delimiters `/*`...`*/` do not match either prefix), in addition
        # to the raw-text blank/`//` checks already below.
        j = i + 1
        while j < n:
            stripped = lines[j].lstrip()
            if stripped == "" or stripped.startswith("//") or code[j].strip() == "":
                masked[j] = True
                j += 1
                continue
            if stripped.startswith("#["):
                masked[j] = True
                j += 1
                continue
            break
        if j >= n:
            i = j
            continue

        header_stripped = code[j].rstrip()
        # `mod foo;`, `struct Foo;`, `extern fn ... ;` etc. are
        # block-keyword-bearing but single-statement; treat as field.
        is_block = (
            bool(BLOCK_KW_RE.search(code[j]))
            and not header_stripped.endswith(";")
            and not header_stripped.endswith(",")
        )
        # Multi-line header for a block ends in `{` on some later line.
        # Walk forward marking the header lines, find the opening `{`,
        # then brace-count to its match.
        if is_block:
            depth = 0
            entered = False
            closed = False
            k = j
            while k < n:
                masked[k] = True
                opens = code[k].count('{')
                closes = code[k].count('}')
                if not entered:
                    if opens > 0:
                        entered = True
                        depth = opens - closes
                        if depth <= 0:
                            k += 1
                            closed = True
                            break
                else:
                    depth += opens - closes
                    if depth <= 0:
                        k += 1
                        closed = True
                        break
                k += 1
            if not closed:
                # The while loop exited by exhausting `n`, not via the
                # depth <= 0 break -- do NOT infer this from `k == n`
                # alone, since a well-formed block that closes on the
                # file's last line also ends with k == n.
                unclosed.append(i + 1)
            i = k
        else:
            # Single-statement item or field/variant. Mask one line.
            masked[j] = True
            i = j + 1
    return masked, unclosed


def name_token_re(name):
    return re.compile(r'(?<![A-Za-z0-9_])' + re.escape(name) + r'(?![A-Za-z0-9_])')


candidates = []        # (file, line_1based, name, allowed, allow_reason)
masked_cache = {}      # path_str -> (lines, masked_flags)

file_lines = []        # (path, lines) for every readable source file
for path in src_files:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        print(f"audit-orphan-producers.sh: skip {path}: {e}", file=sys.stderr)
        continue
    file_lines.append((path, text.splitlines()))

# Only a file whose RAW text mentions the attribute is lexed; mask_cfg_test then
# matches the attribute on the code view. The asymmetry is deliberate, not an
# oversight to be tidied away: blanking literals and dropping comments only
# REMOVE matches (short of a comment spliced inside the attribute itself,
# `#[cfg(/* … */test)]`), so a raw-text miss is a code-view miss, and the
# majority of the corpus that never mentions the attribute stays out of awk.
needs_view = [(path, lines) for path, lines in file_lines
              if any(CFG_TEST_RE.search(l) for l in lines)]
code_views = dict(zip((path for path, _ in needs_view), lex_code_views(needs_view)))

for path, lines in file_lines:
    code, lexer_open_state = code_views.get(path, (None, None))
    if code is None:
        masked, unclosed = [False] * len(lines), []
    else:
        masked, unclosed = mask_cfg_test(lines, code)
    for lineno in unclosed:
        print(f"audit-orphan-producers.sh: WARNING: {path}:{lineno}: "
              f"#[cfg(test)] mask never closes and runs to EOF; any `pub fn` "
              f"below it is hidden from this audit", file=sys.stderr)
    if lexer_open_state is not None:
        print(f"audit-orphan-producers.sh: WARNING: {path}: "
              f"literal/comment lexer state {lexer_open_state} still open "
              f"at EOF; masking below may be wrong", file=sys.stderr)
    masked_cache[str(path)] = (lines, masked)
    for idx, line in enumerate(lines):
        if masked[idx]:
            continue
        m = PUB_FN_RE.match(line)
        if not m:
            continue
        name = m.group(2)
        allowed = False
        allow_reason = ""
        if idx > 0 and not masked[idx - 1]:
            am = G_ALLOW_RE.search(lines[idx - 1])
            if am:
                allowed = True
                allow_reason = am.group(1).strip()
        candidates.append((str(path), idx + 1, name, allowed, allow_reason))

if not quiet:
    print(f"audit-orphan-producers.sh: {len(candidates)} pub-fn candidates; counting callers",
          file=sys.stderr)

by_name = defaultdict(list)
for c in candidates:
    by_name[c[2]].append(c)

# Pre-compute per-file "candidate-free" content: strip line comments,
# drop `use`/`pub use` lines (including multi-line `use foo::{...};`
# blocks where the body lines are bare identifier lists), drop the
# candidate declaration lines themselves so a name doesn't count
# itself as its own caller.
prepped_cache = {}
candidate_decl_lines = defaultdict(set)  # path -> set of line indices (0-based)
for c in candidates:
    candidate_decl_lines[c[0]].add(c[1] - 1)

for path_str, (lines, masked) in masked_cache.items():
    prepped = []
    decl_idx = candidate_decl_lines.get(path_str, set())
    in_use_block = False
    for idx, line in enumerate(lines):
        if masked[idx] or idx in decl_idx:
            prepped.append("")
            continue
        if in_use_block:
            prepped.append("")
            if ";" in line:
                in_use_block = False
            continue
        if USE_RE.match(line):
            prepped.append("")
            if ";" not in line:
                in_use_block = True
            continue
        prepped.append(LINE_COMMENT_RE.sub("", line))
    prepped_cache[path_str] = prepped

# Single pass over corpus: tokenize each prepped line and increment
# per-name per-file hit counters when a token matches a candidate name.
WORD_RE = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
# USE_RE strips `use`/`pub use` lines but NOT `mod`/`pub mod` declarations.
# A fn whose name collides with its module name would otherwise see the
# `pub mod NAME;` declaration as a phantom caller.  MOD_DECL_RE identifies
# the NAME span inside those declarations so it can be skipped.
MOD_DECL_RE = re.compile(r'\bmod\s+([A-Za-z_][A-Za-z0-9_]*)')
all_names = set(by_name.keys())
hits = defaultdict(lambda: defaultdict(int))  # name -> path -> count

# Build the set of names that actually appear as module declarations in the
# corpus.  The `NAME::` path-qualifier skip (below) is scoped to this set so
# that a future fn whose name coincidentally matches an unrelated type's
# path-prefix is never incorrectly excluded.
mod_decl_names = {
    m.group(1)
    for prepped in prepped_cache.values()
    for line in prepped
    if line
    for m in MOD_DECL_RE.finditer(line)
}

for path_str, prepped in prepped_cache.items():
    for line in prepped:
        if not line:
            continue
        # Precompute span set of NAME positions in `mod NAME` / `pub mod NAME`
        # declarations so they can be excluded without misidentifying real calls.
        mod_spans = {m.span(1) for m in MOD_DECL_RE.finditer(line)}
        for m in WORD_RE.finditer(line):
            word = m.group(0)
            if word not in all_names:
                continue
            # Skip the NAME token of a `mod NAME` / `pub mod NAME` declaration;
            # that is a module declaration, not a function call.
            if m.span() in mod_spans:
                continue
            # Skip `NAME::` path-qualifier references (module or type path), but
            # only when NAME is known to be a module (appears in mod_decl_names).
            # This prevents a fn whose name coincidentally matches an unrelated
            # path-prefix from being silently dropped.  Turbofish `NAME::<T>()`
            # calls (`::` followed by `<`) are preserved as real calls.
            # v1 accepted limitation: whitespace between NAME and `::`, or a `::`
            # split across lines, is not detected — same approximation posture as
            # the rest of the script.
            if word in mod_decl_names:
                after = line[m.end():m.end() + 3]
                if after.startswith("::") and not after.startswith("::<"):
                    continue
            hits[word][path_str] += 1

results = []
for name, cands in by_name.items():
    per_file = hits.get(name, {})
    total = sum(per_file.values())
    for path_str, lineno, _, allowed, reason in cands:
        external = total - per_file.get(path_str, 0)
        results.append({
            "file": path_str,
            "line": lineno,
            "name": name,
            "callers": external,
            "allowed": allowed,
            "allow_reason": reason,
        })


def crate_of(path_str):
    parts = Path(path_str).parts
    for i, p in enumerate(parts):
        if p == "crates" and i + 1 < len(parts):
            return parts[i + 1]
    return "unknown"


results.sort(key=lambda r: (crate_of(r["file"]), r["file"], r["line"]))

orphans = [r for r in results if r["callers"] == 0 and not r["allowed"]]
allowed_orphans = [r for r in results if r["callers"] == 0 and r["allowed"]]

if format_ == "json":
    out = {
        "total_pub_fns_scanned": len(results),
        "orphan_count": len(orphans),
        "allowed_count": len(allowed_orphans),
        "orphans": orphans,
        "allowed": allowed_orphans,
    }
    json.dump(out, sys.stdout, indent=2)
    sys.stdout.write("\n")
elif format_ == "markdown":
    print("# Orphan-producer audit (Portfolio approach G)")
    print()
    print("Public functions in `crates/reify-*/src/` whose only callers are")
    print("tests, the defining file itself, comments, or `use`/`pub use`")
    print("re-exports.")
    print()
    print(f"- **Scanned:** {len(results)} `pub fn` declarations across {len(masked_cache)} files")
    print(f"- **Orphan candidates:** {len(orphans)}  (zero non-test callers, no `// G-allow:`)")
    print(f"- **Allow-listed:** {len(allowed_orphans)}  (zero callers; marked legitimate API surface)")
    print()
    if orphans:
        print("## Orphan candidates")
        print()
        print("| Crate | File:Line | Function |")
        print("|---|---|---|")
        for r in orphans:
            print(f"| `{crate_of(r['file'])}` | `{r['file']}:{r['line']}` | `{r['name']}` |")
        print()
    if allowed_orphans:
        print("## Allow-listed (zero callers, intentional)")
        print()
        print("| Crate | File:Line | Function | Reason |")
        print("|---|---|---|---|")
        for r in allowed_orphans:
            print(f"| `{crate_of(r['file'])}` | `{r['file']}:{r['line']}` | `{r['name']}` | {r['allow_reason']} |")
        print()
    print("---")
    print()
    print("Generated by `scripts/audit-orphan-producers.sh`.")
    print("Design: `docs/architecture-audit/g-reviewer-tool-session-prompt.md`.")
else:
    print(f"audit-orphan-producers.sh: unknown format {format_}", file=sys.stderr)
    sys.exit(2)

if strict and orphans:
    sys.exit(1)
PYTHON_SCRIPT
