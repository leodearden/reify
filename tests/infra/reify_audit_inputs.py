"""
reify_audit_inputs.py — the paths reify-audit's PTODO, PDIAG and PDOCCOVER
ratchets read, derived from its Rust source as probe corpora for the infra
selectors that copy those predicates.

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.

Each derivation reads the same definitions tests/infra/test_verify_scope.sh's
PT-DRIFT, PDIAG-DRIFT and PDOCCOVER-DRIFT scrape, so every copy of a predicate
is probed against the one Rust source. A derivation that finds nothing returns
an empty corpus; the caller asserts it is not.
"""

import re
from dataclasses import dataclass
from pathlib import Path

PTODO_RS = "crates/reify-audit/src/ptodo.rs"
PDIAG_RS = "crates/reify-audit/src/pdiag.rs"
PDOCCOVER_RS = "crates/reify-audit/src/pdoccover.rs"
PDOCCOVER_BASELINE_RS = "crates/reify-audit/src/pdoccover_baseline.rs"

_ENDS_WITH_EXTENSION = re.compile(r'ends_with\("\.([a-z]*)"\)')
_TEST_FN = re.compile(r"\s*fn [a-z0-9_]+\(\)")
_ARRAY_START = re.compile(r"for path in \[")
_ARRAY_END = re.compile(r"\s*\] \{")
_QUOTED = re.compile(r'"([^"]*)"')
_LITERAL_ASSERT = re.compile(r'assert!\((!?)is_swept_path\("([^"]*)"')
_ANY_ASSERT = re.compile(r"assert!\(!?is_swept_path\(")
_EXCLUDE_ENTRY = re.compile(r'\s*"([^"]*)",')
_CHUNK_SUFFIX = re.compile(r'starts_with\(CHUNKS_PREFIX\) && path\.ends_with\("([^"]*)"\)')
_ORACLE_PAIR = re.compile(r'starts_with\("([^"]*)"\) && path\.ends_with\("([^"]*)"\)')


def _lines(root: Path, path: str) -> list[str]:
    return (root / path).read_text(encoding="utf-8").splitlines()


def _block(lines: list[str], start: str, end: str) -> list[str]:
    """The first run of lines from one starting `start` to the next starting `end` (sed's /a/,/b/)."""
    for n, line in enumerate(lines):
        if line.startswith(start):
            for m in range(n + 1, len(lines)):
                if lines[m].startswith(end):
                    return lines[n : m + 1]
            return lines[n:]
    return []


def _const(lines: list[str], name: str) -> str:
    """A `pub const NAME: &str = "...";` literal, or "" when absent."""
    pattern = re.compile(rf'pub const {name}: &str = "([^"]*)";')
    found = [match[1] for match in map(pattern.fullmatch, lines) if match]
    return found[0] if found else ""


def ptodo_swept_extensions(root: Path) -> list[str]:
    """ptodo.rs::is_swept_ext's extensions, without the dot."""
    body = _block(_lines(root, PTODO_RS), "pub fn is_swept_ext", "}")
    return sorted({ext for line in body for ext in _ENDS_WITH_EXTENSION.findall(line)})


@dataclass(frozen=True)
class PdiagCorpus:
    """is_swept_path's own unit-test corpus, by the polarity its asserts state."""

    swept: tuple[str, ...]
    not_swept: tuple[str, ...]
    test_calls: int
    read_calls: int
    excluded_prefixes: tuple[str, ...]


def pdiag_corpus(root: Path) -> PdiagCorpus:
    """A `for path in [...]` array takes the polarity of the assert after it; a literal assert,
    its own. test_calls counts the test module's is_swept_path lines, read_calls those in an
    assert form read here, so a reshaped test cannot drop out unseen."""
    lines = _lines(root, PDIAG_RS)
    swept: list[str] = []
    not_swept: list[str] = []
    pending: list[str] = []
    in_array = False
    for line in lines:
        if _TEST_FN.match(line):
            pending = []
        if _ARRAY_START.search(line):
            in_array = True
        elif in_array:
            if _ARRAY_END.match(line):
                in_array = False
            elif quoted := _QUOTED.search(line):
                pending.append(quoted[1])
        elif "assert!(is_swept_path(path)" in line:
            swept.extend(pending)
            pending = []
        elif "assert!(!is_swept_path(path)" in line:
            not_swept.extend(pending)
            pending = []
        elif literal := _LITERAL_ASSERT.search(line):
            (not_swept if literal[1] else swept).append(literal[2])
    starts = [n for n, line in enumerate(lines) if line.startswith("mod tests {")]
    tests = lines[starts[0]:] if starts else []
    code = [line for line in tests if not line.lstrip().startswith("//")]
    excluded = _block(lines, "const SCOPE_EXCLUDE_PREFIXES", "];")
    return PdiagCorpus(
        swept=tuple(swept),
        not_swept=tuple(not_swept),
        test_calls=sum("is_swept_path(" in line for line in code),
        read_calls=sum(bool(_ANY_ASSERT.search(line)) for line in tests),
        excluded_prefixes=tuple(match[1] for match in map(_EXCLUDE_ENTRY.match, excluded) if match),
    )


@dataclass(frozen=True)
class PdoccoverInputs:
    """What the PDOCCOVER scanner reads: (symbol, prefix, suffix) roots and (symbol, path) exact paths."""

    roots: tuple[tuple[str, str, str], ...]
    exact: tuple[tuple[str, str], ...]
    oracle_starts: int
    oracle_pairs: int


def pdoccover_inputs(root: Path) -> PdoccoverInputs:
    """is_chunk_path and in_oracle_scope's prefix/suffix roots; UNITS_PATH and BASELINE_PATH.
    oracle_starts counts in_oracle_scope's starts_with calls, oracle_pairs those read here."""
    lines = _lines(root, PDOCCOVER_RS)
    chunks_prefix = _const(lines, "CHUNKS_PREFIX")
    chunk_body = _block(lines, "fn is_chunk_path(", "}")
    chunk_suffixes = [suffix for line in chunk_body for suffix in _CHUNK_SUFFIX.findall(line)]
    oracle_body = _block(lines, "fn in_oracle_scope(", "}")
    pairs = [pair for line in oracle_body for pair in _ORACLE_PAIR.findall(line)]
    chunk_roots = [("is_chunk_path", chunks_prefix, suffix) for suffix in chunk_suffixes if chunks_prefix]
    exact = (
        ("UNITS_PATH", _const(lines, "UNITS_PATH")),
        ("BASELINE_PATH", _const(_lines(root, PDOCCOVER_BASELINE_RS), "BASELINE_PATH")),
    )
    return PdoccoverInputs(
        roots=(*chunk_roots, *(("in_oracle_scope", prefix, suffix) for prefix, suffix in pairs)),
        exact=tuple((symbol, path) for symbol, path in exact if path),
        oracle_starts=sum(line.count("starts_with(") for line in oracle_body),
        oracle_pairs=len(pairs),
    )
