#!/usr/bin/env python3
"""
assert-module-scc.py — the intra-crate module-SCC ratchet (INV-ARCH-1).

For every workspace crate, measure the strongly connected components of the
graph whose nodes are the crate's top-level modules and whose edges are its
production `crate::<module>` references, then compare them with the committed
baseline. The rule (universe, edges, production text) is PRD
docs/prds/v0_6/dependency-invalidation-stratum.md §5.1.

    check   exit 0 iff every crate's measured SCCs equal its baseline row
    print   write the current measurement to stdout in baseline format

Reading a failure:
    GREW <crate>: {a, b, …}   an SCC appeared, grew or merged. The indented
                              `path:line: u -> v` lines under it are the imports
                              that close it; moving one of them breaks it.
    STALE <crate>: …          a baselined SCC shrank or dissolved. Lower the row
                              in the same diff, so the shrink is recorded.

Lowering the baseline (never redirect `print` onto the file it reads):

    python3 scripts/assert-module-scc.py print > /tmp/module-scc-baseline.json \\
        && mv /tmp/module-scc-baseline.json scripts/module-scc-baseline.json

The gate runs `check` from crates/reify-build-utils/tests/module_scc_ratchet.rs:
always in the merge gate, but in a per-task lane only when reify-build-utils is
affected — so run `check` yourself after moving imports.

Exit codes: 0 measurement matches the baseline; 1 ratchet violation (GREW or
STALE); 2 an input could not be trusted (manifest, baseline or lexer).
"""

from __future__ import annotations

import argparse
import bisect
import json
import os
import re
import subprocess
import sys
import tomllib
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from enum import IntEnum
from itertools import accumulate
from pathlib import Path


class Verdict(IntEnum):
    OK = 0
    VIOLATION = 1
    INPUT_ERROR = 2


class InputError(Exception):
    """An input the measurement cannot trust. Always names the input."""


@dataclass(frozen=True)
class Member:
    package: str
    src: Path


@dataclass(frozen=True)
class Baseline:
    excluded: Mapping[str, str]
    crates: Mapping[str, frozenset[frozenset[str]]]


@dataclass(frozen=True)
class Site:
    """Where one edge is written: a path relative to --root, 1-based line."""
    path: str
    line: int


SCCs = frozenset[frozenset[str]]
Graph = Mapping[str, Mapping[str, Sequence[Site]]]

BASELINE_SCHEMA = 1
CRATE_ROOTS = frozenset({"lib.rs", "main.rs"})
MOD_ROOTS = CRATE_ROOTS | {"mod.rs"}


# --- workspace and baseline -------------------------------------------------

def read_bytes(path: Path, what: str) -> bytes:
    try:
        return path.read_bytes()
    except FileNotFoundError:
        raise InputError(f"{path}: missing {what}") from None
    except OSError as error:
        raise InputError(f"{path}: unreadable {what}: {error.strerror}") from None


def read_text(path: Path, what: str) -> str:
    try:
        return read_bytes(path, what).decode("utf-8")
    except UnicodeDecodeError as error:
        raise InputError(f"{path}: unreadable {what}: {error}") from None


def read_toml(path: Path, what: str) -> dict:
    try:
        return tomllib.loads(read_text(path, what))
    except tomllib.TOMLDecodeError as error:
        raise InputError(f"{path}: unparseable {what}: {error}") from None


def table(document: dict, key: str) -> dict:
    value = document.get(key)
    return value if isinstance(value, dict) else {}


