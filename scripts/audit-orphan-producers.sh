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
# Exits 0 always unless --strict is passed (then exits 1 when orphans
# without `// G-allow:` markers are found).

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/audit-orphan-producers.sh [options]

Options:
  --format FMT   Output format: markdown (default) or json.
  --scope GLOB   Restrict to files matching GLOB (default: crates/reify-*/src).
                 Multiple --scope flags accumulate.
  --quiet        Suppress progress messages on stderr.
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

for tool in python3 git; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "audit-orphan-producers.sh: $tool not on PATH" >&2
        exit 3
    fi
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

[[ $QUIET == 0 ]] && echo "audit-orphan-producers.sh: scanning ${SCOPES[*]}" >&2

SCOPE_ARGS=()
for s in "${SCOPES[@]}"; do
    SCOPE_ARGS+=("$s")
done

python3 - "$FORMAT" "$STRICT" "$QUIET" "${SCOPE_ARGS[@]}" <<'PYTHON_SCRIPT'
import json
import re
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


# --- literal/comment-aware code view ---------------------------------------
#
# strip_literals_and_comments() builds a same-shape "code view" so brace
# counting (and, for cfg(test) item headers, keyword/suffix tests) can
# ignore braces that live inside strings, char literals, and comments.
# These are cheap "is there anything interesting left" / "jump to the next
# interesting position" probes; the heavy lifting (raw-string hash
# counting, char-literal offset tests) is plain string indexing once a
# candidate position is found, not backtracking regexes.
_CODE_SPECIAL_RE = re.compile(r'["\'/]')
_BLOCK_COMMENT_TOKEN_RE = re.compile(r'/\*|\*/')
_STRING_TOKEN_RE = re.compile(r'["\\]')
_HEX_DIGITS = set("0123456789abcdefABCDEF")
_IDENT_CHARS = set(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_"
)


def _char_literal_span(line, j, n):
    """Return the number of characters (starting at `j`, the opening `'`)
    consumed by a char literal at position j, or None if `line[j]` is a
    lifetime/loop-label tick rather than a char literal.

    A `'` opens a char literal ONLY when a closing `'` sits at the exact
    offset the escape-aware grammar implies:
      'X'         -> closing ' at offset 2
      '\\X'       -> closing ' at offset 3   (\\n \\t \\\\ \\' \\" \\0 ...)
      '\\xNN'     -> closing ' at offset 5   (NN hex digits)
      '\\u{...}'  -> closing ' immediately after the matching '}'
    Otherwise it is a lifetime or loop label (`&'a str`, `<'a>`,
    `'outer: loop`) and the caller must treat it as a single blank tick,
    never swallowing forward to the next `'`.
    """
    if j + 1 >= n:
        return None
    if line[j + 1] == '\\':
        if j + 3 < n and line[j + 2] == 'u' and line[j + 3] == '{':
            close = line.find('}', j + 4)
            if close != -1 and close + 1 < n and line[close + 1] == "'":
                return close - j + 2
        if (
            j + 5 < n
            and line[j + 2] == 'x'
            and line[j + 3] in _HEX_DIGITS
            and line[j + 4] in _HEX_DIGITS
            and line[j + 5] == "'"
        ):
            return 6
        if j + 3 < n and line[j + 3] == "'":
            return 4
        return None
    if j + 2 < n and line[j + 2] == "'":
        return 3
    return None


