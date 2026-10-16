# PRD — PRD probe gate: typed outcomes, one vocabulary, one path classifier, one MCP client

**Status:** authored 2026-10-12 (review-all program `review-all-reify-20261010`, stream **S22**, pass 1 — not yet decomposed). Version-agnostic factory infrastructure; lives under `v0_6/` per the program's shared convention.

**Amended 2026-10-13:** Leo reversed ruling R1 on 2026-10-12. The crate-graph overlap detector is retired (#8886), not wired into dark-factory. θ (#8662) no longer touches it, and the crate-closure idea moves to a merge-independence command at dark-factory's rebase re-verify gate (dark_factory #6943 hook, reify #8887, held for Leo). See §11 R1.

**Class:** dev-process infra/tooling (no `.ri` language surface — the docs-truth gate does not fire).
**Approach:** B+H (G5). The probe gate is consumed by every `/prd` decompose session; its records cross Python → JSON → LLM relay → JS; the path classifier feeds the merge gate's scope narrowing. §5 is the contract, §6 the two-way boundary-test sketch.

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12); every file this PRD touches is byte-identical to the review's pinned tree `a030ee7528` (`git diff a030ee7528..main` over them is empty). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

Program contract: `plans/review-all-program-reify-20261012.md` (stream row S22; ruling "Uncontested block"). Finding record: `plans/review-all-reify-20261010.json`.

---

## 1. Goal and consumers (G1)

The PRD gate (`scripts/prd-capability-check.py` = α, `scripts/prd-decompose-verify.py` = pdv, `scripts/prd-decompose-verify.mjs` = the D3 workflow) decides whether a decomposed leaf's premises hold. Today it reports its own outcomes as text inside captured stderr and recovers them with substring and regex matching; it accepts a vacuous `resolves` premise; it spells its closed vocabularies in five places across two languages; the batch disposition is computed only in the `.mjs`, which the tests can reach only by slicing its source. Separately, the rule that maps a changed path to the crates it affects has three homes, and `scripts/` holds two diverged MCP clients for the escalation server.

After this PRD:

- A `/prd` decompose author running `Workflow({scriptPath: "scripts/prd-decompose-verify.mjs"})` gets a disposition computed by one deterministic Python function, from records whose outcome is a field, not a phrase. A premise against a not-yet-written fixture reads `FIXTURE_ABSENT` for every probe kind — including the `rejection` and `resolves` kinds that today **PASS** against a missing fixture (§3, measurement M1) and the `parses` kind that today FAILs and blocks (#8122).
- A `resolves` (or any `check`-kind) premise that constrains nothing is refused at bind time instead of PASSing against a failing `reify check`.
- `prd-decompose-verify.py print-schema` / `prd-capability-check.py --print-schema` are the one published vocabulary; the `.mjs` schemas and prompt vocabulary are checked against them by one test.
- `scripts/affected-crates-lib.sh` and `scripts/occt-scope-lib.sh` read one stdlib Python path classifier and one compile-closure implementation. `scripts/verify.sh::decide_scope` adopts the same module inside S18's Python rewrite (lead ruling R2).
- `scripts/await-merge-landing.py` reaches the escalation server through `scripts/escalation_mcp.py`, which gains await's whole-exchange deadline.

| Mechanism | Consumer |
|---|---|
| α typed `ProbeRun`/`--json` record fields (`fixture`, `fixture_present`, `harness_failure`, `command`) | pdv `classify_record`; the D3 Prover relay (`RESULT_RECORD_SCHEMA`); α's own text renderer and `--grammar-substrate-status`; `tests/infra/test_prd_gate_*.sh` corpus gates |
| per-kind required match in the binding table + α `load_probe_set` non-vacuity rule | `pdv bind` (the Prover's step 2), every committed `tests/prd-gate/*-probe-set.json` |
| `Verdict`/`ProbeKind`/`HarnessFailureKind` enums, `--print-schema`, `pdv print-schema` | pdv (imports α's enums), the `.mjs` schemas and role prompts, the schema-parity test |
| `pdv aggregate` | the `.mjs` Aggregate role; the `/prd` decompose author reading the workflow's top-level return (the overlay's "Reading the result" table) |
| `ProbeContext` + `runner` seam on `main()` | α's CLI, `grammar_substrate_usable`, the α/pdv test suites (public seams instead of private patches) |
| `scripts/reify_path_attribution.py` `attribute` / `closure` CLI + API | `affected-crates-lib.sh::affected_crates`, `occt-scope-lib.sh::_reify_compile_closure`; `decide_scope` via S18's Python planner; #8887's merge-independence command (held for Leo) |
| `escalation_mcp.McpSession` with a call deadline | `await-merge-landing.py`, `docs-truth-sweep.py`, `pprdstatus-escalate.py`, `e3_sources.py` |

## 2. Findings owned (verbatim keys; statements in the record)

| key | short | anchor | sev | closed by |
|---|---|---|---|---|
| fk-d75787147427 | f126 | `scripts/prd-capability-check.py::run_probe` | high | γ |
| fk-1922380bc3aa | f127 | `scripts/prd-decompose-verify.py::bind_premises` | high | δ |
| fk-bf4e8bbb7dda | f128 | `scripts/prd-decompose-verify.py::_BLOCKING_VERDICTS` | medium | ζ |
| fk-6a006c36a78d | f129 | `scripts/prd-decompose-verify.mjs::runWorkflow` | medium | η |
| fk-f34f07d2dde4 | f130 | `scripts/prd-capability-check.py::evaluate` | medium | ε |
| fk-21c29ddc104d | f125 | `scripts/reify_overlap_detector.py::_file_to_crate` | high | θ (the verify.sh home moves to S18, ruling R2) |
| fk-0497ab17e729 | f131 | `scripts/await-merge-landing.py::MergeStatusClient` | medium | κ |

Plus the stream's two heuristic-14 measurements (no finding key): `scripts/test_prd_capability_check.py` (6 partitions) → α, `scripts/test_prd_decompose_verify.py` (4 partitions) → β. Their named partitions and the couplings each must keep are in the record's `h14_measurements` rows for those paths.

## 3. Measured premises (G3 / G6)

Each was re-measured for this PRD at main `8e6ffcac9d`, not inherited from the review.

- **M1 — a rejection premise PASSes against a fixture that does not exist.** `pdv bind` on `{assertion_kind: rejection, fixture: tests/prd-gate/fixtures/s22_does_not_exist.ri, match: {exit_code: 1}}` plus `{assertion_kind: resolves, …same fixture…, match: {}}`, then α `--json` with the built `target/debug/reify`, then `pdv synthesize`: both records `PASS` at `exit_code 1`; synthesize reports `blocks: false, executed: 2, total: 2, fixture_absent: []` — the leaf would read **VERIFIED**. `reify check /nonexistent/zz.ri` prints `Error reading /nonexistent/zz.ri: No such file or directory (os error 2)` and exits 1, which satisfies `exit_code: 1`. `fixture_absent_evidence` only runs for FAIL/UNPROVABLE, so a PASS on a missing fixture is never seen. This strengthens f126's statement: the stderr route misses fixture absence in the PASS direction as well as the grammar kind.
- **M2 — vacuous resolves (f127) reproduced:** in the same run the `resolves` premise with `match: {}` bound at rc 0 and PASSed on exit 1.
- **M3 — no committed probe set relies on an empty `check` match.** Over all 14 `tests/prd-gate/*.json`, no `check` probe has `match: {}`; three carry `stderr_contains` without `exit_code` (`corpus-probe-set.json` probe 2; `objective-inheritance-probe-set.json` probes 0 and 1). The δ non-vacuity rule ("at least one of `exit_code` / `stderr_contains` / `stdout_contains`") refuses none of them.
- **M4 — a Workflow script has no shell.** The Workflow runtime injects `agent`, `parallel`, `pipeline`, `log`, `phase`, `args`, `budget`, `workflow` and nothing else ("No filesystem or Node.js API access"; the `.mjs` header says the same). So `pdv aggregate` can only be reached through an agent that runs it and relays its JSON — exactly how `pdv synthesize` is reached today. Design consequence in §4 D7.
- **M5 — the scenario harness already captures each role's `opts`.** `test_prd_decompose_verify.py::_mjs_scenario_source` runs the whole `.mjs` body under a mock `agent(prompt, opts)`. The schema-parity and prompt-vocabulary checks (ζ) and the `normalizeLeaves` behaviour (η) are testable through it, with no source slicing.
- **M6 — `reify_overlap_detector` is registered nowhere in dark-factory.** `git grep` over dark-factory finds no `register_for_reify` / `reify_overlap_detector` reference outside docs and census files, and `git log -S register_for_reify` finds no commit ever adding one. Its only production lookup, `orchestrator/suffix_graph.py`, calls `get_overlap_detector(None)`, which always returns `DEFAULT_OVERLAP_DETECTOR`. DF task #1897 (done) records "falls back to the §5.1 default path detector". The detector is a production orphan, so the program's G4 row "dark-factory merge queue's import of reify_overlap_detector" names a seam that does not exist. Ruled in §11 R1, reversed 2026-10-12: the detector is retired (#8886).
- **M7 — task #7491 is in flight on `task/7491` and supplies half of f125.** Six commits add `scripts/reify_path_attribution.py`, a stdlib-only mirror of `_is_global` / `_file_to_crate` (with the `examples/*.ri` arm) / `_RI_CORPUS_CRATES` / `_is_crate_manifest` / `_REIFY_DAG_GATE_CRATE`. The commits repoint the detector at it and add the bash-sourcing parity guard `OVERLAP-MIRROR-PARITY` to `tests/infra/test_affected_crates_lib.sh`. The detector's kind-agnostic `_reverse_closure` stays, as recorded divergence (B). This PRD builds on that module; θ depends on #7491 landing. The detector itself is retired after #7491 lands (#8886; R1 as reversed).
- **M8 — `merge_status` returns an `error` key only to refuse a call** (dark-factory `escalation/server.py::merge_status`: `{'error': 'At least one of request_id, branch, or task_id is required'}`; every success shape carries `state`). So `escalation_mcp.tool_payload`'s error-key refusal is correct for await too.
- **M9 — source-shape guards over α exist and must move with it.** `tests/infra/test_tree_sitter_parse_isolation.sh` Block D checks that both of α's launch sites pass `env=`. ε keeps that true or updates Block D in the same diff.

## 4. Resolved design decisions

- **D1 — outcome is a field (f126).** `ProbeRun` gains `command: list[str]`, `harness_failure: HarnessFailure | None` and `fixture_present: bool`. `HarnessFailure` is a frozen dataclass `{kind: HarnessFailureKind, detail: str, cache_denied: bool}`. `HarnessFailureKind` takes one of four values. `LAUNCH` is an `OSError` from `Popen`/`run`. `SETUP` is an `OSError` while building the grammar env or cache dir. `TIMEOUT` means a caller-supplied bound elapsed. `GRAMMAR_LOAD` means tree-sitter reported a grammar load failure. `cache_denied` is meaningful only for `GRAMMAR_LOAD`. `detail` carries the errno text as data. No sentinel is ever written into `stderr`; `stderr` carries only what the tool printed. The record keeps its conventional `exit_code` (127 for launch/setup, 124 for timeout), so its shape stays unchanged for existing readers. No consumer routes on those numbers.
- **D2 — tree-sitter's stderr is parsed once, at the runner boundary.** `_GRAMMAR_LOAD_FAILURE_MARKER` and `_PERMISSION_DENIAL_RE` are applied in exactly one function, which fills `HarnessFailure(GRAMMAR_LOAD, cache_denied=…)`. `observe_with_evidence`, `grammar_substrate_usable` and `main()`'s hint read the field. Four stderr parsers become one boundary parser. The other three are deleted: the `observe` marker test, the `grammar_cache_denied` call sites, and pdv's `_reports_enoent` / `_path_token_re`.
- **D3 — an absent fixture is INDETERMINATE.** `run_probe` measures `os.path.isfile(<absolute fixture>)` immediately before launching. It still launches, so the record keeps executed evidence. When the fixture is absent, the observation is `INDETERMINATE` for every kind, whatever the tool printed or returned, so the verdict is `UNPROVABLE`. The exception is a set `harness_failure`: then `HARNESS_ERROR` still wins, because the machinery failing is the louder fact. The rule is "a probe with no target cannot prove anything": no new verdict, and α's exit code becomes 2, so a committed probe set naming a missing fixture can never exit 0. pdv `classify_record` routes `fixture_present is False` (any non-`HARNESS_ERROR` verdict) to `FIXTURE_ABSENT`. A record that **lacks** the field, or carries `null`, is treated as fixture present. That is fail-closed: a FAIL/UNPROVABLE still blocks. This settles #8122's open "fallback rule" design point.
- **D4 — every binding row states its non-vacuity (f127).** `_ASSERTION_KIND_MAP` values become a frozen `AssertionBinding(probe_kind, observation, required_match)`: `rejection → {exit_code: 1}`, `resolves → {exit_code: 0}`, `parses / produces / ir → {}`, and `value` validated by `_validate_value_predicate` as today. `premise_to_probe` fills each required key the premise omits. A premise whose own value for a required key contradicts it (e.g. `resolves` with `exit_code: 1`) is **refused**, naming kind, key and both values. The ruling's "merged under premise-supplied keys" covers absent keys; a contradiction is a mis-kinded premise, and silently honouring it would turn `resolves` into `rejection` (heuristic 10). The ad-hoc rejection-only `if` blocks in `bind_premises` are deleted. α's `load_probe_set` gains the uniform rule beside the value validator: a `check` probe must carry at least one of `exit_code`, `stderr_contains`, `stdout_contains`, because `match_predicate({})` is unconditionally true. Committed probe sets need no edits (M3).
- **D5 — one vocabulary, layered by owner (f128).** α owns and publishes what α decides: `Verdict` (`PASS`/`FAIL`/`UNPROVABLE`/`HARNESS_ERROR`, now public), `ProbeKind`, `Observation` and `HarnessFailureKind` (`StrEnum`s). `--print-schema` emits `{probe_set, result_record}` JSON schemas. pdv owns what pdv decides: `AssertionKind` (the binding-table keys), `RecordCategory`, `LeafDisposition` (`BLOCKS`/`NOT_VERIFIED`/`VERIFIED`/`UNENUMERATED`), `BatchDisposition` (`BLOCKS`/`INCOMPLETE`/`PASS`) and the `BatchVerdict` / `WorkflowVerdict` dataclasses, serialised with `dataclasses.asdict`. `pdv print-schema` emits `{premises, results, verdict, aggregate_input, aggregate}`, embedding α's `result_record` verbatim. pdv imports α's enums and never re-spells them. The `.mjs` keeps literal schema constants, because it cannot read files (M4). One test asserts each schema an `agent()` call actually received equals `pdv print-schema` (captured via M5). The role prompts interpolate their vocabulary lists from those constants, so prose cannot drift from schema. Placing pdv's vocabularies under pdv's `print-schema` rather than α's is the layering reading of the ruling (heuristic 9: α never imports pdv) — ruled in §11 R3.
- **D6 — the `.mjs` relays; Python decides (f129).** Stage 3 returns a typed per-leaf result: `{label, status: synthesized, verdict}`, `{label, status: unenumerated}` or `{label, status: synthesize_failed, reason}`. The pipeline's `null` stays the dropped-leaf marker. After the pipeline, one **Aggregate** role (haiku, low effort) runs `pdv aggregate` and returns its JSON under `AGGREGATE_SCHEMA`. `pdv aggregate` computes every disposition, the dropped-leaf labels including the length-contract-violation label, the counters and the summary. The `.mjs` keeps `normalizeLeaves`, `leafLabelFor` (the one home of display labels, passed into the aggregate input), the role prompts, and two fail-closed constants: a null Aggregate relay returns `disposition: BLOCKS`, `blocks: true`, and a summary naming the relay failure. `droppedLeafLabels`, `batchVerdict`, and all disposition and counter arithmetic are deleted from JS.
- **D7 — evidence never crosses the extra relay hop.** The aggregate input carries per-leaf counters and labels only (`status`, `blocks`, `blocking`, `executed`, `total`, `malformed`, `fixture_absent`), never `report`. The `.mjs` attaches each leaf's own `report` back by index (a zip, no logic) when it builds `leaf_verdicts` from the aggregate's `leaf_dispositions`. Captured command output is therefore relayed exactly once, by the Synthesize role, as today. Residual risk, accepted: the Aggregate relay can mis-copy a disposition, the same trust level as Synthesize today. The enum schema and the fail-closed null path bound it.
- **D8 — the workflow's top-level return keeps its 13 keys and their meanings.** The keys are `blocks`, `blocking`, `leaf_verdicts`, `summary`, `disposition`, `leaves_total`, `leaves_probed`, `leaves_unenumerated`, `leaves_not_verified`, `unenumerated_leaves`, `not_verified_leaves`, `malformed_records` and `fixture_absent_records`. They are what the `/prd` overlay's "Reading the result" table documents, so no reader migrates.
- **D9 — one ProbeContext, one argv (f130).** `ProbeContext(repo_root, reify_bin, tree_sitter_bin, cache_home_override)` is a frozen dataclass built once, by `ProbeContext.from_environment()` in `main()`. It replaces the separate `_find_repo_root()` calls in `_resolve_reify_bin`, `build_command`, `run_probe`, `grammar_generated` and `evaluate`, and the env reads in `_resolve_reify_bin` / `_resolve_tree_sitter_bin`. The grammar cache dir is still derived per grammar probe (the deliberate not-memoized contract of `_grammar_cache_home` stands). The runner signature becomes `runner(probe, ctx) -> ProbeRun`; `ProbeRun.command` is the argv the runner executed. `evaluate(probe, ctx, runner=run_probe)` records `run.command` and never builds a second argv. The other new signatures: `main(argv, runner=run_probe)` and `grammar_substrate_usable(ctx, runner=run_probe)`. Tests drive `main()` with a fake runner and a fixture-directory `ProbeContext`; no test patches or reads an α private name.
- **D10 — one path-classification module, orthogonal facts, consumer-owned precedence (f125).** Build on #7491's `scripts/reify_path_attribution.py` (name kept). `attribute(path)` returns a frozen `PathFacts` with independent fields:
  - `is_global`
  - `seed_crates` (the `_file_to_crate` rules, including the corpus arm)
  - `unioned_crates` (the manifest → DAG-gate rule)
  - `inert` (`docs/*`, `*.md`, `*.yaml`, `*.yml`)
  - `tree` (`crates` | `gui_tauri` | `gui_src` | `gui_other` | `tests_infra` | `examples` | `other`)

  Each consumer applies its own precedence over these facts. A single precedence-ordered class cannot serve both consumers: `gui/README.md` is `gui=1` to `decide_scope` but inert to `affected_crates`. The globs live only in the module. `compile_closure(metadata, seeds)` is `occt-scope-lib.sh`'s `_REIFY_COMPILE_CLOSURE_PY` lifted verbatim (normal/build vs dev edges, no try/except). It is the one closure: the detector's kind-agnostic `_reverse_closure` and #7491's recorded divergence (B) leave with the detector (#8886; R1 as reversed). The module is stdlib-only, importable by `/usr/bin/python3`, and has a CLI. `attribute [--format tsv|json]` takes paths on stdin, one record per line. `closure SEED...` takes `cargo metadata` JSON on stdin and prints names one per line (the heredoc's contract: non-zero on malformed input). Each consumer calls the CLI **once per invocation over its whole path list**, never per path: #7491 measured 73 s with a per-path fork against 3.9 s fork-free over 4,725 paths.
- **D11 — escalation_mcp is the one client (f131).** `McpSession(url, client_name, call_timeout_seconds=MCP_TIMEOUT_SECONDS)` bounds each **whole** exchange — the initialize pair and each `tools/call` — on a daemon thread. The thread is await's `_within`, moved and renamed `_bounded`. A timeout raises `TimeoutError`, an `OSError` subclass, so existing callers' `except OSError` is unchanged. A transport failure drops the session id; the next `call_tool` re-initializes, which await's poll loop relies on. Decoding is escalation_mcp's: an empty body is `None` and is refused in `tool_payload`, `structuredContent` is preferred, all text items are joined, and an `error` key is a refusal (correct for `merge_status`, M8). `EscalationError` gains structured fields `tool` and `reason` (`refused` | `rpc_error` | `tool_error` | `unparseable` | `empty`). await maps any `EscalationError` / transport error to `ProbeUnavailable` (non-terminal), and `resolve_escalation_url` keeps the `--escalation-url` override but otherwise delegates to `escalation_url_from_mcp_config`. The protocol version is one constant, its value tactical (§10 Q4).
- **D12 — no new infra wrapper, no new manifest row.** Test-file partitions stay under `scripts/` and run from the existing `tests/infra/test_prd_capability_check.sh` / `test_prd_decompose_verify.sh` wrappers (one unittest invocation each). So `tests/infra/run-all-classification.manifest` (S19's artefact) is untouched. New guards are added to existing suites.
- **D13 — adoption ratchets (promoted INV candidate 5).** Each leaf that collapses N copies into one ships a count test on the superseded inline pattern in an existing suite, named in its row: γ, ε, η, θ, κ.

## 5. Contract (H)

### 5.1 α `--json` result record (additive)

```
{capability, probe_kind, verdict, command, exit_code, stdout, stderr,   # unchanged
 fixture:          "<probe.fixture as written in the probe set>",
 fixture_present:  true | false,
 harness_failure:  null | {kind: "launch"|"setup"|"timeout"|"grammar_load",
                           detail: "<errno / tool text>", cache_denied: bool},
 value_observation: …}                                                   # value kind only, unchanged
```
`RESULT_RECORD_SCHEMA` (the `.mjs`) requires `fixture_present` (`boolean | null`, `null` only on the Prover's own no-process `HARNESS_ERROR` fallback) and admits `fixture` and `harness_failure`. `verdict == HARNESS_ERROR` iff `harness_failure is not null` or grammar exit ∉ {0, 1}. No `stderr` substring is a routing input anywhere in α or pdv except the one tree-sitter boundary parser (D2).

### 5.2 `classify_record` precedence (pdv)

1. verdict ∉ {FAIL, UNPROVABLE, HARNESS_ERROR} **and** `fixture_present is not False` → `NON_BLOCKING`
2. verdict == HARNESS_ERROR → `BLOCKING`
3. no executed-probe evidence → `MALFORMED`
4. `fixture_present is False` → `FIXTURE_ABSENT` (any remaining verdict, including a PASS that reached here via the rule-1 guard)
5. otherwise → `BLOCKING`

(Rule 1's guard is what routes M1's PASS-on-missing-fixture to `FIXTURE_ABSENT`; with D3 it can only arise from a relayed record, since α itself now emits UNPROVABLE there.)

### 5.3 Binding table (pdv)

`AssertionBinding(probe_kind: ProbeKind, observation: Observation, required_match: Mapping)`; `premise_to_probe` = required ∪ premise (refuse on conflicting value); `bind_premises` validates only through α `load_probe_set`, which holds the one check-kind rule.

### 5.4 `pdv aggregate`

```
stdin/file: {leaves: [{label}],                                  # submitted batch, in order
             leaf_results: [null                                  # dropped by pipeline()
                            | {label, status: "unenumerated"}
                            | {label, status: "synthesize_failed", reason}
                            | {label, status: "synthesized",
                               verdict: {blocks, blocking, executed, total, malformed, fixture_absent}}]}
stdout: {disposition: BatchDisposition, blocks, blocking, summary,
         leaf_dispositions: [{label, disposition: LeafDisposition, executed, total,
                              malformed, fixture_absent}],          # aligned to leaf_results' non-null entries
         leaves_total, leaves_probed, leaves_unenumerated, leaves_not_verified,
         unenumerated_leaves, not_verified_leaves, malformed_records, fixture_absent_records}
exit: 0 nothing blocks · 1 blocks · 64 usage/parse error (a relay MUST relay stdout on 0 and 1)
```
Semantics are today's `.mjs` semantics, moved: `leaves_probed` counts `executed > 0`, never dispositions; `leaf_results` shorter than `leaves` adds `<dropped-leaf:contract-violation:N>`; zero leaves → `PASS`.

### 5.5 ProbeContext / runner (α)

`ProbeContext.from_environment() -> ProbeContext`; `run_probe(probe, ctx, timeout=None) -> ProbeRun`; `evaluate(probe, ctx, runner=run_probe) -> Result`; `main(argv, runner=run_probe) -> int`; `grammar_generated(ctx)`, `grammar_substrate_usable(ctx, runner=run_probe)`.

### 5.6 Path classifier (`scripts/reify_path_attribution.py`)

`attribute(path) -> PathFacts`; `compile_closure(metadata: dict, seeds: Iterable[str]) -> frozenset[str]`; CLI `attribute [--format tsv|json] < paths`, `closure SEED... < metadata.json`. Consumers: `affected_crates` (global → ALL; seeds/unioned; `inert or tree in {gui_src, tests_infra}` → skip; else → ALL), `decide_scope` (S18: its arm order re-expressed over facts in the Python planner, with its fixture-list and `docs/gui-event-channels.md` policy arms kept as verify policy ahead of the fact map), and #8887's merge-independence command (held for Leo: dependent iff both sides are non-inert and either side is global or the two closures intersect). The overlap detector is not a consumer: it is retired (#8886; R1 as reversed).

### 5.7 McpSession (`scripts/escalation_mcp.py`)

`McpSession(url, client_name, call_timeout_seconds=30)`; `.call_tool(name, arguments) -> dict` (opens lazily; raises `EscalationError(tool, reason)` on refusal, `TimeoutError`/`OSError`/`ValueError`/`HTTPException` on transport); `escalation_url_from_mcp_config(path)` unchanged.

## 6. Boundary-test sketch (both directions)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | α → pdv: rejection premise on a not-yet-written fixture | real `target/*/reify`; fixture path absent | α record `fixture_present: false`, verdict UNPROVABLE, α exit 2; `pdv synthesize` → `fixture_absent: [cap]`, `blocks: false`, `executed: 1` |
| B2 | α → pdv: `parses` premise on a missing fixture (#8122) | grammar substrate usable (else SKIP via `--grammar-substrate-status`) | record `fixture_present: false`; synthesize → `FIXTURE_ABSENT`, not blocking |
| B3 | α → pdv: genuine FAIL quoting ENOENT on the fixture's own line | stub `REIFY_BIN` printing `<fixture>: expected "No such file or directory"`, exit 0; fixture present | record `fixture_present: true`; synthesize → BLOCKING (today mis-routed to FIXTURE_ABSENT) |
| B4 | α harness failures | `REIFY_BIN` nonexistent / grammar cache dir pre-created as a file / tree-sitter stub that hangs under `--grammar-substrate-status` / stub printing the cache-denied signature | `harness_failure.kind` = launch / setup / timeout / grammar_load (`cache_denied: true`); verdict HARNESS_ERROR; exit 70 (75 for substrate status); `stderr` contains no `[HARNESS ERROR` text |
| B5 | relay → pdv: a record lacking `fixture_present` | hand-written FAIL record with evidence, no field | BLOCKING (fail-closed, D3) |
| B6 | pdv bind fills the required match | `{resolves, match: {}}` | bind rc 0, emitted probe `match == {exit_code: 0}`; α on a fixture that fails `reify check` → FAIL |
| B7 | pdv bind conflict | `{resolves, match: {exit_code: 1}}` | bind rc 64, message names `resolves`, `exit_code`, `0` vs `1` |
| B8 | α load refusal | probe set with a `check` probe, `match: {}` | `prd-capability-check.py` rc 64 naming `probe[i]`; all 14 committed probe sets still load |
| B9 | .mjs ↔ schema parity | scenario harness, one leaf | every `opts.schema` the script passed equals the matching `pdv print-schema` member; every `AssertionKind` and `Verdict` value appears in the prompt the script sent to Enumerator / Prover / Adversary |
| B10 | .mjs → aggregate → .mjs | scenario harness whose Aggregate mock runs the real `pdv aggregate` on the payload block it was sent | for each of: zero leaves, one dropped leaf, unenumerated leaf, synthesize-null leaf, short `leaf_verdicts` contract violation, clean leaf — the top-level return equals today's (pinned before η lands) on all 13 keys |
| B11 | aggregate relay dies | Aggregate mock returns null | return `disposition: BLOCKS`, `blocks: true` |
| B12 | `normalizeLeaves` through the public seam | `args` a JSON-stringified 3-leaf array | 3 Enumerator calls, not 1 (replaces the source-sliced `TestMjsNormalizeLeaves`) |
| B13 | classifier ↔ bash | `affected_crates` over every tracked path, before vs after θ | identical output per path set (pinned pre-θ corpus) |
| B14 | classifier ↔ verify.sh | `verify.sh --print-plan --scope staged` over the scope fixtures in `tests/infra/test_verify_scope.sh` | byte-identical plans before vs after S18's adoption (S18's test, recorded here for the seam) |
| B15 | closure ↔ occt | `occt_touching_set` vs `affected_crates reify-kernel-occt seed` | equal (existing regression guard, now over the lifted closure) |
| B16 | McpSession ↔ stub server | `tests/infra/stub_escalation_mcp.py`, a reply that trickles past the deadline / a restarted server forgetting the session / `{'error': …}` payload | `TimeoutError` within the bound; next call re-initializes and succeeds; `EscalationError(reason="refused")` |
| B17 | await end to end | stub server answering `merge_status` `{state: done}` | JSON verdict line `LANDED`, exit 0 — same as today |

## 7. Cross-PRD / cross-stream relationship (G4)

| Other party | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/prd-gate-executable-substrate-verification.md` (D1/D3 parent) | refines | α record, verdict vocabulary (§6 decision 3 kept: no new verdict), evidence rule (§6 decision 4 kept) | this PRD | parent shipped; no edit needed beyond what γ–η's diffs state |
| `.claude/skills/prd/project.md` "Decompose mode — run the substrate-verification workflow" | consumes | "the `.mjs` only drives agents and aggregates" prose + "Reading the result" table | this PRD (η edits the prose; the table's keys are unchanged, D8) | queued in η |
| Enumerator / Synthesize / Prover / Adversary prompts | produces | vocabulary interpolated from schema constants | S22 (program G4 row) | queued in ζ/η |
| task #7491 (in flight, `task/7491`) | consumes | `scripts/reify_path_attribution.py`, `OVERLAP-MIRROR-PARITY` | #7491 | θ depends on it |
| task #8122 (pending) | subsumes | structural fixture presence | this PRD γ | cancel #8122 at decompose, reason "subsumed by S22 γ" |
| task #8178 (pending) | subsumes | await → escalation_mcp | this PRD κ | cancel #8178 at decompose, reason "subsumed by S22 κ" |
| S18 verify.sh (wave 2) | produces for | `decide_scope` reads `attribute`; `scripts/verify-pipeline-paths.txt` row for the module | S18 (ruling R2): its Python planner imports the module rather than porting the bash ladder, and S18 adds the paths row; S22 edits no verify.sh line | cross-reference to S18 |
| dark-factory merge queue (rebase re-verify gate) | reify supplies | a merge-independence command named by `merge_rebase_independence_cmd` (exit 0 independent / 1 dependent / other = error) | dark_factory #6943 (hook) + #8887 (command, held for Leo) | R1 reversed 2026-10-12; the never-wired detector is retired by #8886, dark-factory's registry half by dark_factory #6944 |
| S19 run_all / manifest | none | no new wrapper (D12) | — | — |
| `docs/prds/verify-scope-contract.md` §3 | amends | "`_file_to_crate` is the mapping's SPOT" | θ repoints it at the module | queued in θ |

## 8. Decomposition plan

Task ids were assigned at decompose (pass 2, 2026-10-12). The former leaf ι moved to S18 (ruling R2). Every row's stamps follow the program's shared conventions (`metadata.x_finding_key`, `x_finding_run = review-all-reify-20261010`, `source = review-all`, priority = severity).

- **α (#8633) — Partition `scripts/test_prd_capability_check.py` along its measured 6-way partition** (low; no key — h14 measurement). Move the importlib loader, the value-cell constants and the `_ts_stub_*` helpers into a downward support module `scripts/_pcc_test_support.py` that imports no test file. Each partition file that uses `_grammar_e2e_status` takes its own `_GRAMMAR_STATUS_CALLS_AT_IMPORT` tail snapshot (the coupling the measurement names). `tests/infra/test_prd_capability_check.sh` runs every partition in one unittest invocation. Files: `scripts/test_prd_capability_check.py`, `scripts/_pcc_test_support.py`, `tests/infra/test_prd_capability_check.sh` (+ new partition files under `scripts/`). **Signal:** the wrapper's unittest run lists the same test-id set before and after (both lists recorded in the commit), and no partition file imports another test file. Prereqs: none. Unlocks γ, δ, ε.
- **β (#8639) — Partition `scripts/test_prd_decompose_verify.py` 4-way; the `.mjs` tests move to `scripts/test_prd_decompose_verify_mjs.py`** (low; no key — h14 measurement). Loaders go in `scripts/_pdv_test_support.py`. The ~1,575 node-driven lines import only `_run_node_module` / `_PDV_MJS`. Wrapper as α. **Signal:** identical test-id set before and after; `test_prd_decompose_verify.py` contains no `node` invocation. Prereqs: none. Unlocks γ, δ, ζ, η.
- **γ (#8642) — Typed probe outcome: `harness_failure` and `fixture_present` on ProbeRun and the `--json` record; pdv classifies from fields** (high; keys [fk-d75787147427]; subsumes #8122). Implements D1–D3 and §5.1/§5.2. Deletes:
  - `_BINARY_NOT_FOUND_SENTINEL` and `_PROBE_TIMEOUT_SENTINEL` with their six routing sites
  - pdv's `fixture_absent_evidence` / `_reports_enoent` / `_path_token_re` / `_FIXTURE_ABSENT_*`
  - pdv's read of `pcc._BINARY_NOT_FOUND_SENTINEL`
  - the `fixture_argument(command[-1])` recovery

  The `.mjs` `RESULT_RECORD_SCHEMA` and the Prover's HARNESS_ERROR fallback template carry `fixture_present`. Files: `scripts/prd-capability-check.py`, `scripts/prd-decompose-verify.py`, `scripts/prd-decompose-verify.mjs`, the α/β partition files. **Signal:** B1–B5 run as CLI tests against the real `reify` binary (B3/B4 against stub tools through α's real subprocess path). M1's reproduction now reports `fixture_absent: [<cap>]`, not `VERIFIED`. **Ratchet (INV-5):** a test in the α suite asserts three counts over `scripts/prd-capability-check.py` + `scripts/prd-decompose-verify.py`:
  - zero `_SENTINEL` names and zero `[HARNESS ERROR` string producers (today 2 constants, 6 routing sites, 2 producers);
  - zero errno regexes in pdv (today `_FIXTURE_ABSENT_ERRNO_RE`);
  - `_GRAMMAR_LOAD_FAILURE_MARKER` / `_PERMISSION_DENIAL_RE` referenced from exactly one function, the D2 boundary parser.

  The probe predicates' own `stderr_contains in run.stderr` reads (`match_predicate`, the `ir` arm) are the probe's purpose, not harness self-reporting, and are not counted. Prereqs: α, β.
- **δ (#8644) — Each binding row carries its required match; α refuses a vacuous check probe** (high; keys [fk-1922380bc3aa]). Implements D4 and §5.3. Deletes the two rejection-only `if` blocks in `bind_premises`. The Enumerator prompt's per-kind lines are corrected where they promise a default (`resolves` binds `exit_code: 0` whether or not the premise says so). Files: `scripts/prd-decompose-verify.py`, `scripts/prd-capability-check.py`, `scripts/prd-decompose-verify.mjs`, partition files. **Signal:** B6–B8 as CLI runs (`pdv bind`, `prd-capability-check.py`), and every committed `tests/prd-gate/*.json` still loads. The vacuous-verdict measurement `verdict(observe('check', exit 1, {}), 'present')` is no longer reachable from a loaded probe set. Prereqs: α, β.
- **ε (#8649) — ProbeContext and a runner seam: one repo root, one argv, tests on public seams** (medium; keys [fk-f34f07d2dde4]). Implements D9 and §5.5. Rewrites the α/pdv tests that patch `pcc.run_probe` / `evaluate` / `grammar_substrate_usable` / `_find_repo_root` / `_SUBSTRATE_PROBE_TIMEOUT_S` into `main(argv, runner=…)` + fixture-dir `ProbeContext` tests. Keeps `tests/infra/test_tree_sitter_parse_isolation.sh` Block D true or updates it same-diff (M9). Files: `scripts/prd-capability-check.py`, α partition files, `tests/infra/test_tree_sitter_parse_isolation.sh`. (pdv uses α in-process only for `load_probe_set`, and after γ for its enums, so ε does not touch it.) **Signal:** `prd-capability-check.py --json` on `tests/prd-gate/example-probe-set.json` emits `command` equal to the argv actually executed. It is shown by a stub `REIFY_BIN` that echoes its argv, compared field-for-field. Both test suites pass. **Ratchet (INV-5):** a test asserts zero `pcc\._[A-Za-z]` reads and zero `patch.object(pcc` / `mock.patch("prd_capability_check.` sites across the α/pdv test files. Baseline at `8e6ffcac9d`: 70 `pcc._` reads (69 in the α suite, 1 in pdv's) and 7 `patch.object(pcc, …)` sites, among 79 `mock.patch` sites in all. The remaining `mock.patch` sites on stdlib names (`os.environ`, `subprocess`) are not counted. Prereqs: γ.
- **ζ (#8651) — One published vocabulary: α enums + `--print-schema`, pdv enums + `print-schema`, `.mjs` checked against them** (medium; keys [fk-bf4e8bbb7dda]). Implements D5. Deletes `_BLOCKING_VERDICTS`' literal and the `"HARNESS_ERROR"` literals in pdv, and main()'s hand-built output dict (`asdict`). The Premise / `_parse_premises_file` kind lists become derived; the stale "α's file is NOT edited" / "imported via pcc above" docstrings are corrected. The prompts interpolate vocabulary lines from the schema constants. Files: `scripts/prd-capability-check.py`, `scripts/prd-decompose-verify.py`, `scripts/prd-decompose-verify.mjs`, partition files. **Signal:** `prd-capability-check.py --print-schema` and `prd-decompose-verify.py print-schema` print valid JSON schemas, and B9 passes. Mutation check recorded in the commit: adding a verdict to α's enum alone reds B9. Prereqs: γ, δ.
- **η (#8655) — `pdv aggregate`: the `.mjs` relays dispositions computed in Python; no test slices `.mjs` source** (medium; keys [fk-6a006c36a78d]). Implements D6–D8 and §5.4. Adds the Aggregate role to `meta.phases` (haiku, effort low) and corrects the `.mjs` header. Deletes `droppedLeafLabels`, `batchVerdict` and the JS aggregation tail. Replaces `TestMjsNormalizeLeaves`, `TestMjsDroppedLeafLabels` and `TestMjsResultsSchema` with `pdv aggregate` CLI tests plus scenario tests B10–B12. Rewrites the overlay's D3 paragraph ("the `.mjs` only drives agents and relays; `pdv aggregate` computes the disposition"). Files: `scripts/prd-decompose-verify.py`, `scripts/prd-decompose-verify.mjs`, `scripts/test_prd_decompose_verify_mjs.py`, `.claude/skills/prd/project.md`. **Signal:** B10 — the workflow's 13-key return is unchanged on the six pinned scenarios with the real `pdv aggregate` in the loop. `prd-decompose-verify.py aggregate` on a recorded leaf-results file prints the disposition. **Ratchet (INV-5):** zero occurrences of the anchor string `const _wfResult = await` and of `new Function(` in `scripts/test_prd_decompose_verify*.py`. Prereqs: ζ.
- **θ (#8662) — One path classifier and one compile closure: affected-crates-lib.sh and occt-scope-lib.sh read `reify_path_attribution.py`** (high; keys [fk-21c29ddc104d] — the bash, Python and closure homes; the verify.sh home moves to S18 per ruling R2). Implements D10 and §5.6 for the bash libs. The detector is out of θ's scope since R1's reversal; #8886 retires it:
  - `PathFacts` gains `inert` and `tree`.
  - `compile_closure` is lifted from the `occt-scope-lib.sh` heredoc, with `_reify_compile_closure` kept as a one-line bash shim over `closure` so `occt_touching_set` and its tests keep their entry point.
  - `affected_crates` calls `attribute` once over its argv.
  - `_is_global` / `_file_to_crate` / `_is_noncrate` / `_is_crate_manifest` / `_RI_CORPUS_CRATES` / `_REIFY_DAG_GATE_CRATE` are deleted from bash.
  - `reify_is_inert_path` stays as a thin wrapper over the module because `verify.sh::decide_scope` still calls it; S18 deletes it when its planner imports the module.
  - `OVERLAP-MIRROR-PARITY` (nothing left to mirror) is replaced by B13's pinned corpus.
  - `docs/prds/verify-scope-contract.md` §3's SPOT sentence is repointed at the module. The module's `scripts/verify-pipeline-paths.txt` row is S18's (ruling R2).

  Files: `scripts/reify_path_attribution.py`, `scripts/affected-crates-lib.sh`, `scripts/occt-scope-lib.sh`, `tests/infra/test_affected_crates_lib.sh`, `docs/prds/verify-scope-contract.md`. **Signal:** B13 and B15 green. `source scripts/affected-crates-lib.sh; affected_crates examples/bracket.ri` prints the same six corpus crates' closure as before. `python3 scripts/reify_path_attribution.py attribute <<< examples/bracket.ri` prints the corpus seeds. **Ratchet (INV-5):** `tests/infra/test_affected_crates_lib.sh` counts `case` arms whose pattern begins `crates/`, `gui/src-tauri/`, `examples/` or `tree-sitter-reify/` in `scripts/affected-crates-lib.sh` and `scripts/occt-scope-lib.sh`; the count must be zero (today 6). θ also counts embedded programs in `scripts/*.sh` that define `normal_closure`; that count must be zero (today 1, the `occt-scope-lib.sh` heredoc). `assert-crate-dag.sh`'s forward-adjacency builder answers DAG rules, not a closure, so it is not counted (§9). Full gate (verify-pipeline files). Prereqs: #7491 (out-of-batch, in flight).
- **κ (#8664) — `escalation_mcp.py` is the one MCP client; await-merge-landing uses it** (medium; keys [fk-0497ab17e729]; subsumes #8178). Implements D11 and §5.7. Deletes `MergeStatusClient`, `_read_reply`, `_exchange`, `_jsonrpc_result`, `_tool_payload`, `_within` and `_MCP_HEADERS` from await. `test_await_merge_landing.py`'s private `Handler` stub is replaced by `tests/infra/stub_escalation_mcp.py`. `scripts/verify-pipeline-infra-tests.txt` gains `scripts/escalation_mcp.py    tests/infra/test_await_merge_landing.sh`. Files: `scripts/escalation_mcp.py`, `scripts/await-merge-landing.py`, `tests/infra/test_await_merge_landing.py`, `tests/infra/stub_escalation_mcp.py`, `scripts/verify-pipeline-infra-tests.txt`. **Signal:** B16 and B17. `scripts/await-merge-landing.py --commit <landed sha> --request-id <id>` against the live escalation server still prints a `LANDED` verdict line. `test_docs_truth_sweep.sh` and `test_reify_audit_pprdstatus_wiring.sh` stay green. **Ratchet (INV-5):** a test in `test_await_merge_landing.py` counts modules under `scripts/` that send a JSON-RPC `"initialize"`; the count is 1 (`escalation_mcp.py`). Prereqs: none.
- **λ (#8667) — PRD close: id backfill, terminal stamp, AS-AUTHORED freeze** (low; no key). Per the overlay's decompose-close obligations: backfill real ids into §8, then stamp `SHIPPED` (or the honest terminal token) with the landed leaf ids, the freeze paragraph and the LIVE/AS-AUTHORED map. §5 stays LIVE: α/pdv docstrings defer to it. Apply the matching header to the capability manifest. Docs-only; `task_kind normal`, complexity simple. **Signal:** the committed header. Prereqs: every other leaf (a cancelled sibling counts as satisfied).

DAG: α, β → γ, δ; γ → ε; γ, δ → ζ → η; #7491 → θ; κ independent; all → λ.

G7 (advisory walk, reify family): the umbrella `nothing-vacuous-and-unowned` is what γ and δ close in the gate itself (M1, M2). The one hit this PRD cannot resolve alone is the unwired detector (M6). R1 as reversed on 2026-10-12 retires it (#8886) rather than wiring it. INV-SF-4 `indeterminate-attributable-transient`: D3's UNPROVABLE carries its typed cause (`fixture_present: false`). No other hit.

## 9. Out of scope

- The overlap detector's retirement (#8886) and its gate-level replacement (dark_factory #6943, reify #8887). See R1.
- `verify.sh::decide_scope` adopting the classifier, and the module's `verify-pipeline-paths.txt` row (S18, ruling R2).
- Porting `verify.sh` to Python (S18).
- A shell primitive for Workflow scripts (would remove the D7 relay hop; not ours).
- Executing `delivered_check`s (#6536).
- New probe kinds; changing the verdict vocabulary.
- `scripts/assert-crate-dag.sh`'s own `cargo metadata` adjacency builder. It checks forward-DAG rules, not reverse closure, and S3/S7 own its neighbourhood (INV candidate 7).

## 10. Open questions (tactical)

1. **Partition file names** (α, β). Suggested: `scripts/test_pcc_<area>.py` / `scripts/test_pdv_<area>.py` plus the support modules named in α/β. Decide in α/β.
2. **`attribute` output format for bash** — TSV vs JSON lines. Suggested: TSV for bash, JSON for Python, one `--format` flag. Decide in θ.
3. **Aggregate role model/effort.** Suggested haiku / low, matching Synthesize's relay character. Decide in η.
4. **MCP `protocolVersion` constant.** await uses `2025-03-26`, escalation_mcp and the shared stub `2024-11-05`. Suggested: `2025-03-26` (the version await has exercised against the live server), stub updated to match. Decide in κ.

## 11. Lead rulings (pass 1, 2026-10-12)

- **R1 — the overlap detector is unwired in production (M6).** The pass-1 ruling was: keep θ importing the shared module, and have the lead file a dark-factory task to register it (`register_for_reify`, plus passing `project_id` to the merge-queue overlap lookup). **REVERSED by Leo, 2026-10-12** (interactive /team investigation; decisions_and_rationale memory, project reify):
  - Retire the detector, its two test scripts and its deploy runbook (#8886, after #7491). Wired as designed, it would only have reordered the merge queue. No incident motivated it, and both of its test scripts SKIP-and-PASS in the merge gate.
  - The crate-closure idea moves to the decision that gates safety, dark-factory's rebase re-verify gate. Dark-factory adds a generic, config-named independence hook (dark_factory #6943) and drops its unused registry (dark_factory #6944). Reify supplies a merge-independence command built on θ's CLI (#8887, held until Leo releases it).
  - θ no longer touches the detector.
- **R2 — the verify.sh home** → moves to S18: no bash edits to `scripts/verify.sh` ahead of its Python rewrite. fk-21c29ddc104d lands on θ minus the verify.sh home. S18's planner imports `scripts/reify_path_attribution.py`, and the `verify-pipeline-paths.txt` row is S18's. The former leaf ι is not part of this PRD.
- **R3 — vocabulary home** → layered, as D5 states.
- **R4** → the D4 conflict refusal, D3's UNPROVABLE-on-absent-fixture with fail-closed fallback, and D6/D7's Aggregate relay without evidence are confirmed.