def workspace_members(root: Path) -> tuple[Member, ...]:
    """Every workspace member that has a src/ directory, sorted by package."""
    manifest = root / "Cargo.toml"
    entries = table(read_toml(manifest, "workspace manifest"), "workspace").get("members")
    if not (isinstance(entries, list) and all(isinstance(e, str) for e in entries)):
        raise InputError(f"{manifest}: no [workspace].members list of paths")
    members = []
    for entry in entries:
        member_dirs = sorted(path for path in root.glob(entry) if path.is_dir())
        if not member_dirs:
            raise InputError(f"{manifest}: member {entry!r} matches no directory")
        for member_dir in member_dirs:
            package_manifest = member_dir / "Cargo.toml"
            package = table(read_toml(package_manifest, "package manifest"),
                            "package").get("name")
            if not (isinstance(package, str) and package):
                raise InputError(f"{package_manifest}: no [package].name")
            if (member_dir / "src").is_dir():
                members.append(Member(package=package, src=member_dir / "src"))
    return tuple(sorted(members, key=lambda m: m.package))


def baseline_row(path: Path, crate: str, rows: object) -> SCCs:
    where = f"{path}: crates[{crate!r}]"
    if not isinstance(rows, list):
        raise InputError(f"{where} must be a list of module lists")
    sccs: list[frozenset[str]] = []
    for row in rows:
        if not (isinstance(row, list) and all(isinstance(m, str) for m in row)):
            raise InputError(f"{where}: {row!r} is not a list of module names")
        if len(row) < 2 or len(set(row)) != len(row):
            raise InputError(f"{where}: {row!r} is not two or more distinct modules")
        for earlier in sccs:
            if earlier & set(row):
                raise InputError(
                    f"{where}: rows must be disjoint, but {braces(earlier & set(row))} "
                    "is in two")
        sccs.append(frozenset(row))
    return frozenset(sccs)


def load_baseline(path: Path) -> Baseline:
    """The one home of baseline-shape validation; both subcommands call it."""
    try:
        document = json.loads(read_text(path, "baseline"))
    except json.JSONDecodeError as error:
        raise InputError(f"{path}: unparseable baseline: {error}") from None
    if not isinstance(document, dict):
        raise InputError(f"{path}: the baseline must be a JSON object")
    schema = document.get("schema")
    if type(schema) is not int or schema != BASELINE_SCHEMA:
        raise InputError(f"{path}: schema must be {BASELINE_SCHEMA}, found {schema!r}")
    excluded = document.get("excluded")
    if not (isinstance(excluded, dict)
            and all(isinstance(reason, str) and reason for reason in excluded.values())):
        raise InputError(f"{path}: excluded must map each package to a non-empty reason")
    crates = document.get("crates")
    if not isinstance(crates, dict):
        raise InputError(f"{path}: crates must map each package to its SCC rows")
    return Baseline(
        excluded=dict(excluded),
        crates={crate: baseline_row(path, crate, rows) for crate, rows in crates.items()},
    )


def validate_against_workspace(baseline: Baseline, members: Sequence[Member]) -> None:
    """A row for a removed crate is left to surface as STALE; these cannot."""
    packages = {member.package for member in members}
    for package in sorted(baseline.excluded):
        if package not in packages:
            raise InputError(f"the baseline excludes {package!r}, which is not a "
                             "workspace package with a src/ directory")
        if package in baseline.crates:
            raise InputError(f"the baseline both excludes {package!r} and records "
                             "SCCs for it")


# --- lexing through the shared Rust lexer ------------------------------------

LEXER_LIB = Path(__file__).resolve().parent / "lib_rust_production_view.sh"
LEXER_SPAWN = 'source "$1" && prog="$RUST_LEXER_AWK\n$2" && shift 2 && awk "$prog" "$@"'
FILE_MARK, CODE_MARK, OPEN_MARK = "\x01", "|", "\x02"
LEXER_DRIVER = r"""
function _report_open(file,   state) {
    state = _lexer_open_state()
    if (state != "") print "\002" file "\t" state
}
FNR == 1 {
    if (NR > 1) _report_open(previous)
    _lexer_reset()
    previous = FILENAME
    print "\001" FILENAME
}
{ print "|" _strip_line($0) }
END { if (NR > 0) _report_open(previous) }
"""


def raw_line_count(data: bytes) -> int:
    return data.count(b"\n") + (0 if not data or data.endswith(b"\n") else 1)


