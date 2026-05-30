# Capability manifest — reify-audit-p1-jcodemunch-substrate

Mechanizes G3 + G6 per leaf for `docs/prds/reify-audit-p1-jcodemunch-substrate.md`. Every binding below is **PASS** (no FAIL → batch may queue). Reify evidence forms per `.claude/skills/prd/project.md` → *Capability Manifest*.

**No grammar-fixture bindings:** this slice introduces **no novel `.ri` syntax** — it is pure Rust + an external MCP server. The G3 grammar gate does not fire. The relevant substrate is the jcodemunch v1.108.27 API, verified live this session (see below).

## Substrate verification (G3) — shared by all leaves

Verified against on-disk v1.108.27 source `~/.cache/uv/git-v0/checkouts/222e86ded376d2c0/29faf00/src/jcodemunch_mcp/tools/*.py` + `server.py` (2026-05-30):

| Asserted capability | Evidence | Verdict |
|---|---|---|
| `get_changed_symbols` is commit-range `(repo, since_sha, until_sha)` | `tools/get_changed_symbols.py:88` signature + docstring "between two git commits" | **PASS** |
| `find_references(repo, identifier)` exists; **no file-scope param** | `tools/find_references.py:274` — params are `(repo, identifier, max_results, identifiers, include_call_chain)` | **PASS** (and corrects the slice-1 doc claim) |
| `get_dead_code_v2(repo, min_confidence, …) → dead_symbols[{…confidence,signals}]` | `tools/get_dead_code_v2.py:265` + return docstring | **PASS** |
| `get_untested_symbols(repo, …) → {untested_count, reached_pct, symbols}` | `tools/get_untested_symbols.py:86` + docstring | **PASS** |
| `get_layer_violations(repo, rules?)` reads `.jcodemunch.jsonc` | `tools/get_layer_violations.py:59` | **PASS** |
| `serve --transport streamable-http --host --port` (default 8901) | `server.py main()` argparse + `run_streamable_http_server`; `[http]` extra in `pyproject.toml:30` | **PASS** |
| reify repo is live-indexed | `jcodemunch-watcher.service` active, `watch-claude --repos … /home/leo/src/reify …`, index-schema v16 | **PASS** |
| Exact `repo` id + `storage_path` mapping to the index | **Deferred to L-SERVE live spike** (not a blocker; resolved before any Rust depends on it) | **PASS** (spike-gated) |

## Per-leaf bindings

| Leaf | Capability asserted by its signal | Evidence form | Binding |
|---|---|---|---|
| **L-SERVE** | A live streamable-HTTP serve answers reify queries | wired-on-main: committed `jcodemunch-serve.service` + `scripts/smoke-jcodemunch-serve.sh` exits 0 against the live endpoint (not a mock) | **PASS** |
| **L-TRAIT** | `get_changed_symbols` is **commit-range**, not `(branch, since_epoch)` | anti-inversion: the redesigned trait + `tests/p1.rs` assert the `(since_sha, until_sha)` signature; a grep confirms **no** surviving reference to the old `since_epoch` arg. Premise-fix per G6 §4. | **PASS** |
| **L-TRAIT** | P1 maps done→range via `done_provenance.commit` | field-population: `DoneProvenance.commit` exists (`lib.rs:174`) and is read on the production path; reuses task 4074's P2 mapping mechanism | **PASS** |
| **L-CLIENT** | `RealJCodemunchOps` decodes the **real** wire shapes | field-population + anti-synthetic: decode test runs against fixtures **captured from the live serve** (L-SERVE), not hand-written synthetic JSON | **PASS** |
| **L-CLIENT** | suppression flags (`#[allow]`/`cfg(test)`/`G-allow`) are populated | field-population: `RealJCodemunchOps` reads source at `file:line` (jcodemunch records lack these); detector stays pure (symmetric w/ `GitOps::diff_added_lines`) | **PASS** |
| **L-WIRE** | the production binary uses `RealJCodemunchOps`, not the noop | anti-orphan: `bin/reify-audit.rs:423` `NoopJCodemunchOps` replaced on the production dispatch path; `--pattern P1` no longer trivially exits 0 | **PASS** |
| **L-WIRE** | P5/pre-done stays jcodemunch-free | anti-orphan (negative): lazy connect — the P5/pre-done arm constructs no client | **PASS** |
| **L-PDEAD** | `get_dead_code_v2` produces real Rust findings; **confidence is advisory** | numeric-floor: severity pinned **Low/log-only** because jcodemunch Rust-accuracy is unproven — `bound` (action taken) ≤ the confidence floor, never auto-files. Honest-bound discipline. | **PASS** |
| **L-PUNTESTED** | `get_untested_symbols` reachability (not coverage) | numeric-floor: Low/log-only; signal reports `reached_pct` as-is, asserts no exactness | **PASS** |
| **L-PLAYER** | `get_layer_violations` reads authored reify rules | wired-on-main: `.jcodemunch.jsonc` committed + a seeded forbidden edge fires; clean when none | **PASS** |
| **L-SKILL** | `/audit` documents the new patterns + serve prereq | wired-on-main: `.claude/skills/audit/SKILL.md` references the patterns + `--jcodemunch-url` | **PASS** |
| **L-SMOKE** | **end-to-end**: real binary + live serve → real P1 + P-DEAD findings | anti-synthetic (the C-integration-gate): `scripts/smoke-jcodemunch-audit.sh` runs the real binary against the live serve over a real reify commit range; ≥1 real finding. This is the G2 signal slice-1's noop could never produce. | **PASS** |

## G6 numeric-floor summary

The only numbers in scope are confidence thresholds (`min_confidence=0.5`) and the inherited 14-day P1 grace window. **No leaf asserts a closed-form exactness or an end-to-end accuracy bound** (no FEA/spline/eigensolver numerics here). The floor is "jcodemunch's Rust analysis accuracy is unproven" → every new detector ships at Low/log-only severity, which is *below* (more conservative than) the floor. No `bound ≤ floor` violation possible.
