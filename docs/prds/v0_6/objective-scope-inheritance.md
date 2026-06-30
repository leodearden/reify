# Objective-scope inheritance + enforced dependency-ordered scope resolution (F-inherit)

**Milestone:** v0_6 · **Status:** active (authored in interactive `/prd` session, 2026-06-24, under G1–G6+META) · **Approach:** B + H
**Cluster:** `cost-optimisation` (foundation). **Subject:** spec **§10.5 objective inheritance / narrowest-scope-wins** + **§10.6 enforced dependency-ordered (true leaf-first) scope resolution**.

This is the **semantic + ordering substrate** prerequisite for any whole-model / cross-scope objective. It is deliberately *not* the merged cross-scope solve — that is the downstream consumer **M-WHOLE** (`whole-model-objective-coupling.md`, task #4785), which owns the merged `ResolutionProblem` builder and the optimiser back-end.

---

## §0 — Purpose and scope

Spec §10.5 says "optimization objectives are scoped to the containing entity; **narrowest scope wins**," and §10.6 prescribes **bottom-up resolution**: resolve leaf scopes first, freeze them, then resolve parents. **Neither is implemented today.** Two concrete gaps (verified in-source 2026-06-24):

1. **Resolution is source-order, not dependency-order.** The resolution loop iterates `for template in &module.templates` in stored/source order (`crates/reify-eval/src/engine_eval.rs:2906`); `crates/reify-compiler/src/scc.rs` only *tags* `is_recursive`, it never reorders templates. The spec's "leaf-first" contract is **aspirational** — it happens to hold only when source order already matches data-flow order.
2. **Objectives are strictly scope-local; inheritance does not exist.** `build_solver_problem` (`engine_eval.rs:1026`) attaches only `template.objective`; an objective-less scope falls straight through to the synthesized centrality default (`scope_qualifies_for_centrality`, `engine_eval.rs:1126`) or first-feasible. A contained scope **never** inherits its container's objective.

F-inherit closes exactly these two gaps **at structure-def-template scope** (the existing resolution unit). The numeric joint-optimization payoff (a parent objective spanning child auto params, `minimize cost(self.descendants)`) is **out of scope** — it needs the merged solve owned by M-WHOLE.

### §0.1 — What this is NOT (scope boundaries)

- **NOT a per-occurrence / merged cross-scope solve.** F-inherit keeps the existing **per-template** solve; it changes the *order* templates resolve in and *which objective governs* an objective-less template — it never splits a template's solve per occurrence, never merges two scopes' auto cells into one `ResolutionProblem`. The merged/clustered solve (the "pre-solve clustering pass," the cross-scope `ResolutionProblem` builder) is **M-WHOLE task β/γ**. (Q1 resolution, 2026-06-24.)
- **NOT per-occurrence (inline sub-body) objectives.** `sub bracket : Bracket { minimize mass }` attaches an objective to a *single occurrence*. Today it is **silently dropped** (the compiler `MemberDecl::Sub` arm reads only `spec_param_overrides`; body `minimize`/`maximize` decls are never read — `entity.rs:2069`). F-inherit replaces the silent drop with a **loud diagnostic** and defers the per-occurrence objective model to M-WHOLE (Q2 resolution, 2026-06-24). §10.5 inheritance in F-inherit flows along **top-level structure-def objectives** through the containment DAG.
- **NOT coupling *resolution*.** A genuine read-cycle between scopes stays an approximation flagged by `W_SCOPE_COUPLING` (the existing sensor, shipped by `constraint-solver-completion.md` task λ / #4020). Fixed-point iteration across coupled scopes is M-WHOLE.

---

## §1 — Consumer and user-observable surface (G1)

**Primary consumer:** **`docs/prds/v0_6/whole-model-objective-coupling.md` (M-WHOLE, task #4785)** — the deferred milestone whose `minimize cost(self.descendants)` whole-assembly objective is *structurally impossible* until (a) scopes resolve in dependency order and (b) the §10.5 inheritance/narrowest-scope-wins semantic exists. M-WHOLE's stub explicitly names both as missing preconditions. F-inherit produces them; M-WHOLE consumes them.

**User-observable surfaces (this PRD's own leaves — G1 is satisfied here, not only by the downstream PRD):**

| Mechanism | Consumer (user-observable surface) |
|---|---|
| Dependency-ordered (read-DAG topo) scope resolution | `reify eval`: a scope reading a *later-declared* scope's auto cell now resolves against that cell's **solved** value (not a stale/default); CI fixture asserts the dependent cell's computed value. |
| §10.5 objective inheritance (objective-less contained scope → nearest container's objective) | `reify explain`: the child cell's provenance shows it is **governed by the inherited objective** from the named container (new `inherited_from`), where today it shows synthetic-centrality/none. |
| `W_OBJECTIVE_INHERIT_AMBIGUOUS` (multi-container reuse) | `reify check`: a structure reused under ≥2 differently-objective'd parents emits the code instead of silently picking one. |
| `W_SUBBODY_OBJECTIVE_IGNORED` (dropped sub-body objective) | `reify check`: `sub x : T { minimize … }` emits the code naming M-WHOLE, replacing today's silent drop. |
| CI inheritance example | `examples/objective_inheritance.ri` resolves under `reify eval` + `reify explain` in CI, exercising **inheritance governance (BT5)** end-to-end (back-compat byte-identity is the BT1/BT2/BT7 corpus). The surface-`.ri` BT3 *ordering* observable has no surface manifestation (builder-level only — β's `resolve_order.rs` corpus) and is M-WHOLE's (§11). |

**Engine-integration sub-check (G1).** Every mechanism plugs into the catalogued **§3.5 ConstraintSolver** seam (`engine-integration-norm.md`) — it extends the `reify-eval` resolution loop and the compiler's objective/containment lowering. **No new engine seam.** No orphan-producible `pub fn` in a `kernel-*` crate.

---

## §2 — Approach: B + H

G5 fires hard: this touches the **load-bearing resolution loop** (the core domain engine path, §3.5 ConstraintSolver seam), cross-crate blast radius ≥ 3 (`reify-eval` resolution + inheritance, `reify-ir` provenance, `reify-compiler` containment/sub-body diagnostic, `reify-core` diagnostic codes), and it has a named cross-PRD consumer (M-WHOLE). So **B + H**: a contract section (§6) pins the inheritance precedence, the resolution-order rule, and their invariants; a boundary-test sketch (§7) faces both the producer side (compiler containment + objective lowering) and the consumer side (eval resolution loop). The phase-2 integration-gate task (γ) names that sketch as its observable signal.

---

## §3 — Sketch of approach

### §3.1 — The containment relation (foundation)

No parent/containment map exists today; the only signals are the entity-name path prefix (`System.bracket.thickness`) and `TopologyTemplate.sub_components` (each `SubComponentDecl.structure_name` references a child structure def). F-inherit builds a **containment relation** over `module.templates`: a reverse index `child-template → containing template(s)` derived from every template's `sub_components`. From it, `nearest_container_objective(template, module)` returns:
- `Inherited(set, container_name)` when the template is contained by **exactly one** chain reaching an objective-bearing ancestor (walk up to the nearest container with a non-`None` `objective`);
- `Ambiguous` when the template is contained by ≥2 distinct containers carrying **different** objectives (→ §3.4 diagnostic, no inheritance);
- `None` when no container has an objective.

Containment cycles (`A` contains `B` contains `A`) are already pre-detected and tagged `is_recursive` by `scc.rs`; the walk treats a recursive containment edge as a terminating leaf (no infinite ascent).

### §3.2 — §10.5 objective inheritance (narrowest-scope-wins)

Insert inheritance into the objective-governance decision, **between** the scope's own objective and the synthesized centrality default. The precedence (spec §10.5 + §10.7):

> **own objective > inherited nearest-container objective > centrality default > first-feasible**

Mechanism: in the resolution loop's objective-selection (the `template.objective` / `scope_qualifies_for_centrality` fork at `engine_eval.rs:2891`), when `template.objective == None`, consult `nearest_container_objective`. If it returns `Inherited(set, container)`, attach `set` to the child's `ResolutionProblem` (`build_solver_problem`) and **suppress** centrality synthesis for that scope. The solver evaluates the inherited objective's terms against the frozen `current_values` plus the child's varying auto params — the existing `eval_objective` machinery, unchanged. **No expression rewriting**; `ObjectiveSet` terms are `CompiledExpr`s over globally-scoped `ValueCellId`s.

**Honesty boundary (load-bearing).** Under the per-template bottom-up solve, an inherited objective only *moves* the child's resolved values when its terms reference the **child template's own auto cells**. An aggregate objective that reads only the parent's cells (e.g. a parent `total_cost` aggregate) is *degenerate* w.r.t. the child's params — the child gains the **governance** (provenance shows the inherited objective; centrality is suppressed) but no numeric drive. That is the correct, honest substrate: the §10.5 rule is in place; the *joint* optimization that makes aggregate objectives bite is the M-WHOLE merged solve. The degenerate cross-scope read is flagged by the existing `W_SCOPE_COUPLING`. F-inherit's inheritance **observable** is therefore the **provenance/governance** (`reify explain`), not an overclaimed optimum.

### §3.3 — Enforced dependency-ordered resolution (true leaf-first)

Replace the source-order resolution walk with a **topological order over the cross-scope read-DAG**: scope `B` depends on scope `A` (`A` resolves first) iff a constraint or objective in `B` reads an auto cell owned by `A`. This is exactly the read relation `detect_scope_coupling` already computes (`engine_eval.rs:562`, via `extract_dependency_trace` over each template's constraints/objective) — F-inherit **graduates that read-set sensor from a post-hoc detector into a pre-solve orderer**. (The sensor's *clustering*/merged-solve graduation is M-WHOLE; F-inherit only orders.)

- **Acyclic read-DAG:** stable topological sort, **ties broken by source order**. Every scope's cross-scope reads then resolve to a value produced earlier in the walk (true bottom-up). Scopes with no data-flow between them keep their relative source order (so back-compat is maximal — §6 INV-2).
- **Cyclic read-DAG (an SCC of ≥2 scopes):** irreducible coupling. Resolve SCC members in source order and **retain `W_SCOPE_COUPLING`** for the crossing cells (now meaning "irreducible cycle," the residual approximation M-WHOLE resolves).

Containment edges with **no** cross-scope read impose **no** ordering constraint — order only matters where data flows, and the read-DAG captures exactly that.

### §3.4 — Loud diagnostics (replacing two silent gaps)

- **`W_OBJECTIVE_INHERIT_AMBIGUOUS`** (new, `reify-core`): emitted when `nearest_container_objective` returns `Ambiguous` (a structure reused under ≥2 differently-objective'd containers). No inheritance is applied; the scope falls to its centrality/feasibility default. Names the containers + the structure.
- **`W_SUBBODY_OBJECTIVE_IGNORED`** (new, `reify-core`): emitted by the compiler `MemberDecl::Sub` arm when `sub.body` contains a `minimize`/`maximize` decl. Replaces today's silent drop; the message states per-occurrence objectives land with whole-model coupling (M-WHOLE). Honors the project "loud diagnostics over silent defaults" norm.

### §3.5 — Legibility surface (inheritance provenance)

Extend `ObjectiveProvenance` (`reify-ir/src/constraint.rs:169`) with `inherited_from: Option<String>` — `Some(container)` when the governing objective was inherited, `None` for own/centrality/feasibility. `reify explain` (`reify-cli/src/main.rs`, the per-cell provenance printer at ~main.rs:1581) prints "governed by objective inherited from `<container>`" for inherited cells, distinct from the existing synthetic-centrality line. This reuses the shipped `reify explain` / `ObjectiveProvenance` substrate from `constraint-solver-completion.md` (tasks θ/ι, landed).

---

## §4 — Resolved design decisions

1. **Scope unit = structure-def template** (Q1, 2026-06-24). Containment from `sub_components`; per-occurrence merged solve is M-WHOLE. The per-template solve is unchanged in *structure* — only its *order* and its *objective selection* change.
2. **Inheritance precedence: own > inherited-nearest-container > centrality default > first-feasible** (§10.5 + §10.7). Centrality synthesis is suppressed iff an inheritable container objective exists.
3. **Inheritance attaches the container's `ObjectiveSet` verbatim** to the child's `ResolutionProblem`; no expression rewriting; the existing `eval_objective` evaluates terms against frozen `current_values` + the child's varying auto params.
4. **Honest observable = governance/provenance, not optimum** (§3.2). The numeric joint-optimization of aggregate inherited objectives is M-WHOLE; degenerate cross-scope reads are flagged `W_SCOPE_COUPLING`.
5. **Resolution order = stable topological sort over the cross-scope read-DAG, ties broken by source order** (§3.3). Cycles → SCC source-order + retained `W_SCOPE_COUPLING`. Containment-only (no data-flow) edges impose no order.
6. **Two silent gaps become loud diagnostics** (Q2, 2026-06-24): `W_OBJECTIVE_INHERIT_AMBIGUOUS` (multi-container reuse) and `W_SUBBODY_OBJECTIVE_IGNORED` (dropped sub-body objective). Per-occurrence objective wiring is deferred to M-WHOLE.
7. **Back-compat is a hard invariant** (G6 branch-3, §6 INV-2): single-scope, read-uncoupled multi-scope, and already-correctly-ordered multi-scope models resolve to **byte-identical `reify eval` output**. Guarded by a recorded-baseline regression corpus (ζ).

**Breadcrumb (deferred alternatives, record at the implementation site with "at time of writing" framing):** the *occurrence-tree* scope model (spec-faithful, wires sub-body objectives, no reuse ambiguity) was considered and **rejected for F-inherit** because per-occurrence objective resolution overlaps M-WHOLE's merged-`ResolutionProblem`-builder (task β) — adopting it here would recreate the G4 reciprocal-ownership failure. It is the natural shape M-WHOLE adopts.

---

## §5 — Substrate gate (G3)

**Grammar gate: clean.** The §10.5 example fragments — `minimize total_cost` (structure-level), `sub bracket : Bracket { minimize mass }` (specialization-scope objective), and the objective-less `sub housing : Housing { }` — all parse (`tree-sitter parse --quiet` exit 0, verified 2026-06-24). F-inherit introduces **no novel syntax**; it is pure resolution semantics. Every task carries `grammar_confirmed: true`. No grammar prerequisite task is queued.

**Semantic/behavioral substrate (verified wired on main, 2026-06-24):**

| Assumed capability | Evidence (file:line) | State |
|---|---|---|
| Per-template resolution loop (the reorder site) | `reify-eval/src/engine_eval.rs:2906` | exists |
| `build_solver_problem` (objective-attach site) | `engine_eval.rs:1026` | exists |
| `scope_qualifies_for_centrality` (the gate inheritance composes with) | `engine_eval.rs:1126` | exists |
| `detect_scope_coupling` read-set extraction (the orderer's input) | `engine_eval.rs:562` (`extract_dependency_trace`) | exists |
| `W_SCOPE_COUPLING` (`DiagnosticCode::ScopeCoupling`) | shipped by `constraint-solver-completion.md` λ / #4020 | landed |
| `sub_components` / `SubComponentDecl.structure_name` (containment source) | `reify-compiler/src/types.rs` | exists |
| `SubDecl.body : Option<Vec<MemberDecl>>` (sub-body objective AST) | `reify-ast/src/decl.rs:338` | exists |
| `MemberDecl::Sub` compiler arm (the silent-drop site to diagnose) | `reify-compiler/src/entity.rs:2069` | exists |
| `ObjectiveProvenance { scope, synthetic_centrality }` (provenance to extend) | `reify-ir/src/constraint.rs:169` | landed |
| `reify explain` per-cell provenance printer | `reify-cli/src/main.rs:156`, `:1581` | landed |

**New capabilities F-inherit produces (no fiction — each is a deliverable, produced upstream within the batch):** the containment relation + `nearest_container_objective` (task α), the read-DAG topo orderer (task β), `ObjectiveProvenance.inherited_from` + inheritance attach (task γ), `W_OBJECTIVE_INHERIT_AMBIGUOUS` (δ), `W_SUBBODY_OBJECTIVE_IGNORED` (ε).

---

## §6 — Contract: the resolution-order + objective-governance seam (B + H)

The seam crosses three boundaries: the compiler (produces `sub_components` + per-scope objectives), the IR (`ObjectiveProvenance`), and the eval resolution loop (consumes both to order + govern). Pinned rules + invariants:

### §6.1 — Objective-governance precedence (the §10.5 rule)

For each template `T` resolved with an active solver, the governing objective is the **first** of:
1. `T.objective` (own) — narrowest scope, always wins.
2. `nearest_container_objective(T) == Inherited(set, _)` — the §10.5 inheritance.
3. `scope_qualifies_for_centrality(T)` — the synthesized Chebyshev-centre default (§10.7), **only** when (1) and (2) are absent.
4. first-feasible (no objective).

### §6.2 — Resolution-order rule (the §10.6 enforcement)

`resolve_order(module)` returns a permutation of `module.templates` that is a **stable topological sort** of the cross-scope read-DAG (edge `A → B` iff `B` reads an auto cell owned by `A`), ties broken by source index. SCCs of size ≥ 2 are emitted in source order internally.

### §6.3 — Invariants (all MUST hold; boundary-tested in §7)

- **INV-1 (bottom-up):** for an acyclic read-DAG, every scope's cross-scope auto-cell reads resolve to a value produced **earlier** in `resolve_order` (no scope reads an unresolved cross-scope auto cell).
- **INV-2 (back-compat identity, G6 branch-3):** for any model that is single-scope, OR has no cross-scope auto-cell reads, OR is already in a valid dependency order, `reify eval` resolved-value output is **byte-identical** to the source-order baseline. (Guaranteed by the *stable, source-tie-broken* sort.)
- **INV-3 (precedence):** §6.1 holds exactly; centrality synthesis fires **iff** neither an own nor an inheritable container objective exists.
- **INV-4 (inheritance provenance):** an inherited resolution records `ObjectiveProvenance.inherited_from = Some(container)`; own/centrality/feasibility record `None`.
- **INV-5 (no per-occurrence solve):** F-inherit never splits a template's solve per occurrence and never merges two templates' auto cells into one `ResolutionProblem`. The merged/clustered solve is M-WHOLE's.
- **INV-6 (loud, not silent):** a multi-container inheritance ambiguity and a dropped sub-body objective each emit a `W_*` diagnostic; neither is ever silent.
- **INV-7 (cycle safety):** an irreducible read-cycle does not deadlock or panic — SCC source-order fallback + retained `W_SCOPE_COUPLING`.

### §6.4 — Error/diagnostic semantics

- `W_OBJECTIVE_INHERIT_AMBIGUOUS` (`reify check`/eval): structure reachable as a sub of ≥2 containers with distinct objectives; names the containers + structure; no inheritance applied.
- `W_SUBBODY_OBJECTIVE_IGNORED` (`reify check`/compile): a `sub … { minimize/maximize … }` body objective; names M-WHOLE as the owner of per-occurrence objectives.
- `W_SCOPE_COUPLING` (existing, retained): now fires for **irreducible read-cycles** after dependency-ordering (acyclic coupling is resolved by the ordering, not warned).

---

## §7 — Boundary-test sketch (B + H — the integration-gate signal)

Scenarios facing both the compiler-producer side (containment + objective lowering) and the eval-consumer side (resolution loop). The phase-2 integration-gate task (γ) names this table as its observable signal; the regression corpus + CI example (ζ) commits it.

| # | Scenario | Preconditions | Postcondition (asserted) |
|---|---|---|---|
| BT1 | Single-scope back-compat | one structure def, auto + objective | `reify eval` resolved values **byte-identical** to recorded baseline (INV-2) |
| BT2 | Uncoupled 2-scope back-compat | two scopes, **no** cross-scope reads, declared in either order | both resolve byte-identical to source-order baseline regardless of declaration order (INV-1, INV-2) |
| BT3 | Acyclic coupling reorder | scope `B` reads scope `A`'s auto cell; source order is `B` then `A` | after ordering, `A` resolves first; `B`'s dependent cell computes from `A`'s **solved** value (not default); `reify eval` shows the corrected value (INV-1). **Observable at the *builder* level only** (β's `resolve_order.rs` / `scope_coupling.rs` corpus): a cross-sub *surface* read compiles to an instance-scoped id (`Parent.c.x`, not `Child.x`), so it never surfaces a solved cross-scope auto — the surface-`.ri` `reify eval` BT3 observable requires the merged cross-scope solve and is **M-WHOLE's** (§11). |
| BT4 | Irreducible cycle | `A` reads `B`, `B` reads `A` | no panic/deadlock; SCC source-order fallback; `W_SCOPE_COUPLING` names both scopes + crossing cell (INV-7) |
| BT5 | Inheritance governance | objective-less child template contained by a single `minimize`-bearing parent | `reify explain` shows the child's auto cell governed by the inherited objective, `inherited_from = parent`; centrality **not** synthesized (INV-3, INV-4) |
| BT6 | Narrowest-scope-wins | child template with its **own** objective, under an objective-bearing parent | child keeps its own objective; `inherited_from = None` (INV-3) |
| BT7 | No inheritable ancestor (back-compat) | objective-less top-level scope, continuous + auto + inequality | centrality default still fires; `synthetic_centrality = true`, `inherited_from = None` (INV-2, INV-3) |
| BT8 | Multi-container ambiguity | structure reused under two parents with **distinct** objectives | `W_OBJECTIVE_INHERIT_AMBIGUOUS`; no inheritance; falls to centrality/feasibility (INV-6) |
| BT9 | Sub-body objective diagnosed | `sub x : T { minimize … }` | `W_SUBBODY_OBJECTIVE_IGNORED` naming M-WHOLE; **not** silent (INV-6) |

---

## §8 — Decomposition plan

Greek labels; actual IDs assigned at decompose time. Phase order = dependency order.

**Phase 1 — foundation (the two substrate halves).**
- **α — Scope-containment relation + `nearest_container_objective`.** Build the `child-template → container(s)` reverse index from `sub_components`; implement the nearest-objective-bearing-container walk with `Inherited | Ambiguous | None` outcomes; recursive-containment-safe. Modules: `reify-eval` (resolution-loop home) — `[]` (architect confirms whether a compiler-side helper is cleaner). *Intermediate* — unlocks γ (inheritance) and δ (ambiguity diagnostic). Signal: unit test on a 3-level nested fixture asserts the resolved container chain + the ambiguity outcome (surfaced E2E through the γ gate). `grammar_confirmed: true`.
- **β — Dependency-ordered scope resolution (read-DAG topo) replacing source-order.** Graduate `detect_scope_coupling`'s read-set extraction into a pre-solve **stable** topological sort (ties → source order); SCC fallback + retained `W_SCOPE_COUPLING`; rewire the resolution loop (`engine_eval.rs:2906`) to walk `resolve_order(module)`. Modules: `reify-eval` (`engine_eval.rs`). *Intermediate* — unlocks γ. Signal: unlocks γ; BT2/BT4 behaviors surfaced via γ + ζ; **BT3 is builder-level** (`resolve_order.rs` corpus) — no surface-`.ri` manifestation (§7 BT3 note, §11). `grammar_confirmed: true`.

**Phase 2 — integration gate (vertical slice, leaf, B+H boundary signal).**
- **γ — §10.5 objective inheritance, end-to-end + provenance.** Wire α's `nearest_container_objective` into the objective-selection fork (`engine_eval.rs:2891` / `build_solver_problem`) per §6.1; suppress centrality when inheriting; add `ObjectiveProvenance.inherited_from` and surface it in `reify explain`; compose with β's ordering. **Leaf — names the §7 boundary-test sketch as its signal (BT1, BT2, BT5, BT6, BT7).** Signal: `reify explain` on an objective-less-child-under-`minimize`-parent fixture shows the child cell governed by the inherited objective with `inherited_from = parent` (vs. centrality today); AND single-scope (BT1) + uncoupled-2-scope (BT2) fixtures' `reify eval` output is byte-identical to a recorded baseline (G6 branch-3). Modules: `reify-eval`, `reify-ir` (provenance), `reify-cli` (explain line). Depends on α, β. `grammar_confirmed: true`.

**Phase 3 — loud diagnostics.**
- **δ — `W_OBJECTIVE_INHERIT_AMBIGUOUS` (multi-container reuse).** Emit per §3.4/§6.4 when α reports `Ambiguous`. Modules: `reify-eval`, `reify-core` (code). *Leaf.* Signal: `reify check` on a structure reused under two distinct-objective parents exits with / prints `W_OBJECTIVE_INHERIT_AMBIGUOUS` naming both containers (BT8). Depends on α, γ. `grammar_confirmed: true`.
- **ε — `W_SUBBODY_OBJECTIVE_IGNORED` (dropped sub-body objective).** In the compiler `MemberDecl::Sub` arm (`entity.rs:2069`), detect a `minimize`/`maximize` in `sub.body` and emit the diagnostic, replacing the silent drop. Modules: `reify-compiler` (`entity.rs`), `reify-core` (code). *Leaf.* Signal: `reify check` on `sub x : T { minimize … }` prints `W_SUBBODY_OBJECTIVE_IGNORED` naming M-WHOLE (BT9) — previously nothing. Independent root (compiler-side). `grammar_confirmed: true`.

**Phase 4 — back-compat proof + CI example (terminal).**
- **ζ — Back-compat regression corpus + CI `.ri` inheritance example.** Commit (1) a recorded-baseline regression corpus proving INV-2 byte-identity on the back-compat set (BT1, BT2, BT7), (2) `examples/objective_inheritance.ri` exercising **inheritance governance (BT5)** under `reify eval` + `reify explain` in CI — the surface-`.ri` BT3 *ordering* observable has no surface manifestation (builder-level only, β's `resolve_order.rs` corpus) and is deferred to M-WHOLE (§11; BT3 stays covered by β's corpus), and (3) diagnostic fixtures asserting δ/ε fire (BT8, BT9). Modules: `reify-eval` tests, `examples/`, `tests/prd-gate/fixtures` — `[]`. **Leaf, terminal — what M-WHOLE depends on.** Signal: the corpus + example run green in CI; the back-compat corpus asserts byte-identity (G6 branch-3) and the example resolves the inheritance-governance behavior (BT5). Depends on γ, δ, ε. `grammar_confirmed: true`.

**DAG:** α → γ; β → γ; {α, γ} → δ; ε (root); {γ, δ, ε} → ζ.

**No companion-correction phase needed** — M-WHOLE already references this PRD as a precondition (its stub names §10.5 inheritance + dependency-ordered resolution as the missing semantic prerequisites); no other PRD's prose needs correcting by F-inherit's resolution.

---

## §9 — Pre-conditions for activating

- **Landed substrate (on main, no dep edge needed — verify against local main):** `constraint-solver-completion.md` tasks — centrality default η/#4013, `W_SCOPE_COUPLING` λ/#4020, `ObjectiveProvenance` θ, `reify explain` ι. F-inherit extends these in place.
- **No upstream task prerequisite** — F-inherit is a foundation; its tasks build on landed substrate only.
- **Downstream back-edge (wired at decompose):** M-WHOLE (#4785) gains a dependency on F-inherit's terminal task ζ.

---

## §10 — Cross-PRD relationship (G4)

This PRD **owns** the scope-resolution order + the §10.5 objective-inheritance/governance semantic. The seam-owner table:

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `whole-model-objective-coupling.md` (M-WHOLE, #4785) | produces-for | dependency-ordered resolution + §10.5 inheritance/narrowest-scope-wins; M-WHOLE's merged solve + clustering pass + optimiser back-end consume this substrate | **F-inherit** (this PRD owns inheritance + ordering); M-WHOLE owns the merged `ResolutionProblem` builder + per-occurrence model | F-inherit produces; back-edge wired at decompose |
| `constraint-solver-completion.md` (landed) | extends | `ObjectiveProvenance`, `reify explain`, centrality gate, `W_SCOPE_COUPLING`, `detect_scope_coupling` | F-inherit extends in place (no reciprocal ownership) | landed substrate |
| `continuous-cost-minimisation.md` (sibling, in flight) | adjacent | Money-objective robustness-floor default applies to whatever objective governs a scope — orthogonal to *which* objective inheritance selects | disjoint — no shared mechanism | independent |
| `engine-integration-norm.md §3.5 ConstraintSolver` | extends | resolution loop / objective selection | F-inherit | extension owned here |

**No reciprocal-ownership ambiguity.** The single genuine ownership line is F-inherit ↔ M-WHOLE: **F-inherit = ordering + inheritance semantic (per-template scope); M-WHOLE = merged cross-scope solve + clustering + per-occurrence objectives + optimiser back-end.** The §4 breadcrumb records why the per-occurrence model stays on M-WHOLE's side of that line.

---

## §11 — Out of scope

- **Merged cross-scope solve / pre-solve clustering pass / cross-scope `ResolutionProblem` builder** — M-WHOLE (its tasks α/β/γ).
- **Surface-`.ri` BT3 observable** — a scope reading another scope's *solved* `auto` cell with `reify eval` displaying the dependent cell as a non-undef cross-scope-derived value. Requires the merged cross-scope solve above, so it is **M-WHOLE's** (its ε CI-`.ri` leaf). F-inherit covers BT3 at the **builder level** (β's `resolve_order.rs` / `scope_coupling.rs` corpus); cross-sub surface reads compile to instance-scoped ids and never surface a solved auto, so ζ (#4826)'s example exercises **BT5 governance** instead. *(Rescope: /unblock esc-4826-13, 2026-06-30.)*
- **Per-occurrence (inline sub-body) objectives** — diagnosed loudly here (ε), wired by M-WHOLE's occurrence model. F-inherit's inheritance flows along top-level structure-def objectives via the containment DAG.
- **Numeric joint-optimization of aggregate inherited objectives** — degenerate under the per-template bottom-up solve (flagged `W_SCOPE_COUPLING`); the real optimization is M-WHOLE's merged solve.
- **Coupling *resolution* (fixed-point iteration across coupled scopes)** — §10.6 mandates detection + ordering only; iterative resolution is M-WHOLE.
- **Optimiser back-end choice (Nelder-Mead vs global/MINLP)** — M-WHOLE's first expanded deliverable.

---

## §12 — G6 premise-validity notes (verified)

- **BT1/BT2/BT7 back-compat byte-identity (branch-3 / identity):** achievable by construction — the *stable, source-tie-broken* topological sort preserves source order for order-independent scopes, and inheritance is a no-op for scopes with no inheritable container. Identity, not a guessed numeric bound. **Pass.**
- **BT5 inheritance governance (branch-3, end-to-end):** every required capability — containment walk (α), objective attach (γ over `build_solver_problem`), `ObjectiveProvenance.inherited_from` (γ), `reify explain` (landed) — is delivered by this task or an **upstream** prerequisite (α, β). Nothing is owed by a downstream task. The signal is **provenance/governance** (a `reify explain` output line), *not* a numeric optimum — deliberately, because the per-template bottom-up solve cannot make an aggregate inherited objective bite (that capability lives in M-WHOLE, which *depends on* this PRD — the §3.2 honesty boundary; the esc-3436-210 "demanded output its dependency set couldn't produce" failure shape is explicitly avoided). **Pass.**
- **BT8/BT9 diagnostics (branch-4, negative assertion):** both are *rejection/diagnostic* assertions — at decompose the capability manifest binds each via an authored fixture that runs `reify check` and **observes the `W_*` code fire** (not "the test motivates the diagnostic"). The substrate to *detect* each condition exists (α's ambiguity outcome; `sub.body` AST). **Pass** (bound at decompose).
- **BT4 cycle safety (branch-3):** SCC detection (`scc.rs` Tarjan) + source-order fallback are existing/derivable; no new numerical capability. **Pass.**

---

## §13 — Open questions (tactical, deferred to impl)

1. **Crate home for the containment relation (α).** `reify-eval` at resolve time (no compiler data-model change) vs. a `reify-compiler` helper baked alongside `sub_components`. Suggested: `reify-eval` (keeps it next to the resolution loop + read-DAG). Decide during α.
2. **`nearest_container_objective` ascent over recursive containment.** Treat a recursive (`is_recursive`) containment edge as a terminating leaf. Confirm against a self-referential fixture. Decide during α.
3. **`W_*` granularity for ambiguity (δ).** One warning per ambiguous structure vs. per container pair. Suggested: per structure, listing the containers. Decide during δ.
4. **CI example shape (ζ).** A minimal 2-scope inherit + a 2-scope acyclic-reorder, vs. a single combined model. Suggested: two small fixtures (one per behavior) for clearer failure attribution. Decide during ζ.