def lex_crate(files: Sequence[Path]) -> dict[Path, list[str]]:
    """Each file's lines as the shared lexer's production-code view."""
    if not files:
        return {}
    expected = {path: raw_line_count(read_bytes(path, "source file")) for path in files}
    try:
        result = subprocess.run(
            ["bash", "-c", LEXER_SPAWN, "bash", str(LEXER_LIB), LEXER_DRIVER,
             *map(str, files)],
            capture_output=True, env={**os.environ, "LC_ALL": "C"},
        )
    except OSError as error:
        raise InputError(f"{LEXER_LIB.name}: could not start the lexer: {error}") from None
    if result.returncode != 0:
        raise InputError(
            f"{LEXER_LIB.name}: lexer exited {result.returncode}: "
            f"{result.stderr.decode('utf-8', errors='replace').strip()}")
    by_name: dict[str, list[str]] = {}
    current: list[str] = []
    for record in result.stdout.decode("utf-8", errors="replace").split("\n")[:-1]:
        mark, body = record[:1], record[1:]
        if mark == FILE_MARK:
            current = by_name.setdefault(body, [])
        elif mark == CODE_MARK:
            current.append(body)
        elif mark == OPEN_MARK:
            name, state = body.rsplit("\t", 1)
            raise InputError(f"{name}: lexer state unbalanced at EOF ({state})")
        else:
            raise InputError(f"unexpected lexer output record {record!r}")
    lexed = {}
    for path in files:
        lines = by_name.get(str(path), [])
        if len(lines) != expected[path]:
            raise InputError(
                f"{path}: lexer emitted {len(lines)} lines for {expected[path]}")
        lexed[path] = lines
    return lexed


# --- edges --------------------------------------------------------------------

ROOT_PATH_RE = re.compile(
    r"(?<![A-Za-z0-9_])(crate\s*::|(?:super\s*::\s*)+)\s*(\{|[A-Za-z_][A-Za-z0-9_]*)")
INLINE_MOD_RE = re.compile(r"(?<![A-Za-z0-9_])mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{")
IDENT_RE = re.compile(r"\s*([A-Za-z_][A-Za-z0-9_]*)")
LIST_SKIP = frozenset({"self", "super"})
CLOSERS = {"[": re.compile(r"[\[\]]"), "{": re.compile(r"[{}]")}


def matching_close(text: str, open_index: int) -> int | None:
    """Offset of the bracket that closes the `[` or `{` at open_index."""
    opener = text[open_index]
    depth = 0
    for match in CLOSERS[opener].finditer(text, open_index):
        depth += 1 if match.group() == opener else -1
        if depth == 0:
            return match.start()
    return None


@dataclass(frozen=True)
class Nesting:
    """How far below the crate root each offset of one file sits: the file's
    own module depth plus the inline `mod NAME { … }` blocks around it."""
    file_depth: int
    inline_spans: tuple[tuple[int, int], ...]

    def depth_at(self, offset: int) -> int:
        return self.file_depth + sum(start < offset < end
                                     for start, end in self.inline_spans)


def nesting(src: Path, path: Path, text: str) -> Nesting:
    parts = path.relative_to(src).parts
    file_depth = len(parts) - 1 if parts[-1] in MOD_ROOTS else len(parts)
    spans = []
    for match in INLINE_MOD_RE.finditer(text):
        close = matching_close(text, match.end() - 1)
        spans.append((match.end() - 1, len(text) if close is None else close))
    return Nesting(file_depth, tuple(spans))


