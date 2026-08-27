# Reify PRD overlay

Project specialization for the generic `/prd` skill (`~/.claude/skills/prd/`, → `dark-factory/skills/prd/`). The generic skill reads this file at Step 0 and applies it as authoritative extensions/overrides to its gates. **This directory has no `SKILL.md` by design** — see `README.md`.

## Identity & paths

- **project_id:** `reify`
- **project_root:** `/home/leo/src/reify`
- **PRD path:** `docs/prds/<vM_N>/<slug>.md`, where `<vM_N>` is the milestone dir (`v0_3`, `v0_4`, `v0_5`); root-level `docs/prds/` for version-agnostic foundations.
- **Substrate-confirmed metadata field:** `grammar_confirmed` (bool): true iff the task's mechanism uses existing grammar, false if it queues grammar work.

## Landing PRD artifacts (author-mode Stage 10 / decompose Step 5.5)

Commit the PRD `.md`, its `.capability-manifest.md`, the `.capability-manifest.yaml` sidecar and any `.ri` fixtures **directly on `main` in project_root, in ONE commit**. Do **not** route them through the orchestrator merge queue, and do **not** use `scripts/land.sh`.

Both branch routes stamp `DF_VERIFY_ROLE=merge`, which makes `scripts/verify.sh` force `--scope all` (contract C2, `verify.sh:831`) and `--profile both` (`:654`); dark-factory's INV-1 (`verify.py:5499`, DF task 2883) additionally refuses the docs-only trivial pass at merge role and escalates it to the full global gate, because an adoptable merge verdict must never be stamped from zero evidence. A docs-only merge therefore costs a full cold debug+release workspace build — reify budgets `merge_verify_cold_command_timeout_secs: 10800` (3 h). A commit on `main` instead runs `hooks/pre-commit` → `verify.sh all --profile debug --scope staged --include-infra` with `DF_VERIFY_ROLE` unset (role `task`, so no C2 force), where `decide_scope` classifies `docs/*|*.md|*.yaml|*.yml` **plus `tests/prd-gate/fixtures/*.ri`** (task #5536) as no-heavy-checks: seconds — so the one-commit landing really is seconds *including* the fixtures.

