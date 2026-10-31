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
| the empty-fixture guard is generalised, not deleted; constraining keys | **[Amended 2026-10-20, retro-critic R14 M1/M2/M8]** α `load_probe_set` refuses an empty `fixture` for every kind (rc 64 naming `probe[i]`); a constraining key is an integer `exit_code` or a non-empty string signature; B8 gains those cases; δ owns the unknown-key refusal, #9376 the type rules | PASS (δ builds it) |
| delivered | `class AssertionBinding` present in `scripts/prd-decompose-verify.py`; `assertion_kind == "rejection" and not premise\.match` absent in `scripts/prd-decompose-verify.py`; vacuous-check refusal: manual (B6–B8) | — |

## ε — ProbeContext and runner seam *(leaf; fk-f34f07d2dde4)*

| Capability | Evidence | Verdict |
|---|---|---|
| two argv derivations per evaluation exist | `grep:scripts/prd-capability-check.py cmd = build_command(probe, repo_root=repo_root)` in both `run_probe` and `evaluate` | PASS |
| `main()` has no runner seam | `grep:scripts/prd-capability-check.py results = [evaluate(probe) for probe in probes]` | PASS |
| private-reach baseline | `run:` `grep -oE 'pcc\._[A-Za-z_]+'` → 69 (α suite) + 1 (pdv suite); `patch.object(pcc` → 7 | PASS |
| source-shape guard that must move with the launch sites | `grep:tests/infra/test_tree_sitter_parse_isolation.sh Block D` | PASS |
| producer of `ProbeRun` fields ε extends | `producer:γ` (upstream) | PASS |
| producer of the vocabulary the ratchet's `pcc._HARNESS_ERROR` / `pcc._VALID_PROBE_KINDS` reads move to | **[Amended 2026-10-20, retro-critic R14 M5]** `producer:ζ` (edge `#8649 ← #8651`); the public seams are named in PRD §5.5 (`grammar_cache_home(ctx)`, `grammar_fingerprint(ctx)`, a substrate-timeout field, `main(argv, runner=run_probe, ctx=None)`) | PASS |
| one binary-trust home | **[Amended 2026-10-20, retro-critic R14 M6]** `grep:scripts/reify-bin-freshness.sh WHY SHA-IDENTITY, NOT MTIME`; ProbeContext prefers the `target/.reify-bin-sha` match, else requires `REIFY_BIN`; the 2026-10-18 mtime widening is superseded | PASS |
| multi-line private patches the single-line check cannot see | **[Amended 2026-10-20, retro-critic R14 polarity]** `run: git grep -E 'patch\.object\($' main scripts/` → 3 hits in `scripts/test_pcc_grammar_substrate.py` | PASS |
| delivered | `class ProbeContext` present in `scripts/prd-capability-check.py`; `def main\(argv[^)]*runner` present in `scripts/prd-capability-check.py`; `patch\.object\(\s*pcc\s*,` absent in `scripts/`; `patch\.object\($` absent in `scripts/` | — |

## ζ — One published vocabulary *(leaf; fk-bf4e8bbb7dda)*

| Capability | Evidence | Verdict |
|---|---|---|
| the drifted copies exist | `grep:scripts/prd-decompose-verify.py _BLOCKING_VERDICTS = frozenset({"FAIL", "UNPROVABLE", "HARNESS_ERROR"})`; `grep:scripts/prd-capability-check.py _HARNESS_ERROR = "HARNESS_ERROR"`; Premise docstring `"rejection" \| "parses" \| "resolves" \| "produces" \| "ir"` (no `value`) | PASS |
| the `.mjs` cannot read a schema file at run time | Workflow runtime: "No filesystem or Node.js API access" (PRD §3 M4) | PASS |
| the test seam that sees each role's schema exists | `grep:scripts/test_prd_decompose_verify.py def _mjs_scenario_source` — mock `agent(prompt, opts)` (PRD §3 M5) | PASS |
| producers of the record fields and binding table ζ publishes | `producer:γ`, `producer:δ` (upstream) | PASS |
| the relay admits `fixture_present: null` on any verdict (fail-open for PASS) | **[Amended 2026-10-20, retro-critic R14 M3/M4]** γ's `RESULT_RECORD_SCHEMA` `fixture_present: { type: ["boolean", "null"] }`; ζ restricts `null` to `HARNESS_ERROR` (if/then), routes a non-bool `fixture_present` to MALFORMED (B5′), and pdv's print-schema owns the relay members `results` / `verdict` | PASS (ζ builds it) |
| delivered | `"--print-schema"` present in `scripts/prd-capability-check.py`; `_BLOCKING_VERDICTS\b` absent in `scripts/prd-decompose-verify.py` (re-anchored on the name, R14 polarity; 3 hits today); schema parity: manual (B9) | — |

## η — `pdv aggregate` *(leaf; fk-6a006c36a78d)*

