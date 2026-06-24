# Capability manifest — objective-scope-inheritance (F-inherit)

Mechanizes G3 + G6 per leaf for `objective-scope-inheritance.md`. Built at decompose time (2026-06-24) by direct source inspection on main (commit `07b42d2fbc`); re-runnable via `scripts/prd-decompose-verify.mjs` at dispatch. **All bindings PASS — no FAIL, batch unblocked.**

Tasks: α=4821, β=4822 (intermediates) · γ=4824, δ=4825, ε=4823, ζ=4826 (leaves). DAG: α→γ; β→γ; {α,γ}→δ; ε(root); {γ,δ,ε}→ζ.

Evidence legend: `grep:file:line wired` = present on the production path on main · `producer:task-N upstream` = delivered by a transitive **upstream** dependency · `grammar-fixture` = parses with 0 ERROR nodes · `rejection-check` = authored X + observed the `W_*` fire (branch-4 negative-assertion mandate; bound at impl, substrate-to-detect verified here).

---

## γ (4824) — §10.5 inheritance end-to-end + provenance (integration gate, LEAF)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `build_solver_problem` (objective-attach site) | capability→producer (anti-orphan) | `grep:crates/reify-eval/src/engine_eval.rs:1026` wired (resolution loop calls it) | **PASS** |
| `scope_qualifies_for_centrality` (gate to suppress) | capability→producer | `grep:crates/reify-eval/src/engine_eval.rs:1126` wired (`engine_eval.rs:2894` fork) | **PASS** |
| `ObjectiveProvenance{scope,synthetic_centrality}` (record to extend) | capability→producer | `grep:crates/reify-ir/src/constraint.rs:169` wired (populated `engine_eval.rs:3040`) | **PASS** |
| `reify explain` per-cell provenance printer | capability→producer | `grep:crates/reify-cli/src/main.rs:156` + `:1581` wired (landed, constraint-solver-completion ι) | **PASS** |
| `nearest_container_objective` (the inheritance lookup) | capability→producer + DAG-direction | `producer:task-4821(α) upstream` (γ depends_on α) | **PASS** |
| read-DAG dependency ordering | capability→producer + DAG-direction | `producer:task-4822(β) upstream` (γ depends_on β) | **PASS** |
| `ObjectiveProvenance.inherited_from` (new field) | self-produced | added by THIS task (γ) | **PASS** |
| §10.5 example syntax (`minimize total_cost`, objective-less `sub housing : Housing {}`) | grammar-fixture | `tree-sitter parse --quiet` exit 0 (verified 2026-06-24) | **PASS** |
| BT1/BT2 back-compat byte-identity | numeric-floor (identity, branch-3) | achievable **by construction** — stable topo sort with source-index tie-break preserves order for order-independent scopes; identity, NOT a guessed bound | **PASS** |
| inheritance governance (end-to-end, branch-3) | DAG-direction (anti-inversion) | every required capability delivered by γ itself or **upstream** α/β; observable is **provenance** (`reify explain` line), NOT a numeric optimum the dependency set can't produce — the aggregate-objective optimum is owned by **M-WHOLE #4785, which depends on this PRD** (honesty boundary §3.2; avoids the esc-3436-210 inverted-dependency trap) | **PASS** |

## δ (4825) — W_OBJECTIVE_INHERIT_AMBIGUOUS (LEAF)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Ambiguity detection (`Ambiguous` outcome) | capability→producer + DAG-direction | `producer:task-4821(α) upstream` (δ depends_on α) | **PASS** |
| `W_OBJECTIVE_INHERIT_AMBIGUOUS` (new code) | self-produced | added by THIS task (reify-core) | **PASS** |
| signal asserts the diagnostic **fires** | rejection-mechanism (anti-silent-accept, branch-4) | `rejection-check:multi-container-reuse` — authored fixture (one structure as a sub under two distinct-objective parents) + `reify check` **observes the code** (bound at impl; substrate-to-detect = α's `Ambiguous`, verified upstream) | **PASS** (observed at impl) |

## ε (4823) — W_SUBBODY_OBJECTIVE_IGNORED (LEAF, independent root)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `sub.body` minimize/maximize MemberDecl (the dropped objective) | capability→producer | `grep:crates/reify-ast/src/decl.rs:338` (`SubDecl.body : Option<Vec<MemberDecl>>`) wired; CST `(specialization_body (minimize_declaration …))` parses exit 0 | **PASS** |
| `MemberDecl::Sub` compiler arm (the silent-drop site) | capability→producer | `grep:crates/reify-compiler/src/entity.rs:2069` wired (reads only `spec_param_overrides` today — the gap) | **PASS** |
| `W_SUBBODY_OBJECTIVE_IGNORED` (new code) | self-produced | added by THIS task (reify-core) | **PASS** |
| signal asserts the diagnostic **fires** | rejection-mechanism (anti-silent-accept, branch-4) | `rejection-check:sub-body-minimize` — authored `sub x : T { minimize <expr> }` + `reify check` **observes the code** (replaces today's silent drop; substrate-to-detect = `sub.body` AST, verified) | **PASS** (observed at impl) |

## ζ (4826) — back-compat corpus + CI example (LEAF, terminal)

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `examples/objective_inheritance.ri` parses | grammar-fixture | §10.5 fragments parse exit 0 (verified 2026-06-24) | **PASS** |
| BT1/BT2/BT7 `reify eval` byte-identity to baseline | numeric-floor (identity, G6 branch-3) | achievable by construction (stable source-tie-broken sort + inheritance no-op for un-contained scopes); recorded baseline, NOT a guessed bound | **PASS** |
| δ/ε diagnostics fire in the corpus | capability→producer + DAG-direction | `producer:task-4825(δ),4823(ε) upstream` (ζ depends_on both) | **PASS** |
| ordering + inheritance behaviors (BT3/BT5) | capability→producer + DAG-direction | `producer:task-4824(γ) upstream` (ζ depends_on γ) | **PASS** |

---

## Field-population / numeric-floor — N/A

No leaf samples a result field (`result.stress` / `mode.shape`) — F-inherit operates on objective *governance* and resolution *order*, not result-value population, so the empty-value-sentinel (`Value::Undef`) check does not apply. No leaf asserts an absolute numerical-accuracy bound — the only numeric postconditions are **identity** (back-compat byte-equality), achievable by construction, so the numeric-floor check is vacuous (no `bound ≤ floor` risk).
