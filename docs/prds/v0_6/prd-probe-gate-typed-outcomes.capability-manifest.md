# Capability manifest — `prd-probe-gate-typed-outcomes` (review-all stream S22)

PRD: `docs/prds/v0_6/prd-probe-gate-typed-outcomes.md`. Machine-readable twin: `prd-probe-gate-typed-outcomes.capability-manifest.yaml`.
Drafted in author mode, pass 1 (2026-10-12): every binding was checked at main `8e6ffcac9d`, where every file in scope is byte-identical to the review's pinned tree `a030ee7528`. The decompose pass re-walks these and runs the D3 workflow; nothing below is a D3 result.

Evidence vocabulary: `grep:<file> <symbol>` (present on main) · `run:` (a command executed for this manifest, with its output) · `producer:<leaf|task-N>` (upstream in the DAG) · `rejection-check` (the rejection was observed to fire, or observed NOT to fire where the leaf adds it).

---

## α — Partition `scripts/test_prd_capability_check.py` *(intermediate → γ, δ, ε)*

| Capability | Evidence | Verdict |
|---|---|---|
| the six named partitions exist as top-level symbols | record `h14_measurements` row for the path; `grep:scripts/test_prd_capability_check.py class TestRunProbe` / `class TestGrammarSubstrateUsable` / `class _MainHarness` | PASS |
| the wrapper is the single runner | `grep:tests/infra/test_prd_capability_check.sh python3 "$REPO_ROOT/scripts/test_prd_capability_check.py"`; `run-all-classification.manifest` row `test_prd_capability_check.sh pool` | PASS |
| the import-time coupling the measurement names exists | `grep:scripts/test_prd_capability_check.py _GRAMMAR_STATUS_CALLS_AT_IMPORT` | PASS |

## β — Partition `scripts/test_prd_decompose_verify.py` *(intermediate → γ, δ, ζ, η)*

| Capability | Evidence | Verdict |
|---|---|---|
| the `.mjs` partition imports nothing from the Python harness | record `h14_measurements` row (only `_run_node_module`, `_PDV_MJS`); `grep:scripts/test_prd_decompose_verify.py def _run_node_module` | PASS |
| wrapper single runner | `grep:tests/infra/test_prd_decompose_verify.sh scripts/test_prd_decompose_verify.py` | PASS |

## γ — Typed probe outcome *(leaf; fk-d75787147427; subsumes #8122)*