def strip_literals_and_comments(lines):
    """Return a list of strings, PARALLEL to `lines` (same length, and
    each entry the same length as the corresponding input line), with
    every comment and string/char/raw-string-literal byte replaced by a
    space -- so a caller that only needs brace counts or a header's
    keyword/suffix shape sees code only.

    Handles, in precedence order: block comments (`/* ... */`, which
    NEST), raw strings (opened by `(b?r)(#*)"`, only when the preceding
    byte is not an identifier character; closed by `"` followed by
    exactly the opening hash count), normal/byte strings (`"`, opened
    directly or after a `b`; `\\` escapes the next byte; may span lines),
    line comments (`//` to EOL), and char literals vs. lifetime/loop-label
    ticks (see `_char_literal_span`).

    State (which of the above we are inside, plus block-comment nesting
    depth and raw-string hash count) is carried ACROSS lines within one
    call, and reset at the start of each call -- `mask_cfg_test` invokes
    this once per file, so a misparse is bounded to a single file.
    """
    out = []
    state = "code"  # "code" | "block_comment" | "raw_string" | "string"
    block_depth = 0
    raw_hashes = 0

    for line in lines:
        n = len(line)

        # Whole-line fast path: in code state with none of the trigger
        # characters present, the line cannot contain a comment or literal
        # opener, so it passes through unchanged.
        if state == "code" and not _CODE_SPECIAL_RE.search(line):
            out.append(line)
            continue

        buf = []
        i = 0
        while i < n:
            if state == "block_comment":
                m = _BLOCK_COMMENT_TOKEN_RE.search(line, i)
                if m is None:
                    buf.append(' ' * (n - i))
                    i = n
                    continue
                j = m.start()
                buf.append(' ' * (j - i))
                if line[j] == '/':  # nested "/*"
                    block_depth += 1
                else:  # "*/"
                    block_depth -= 1
                    if block_depth <= 0:
                        state = "code"
                        block_depth = 0
                buf.append('  ')
                i = j + 2
                continue

            if state == "raw_string":
                close = '"' + ('#' * raw_hashes)
                idx = line.find(close, i)
                if idx == -1:
                    buf.append(' ' * (n - i))
                    i = n
                    continue
                buf.append(' ' * (idx - i + len(close)))
                i = idx + len(close)
                state = "code"
                continue

            if state == "string":
                m = _STRING_TOKEN_RE.search(line, i)
                if m is None:
                    buf.append(' ' * (n - i))
                    i = n
                    continue
                j = m.start()
                buf.append(' ' * (j - i))
                if line[j] == '\\':
                    if j + 1 < n:
                        buf.append('  ')
                        i = j + 2
                    else:
                        buf.append(' ')
                        i = j + 1
                    continue
                buf.append(' ')
                i = j + 1
                state = "code"
                continue

            # state == "code"
            m = _CODE_SPECIAL_RE.search(line, i)
            if m is None:
                buf.append(line[i:])
                i = n
                continue
            j = m.start()
            ch = line[j]

            if ch == '/':
                if j + 1 < n and line[j + 1] == '/':
                    buf.append(line[i:j])
                    buf.append(' ' * (n - j))
                    i = n
                elif j + 1 < n and line[j + 1] == '*':
                    buf.append(line[i:j])
                    buf.append('  ')
                    state = "block_comment"
                    block_depth = 1
                    i = j + 2
                else:
                    # Lone '/' (division, path separator, ...): not a
                    # comment opener, copy verbatim.
                    buf.append(line[i:j + 1])
                    i = j + 1
                continue

            if ch == '"':
                # Backward scan for a raw-string opener: (b?r)(#*)" with a
                # non-identifier byte (or start of line) immediately before.
                h = 0
                k = j - 1
                while k >= 0 and line[k] == '#':
                    h += 1
                    k -= 1
                raw_start = None
                if k >= 0 and line[k] == 'r':
                    cand = k
                    pre = k - 1
                    if pre >= 0 and line[pre] == 'b':
                        cand = pre
                        pre -= 1
                    if pre < 0 or line[pre] not in _IDENT_CHARS:
                        raw_start = cand
                if raw_start is not None:
                    buf.append(line[i:raw_start])
                    buf.append(' ' * (j - raw_start + 1))
                    state = "raw_string"
                    raw_hashes = h
                    i = j + 1
                else:
                    buf.append(line[i:j])
                    buf.append(' ')
                    state = "string"
                    i = j + 1
                continue

            # ch == "'"
            span = _char_literal_span(line, j, n)
            if span is None:
                buf.append(line[i:j])
                buf.append(' ')
                i = j + 1
            else:
                buf.append(line[i:j])
                buf.append(' ' * span)
                i = j + span
            continue

        out.append(''.join(buf))

    return out


def mask_cfg_test(lines):
    """Mark lines belonging to `#[cfg(test)]`-attributed items.

    Three item shapes:
      1. Block item (`fn`, `mod`, `impl`, `struct`, `enum`, `trait`, `union`):
         mask via brace counting until depth returns to zero.
      2. Single-statement item (`use ...;`, `const ...;`): mask the line.
      3. Struct field / enum variant / match arm with `#[cfg(test)]`
         (line ends with `,`): mask the field line only.

    Brace counts, and the item-shape test that decides whether an item is
    a block (`fn`/`mod`/`impl`/`struct`/`enum`/`trait`/`union`) or a
    single statement/field (its `;`/`,` suffix), are computed from a
    literal/comment-stripped "code view" (see
    `strip_literals_and_comments`), so `{`/`}` and keyword/suffix text
    inside string, raw string, char, or byte-string literals and
    line/block comments do not perturb either decision. Two
    approximations remain: the `#[cfg(test)]` attribute match itself
    (`CFG_TEST_RE.search` below) still reads raw line text, and no full
    Rust lexing is performed beyond what these two checks need.
    """
    masked = [False] * len(lines)
    n = len(lines)
    if not any(CFG_TEST_RE.search(l) for l in lines):
        return masked
    code = strip_literals_and_comments(lines)
    i = 0
    while i < n:
        if not CFG_TEST_RE.search(lines[i]):
            i += 1
            continue
        masked[i] = True
        # Skip blank lines, line comments, and stacked attributes to find
        # the actual item header.
        j = i + 1
        while j < n:
            stripped = lines[j].lstrip()
            if stripped == "" or stripped.startswith("//"):
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
                            break
                else:
                    depth += opens - closes
                    if depth <= 0:
                        k += 1
                        break
                k += 1
            i = k
        else:
            # Single-statement item or field/variant. Mask one line.
            masked[j] = True
            i = j + 1
    return masked


def name_token_re(name):
    return re.compile(r'(?<![A-Za-z0-9_])' + re.escape(name) + r'(?![A-Za-z0-9_])')


candidates = []        # (file, line_1based, name, allowed, allow_reason)
masked_cache = {}      # path_str -> (lines, masked_flags)

for path in src_files:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        print(f"audit-orphan-producers.sh: skip {path}: {e}", file=sys.stderr)
        continue
    lines = text.splitlines()
    masked = mask_cfg_test(lines)
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