def crate_list_names(text: str, open_brace: int) -> list[tuple[str, int]]:
    """(name, offset) of each depth-1 element head of the `{…}` at open_brace."""
    names = []
    depth = 0
    for index in range(open_brace, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                break
        if depth == 1 and char in "{,":
            head = IDENT_RE.match(text, index + 1)
            if head and head.group(1) not in LIST_SKIP:
                names.append((head.group(1), head.start(1)))
    return names


def referenced_modules(text: str, nesting: Nesting) -> Iterable[tuple[str, int]]:
    """(name, offset) of every `crate::name` and `crate::{name, …}` head, and of
    the same heads after a `super::` chain that climbs exactly to the crate root
    (a shorter chain stays inside the file's own top-level module)."""
    for match in ROOT_PATH_RE.finditer(text):
        supers = match.group(1).count("super")
        if supers and supers != nesting.depth_at(match.start()):
            continue
        if match.group(2) == "{":
            yield from crate_list_names(text, match.start(2))
        else:
            yield match.group(2), match.start(2)


# --- production text: test-only cfg items ----------------------------------------

ATTRIBUTE_OR_BRACE_RE = re.compile(r"[{}]|#\s*\[")
CFG_ATTRIBUTE_RE = re.compile(r"\s*cfg\s*\((.*)\)\s*", re.DOTALL)
CFG_CALL_RE = re.compile(r"(all|any|not)\((.*)\)", re.DOTALL)
NEXT_ATTRIBUTE_RE = re.compile(r"\s*#\s*\[")
VISIBILITY_RE = re.compile(r"\s*pub\b(\s*\([^()]*\))?")
ITEM_TOKEN_RE = re.compile(r"[()\[\]{};,]")
ITEM_KEYWORDS = frozenset({
    "mod", "fn", "struct", "enum", "union", "impl", "trait", "const", "static",
    "use", "type", "extern", "macro_rules", "unsafe", "async",
})


@dataclass(frozen=True)
class TestOnly:
    """A file's test-only text: sorted [start, end) spans of cfg-test items,
    and the names of test-only `mod NAME` declarations at its top level."""
    spans: tuple[tuple[int, int], ...]
    module_names: frozenset[str]

    def covers(self, offset: int) -> bool:
        index = bisect.bisect_right(self.spans, (offset, float("inf"))) - 1
        return index >= 0 and offset < self.spans[index][1]


def split_cfg_args(args: str) -> list[str]:
    parts, depth, start = [], 0, 0
    for index, char in enumerate(args):
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(args[start:index])
            start = index + 1
    parts.append(args[start:])
    return [part for part in parts if part]


def cfg_implies_test(predicate: str) -> bool:
    """True when every build that satisfies the cfg predicate is a test build."""
    squeezed = re.sub(r"\s+", "", predicate)
    if squeezed == "test":
        return True
    call = CFG_CALL_RE.fullmatch(squeezed)
    if call is None:
        return False
    operator, args = call.group(1), split_cfg_args(call.group(2))
    if operator == "all":
        return any(cfg_implies_test(arg) for arg in args)
    if operator == "any":
        return bool(args) and all(cfg_implies_test(arg) for arg in args)
    return False


def item_head(text: str, pos: int) -> int:
    """Skip further outer attributes and a visibility, to the item's first word."""
    while (attribute := NEXT_ATTRIBUTE_RE.match(text, pos)) is not None:
        close = matching_close(text, attribute.end() - 1)
        if close is None:
            return len(text)
        pos = close + 1
    visibility = VISIBILITY_RE.match(text, pos)
    return visibility.end() if visibility else pos


def item_end(text: str, head: int, is_item: bool) -> int:
    """End of the attributed thing at head: after a depth-0 `;`, after the `}`
    that closes its body, after a depth-0 `,` when it is not an item (a field,
    variant, initialiser or arm), and never past its parent's closer."""
    depth = 0
    for match in ITEM_TOKEN_RE.finditer(text, head):
        token = match.group()
        if token in "([{":
            depth += 1
        elif token in ")]}":
            if depth == 0:
                return match.start()
            depth -= 1
            if depth == 0 and token == "}":
                return match.end()
        elif depth == 0 and (token == ";" or not is_item):
            return match.end()
    return len(text)


def scan_test_only(text: str) -> TestOnly:
    spans: list[tuple[int, int]] = []
    module_names: set[str] = set()
    depth = 0
    pos = 0
    while (match := ATTRIBUTE_OR_BRACE_RE.search(text, pos)) is not None:
        pos = match.end()
        if match.group() in "{}":
            depth += 1 if match.group() == "{" else -1
            continue
        close = matching_close(text, match.end() - 1)
        if close is None:
            continue
        pos = close + 1
        cfg = CFG_ATTRIBUTE_RE.fullmatch(text, match.end(), close)
        if cfg is None or not cfg_implies_test(cfg.group(1)):
            continue
        head = item_head(text, pos)
        keyword = IDENT_RE.match(text, head)
        word = keyword.group(1) if keyword else ""
        if word == "mod" and depth == 0:
            name = IDENT_RE.match(text, keyword.end())
            if name is not None:
                module_names.add(name.group(1))
        pos = item_end(text, head, word in ITEM_KEYWORDS)
        spans.append((match.start(), pos))
    return TestOnly(tuple(spans), frozenset(module_names))


def moddir(path: Path) -> Path:
    """Where the out-of-line modules a file declares live."""
    return path.parent if path.name in MOD_ROOTS else path.parent / path.stem


def test_module_files(files: Iterable[Path], scans: Mapping[Path, TestOnly]) -> set[Path]:
    """Files of every test-only module declaration, at any depth."""
    roots = {moddir(path) / name for path, scan in scans.items()
             for name in scan.module_names}
    root_files = {root.parent / f"{root.name}.rs" for root in roots}
    return {path for path in files
            if path in root_files or not roots.isdisjoint(path.parents)}


# --- one crate ------------------------------------------------------------------

def top_level_module(src: Path, path: Path) -> str | None:
    parts = path.relative_to(src).parts
    if len(parts) == 1:
        return None if parts[0] in CRATE_ROOTS else path.stem
    return parts[0]


def module_files(src: Path, files: Iterable[Path]) -> dict[str, list[Path]]:
    modules: dict[str, list[Path]] = {}
    for path in files:
        module = top_level_module(src, path)
        if module is not None:
            modules.setdefault(module, []).append(path)
    return modules


def line_starts(lines: Sequence[str]) -> list[int]:
    return list(accumulate((len(line) + 1 for line in lines[:-1]), initial=0))


def module_graph(root: Path, member: Member) -> Graph:
    files = sorted(member.src.rglob("*.rs"))
    lexed = lex_crate(files)
    texts = {path: "\n".join(lines) for path, lines in lexed.items()}
    scans = {path: scan_test_only(text) for path, text in texts.items()}
    excluded = test_module_files(files, scans)
    modules = module_files(member.src, (p for p in files if p not in excluded))
    graph: dict[str, dict[str, list[Site]]] = {}
    for module, paths in sorted(modules.items()):
        edges: dict[str, list[Site]] = {}
        for path in paths:
            starts = line_starts(lexed[path])
            site_path = path.relative_to(root).as_posix()
            text_nesting = nesting(member.src, path, texts[path])
            for target, offset in referenced_modules(texts[path], text_nesting):
                if (target != module and target in modules
                        and not scans[path].covers(offset)):
                    edges.setdefault(target, []).append(
                        Site(site_path, bisect.bisect_right(starts, offset)))
        graph[module] = edges
    return graph


def tarjan_sccs(graph: Graph) -> SCCs:
    """Strongly connected components of two or more nodes."""
    index: dict[str, int] = {}
    lowlink: dict[str, int] = {}
    stack: list[str] = []
    on_stack: set[str] = set()
    components: set[frozenset[str]] = set()

    def visit(node: str) -> None:
        index[node] = lowlink[node] = len(index)
        stack.append(node)
        on_stack.add(node)
        for successor in sorted(graph.get(node, {})):
            if successor not in index:
                visit(successor)
                lowlink[node] = min(lowlink[node], lowlink[successor])
            elif successor in on_stack:
                lowlink[node] = min(lowlink[node], index[successor])
        if lowlink[node] == index[node]:
            component = set()
            while True:
                popped = stack.pop()
                on_stack.discard(popped)
                component.add(popped)
                if popped == node:
                    break
            if len(component) >= 2:
                components.add(frozenset(component))

    for node in sorted(graph):
        if node not in index:
            visit(node)
    return frozenset(components)


# --- comparison and rendering -------------------------------------------------

def braces(names: Iterable[str]) -> str:
    return "{" + ", ".join(sorted(names)) + "}"


def compare(crate: str, measured: SCCs, baseline: SCCs, graph: Graph) -> list[str]:
    def in_one_baseline_scc(u: str, v: str) -> bool:
        return any(u in scc and v in scc for scc in baseline)

    lines = []
    grew = [scc for scc in measured if not any(scc <= old for old in baseline)]
    for scc in sorted(grew, key=sorted):
        lines.append(f"GREW {crate}: {braces(scc)}")
        closing = sorted(
            (u, v, site.path, site.line)
            for u in scc for v, sites in graph[u].items() if v in scc
            if not in_one_baseline_scc(u, v)
            for site in sites)
        lines.extend(f"  {path}:{line}: {u} -> {v}" for u, v, path, line in closing)
    for old in sorted(baseline, key=sorted):
        if old in measured or any(old <= scc for scc in grew):
            continue
        remaining = [braces(scc) for scc in sorted(measured, key=sorted) if scc <= old]
        lines.append(
            f"STALE {crate}: baseline {braces(old)} measured "
            f"{' '.join(remaining) or '{}'} — lower the baseline")
    return lines


def render_baseline(excluded: Mapping[str, str], measured: Mapping[str, SCCs]) -> str:
    document = {
        "schema": BASELINE_SCHEMA,
        "excluded": dict(sorted(excluded.items())),
        "crates": {
            crate: sorted(sorted(scc) for scc in sccs)
            for crate, sccs in sorted(measured.items()) if sccs
        },
    }
    return json.dumps(document, indent=2, ensure_ascii=False) + "\n"


# --- entry point -----------------------------------------------------------------

def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    for name, purpose in (("check", "compare the measurement with the baseline"),
                          ("print", "print the measurement in baseline format")):
        command = commands.add_parser(name, help=purpose)
        command.add_argument("--root", type=Path,
                             default=Path(__file__).resolve().parents[1],
                             help="workspace root (default: this checkout)")
        command.add_argument("--baseline", type=Path,
                             help="baseline file (default: "
                                  "<root>/scripts/module-scc-baseline.json)")
    return parser.parse_args(argv)


def run(command: str, root: Path, baseline_path: Path) -> Verdict:
    baseline = load_baseline(baseline_path)
    members = workspace_members(root)
    validate_against_workspace(baseline, members)
    graphs = {member.package: module_graph(root, member)
              for member in members if member.package not in baseline.excluded}
    measured = {crate: tarjan_sccs(graph) for crate, graph in graphs.items()}
    if command == "print":
        sys.stdout.write(render_baseline(baseline.excluded, measured))
        return Verdict.OK
    violations = [
        line
        for crate in sorted(set(measured) | set(baseline.crates))
        for line in compare(crate, measured.get(crate, frozenset()),
                            baseline.crates.get(crate, frozenset()),
                            graphs.get(crate, {}))
    ]
    if violations:
        print("\n".join(violations))
        print("module-scc: FAIL — GREW: move a closing import; "
              "STALE: lower the baseline (command in --help)")
        return Verdict.VIOLATION
    excluded = ", ".join(sorted(baseline.excluded)) or "none"
    print(f"module-scc: OK ({len(measured)} crates measured; excluded: {excluded})")
    return Verdict.OK


def main(argv: Sequence[str]) -> int:
    args = parse_args(argv)
    baseline_path = args.baseline or args.root / "scripts" / "module-scc-baseline.json"
    try:
        return run(args.command, args.root, baseline_path)
    except (InputError, OSError) as error:
        print(f"assert-module-scc: {error}", file=sys.stderr)
        return Verdict.INPUT_ERROR


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