| Capability | Evidence | Verdict |
|---|---|---|
| aggregation lives only in the `.mjs` | `grep:scripts/prd-decompose-verify.mjs const disposition = anyBlocks`, `function droppedLeafLabels`, `function batchVerdict` | PASS |
| three test classes slice `.mjs` source | `grep:scripts/test_prd_decompose_verify.py const ANCHOR = "const _wfResult = await"` (two harnesses: `_HeadSliceHelperMixin`, `TestMjsResultsSchema`) | PASS |
| an agent can run a Python subcommand and relay its JSON | the existing Synthesize role does exactly this (`grep:scripts/prd-decompose-verify.mjs python3 scripts/prd-decompose-verify.py synthesize`) | PASS |
| the 13-key top-level return the overlay documents | `grep:.claude/skills/prd/project.md Reading the result`; `grep:scripts/prd-decompose-verify.mjs function batchVerdict` | PASS |
| producer of the published dispositions / schemas | `producer:ζ` (upstream) | PASS |
| the `leaf_verdicts` element shape | **[Amended 2026-10-20, retro-critic R14 M7/m5]** today `{leafLabel, blocks, blocking, report, malformed, fixture_absent, executed, total, disposition}`; PRD §5.4 carries `blocks`, `blocking`, `report_kind` in `leaf_dispositions`, B10 scoped to it; #7569's overlay `blocking` row is absorbed (cancelled, `x_subsumed_by = 8655`) | PASS |

## θ — One path classifier and one compile closure *(leaf; fk-21c29ddc104d minus the verify.sh home, which S18 carries — ruling R2)*

| Capability | Evidence | Verdict |
|---|---|---|
| the stdlib attribution module exists | `producer:task-7491` (upstream, in flight on `task/7491`: `scripts/reify_path_attribution.py`, `attribute`, `PathAttribution`, stdlib-only, `/usr/bin/python3`-importable per its commit `7a519f2cf9`) — drift noted 2026-10-20 (retro-critic R14): #7491 and θ #8662 are done and on main | PASS (producer landed) |
| the closure θ lifts exists in one heredoc | `grep:scripts/occt-scope-lib.sh _REIFY_COMPILE_CLOSURE_PY=$(cat <<'PYEOF'` | PASS |
| the bash ladder θ deletes exists (6 arms) | `grep:scripts/affected-crates-lib.sh _file_to_crate()`, `_is_global()`, `_is_crate_manifest()`, `_is_noncrate()` | PASS |
| a module invoked from a sourced lib is NOT auto-registered as verify-pipeline | `grep:scripts/verify-pipeline-paths.txt NON-source-derivable` (only `source` lines and `add()`/`add_tool()` `*.sh` paths are derived). The row is S18's (ruling R2); θ's own diff touches `affected-crates-lib.sh`, so θ itself takes the full gate | PASS |
| per-path forking is too slow | #7491 commit `be5edd1239`: 73 s per-path vs 3.9 s fork-free over 4,725 paths | PASS |

## κ — One MCP client *(leaf; fk-0497ab17e729; subsumes #8178)*

| Capability | Evidence | Verdict |
|---|---|---|
| two clients exist | `grep:scripts/await-merge-landing.py class MergeStatusClient`; `grep:scripts/escalation_mcp.py class McpSession` | PASS |
| the deadline mechanism to lift | `grep:scripts/await-merge-landing.py def _within` | PASS |
| an `error`-key refusal is correct for `merge_status` | dark-factory `escalation/server.py::merge_status` returns `{'error': …}` only on missing arguments; every success shape carries `state` (PRD §3 M8) | PASS |
| a shared stub server exists | `grep:tests/infra/stub_escalation_mcp.py class StubEscalationServer` | PASS |
| other importers keep their `except OSError` contract | `TimeoutError` ⊂ `OSError`; importers `scripts/docs-truth-sweep.py`, `scripts/pprdstatus-escalate.py`, `docs/prds/v0_6/e3-granularity/analysis/e3_sources.py` | PASS |
| the next MCP client waits on κ | **[Amended 2026-10-20, retro-critic R14 M9/m2]** #8780 (`scripts/lane-task-status.py`) depends on #8664 and uses `McpSession`; if S18 τ #9312 lands first, κ's infra-tests row becomes a `# guards:` header | PASS |

## λ — PRD close *(leaf; no key)*

| Capability | Evidence | Verdict |
|---|---|---|
| terminal-status vocabulary and freeze shape | `.claude/skills/prd/project.md` "PRD terminal status — closed vocabulary + decompose-close stamp" | PASS |
| producers | `producer:α…κ` (all upstream; no ι) | PASS |

---

**FAIL bindings:** none. **Rulings applied (pass 2):** R1 keep the detector importing the module (REVERSED by Leo 2026-10-12: the detector is retired by #8886, so θ's detector capability row was removed 2026-10-13, here, in the `.yaml` sidecar and in #8662's metadata); R2 the former ι and the paths row moved to S18; R3 layered schemas.

## Retro-critic sitting amendments (2026-10-20)

Ruling R14 (`plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R14`). Each change is written three times: this file, the `.yaml`, and the task's `metadata.delivered_checks`.

- ε #8649: new check `tests-reach-no-alpha-privates-multiline` (`patch\.object\($` absent in `scripts/`; 3 hits at main, so red today). The other three checks are unchanged.
- ζ #8651: `pdv-blocking-verdicts-not-literal` re-anchored on the name, `_BLOCKING_VERDICTS\b` absent in `scripts/prd-decompose-verify.py` (3 hits at main). `alpha-print-schema` is unchanged.
- δ #8644, η #8655, κ #8664: bindings updated (M1/M2/M8, M7/m5, M9/m2); checks unchanged. #7569 is cancelled into η.
- θ #8662: drift noted: #7491 and θ are done.
