# jcodemunch Rust reachability spike (PRD task θ)

**PRD:** `docs/prds/jcodemunch-substrate-restoration.md` task θ (Phase 3).
**Manifest:** `docs/prds/jcodemunch-substrate-restoration.capability-manifest.md` §2/θ.
**Measured:** 2026-08-23, by the task steward (esc-6113-2 handler).

> **Both outcomes are a pass.** This spike asked whether jcodemunch's reachability
> heuristic can be taught about Rust roots from outside the tool. It cannot. That
> is a delivered verdict, not a blocker.

---

## 0. Substrate under measurement

| Item | Value |
|---|---|
| Serving build | `jcodemunch_mcp-1.108.55` (`~/.local/share/uv/tools/jcodemunch-mcp`) |
| Repo id | `local/reify-4ae45bbd` |
| Index `source_root` | `/home/leo/src/reify` |
| Index `git_head` | `b79cecf53acea75a479e8faa3848c78d9e648e09` |
| Index `index_version` | 16 |
| Index `indexed_at` | `2026-08-17T18:11:21.759174` |
| Indexed files | 2761 |
| Indexed symbols | 55890 |
| `total_analysed` (function+method) | 34595 |

This is the **full reify index**, not the 48-file `reify-audit`-only index the PRD
§2.6 measurement used. Every number below was reproduced fresh in this session
against that full index; none are copied from the PRD.

**Freshness caveat (observed, not a defect in this record):** the index is from
2026-08-17 and re-indexing was refused in the originating implementer session
(`scripts/jcodemunch-index-reify.sh` → `E_JC_INDEX_RUN_FAILED`, underlying
`uvx` error `Permission denied (os error 13)` creating a temp file under
`~/.local/share/uv/tools/`). The verdict below rests on **source-level facts about
the tool's own code**, which no re-index can change, so staleness does not
weaken it.

---

## 1. BEFORE — measured baseline

### 1.1 PDEAD substrate: `get_dead_code_v2`

`reify-audit`'s PDEAD detector calls `get_dead_code_v2`
(`crates/reify-audit/src/jcodemunch_client.rs:1127`).

Call: `get_dead_code_v2(repo="local/reify-4ae45bbd", min_confidence=0.5,
max_results=0, file_pattern="crates/reify-audit/**")`

| Metric | Value |
|---|---|
| Findings returned | **469** |
| Findings carrying `unreachable_file` | **469 / 469 (100%)** |
| Signal set `['unreachable_file','no_callers','not_barrel_exported']` @ 1.0 | 251 |
| Signal set `['unreachable_file','not_barrel_exported']` @ 0.67 | 214 |
| Signal set `['unreachable_file','no_callers']` @ 0.67 | 4 |
| Findings NOT carrying `unreachable_file` | **0** |

Signal 1 (`unreachable_file`) fires on **every single finding**. Representative
rows — note that these are all `#[test]` functions inside `#[cfg(test)] mod tests`:

```
crates/reify-audit/src/bin/reify-audit.rs::exit_code_caps_high_severity_at_254   754  1.0  ['unreachable_file','no_callers','not_barrel_exported']
crates/reify-audit/src/bin/reify-audit.rs::parse_args_empty_returns_defaults     784  1.0  ['unreachable_file','no_callers','not_barrel_exported']
crates/reify-audit/src/bin/reify-audit.rs::parse_args_unknown_flag_returns_err   801  1.0  ['unreachable_file','no_callers','not_barrel_exported']
crates/reify-audit/src/bin/reify-audit.rs::pdead_and_puntested_not_in_default_sweep 971 1.0 ['unreachable_file','no_callers','not_barrel_exported']
crates/reify-audit/src/bin/reify-audit.rs::ptodo_in_default_sweep               1216  1.0  ['unreachable_file','no_callers','not_barrel_exported']
```

**Correction to the PRD's claim about `main`.** The task description asserts PDEAD
flags `bin/reify-audit.rs::main` at confidence 1.0. Against the **full** index it
does not appear at the default `min_confidence=0.5`. Re-running at
`min_confidence=0.33` (`file_pattern="crates/reify-audit/src/bin/*.rs"`) shows why:

```
crates/reify-audit/src/bin/ptodo-baseline-gen.rs::main   64  0.33  ['unreachable_file']
crates/reify-audit/src/bin/reify-audit.rs::main         504  0.33  ['unreachable_file']
```