| Capability | Evidence | Verdict |
|---|---|---|
| the sentinel routing γ deletes exists (6 sites) | `grep:scripts/prd-capability-check.py _BINARY_NOT_FOUND_SENTINEL in run.stderr` (observe_with_evidence, grammar_substrate_usable), `_PROBE_TIMEOUT_SENTINEL in run.stderr` (both), `run.stderr.split(_BINARY_NOT_FOUND_SENTINEL`; `grep:scripts/prd-decompose-verify.py pcc._BINARY_NOT_FOUND_SENTINEL in stderr` | PASS |
| the absent-fixture vacuity γ closes is live (signal premise) | `run:` bind + α `--json` (`target/debug/reify`) + synthesize on a rejection premise against a nonexistent fixture → `PASS`, `blocks: false, executed: 2, total: 2, fixture_absent: []` (PRD §3 M1) | PASS (defect observed) |
| `reify check` on a missing fixture exits 1 with ENOENT text | `run: target/debug/reify check /nonexistent/zz.ri` → `Error reading /nonexistent/zz.ri: No such file or directory (os error 2)`, rc 1 | PASS |
| grammar probes give no ENOENT text on a missing fixture (#8122 premise) | task #8122 measured `Error: No files were found at or matched by the provided pathname/glob` (tree-sitter 0.26.8); `grep:scripts/prd-decompose-verify.py GRAMMAR PROBES ARE NOT COVERED` | PASS |
| the relay schema accepts additive fields | `grep:scripts/prd-decompose-verify.mjs const RESULT_RECORD_SCHEMA` — no `additionalProperties: false` | PASS |
| verdict vocabulary unchanged (parent PRD §6 decision 3) | D3 reuses `UNPROVABLE`; `grep:scripts/prd-decompose-verify.py PRD §6 decision 3 fixes the verdict vocabulary` | PASS |

## δ — Required match per binding row *(leaf; fk-1922380bc3aa)*

| Capability | Evidence | Verdict |
|---|---|---|
| vacuous resolves binds and PASSes today (signal premise) | `run:` bind of `{resolves, match: {}}` rc 0 → α verdict `PASS` at exit 1 (PRD §3 M2) | PASS (defect observed) |
| `match_predicate({})` is unconditionally true | `grep:scripts/prd-capability-check.py An empty match dict {} is always satisfied` | PASS |
| the new α refusal refuses no committed probe set | `run:` scan of all 14 `tests/prd-gate/*.json`: zero `check` probes with `match: {}` (PRD §3 M3) | PASS |
| rejection-mechanism: the δ refusal fires | rejection-check deferred to δ's own B7/B8 tests (the refusal does not exist yet; today bind rc 0 on the vacuous premise is the observed absence) | PASS (absence observed; δ builds it) |

## ε — ProbeContext and runner seam *(leaf; fk-f34f07d2dde4)*

| Capability | Evidence | Verdict |
|---|---|---|
| two argv derivations per evaluation exist | `grep:scripts/prd-capability-check.py cmd = build_command(probe, repo_root=repo_root)` in both `run_probe` and `evaluate` | PASS |
| `main()` has no runner seam | `grep:scripts/prd-capability-check.py results = [evaluate(probe) for probe in probes]` | PASS |
| private-reach baseline | `run:` `grep -oE 'pcc\._[A-Za-z_]+'` → 69 (α suite) + 1 (pdv suite); `patch.object(pcc` → 7 | PASS |
| source-shape guard that must move with the launch sites | `grep:tests/infra/test_tree_sitter_parse_isolation.sh Block D` | PASS |
| producer of `ProbeRun` fields ε extends | `producer:γ` (upstream) | PASS |

## ζ — One published vocabulary *(leaf; fk-bf4e8bbb7dda)*

| Capability | Evidence | Verdict |
|---|---|---|
| the drifted copies exist | `grep:scripts/prd-decompose-verify.py _BLOCKING_VERDICTS = frozenset({"FAIL", "UNPROVABLE", "HARNESS_ERROR"})`; `grep:scripts/prd-capability-check.py _HARNESS_ERROR = "HARNESS_ERROR"`; Premise docstring `"rejection" \| "parses" \| "resolves" \| "produces" \| "ir"` (no `value`) | PASS |
| the `.mjs` cannot read a schema file at run time | Workflow runtime: "No filesystem or Node.js API access" (PRD §3 M4) | PASS |
| the test seam that sees each role's schema exists | `grep:scripts/test_prd_decompose_verify.py def _mjs_scenario_source` — mock `agent(prompt, opts)` (PRD §3 M5) | PASS |
| producers of the record fields and binding table ζ publishes | `producer:γ`, `producer:δ` (upstream) | PASS |

## η — `pdv aggregate` *(leaf; fk-6a006c36a78d)*

| Capability | Evidence | Verdict |
|---|---|---|
| aggregation lives only in the `.mjs` | `grep:scripts/prd-decompose-verify.mjs const disposition = anyBlocks`, `function droppedLeafLabels`, `function batchVerdict` | PASS |
| three test classes slice `.mjs` source | `grep:scripts/test_prd_decompose_verify.py const ANCHOR = "const _wfResult = await"` (two harnesses: `_HeadSliceHelperMixin`, `TestMjsResultsSchema`) | PASS |
| an agent can run a Python subcommand and relay its JSON | the existing Synthesize role does exactly this (`grep:scripts/prd-decompose-verify.mjs python3 scripts/prd-decompose-verify.py synthesize`) | PASS |
| the 13-key top-level return the overlay documents | `grep:.claude/skills/prd/project.md Reading the result`; `grep:scripts/prd-decompose-verify.mjs function batchVerdict` | PASS |
| producer of the published dispositions / schemas | `producer:ζ` (upstream) | PASS |

## θ — One path classifier and one compile closure *(intermediate → ι; supplies f125's module)*

| Capability | Evidence | Verdict |
|---|---|---|
| the stdlib attribution module exists | `producer:task-7491` (upstream, in flight on `task/7491`: `scripts/reify_path_attribution.py`, `attribute`, `PathAttribution`, stdlib-only, `/usr/bin/python3`-importable per its commit `7a519f2cf9`) | PASS (producer upstream; not yet on main) |
| the closure θ lifts exists in one heredoc | `grep:scripts/occt-scope-lib.sh _REIFY_COMPILE_CLOSURE_PY=$(cat <<'PYEOF'` | PASS |
| the bash ladder θ deletes exists (6 arms) | `grep:scripts/affected-crates-lib.sh _file_to_crate()`, `_is_global()`, `_is_crate_manifest()`, `_is_noncrate()` | PASS |
| the detector's second closure exists | `grep:scripts/reify_overlap_detector.py def _reverse_closure` | PASS |
| a module invoked from a sourced lib is NOT auto-registered as verify-pipeline | `grep:scripts/verify-pipeline-paths.txt NON-source-derivable` (only `source` lines and `add()`/`add_tool()` `*.sh` paths are derived) → θ adds the row | PASS |
| per-path forking is too slow | #7491 commit `be5edd1239`: 73 s per-path vs 3.9 s fork-free over 4,725 paths | PASS |

## ι — `decide_scope` reads the facts *(leaf; fk-21c29ddc104d)*

| Capability | Evidence | Verdict |
|---|---|---|
| the third ladder exists | `grep:scripts/verify.sh decide_scope()` arms `crates/*)`, `gui/src-tauri/*)`, `tree-sitter-reify/*)`, `examples/*.ri)` | PASS |
| verify.sh policy arms that stay | `grep:scripts/verify.sh case "$_RUST_COUPLED_RI_FIXTURES" in`, `docs/gui-event-channels.md)` | PASS |
| plan-parity instrument exists | `grep:tests/infra/test_verify_scope.sh` scope cases; `verify.sh --print-plan` | PASS |
| producer of the batched `attribute` CLI | `producer:θ` (upstream) | PASS |

## κ — One MCP client *(leaf; fk-0497ab17e729; subsumes #8178)*

| Capability | Evidence | Verdict |
|---|---|---|
| two clients exist | `grep:scripts/await-merge-landing.py class MergeStatusClient`; `grep:scripts/escalation_mcp.py class McpSession` | PASS |
| the deadline mechanism to lift | `grep:scripts/await-merge-landing.py def _within` | PASS |
| an `error`-key refusal is correct for `merge_status` | dark-factory `escalation/server.py::merge_status` returns `{'error': …}` only on missing arguments; every success shape carries `state` (PRD §3 M8) | PASS |
| a shared stub server exists | `grep:tests/infra/stub_escalation_mcp.py class StubEscalationServer` | PASS |
| other importers keep their `except OSError` contract | `TimeoutError` ⊂ `OSError`; importers `scripts/docs-truth-sweep.py`, `scripts/pprdstatus-escalate.py`, `docs/prds/v0_6/e3-granularity/analysis/e3_sources.py` | PASS |

## λ — PRD close *(leaf; no key)*

| Capability | Evidence | Verdict |
|---|---|---|
| terminal-status vocabulary and freeze shape | `.claude/skills/prd/project.md` "PRD terminal status — closed vocabulary + decompose-close stamp" | PASS |
| producers | `producer:α…κ` (all upstream) | PASS |

---

**FAIL bindings:** none. **Open seams that change bindings:** PRD §11 R1 (θ's detector half and the program's DF seam row), R2 (ι's owner), R3 (where ζ's schemas live).