**Where `.ri` fixtures go (standard, Leo 2026-08-21).** A committed `.ri` fixture that exists as PRD or capability-manifest evidence lives in **`tests/prd-gate/fixtures/`**, and is cited by **full repo-relative path** — never bare `fixtures/<name>.ri`, never a bare stem. `docs/prds/**/fixtures/` is **DEPRECATED**; do not put new fixtures there (#6431 is emptying it). The other three tiers, keyed on who *consumes* the file rather than who motivated it: ephemeral decompose-time probes stay in `/tmp/prd-gate-fixtures/` (`references/grammar-gate.md`) and are never committed; a user-facing design that must keep working goes to `examples/`, graduating to `examples/best_practices/` + `INDEX.md`; a fixture that is purely one crate's test detail and is *not* PRD evidence goes to `crates/<crate>/tests/fixtures/`. A fixture read by a compiled test target does **not** get special placement — it gets **registration** (see the re-baselining trap below).

This is **sanctioned**, not a main-gate violation: `hooks/pre-commit` runs `project-checks` on `main` and then calls `main_gate_mark`, so `hooks/reference-transaction` logs the ref move as SANCTIONED. The `CLAUDE.md` invariant forbids moving `main` *without* a gate (`merge --no-verify`, `update-ref`, `reset`, `commit-tree`) — a hook-gated commit is not that.

Sequence — **flip first, commit the stamped sidecar once**:

1. Write the artifacts as untracked files at their canonical paths.
2. File the batch (`planning_mode=True`), wire deps, then `commit_planning` — it stamps the on-disk sidecar **in place** and copies `delivered_checks` into producer metadata. Never land an unstamped sidecar and stamp afterwards.
3. `git add <each exact file path>`.
4. `git commit --only <same exact paths> -F <msgfile>`.

Traps:

- **Never `--no-verify` on `main`.** `reify.mainGate.enforce` is `true`; `--no-verify` skips `pre-commit`, so no sentinel is marked, `hooks/reference-transaction` sees an UNSANCTIONED main move and **hard-aborts the transaction**. This is dark-factory's docs-only-commit habit — it is fatal here.
- **Never `git checkout -b` in project_root** — it moves the running orchestrator's checkout off `main`.
- Confirm `HEAD == main` before committing; the offline lane can leave project_root on another branch.
- **Never `git add -A` or a directory** — project_root is a shared hot checkout with concurrent `/prd` sessions and the orchestrator; a directory add sweeps their untracked files into your commit. List every path explicitly. (`--only` in step 4 likewise ignores foreign *staged* state; a pathspec commit alone fails on untracked files, which is why step 3 exists.)
- `pre-commit` can exceed a default 2-minute tool timeout — pass `timeout >= 300000`.
- **Re-baselining (or renaming) an EXISTING `.ri` fixture that a Rust test reads** escalates `pre-commit` to the full workspace suite **only if that fixture is registered** — i.e. it lives under `tests/prd-gate/fixtures/` and its basename is in `_RUST_COUPLED_RI_FIXTURES` (`scripts/verify.sh:1080`, arm at `:1176`). Read that list in `scripts/verify.sh` rather than trusting a copy (most `stdlib_ns_*` / `compiler_type_hygiene_*` fixtures are **not** members). Registration is kept honest behaviourally: `tests/infra/test_verify_scope.sh`'s PG-DRIFT re-derives the coupled set from `git grep` over tracked sources on every infra run and goes RED on an unregistered reference. Such an edit belongs on a task branch through the merge queue, not in the docs-landing commit. Adding a NEW fixture does not escalate.
  **Known hole, do not rely on the fast path here:** a fixture under `docs/prds/**/fixtures/` gets **no** escalation at all — `decide_scope`'s `docs/*|*.md|*.yaml|*.yml)` arm (`verify.sh:1216`) is unconditional, with no allowlist and no drift guard — yet five such fixtures are `include_str!` build inputs to compiled test targets that pin exact values. Editing one lands GREEN on `main` and reds an unrelated merge later. That is precisely why the directory is deprecated; #6431 empties it and #6077 closes the wider `docs/`↔compiled-test class.
- **A landed `delivered_check` correction is not one write.** `commit_planning` copies `delivered_checks` into producer task metadata exactly ONCE, at landing (Sequence step 2 above) — a later fix to a manifest pattern must be applied BY HAND to the producer task's `metadata.delivered_checks` too, or the dispatch gate keeps evaluating the stale descriptor. See "Capability Manifest — reify evidence forms" → "Correcting a landed check touches TWO surfaces plus TWO twins."

Landing PRD `.md` and manifest/sidecar via *separate* vehicles is how `resolution-unification` and `stdlib-namespace` ended up with their `.md` on `main` and their manifest halves stranded untracked in project_root (2026-07-25). One commit, all artifacts.

Committing in place also obviates the branch-landing sidecar dance (copying `commit_planning`'s `yaml.dump`-rewritten sidecar back into the branch and `cmp`-ing every file to avoid a dirty tracked file in project_root): the on-disk and committed copies are identical by construction.

## Provenance & portfolio

This skill operationalizes the **2026-05-12 architecture audit**: ~19/44 mechanism clusters fit the **incomplete/ill-formed implementation chain** pattern (memory `preferences_implementation_chain_naming`). The dominant prevention is discipline at PRD-authoring and decomposition time, before any task reaches the orchestrator.

Portfolio approaches baked in: **A** (consumer-first → G1), **D** (user-observable leaf → G2), **E** (cross-PRD seam ownership → G4), **H** (design-first / contracts / two-way boundary tests → G5), plus the grammar gate (→ G3). **C-as-integration-gate** is the task-DAG-shape the decompose mode produces (G2 escape hatch). See `preferences_implementation_chain_portfolio`. **F** (audit cadence + tracking infra) and **G** (corpus-level reviewer lint) are out of scope here.

Audit docs the skill may cite at G4 / META time:
- `docs/architecture-audit/README.md` — three-phase shape, motivation.
- `docs/architecture-audit/audit-brief.md` — failure-mode catalog (F1–F7); the "mechanism" definition (one-sentence end-to-end test).
- `docs/architecture-audit/phase-3-files-synthesis.md` — cluster table (`C-NN`); §2 Pattern 1, §5 surprises.
- `docs/architecture-audit/phase-3-scaffold-pattern-critique.md` — Type A/B/C decomposition + the seven approaches.
- `docs/architecture-audit/phase-3-breadcrumb-map.md` — §3 contested-ownership pairs.
- `docs/architecture-audit/gap-register.md` — GR-IDs cited at G4 / META.

## G1 — integration-seam catalogue + examples

**Engine-integration sub-check.** If a mechanism is an in-engine seam (kernel module, dispatcher, walk, hook, runtime trampoline), its named consumer must plug into one of the 7 in-engine seams in `docs/prds/v0_3/engine-integration-norm.md` §3:

| § | Seam |
|---|---|
| §3.1 | op-execute |
| §3.2 | realization-kind dispatch |
| §3.3 | multi-kernel dispatch |
| §3.4 | ComputeNode dispatch (per `compute-node-contract.md`) |
| §3.5 | ConstraintSolver |
| §3.6 | freshness-only walk |
| §3.7 | KernelAttributeHook |

(§3.8 OptimizedImpl is deprecated; don't cite it for new work.) A NEW seam not in the catalogue is itself a cross-PRD design question — author a norm extension first (or fold into G4). The norm prevents kernel-module-callable-in-isolation drift (cluster C-14 / GR-017). Cite the relevant §3.N as the consumer in "Sketch of approach" or "Cross-PRD relationship".

**Audit examples of the producer-orphan failure:** C-02 (ComputeNode dispatch — producer built, FEA #16 consumer pending for months), C-10 (selector_vocabulary_v2 — 22+ fns in `reify-eval`, none in the eval dispatch table), C-17 (OpenVDB ingestion — full FFI module, `reify-eval` doesn't depend on the crate), C-25 (build_doc_model — HTML formatter exists, CLI uses `render_html_stub`).

## G2 — signal vocabulary + examples

Reify user-observable signal types (extend the generic menu):
- CLI output difference (`reify check ...` emits a diagnostic; `reify <subcmd>` returns specific text).
- Viewport / GUI state change observable via debug MCP (mesh count, screenshot delta, store_state assertion).
- LSP behaviour (hover content, completion item, diagnostic emission).
- A stdlib `.ri` example that exercises the new path and runs in CI.
- A user-facing diagnostic (`E_*` / `W_*` code visible to the end user).

Policy source: `feedback_task_chain_user_observable`. **Reject** "a unit test passes against synthetic input" as a leaf signal — the C-02 example (tasks 3380/3381/3382/3385 each passed unit tests against synthetic inputs and closed cleanly; no user observed anything different). Audit examples of fake-done leaves (cluster C-07): task 2954 (screenshot_window — closed via docs-only commit), 2657 (Manifold MeshGL walk — trait wiring landed, the walk stubbed), 2967 (auto-resolve panel — frontend ready, backend event source absent), 2699 (topology selectors — `done` with `reopen_reason` listing 11 missing dispatch arms).

## G3 — substrate verifier (grammar AND semantic/behavioral)

Reify's substrate verifier has three probe vectors, all empirically grounded in `docs/prds/prd-gate-executable-substrate-verification.md §3`:

1. **Grammar premises — `tree-sitter parse --quiet <fixture.ri>`** (the grammar gate). Full mechanics, fixture-extraction heuristics, the exact command, "what counts as novel syntax", and the documented C-06 grammar-fiction precedents are in **`references/grammar-gate.md`** (`feedback_prd_grammar_gate`). Run at author Stage 2 (fail-fast); re-run at decompose Step 1.

2. **Semantic/behavioral premises — `reify check <fixture.ri>`**. Observes arg-vs-param rejection, type-name resolution, and member-access lowering. **Negative-assertion sentinel:** `reify check` exits 0 + `All constraints satisfied.` + no diagnostic where a rejection was asserted = silent-accept = FAIL (example: `revolute("not-an-axis", …)` — task 4575).

3. **Eval/IR probe (eval-error-signature)** — where `check` is insufficient. `CompiledExprKind::CrossSubGeometryRef` emission in `crates/reify-compiler/src/expr.rs` panics in `eval_expr`; authoring the scenario and running `reify eval` reveals the real IR shape via the panic signature (example: task 4358 — assumed IndexAccess, real shape betrayed by CrossSubGeometryRef panic).

**Four semantic-substrate worked examples (PRD §3/§10):**
- **4575 — arg-vs-param rejection (silent-accept):** `reify check` on `revolute("not-an-axis", …)` exits 0 + `All constraints satisfied.` + no rejection diagnostic. The negative-assertion sentinel fires — the compiler does **no** nominal arg-vs-param rejection for concrete params.
- **4577 — resolve_type_name:** `param t : Transform3` → `reify check` exits 1, diagnostic `error: unresolved type: Transform3`.
- **4437 — member-access lowering-to-ValueRef:** member access on a TypeParam-typed param → poison literal (not ValueRef); surfaces as a diagnostic at `reify check` time.
- **4358 — constraint-IR shape via eval-error proxy:** NOT reachable via `check`; `reify eval` surfaces the `CompiledExprKind::CrossSubGeometryRef` panic in `eval_expr` (`crates/reify-compiler/src/expr.rs`), betraying the real IR shape vs the assumed IndexAccess.

## Decompose mode — run the substrate-verification workflow

At decompose time, invoke the D3 verification workflow **before finalising the leaf batch**:

```
Workflow({scriptPath: "scripts/prd-decompose-verify.mjs"})
```

Per leaf the workflow runs three roles: **Enumerator** → **Prover ‖ Adversary** → **Synthesize**. The Enumerator extracts every premise the leaf signal asserts and enforces the negative-assertion mandate (every "X is rejected" must become a probe that observes the rejection actually fires). Prover and Adversary run in parallel: the Prover authors a probe per premise and runs it through α (`scripts/prd-capability-check.py`); the Adversary independently hunts unlisted premises and falsifications. Synthesize aggregates results. The deterministic harness is `scripts/prd-decompose-verify.py`.

**Blocks the batch** on any `FAIL`/`UNPROVABLE`/`HARNESS_ERROR` with captured command output attached — instead of tabulating an unexecuted promise. (`UNPROVABLE` blocks the same as `FAIL`: "no probe vector can currently observe this" is as dangerous as "the premise is false".)

The script is at `scripts/prd-decompose-verify.mjs` (committed to git — **not** `.claude/workflows/`, which is `.gitignored`), so the path is stable and D4 can re-run it at dispatch time.

## Decompose mode — metadata.files authoring rule (tight-or-empty, never a directory)

For each leaf's `metadata.files`, name a path **ONLY** when the task text gives a high-confidence file anchor — the PRD/task names the file explicitly, or there is exactly one obvious file for the change.

If you would name a directory (any path with no recognized code extension), or you are unsure which files, or the change is a broad refactor of unknown extent → file `[]` and defer to the architect (BRE acquires the real footprint before editing).

**NEVER put a directory in `metadata.files`.** `[]` is first-class and subsumes the broad-refactor case — there is **no** refactor exception. Under-declaration is the safe error direction (BRE acquire-before-edit); over-declaration serializes dispatch.

**Deterministic guard:** Before each leaf's `submit_task`, run `scripts/lock-charter-guard.sh check <files...>`. Exit 1 (REJECT) → do NOT file — rewrite the offending entry to a file anchor or drop to `[]`, re-run. See `.claude/skills/prd/references/decompose-mode.md` Step 3 extension for the full procedure.

## G4 — known contested-ownership pairs

From `docs/architecture-audit/phase-3-breadcrumb-map.md` §3 — three genuinely contested seams (don't introduce a fourth without resolving ownership):
1. `persistent-naming-v2 ↔ multi-kernel` — Manifold MeshGL walk / `propagate_attributes` for ManifoldKernel.
2. `imported-field-source ↔ multi-kernel` — OpenVDB dispatcher/consumer boundary.
3. `topology-selectors ↔ persistent-naming-v2` — `try_eval_topology_selector` dispatch arms.

Plus mild-contradiction: `structural-analysis-fea ↔ structural-analysis-shells` (each notes the other landed code ahead of itself). GR-IDs may be cited from `gap-register.md`.

## G5 — load-bearing seams

High-stakes seams that trigger the B+H prompt (any one is sufficient): **FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser**. Worked precedent: `compute-node-contract.md` had to be retrofitted as the H component for cluster C-02 after months of producer tasks closed without integration (`feedback_orchestrator_narrow_locks_favor_upfront_design`). Default **yes** for these seams, **no** for self-contained features (a single new diagnostic, a single stdlib helper). Approach E (G4) overlaps and is checked separately; a high-stakes PRD typically triggers both. Generic thresholds (blast radius ≥ 3 crates, mechanism count ≥ ~8, cross-PRD consumers ≥ 2) apply unchanged.

## G6 — domain: numerical

Reify is numerically heavy; G6 branches 1 (numeric bound) and 2 (closed-form exactness) fire often. Domain hazards where domain intuition is fragile: FEA numerics (P1-tet **bending lock** — slender columns can't reach tight accuracy at practical mesh density), boundary-condition mapping (pointwise Dirichlet realizes fixed-pin `k≈0.67–0.70`, not fixed-fixed `k=0.5`), spline **end-conditions** (a cubic spline reproduces a cubic only under clamped / not-a-knot, never **natural** — natural forces `M[0]=M[N]=0`), eigensolver conditioning. Worked cautionary examples (all surfaced at execution time, 2026-05-26/27, because a false premise was frozen into a RED test):
- **esc-3436-210** (`multi-kernel-phase-3.md` §8 task ε) — end-to-end capability misattributed: ε's signal demanded output its dependency set couldn't produce; the capability lived in tasks that **depend on** ε. (Branch 3.)
- **esc-3453-5/6** (`buckling-eigensolver.md` §13 task δ) — guessed 5% accuracy bound (bending lock gave 9–10%) + wrong BC mapping. "Tuned" fixture comment was aspirational. (Branches 1+2.)
- **esc-3770-1** (`trajectory-input-shaping.md` §11 task β) — asserted a natural cubic spline reproduces a general cubic to 1e-12; provably impossible. (Branch 2.)

## Gate-test drift-guard registration (authoring + decompose check)

**Trigger:** a task adds a new gate-resident test — a `crates/*/tests/*.rs` integration test that stays on the merge/task gate (not excluded as heavy per `scripts/heavy-test-filter-lib.sh` / `tests/infra/test_verify_gate_exclude_heavy.sh`), or a new `tests/infra/test_*.sh`.

**Author-mode rule (same-diff):** that task's own diff must carry the corresponding drift-guard registration(s) — a bucket row in `tests/infra/run-all-classification.manifest` for a new `tests/infra/test_*.sh` (declared-vs-discovered drift caught by `tests/infra/test_run_all_classification.sh`); the wallclock-bounds registration for any new elapsed-time/wall-clock assertion (`tests/infra/test_no_new_wallclock_upper_bounds.sh`); and nextest heavy/smoke partition entries in `.config/nextest.toml` where applicable (`tests/infra/test_nextest_slow_priority.sh`). **OR** the registration is a hard upstream dependency wired via `add_dependency` — never a sibling ordered only by PRD prose.

**Decompose-mode rule:** reject a leaf batch where the registration task is downstream of, or unordered with respect to, the test-adding task. No deterministic guard automates this (unlike the metadata.files rule above) — the /prd session reading this subsection is the enforcement, at author Stage 2 and again at decompose time.

**Worked case — esc-4914-162 (2026-07-01):** task 4914 landed `crates/reify-solver-elastic/tests/solver_gate_smoke.rs` (gate-resident smoke binary, offline-deep-test-lane PRD task A3) without its drift-guard registrations (`test_run_all_classification.sh` + `test_no_new_wallclock_upper_bounds.sh`), turning main RED for every subsequent merge; the registration (A6) was ordered after A3 by PRD prose only, not a hard `add_dependency` edge — the A3-before-A6 failure this check exists to catch.

## Docs-truth gate (language/stdlib/tooling surface)

**Trigger:** the PRD adds or changes anything a `.ri` author can observe — grammar/syntax, builtins or stdlib functions and their signatures, geometry/query/transform semantics, diagnostics, or CLI/GUI behavior a design session relies on (collectively: language surface).

**Author-mode rule:** the PRD must carry deliverable leaves for all four of:
1. **Doc-chunk update, registry-verified.** The affected `crates/reify-mcp/src/tools/chunks/*.md` chunk(s) updated in this PRD, with every documented signature verified against the compiler arms/registries (`crates/reify-compiler/src/{geometry,geometry_curve,geometry_transform,geometry_modify,geometry_boolean}.rs`, the `units.rs` name registries). Acceptance: each documented signature compiles as written in a smoke `.ri`.
2. **Exemplar-corpus update.** If the change introduces or alters an authoring idiom, a leaf adds/updates a worked example under `examples/best_practices/` (auto-compile-gated by `crates/reify-compiler/tests/examples_smoke.rs`; keep normative claims in code/constraints, not comments) plus its `INDEX.md` line. Where the new feature enables a cleaner idiom than existing corpus or stdlib code uses, updating those exemplars is in scope for this PRD. (Corpus seeded by task #5397.)
3. **reify-design cheatsheet index update.** A one-line index entry in `.claude/skills/reify-design/SKILL.md` pointing at the corpus file — not an inline playbook.
4. **Discoverability acceptance.** The leaf's signal includes intent-level findability: an author who knows the *goal* (e.g. "check two parts don't collide") but not the feature name finds the mechanism from the chunks or the corpus index — the right topic chunk / index line names the capability in intent terms.

**Decompose-mode rule:** reject a leaf batch where a language-surface capability leaf has no same-PRD doc-chunk leaf, or where the doc leaves are ordered by prose only — wire them same-diff or as real `add_dependency` edges.

**Worked case — 2026-07-24 language review:** ~40% of the printer_v01 dogfood session's CLI spend went to probing what the language can do. The static interference/clearance oracle (`interferes`/`min_clearance`/`intersects`/`distance`) had ZERO chunk presence and the session shipped mechanically-checkable interferences (#5389); phantom chunk signatures `rotate(geo,axis,angle)` / `translate(geo,vector)` cost live probe cycles (#5347/#5364). Each was language surface that landed without its doc leaf.

## Code anchors in PRD prose — cite by symbol; date any hard anchor

**The rule is SHARED, not reify's.** Normative text: `~/.claude/skills/prd/references/author-mode.md` → *"Code anchors in PRD prose — cite by symbol; date any hard anchor"* (promoted out of this overlay by task #6357, dark-factory `cbb6b8c3f4`; re-walk pointer at `references/decompose-mode.md` Step 1). Read it there — cite by symbol/file/section-banner; a hard `path:line` only under an as-of SHA + date in the document header; never re-anchor an already-dated snapshot; the reject/fix obligation covers only anchors the session itself introduces or edits. **Do not restate any of that here** (design-invariant no-lockstep-duplication). This section carries only what is reify-specific.

**Reify symbol forms.** The "cite the symbol" default resolves here to Rust paths and fixture registries — `Mesh::validate`, `dispatch_volume_mesh`, `_RUST_COUPLED_RI_FIXTURES` — or a `##`/banner name in the PRD itself.

**In-corpus exemplar header** (what the shared rule's escape hatch looks like when landed here) — the header of `docs/prds/v0_6/engine-build-hardening.md`: "**Code anchors** verified against main `bc3771221f` (2026-07-06). Main moves fast — cite-by-symbol; re-locate lines at implementation time." The pre-fix `kernel-seam-contracts.md` header is the local counterexample the shared rule forbids: it asserted its line numbers were current, with no SHA, against a HEAD now thousands of commits back.

**Provenance.** The "never re-anchor a dated snapshot" clause is Leo's ratified ruling, 2026-08-19; the same principle governs dated `docs/notes/` survey files.

**Enforcement posture.** Reify ships no deterministic guard for this — the /prd session is the enforcement, per the shared rule. Retrospective/detection half: specified but not built as of 2026-08-19 — `reify-audit` accepts no `PPRDSTATUS` arm (nor a #5872 equivalent) yet, so the shared rule plus this section are the sole enforcement until #6346 (leaf/status prose) and #5872 (stale hard `path:line` cites) land.

**Measured 2026-08-19** (dated context, not a live claim): 522 tracked `.md` under `docs/` carried 2,678 fully-qualified path+line cites across 571 distinct paths, 56 of which no longer exist; 285 of 352 tracked `.md` under `docs/prds/` (recursive, including `v0_N/` subdirectories) carried line anchors and only 12 recorded the commit they were true at.

## PRD terminal status — closed vocabulary + decompose-close stamp

**Measured 2026-08-19** (dated context, not a live claim): ~200 of ~350 tracked `.md` under `docs/prds/` (recursive, including `v0_N/` subdirectories) carry a `Status:` marker across ~19 distinct values in four syntactic families, and only 5 had ever reached a terminal one — 4 `SHIPPED` (`v0_6/data-carrying-enums.md`, `v0_6/generic-data-carrying-enums.md`, `v0_6/result-and-fallback.md`, `kernel-seam-contracts.md`) plus 1 `SUPERSEDED` (`v0_6/process-dfm-geometry-metrology.md`). All 5 were stamped by hand after the fact — three in one 2026-08-06 batch, one at a 2026-06-08 PRD split, and `kernel-seam-contracts.md` by this programme's own 2026-08-19 fix — none by any decompose-close step, which is the gap this section closes. A completed plan that still reads as live work manufactures counterfactual claims — the `kernel-seam-contracts.md` recurrence (#6214, #6232, #6274, plus `esc-6232-5`).

**Terminal vocabulary — closed, exactly three values.** A PRD's status is terminal iff the first token after its `Status` label, matched case-insensitively, is one of:
- **`SHIPPED`** — every decomposition leaf reached `done`. Cancelled leaves are tolerated alongside, provided at least one leaf landed.
- **`SUPERSEDED`** — replaced by a named successor PRD; name the successor on the same line.
- **`WITHDRAWN`** — abandoned; every leaf is `cancelled` and there is no successor.

Anything else is non-terminal. The **live-side vocabulary stays free-form** — only the terminal set is closed, which is all that's needed to make terminality machine-checkable, and closing the live side would oblige a corpus-wide header migration that `esc-6232-5` ruled out. **This list is the contract source, not `reify-audit --pattern PPRDSTATUS` (#6346) — the detector is the consumer, and must check against exactly this list when built** (detection status: see **Enforcement posture** above, not restated here). #6346's description was reconciled to exactly this three-value set on 2026-08-19 (`esc-6349-1`). Changing this list is a change to that detector's contract.

**Marker form.** The terminal token is matched **case-insensitively** against the first token immediately following the `Status` label, within the first ~10 lines of the file — case-insensitivity is required, not cosmetic: `docs/prds/auto-type-param-resolution.md` (`Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md …`, produced by #4438, all-`done` leaves) is genuinely terminal but Title Case, so a case-*sensitive* match would misclassify a correctly-stamped PRD as non-terminal and a conformant detector would fire a false finding at a human escalation. Case-insensitivity does not admit the `docs/prds/kinematic-constraints.md` trap (`Status: deferred — superseded by …`, produced by #3847): its first token is `deferred`, not `superseded`, so it is correctly rejected under any casing. ALL CAPS (`SHIPPED`/`SUPERSEDED`/`WITHDRAWN`) remains the **preferred** authoring form; quoted by name, not line: the header of `docs/prds/v0_6/data-carrying-enums.md` (`**Status:** **SHIPPED (v0.6)** — …`) and the header of `docs/prds/kernel-seam-contracts.md` (`**Status: SHIPPED.** All 16 decomposition leaves have landed — …`). Both are accepted, so the two ratified exemplars stay conformant rather than becoming retroactively wrong. `docs/prds/v0_6/generic-data-carrying-enums.md`, `docs/prds/v0_6/result-and-fallback.md`, and — case-insensitively — `docs/prds/auto-type-param-resolution.md` already carry a conformant marker, so **none is a migration candidate, and a detector must not fire on any of them.** One known partial remains: `docs/prds/v0_6/process-dfm-geometry-metrology.md` carries a conformant `SUPERSEDED` *token* but defers its successors to a `## Split` section instead of naming them on the Status line, so it satisfies this **Marker form** rule while missing the same-line successor that the vocabulary above requires. Treat it as a one-line touch-up, not a re-stamp.

**Freeze header — the ratified shape; do not invent a new one.** Three parts, per the headers of `docs/prds/v0_6/data-carrying-enums.md` and `docs/prds/kernel-seam-contracts.md` as landed in `edd9703fae`:
1. The terminal token plus the **landed leaf task IDs** (e.g. `α #5102 … ξ #5116`).
2. The sentence "The body below is the AS-AUTHORED design record ... they are not current statements of fact."
3. An explicit **LIVE vs AS-AUTHORED map** naming which sections remain maintained. Load-bearing criterion: a section that production code *defers to* (rustdoc citing it as the authority) is LIVE, because a false claim there propagates into the code.

Apply the same as-authored header to the PRD's `.capability-manifest.md`.

**Section-level variant**, for a superseded block inside an otherwise-live PRD: `docs/prds/v0_3/compute-node-contract.md` Phase 9 — "retained for historical record; do not implement it as written", naming the successor task IDs.

**Cite task IDs, never task status.** Decomposition leaf rows carry `#NNNN` and say nothing about `done` / `deferred` / `pending`. The ID is immutable and queryable; a status word written into prose rots the moment the task moves. Worked defect: the pre-fix `kernel-seam-contracts.md` read "Adopt existing task #4876 (`deferred`, high)" while #4876 was actually `done` — the defect #6355 exists to detect.

**Decompose-close obligations** (normative; applies at decompose-mode Step 5.5–6, before hand-back). The session **must** do both:
1. **Backfill the real task IDs** into every decomposition-plan leaf row of the PRD, replacing Greek-label-only rows, and commit that edit beside the stamped sidecar using the same `git commit --only` vehicle as Step 5.5. This closes the defect named in `esc-6232-5`: `kernel-seam-contracts.md`'s decomposition-section header read "task IDs assigned at decompose time" while containing none, which made leaf state mechanically unresolvable from the document for six weeks — the single structural cause of the recurrence, and what makes leaf state machine-checkable at all.
2. **File a PRD-close leaf** in the same batch, wired by real `add_dependency` edges to depend on every other leaf. Its deliverable is the terminal stamp itself: set the Status marker to the terminal token, name the landed leaf IDs, add the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and apply the matching header to the `.capability-manifest.md`. Its user-observable signal is the committed header. **Cancelled-dependency disposition:** a `cancelled` sibling leaf counts as satisfied for the close leaf's dependency edge — both SHIPPED (which tolerates cancelled leaves alongside landed ones) and WITHDRAWN (where every leaf is cancelled) require the close leaf to stay dispatchable against a cancelled dependency; if the scheduler nonetheless treats the edge as unmet, the decompose steward removes it by hand and applies the stamp directly in a docs-only commit rather than leaving the close leaf permanently blocked. In-corpus precedent for this leaf's **dependency/close shape only** — a final leaf depending on every sibling, filed at decompose time: #4438 (`auto-type-param completion θ`) and #3847 (`KCC-θ`). Both predate this rule and their resulting headers are **non-conformant** with it (#4438 produced a Title-Case `Status: Superseded by …` header; #3847 left `docs/prds/kinematic-constraints.md` at `Status: deferred — superseded by …` and never flipped the marker) — copy the **data-carrying-enums** / **kernel-seam-contracts** header shape above for the deliverable text, not either precedent's own output.

Without (2), nothing ever stamps a terminal status — which is why only 5 of ~350 PRDs ever had one, every one of them stamped by a retroactive hand-fix rather than by the decomposition that produced it.

**Do not retro-migrate the corpus.** Per `esc-6232-5`'s ruling, the ~35 same-profile docs need per-doc judgement (still-active PRD vs completed plan vs capability manifest vs dated snapshot that must not be retroactively edited) and are adjudicated in the #6346 → #6347 sitting, not by a sweep from this rule.

## Capability Manifest — reify evidence forms

Mechanizes `gates.md` → *Capability Manifest — mechanizing G3 + G6 per leaf* for reify. **Manifest path:** `docs/prds/<vM_N>/<slug>.capability-manifest.md` (commit beside the PRD).

- **Empty-value sentinel (field-population check).** Reify's failure sentinel is `Value::Undef` (also `None` option-defaults and trivial constructor placeholders like the `{ ElasticResult() }` contract body). A result-field capability PASSES only if grep shows the **producer** writes a real `Value::Field{source: Sampled, …}` / non-`Undef` value on the production path (`crates/reify-eval/src/compute_targets/*.rs`, `crates/reify-eval/src/modal_ops.rs`). It FAILS (`declared-only`) if the only sampleable construction lives in a `tests/` module or a `significance_filter.rs` unit-test helper.
- **Wired-on-main evidence (anti-orphan).** Production entry paths to grep: the reify-eval dispatch tables + `engine_eval.rs` / `engine_build.rs` walks, the `@optimized`/ComputeNode registry (`compute_targets/mod.rs`), and the GUI `gui/src-tauri/src/engine.rs` `MeshData.scalar_channels`/`displaced_positions` path. A symbol present only under `tests/`, or declared but absent from the dispatch table, FAILS (`test-only`/`declared-only`) — precedents C-10 `selector_vocabulary_v2` (22+ fns, none in the eval dispatch table) and C-02 ComputeNode (producer built, consumer pending months).
- **Grammar-fixture (anti-mismatch).** Reuse the G3 grammar gate (`references/grammar-gate.md`): each novel syntax fragment is a committed `.ri` fixture that `tree-sitter parse --quiet` accepts with 0 ERROR nodes, OR names an upstream grammar-producer task (e.g. DCE `3936`). Cite the fixture path as manifest evidence.
- **Numeric floor.** The G6 domain hazards (P1-tet bending lock, Dirichlet `k≈0.67–0.70`, spline end-conditions, Duhamel `O((ΩΔt)²)`, eigensolver conditioning) are the floors; assert `bound > floor`.

**Worked precedent corpus** (the manifest's cautionary set — 2026-05-30 premise-review, report at `.orchestrator-scratch/v0_6-premise-review-report-2026-05-30.md`). Each is a binding the manifest would have FAILED *before* dispatch:
- `field-population`: esc-2962-33 (`ElasticResult.{stress,displacement}` = `Undef`), §3-C / task 3823 (`ModalResult.shape` Φ = `Undef`), task 3015 (superposition `linear_combine` over `Undef` fields).
- `producer-absent` / wrong-layer: esc-3005-32 (cache-reuse capability lives in reify-eval, not the task's reify-expr/reify-stdlib scope), esc-2929-40 (per-Support source-span provenance absent from value model + ComputeFn signature).
- `declared-only` / `test-only`: esc-3845-77 (bind/couple/prismatic are bare `eval_builtin`s, no compiler signature), esc-3607-59 (no on-disk geometry persistence; RealizationCache is in-memory per-Engine).
- grammar / substrate: esc-2998-47 (ConvergenceStatus payload enum — resolved by **gating on the DCE cluster `3946`**, which adds named-field payload variants, rather than a C-style re-spec), the C-06 grammar-fiction precedents.
- `bound≤floor`: esc-3821-44 (Duhamel `1e-9` ≪ `O((ΩΔt)²)≈2e-3` floor), esc-3453 buckling (`5%` < `9–10%` bending lock → 4066).

**Scoping an `expect: absent` check — bind it to the construct, not the bare token.**
1. Scope an `absent` pattern to the CONSTRUCT that would carry the defect, never to the bare token. A bare-token grep cannot distinguish code from a comment, and it also forbids legitimate unrelated uses of the token elsewhere in the same file. The collision is likely, not exotic: the clearest way to document "we stopped depending on X" is to write X's name, so the fix and the check race each other.
2. When you narrow, name the accepted gap inline in the manifest (a comment beside the descriptor) and say which behavioural test covers it. A narrowed pattern buys comment-immunity at the cost of missing indirection (e.g. aliasing the flag before the gate).
3. Prefer asserting the POSITIVE delivered construct where one exists; `absent` is the weaker form — it can only say a shape is missing, never that the replacement is present and correct.

Descriptor grammar (`kind`/`pattern`/`expect`/`paths`; pattern-anchored, never file:line) is normative in the shared skill — `~/.claude/skills/prd/references/decompose-mode.md` Step 2.5 — and is not restated here.

Worked case — **esc-6739-1** (2026-08-27, capability `mount-no-longer-gated-on-active`, task 6725, `docs/prds/v0_6/solver-legibility-telemetry.capability-manifest.yaml`):
- Before: `pattern: autoResolve\.active`, `expect: absent`, `paths: [gui/src/App.tsx]` matched the implementer's own explanatory comment at `gui/src/App.tsx:786` (2026-08-27) and fired a critical DEP_CAPABILITY_NOT_DELIVERED escalation that blocked dependent task 6739 — even though the capability WAS delivered, data-gated at `gui/src/App.tsx:794` (2026-08-27).
- After: `pattern: when=\{[^}]*autoResolve\.active` — still matches the historical `<Show when={engineStore.state.autoResolve.active}>` gate, and does not match the prose comment.
- Accepted gap, recorded inline in the live manifest: misses a regression reintroduced via indirection (e.g. aliasing the flag before the `Show`); `gui/src/__tests__/App.test.tsx:6515` (2026-08-27, "AutoResolvePanel mounts on DATA and stays readable after the loop completes") is the behavioural cover.
- Positive alternative that existed here (rule 3): `pattern: autoResolve\.iterations\.length > 0`, `expect: present` — present at `gui/src/App.tsx:794` (2026-08-27).

**Correcting a landed check touches TWO surfaces plus TWO twins.** The dispatch gate evaluates the DEPENDENCY TASK's `metadata.delivered_checks`, not the manifest file on disk — the evaluator is `Scheduler._compute_delivered_check_cache` in dark-factory `orchestrator/src/orchestrator/scheduler.py` (cite by symbol only; the file is ~7k lines and moves constantly). Editing the `.yaml` (or the `.md`) alone therefore does not change the running gate — the stamped task metadata must also be updated, via `update_task` on the producer task. Keep the `.yaml` and `.md` manifest twins in sync with each other too: `commit_planning` stamps them together at landing (see "Landing PRD artifacts" above), but a later correction has no such mechanism and must update both by hand. A correction is therefore three writes: `.yaml` descriptor, `.md` twin row, producer task `metadata.delivered_checks`.

Worked evidence: in esc-6739-1 the docs commit `021591e211` (2026-08-27) narrowed only the `.yaml`, leaving both the task metadata and the `.md` twin stale; without the metadata write the escalation would have re-fired within roughly 3 scheduler ticks. All three surfaces are consistent as of 2026-08-27 — `59097d9bc0` synced the `.md` twin, and task 6725's metadata now carries the narrowed pattern. Mechanism note, so nobody waits or asks for a restart: a corrected descriptor is picked up on the very NEXT scheduler tick — no restart, no waiting for main to advance. The delivered-check cache key folds in a `descriptor_digest` (`_delivered_checks_descriptor_digest`, same file), so a changed descriptor is a cache MISS by design (the esc-2911-1/2 self-heal).

## Author-mode Stage 2 — Reify mechanism patterns to surface

- **GR-001 family.** If the PRD assumes struct-ctor runtime evaluation (`Material(...)`, `LoadCase(...)`), confirm it gates on `gap-register.md` GR-001 (resolution: `docs/prds/v0_3/structure-instance-runtime.md` once authored).
- **ComputeNode dispatch.** Mechanisms routing through `@optimized` or `Engine::insert_compute_node` consume `compute-node-contract.md` §4 / §5 (shipped; PRDs after 2026-05-12 can rely on it).
- **`Field<X,Y>` in param position.** Tracked by task #3117 — does not parse in param context as of 2026-05-12. PRDs assuming it work should reference the task as a prerequisite.

## Exemplars

- `docs/prds/v0_3/compute-node-contract.md` — **gold standard, B+H full shape**: §0 supersession + cross-PRD ref, §1 GR-001 link, §2–§6 contract sections (CancellationHandle, Dispatch registry, OpaqueState transfer, Consumer policy), §7 boundary-test sketch facing both ways, §8 vertical-slice DAG with per-leaf observable signals, §9 open (tactical) questions. New PRDs match it conceptually, not by literal numbering.
- `docs/prds/v0_3/structural-analysis-fea.md` — **bare B, large decomposition**.
- `docs/prds/v0_3/mesh-morphing.md` — **bare B, smaller; strong "Relationship to other PRDs"** (G4 exemplar).

## Anti-triggers (Reify-specific)

- Authoring `.ri` design files (parametric parts/assemblies) → `/reify-design`, not `/prd`.

## Memory namespace

`project_id="reify"`. Relevant slugs:
- `preferences_implementation_chain_portfolio` — the 8-approach portfolio.
- `preferences_implementation_chain_naming` — terminology.
- `feedback_task_chain_user_observable` — G2 source.
- `feedback_prd_grammar_gate` — G3 source.
- `feedback_orchestrator_narrow_locks_favor_upfront_design` — why G5 tilts toward H.
- `feedback_commit_prds_before_referencing_tasks` — author commits before decompose references.
- topic `docs-prd-landing` (canonical, 2026-07-25) — the "Landing PRD artifacts" section above. It **supersedes five contradictory predecessors** (2026-06-02 / 06-24 / 07-01 / 07-06 / 07-11), all deleted. If you recall a "land docs via the merge queue", "`scripts/land.sh`", or "`git merge --ff-only`" landing procedure, it is deleted and wrong — follow the section above.
- `feedback_planning_mode_scope` — why decompose uses planning_mode=True.
- `procedural_fused_memory_two_phase_writes` — submit_task + resolve_ticket (planning_mode=False only).
- `preferences_bookmark_task_pattern` — bookmark/deferred-batch lifecycle.
- `preferences_cross_prd_deps_real_edges` — all deps are real `add_dependency` edges.
- `procedural_set_task_status_semantics` — comma-separated bulk IDs.
- `feedback_blocked_vs_pending_semantics` — scheduler handles unmet-deps tasks.
- `feedback_trickle_ticket_submissions` — don't switch off planning_mode to paper over a closed gate.
- `project_phantom_done_metadata_files_strip_may09` — the "metadata.files missing" decompose edge case.
- topic `docs-prd-terminal-status` (canonical, 2026-08-19) — the "Code anchors in PRD prose" and "PRD terminal status" sections above (prevention: cite-by-symbol-or-dated-SHA; closed terminal vocabulary + mandatory decompose-close leaf). Detection is owned elsewhere, not by this overlay: `reify-audit --pattern PPRDSTATUS` (#6346; detection status: see **Enforcement posture** in the "Code anchors in PRD prose" section above).