`main` **is** classified `unreachable_file` — the heuristic has no idea it is a
Rust binary root. It escapes the 0.5 default only by accident: signals 2 and 3
happen not to fire, because the call graph is a word-boundary text match (the
token `main` occurs constantly in this repo — git branch names, strings) and
`not_barrel_exported` is suppressed by a name collision with a `mod.rs` export
(`mod.rs` is in jcodemunch's `_BARREL_FILENAMES`). Under any repo where those
accidents do not hold, `main` lands at 1.0. **Signal 1 fires on `main` in both
binaries**, which is the fact this spike is about.

`framework_warning` was **absent** from the full-index response. This is not
evidence of health: the warning is gated on `entry_point_count == 0`, and
`entry_point_count` is satisfied by `gui/package.json` `main`/`exports` entries
via `_package_json_entries()`. **Zero** of the 12 filenames in jcodemunch's
`_ENTRY_POINT_FILENAMES` exist anywhere in reify:

```
$ git ls-files | grep -cE '(^|/)(__main__\.py|conftest\.py|manage\.py|wsgi\.py|asgi\.py|setup\.py|app\.py|main\.py|run\.py|cli\.py|celery\.py|Makefile)$'
0
$ git ls-files | grep -c '/main\.rs$'   # 2
$ git ls-files | grep -c '/lib\.rs$'    # 34
```

So the warning is **suppressed by a single JS package manifest** while the
underlying condition it warns about — no Rust entry point is recognised — is
fully in force across all 34595 analysed symbols. On the smaller
`reify-audit`-only index (no `package.json`) the same condition surfaced the
warning, which is what the PRD §2.6 measurement saw.

### 1.2 PUNTESTED substrate: `get_untested_symbols` (G6 scope extension)

`reify-audit`'s PUNTESTED detector calls `get_untested_symbols`
(`crates/reify-audit/src/jcodemunch_client.rs:1144`).

Call: `get_untested_symbols(repo="local/reify-4ae45bbd", min_confidence=0.5,
max_results=100)`

| Metric | Value |
|---|---|
| `untested_count` | 100 (capped by `max_results`) |
| `total_non_test_symbols` | 19029 |
| `reached_pct` | 99.5 |

Every returned row is `confidence=1.0, reason='unreached'`. The returned set is
**dominated by symbols that are themselves tests**:

```
crates/reify-ast/src/decl.rs::is_real_true_whole_number_stays_real       1214  1.0  unreached
crates/reify-ast/src/decl.rs::is_real_false_nan_classifies_as_lossy_real 1244  1.0  unreached
crates/reify-ast/src/decl.rs::test_annotation_returns_true               1281  1.0  unreached
crates/reify-audit/src/bin/reify-audit.rs::parse_args_empty_returns_defaults 784 1.0 unreached
crates/reify-audit/src/bin/reify-audit.rs::ptodo_in_default_sweep        1216  1.0  unreached
```

And it **does** flag both `main`s at 1.0 — the claim the PRD makes about PDEAD is
true of PUNTESTED:

```
crates/reify-audit/src/bin/ptodo-baseline-gen.rs::main    64  1.0  unreached
crates/reify-audit/src/bin/reify-audit.rs::main          504  1.0  unreached
```

Scoped run for a per-crate comparand — `get_untested_symbols(min_confidence=0.5,
max_results=15, file_pattern="crates/reify-audit/**")`:

| Metric | Value |
|---|---|
| `untested_count` | 15 (capped) |
| `total_non_test_symbols` | 500 |
| `reached_pct` | 97.0 |

**κ's premise is now measured, not inferred.** PUNTESTED does not literally share
`get_dead_code_v2`'s Signal-1 code, but it shares the *same closed substrate*: its
`_is_test_file` is imported directly from `find_dead_code`
(`get_untested_symbols.py:14`) and is purely **path**-based —

```python
def _is_test_file(file_path: str) -> bool:
    fp = file_path.replace("\\", "/"); fn = fp.rsplit("/", 1)[-1]
    base = fn.rsplit(".", 1)[0] if "." in fn else fn
    return ("/tests/" in fp or "/test/" in fp or "/__tests__/" in fp
            or fn.startswith("test_") or fn.endswith("_test.py")
            or fn == "conftest.py" or base.endswith(".spec") or base.endswith(".test"))
```

Rust's dominant test idiom is an **in-file** `#[cfg(test)] mod tests` block, so
every such test lives in a path that fails all eight of those clauses. The
consequence is symmetric to PDEAD's: tests are counted as untested source, and
the same tests are the "callers" whose absence PDEAD then also reports.

---

## 2. Configuration attempted, verbatim

### 2.1 Attempt A — pass `entry_point_patterns` to `get_dead_code_v2`

**Not possible.** `entry_point_patterns` is a parameter of `find_dead_code`
(the v1 tool), not of `get_dead_code_v2`. The v2 MCP handler passes exactly six
arguments and `entry_point_patterns` is not among them
(`jcodemunch_mcp/server.py:4423`):

```python
elif name == "get_dead_code_v2":
    from .tools.get_dead_code_v2 import get_dead_code_v2
    result = await asyncio.to_thread(
        functools.partial(
            get_dead_code_v2,
            repo=arguments["repo"],
            min_confidence=arguments.get("min_confidence", 0.5),
            include_tests=arguments.get("include_tests", False),
            max_results=arguments.get("max_results", 100),
            file_pattern=arguments.get("file_pattern"),
            storage_path=storage_path,
        )
    )
```

The v2 function signature agrees (`tools/get_dead_code_v2.py:265`):

```python
def get_dead_code_v2(repo, min_confidence=0.5, include_tests=False,
                     max_results=100, file_pattern=None, storage_path=None) -> dict:
```

The tool's own `framework_warning` text — *"Pass `entry_point_patterns` to identify
framework-specific roots"* — **names a parameter this tool does not accept.** It is
copy-paste from `find_dead_code`.

### 2.2 Attempt B — declare roots in `.jcodemunch.jsonc`

Applied to `/home/leo/src/reify/.jcodemunch.jsonc` (verbatim, three spellings at
once to maximise the chance of a hit):

```jsonc
{
  "entry_point_patterns": [
    "crates/*/src/main.rs",
    "crates/*/src/lib.rs",
    "crates/*/src/bin/*.rs",
    "gui/src-tauri/src/main.rs",
    "gui/src-tauri/src/lib.rs"
  ],
  "entry_points": [
    "crates/*/src/main.rs",
    "crates/*/src/lib.rs",
    "crates/*/src/bin/*.rs"
  ],
  "dead_code": {
    "entry_point_patterns": ["crates/*/src/main.rs", "crates/*/src/lib.rs", "crates/*/src/bin/*.rs"],
    "test_attributes": ["#[test]", "#[cfg(test)]", "#[tokio::test]"]
  },
  "architecture": { /* ... existing layer rules, unchanged ... */ }
}
```

(reverted after measurement; `.jcodemunch.jsonc` is unchanged on `main`.)

### 2.3 Attempt C — `find_dead_code` (v1) *with* `entry_point_patterns`

For completeness, the knob **does** work on the tool that owns it:

```
find_dead_code(repo="local/reify-4ae45bbd", granularity="file", min_confidence=0.8,
               entry_point_patterns=["crates/*/src/main.rs","crates/*/src/lib.rs",
                                     "crates/*/src/bin/*.rs","**/*.rs"])
→ live_root_count=1789   dead_file_count=433   dead_symbol_count=0
  analysis_notes: "Entry points detected: 1789", "Total files analyzed: 2761"
```

All 433 remaining "dead files" are `Cargo.toml`, `*.json`, `*.sh`, `*.yaml` and
similar non-source artefacts. This is a blunt instrument, not a fix: it removes
findings by declaring the entire `.rs` surface a root, and it is a **different
tool** from the one PDEAD calls.

---

## 3. AFTER — measurement under Attempt B

Same call as §1.1's drill-down, with the §2.2 config in place:

`get_dead_code_v2(repo="local/reify-4ae45bbd", min_confidence=0.33,
max_results=0, file_pattern="crates/reify-audit/src/bin/*.rs")`

**Output is byte-for-byte identical to the BEFORE run.** Same 68 rows, same
confidences, same signal sets, including:

```
crates/reify-audit/src/bin/ptodo-baseline-gen.rs::main   64  0.33  ['unreachable_file']
crates/reify-audit/src/bin/reify-audit.rs::main         504  0.33  ['unreachable_file']
```

`unreachable_file` still fires on `main` in both binaries and on every `#[test]`
function. No key of `.jcodemunch.jsonc` had any effect.

This is expected from source: `tools/get_dead_code_v2.py` imports no config
module at all. Its complete import list is

```python
import json, re, time
from collections import deque
from typing import Optional
from ..storage import IndexStore
from ..parser.imports import resolve_specifier
from ._utils import resolve_repo as _resolve_repo
from ._call_graph import _word_match, build_symbols_by_file
from ..parser.context._route_utils import ENTRY_POINT_DECORATOR_RE
```

There is no `_config` / `config` import, so no `.jcodemunch.jsonc` key and no
`~/.code-index/config.jsonc` key can reach it. (`.jcodemunch.jsonc` *is* read —
by `get_layer_violations`, via `_cfg.get("architecture", {}, repo=repo)` — which
is why reify's existing layer rules work and why this was worth testing rather
than assuming.)

---

## 4. VERDICT

VERDICT: NOT-CONFIGURABLE

**VERDICT: NOT-CONFIGURABLE.**

The closed part of the heuristic is **Signal 1 of `get_dead_code_v2`** — the
entry-point set that seeds `_reachable_from_entry_points()`. That set has exactly
two sources, both hardcoded in `jcodemunch_mcp/tools/get_dead_code_v2.py`, and
neither is reachable from any configuration file, MCP tool argument, or
environment variable:

1. **`_ENTRY_POINT_FILENAMES`** — a module-level `frozenset` literal (line 35):

   ```python
   _ENTRY_POINT_FILENAMES = frozenset({
       "__main__.py", "conftest.py", "manage.py", "wsgi.py", "asgi.py",
       "setup.py", "app.py", "main.py", "run.py", "cli.py", "celery.py",
       "Makefile",
   })
   ```

   Twelve names, eleven Python and one `Makefile`. No `main.rs`, no `lib.rs`, no
   Cargo `[[bin]]` awareness. Matched by exact **basename** equality
   (`_is_entry_point`), so no glob, prefix or pattern can widen it.

2. **`_package_json_entries()`** — reads `main`/`module`/`exports`/`bin` from any
   indexed `package.json`. There is **no `Cargo.toml` analogue**: the string
   `Cargo` does not appear in `get_dead_code_v2.py`, and `framework_profiles.py`
   defines no Rust profile at all (`grep -i 'rust|cargo|\.rs'` → no matches).
   Cargo already declares reify's roots — `[[bin]]`, `[lib]`, `src/main.rs`,
   `src/bin/*.rs` — and jcodemunch reads none of it.

Two further closed surfaces compound this and would each independently need
fixing even if entry points were configurable:

3. **`_BARREL_FILENAMES` (Signal 3)** is likewise a hardcoded frozenset. It does
   contain `mod.rs`, but `not_barrel_exported` encodes the JS/TS/Python
   re-export-from-a-barrel model; Rust's `pub use` / visibility system is not
   that model, and the signal's only Rust effect observed here was the accidental
   name-collision suppression that hides `main` at the 0.5 default.

4. **`_is_test_file` (shared by PDEAD's test-skip and all of PUNTESTED)** is a
   hardcoded path-shape predicate with no attribute awareness. Rust's
   `#[cfg(test)] mod tests` is in-file, so `include_tests=False` — PDEAD's only
   test-related knob — cannot exclude Rust tests, and PUNTESTED cannot recognise
   them as tests. This is the single largest contributor to both tools'
   false-positive rate on this repo.

`entry_point_patterns` — the one externally-reachable knob in this area — is
wired only to `find_dead_code` (v1), which is **not** the tool either detector
calls, and which "fixes" the problem only by declaring the whole `.rs` surface a
root (§2.3), yielding zero symbol findings rather than correct ones.

### Consequences for downstream tasks

- **Task ι (activate PDEAD)** — gated on a CONFIGURABLE verdict. Not met.
  Activating PDEAD as measured would emit 469 findings for `crates/reify-audit/**`
  alone, 100% carrying a signal that fires unconditionally on Rust.
- **Task κ (activate PUNTESTED)** — gated on the same verdict. Not met, and now
  backed by a measurement rather than an inference: 100 findings at confidence
  1.0, dominated by `#[test]` functions reported as untested, plus both binary
  `main`s.

Per this task's charter, **a human reading this record cancels ι and κ.** This
task did not change sibling task state.

Fixing this requires an upstream change to jcodemunch — a Rust framework profile
plus `Cargo.toml` root parsing and `#[cfg(test)]`/`#[test]` attribute awareness —
or a reify-side pre-filter that discards findings whose signal set is dominated by
`unreachable_file`. Both are out of scope for this spike.
