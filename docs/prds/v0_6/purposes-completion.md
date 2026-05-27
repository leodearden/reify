# Purposes Completion (multi-ref binding, let, guarded blocks, std.determinacy.purposes)

- **Milestone:** v0.6 (spec-gap-fill; ignore the §9.5/§11 version labels — these are unconditional spec promises)
- **Status:** deferred (orchestrator stopped; this PRD is authored + decomposed, batch held `deferred`)
- **Batch:** `spec-gap-2026-05-27`, cluster `purposes-completion`
- **Approach:** B + H (contracts + two-way boundary tests) — blast radius ≥ 4 crates, mechanism count ≥ 8, touches the determinacy-model activation seam.

## §0 — Purpose and scope

Spec §4.4 / §9.5 define **purposes**: named, parameterized, activatable declaration kinds that inject determinacy constraints + objectives over an entity's evaluation-graph state. Today the compiler accepts only the narrowest subset: a single-`StructureRef`-param purpose body containing `constraint` / `minimize` / `maximize` members. Three spec-promised body features are rejected at **compile time** with explicit diagnostics, and the spec-promised standard purposes have no stdlib home:

1. **Multi-`StructureRef` purpose params** (§4.4, §9.5) — `purpose fits_within(part : Structure, envelope : Structure)` is rejected (`traits.rs:286-301`). Per-param identity is lost: the compile-time ValueCellId stamp uses `scope.entity_name == purpose_name` for **every** member ref, so `part.length` and `envelope.length` would both stamp `ValueRef(fits_within, length)`. The original analysis was task-2201 (done — it filed the *reject* diagnostic, not the fix). This PRD designs the binding scheme.
2. **Purpose `let` bindings** (§4.4 grammar `purpose_member ::= … | let_decl`) — rejected (`traits.rs:333-350`, `DiagnosticCode::PurposeLetUnsupported`). `CompiledPurpose` has no storage for let exprs; `activate_purpose` only injects constraints.
3. **Purpose guarded blocks** (`where <cond> { … } [else { … }]`) — rejected (`traits.rs:351-361`). The `GuardedGroupDecl` AST already exists and `lower_guarded_block` already runs for entity bodies; purposes just refuse it.
4. **`std.determinacy.purposes` stdlib module** (§11) — the spec promises `design_review` and `simulation_ready` as standard purposes; **no stdlib `.ri` defines any purpose** (verified: zero `purpose` decls under `crates/reify-compiler/stdlib/`). This module *depends on* features 1–3 to express anything non-trivial (a guarded `simulation_ready` needs `where`; a useful `design_review` wants `let` for readable intermediate quantities).

**User-observable end state:** an engineer writes a 2-entity purpose, a purpose with a `let`, or a guarded purpose in a `.ri` file, then runs `reify check --purpose <name>=<entity>` and sees the purpose's constraints participate in the satisfied/violated/indeterminate report (exit code + text) — and can activate the stdlib `simulation_ready` purpose against their own structure without writing it themselves.

## §1 — Spec grounding

- **§4.4 Purpose Declarations** — `purpose_decl ::= 'pub'? 'purpose' IDENT type_params? '(' purpose_params ')' '{' purpose_member* '}'`; params are **entity references** (entity-kind selectors `Structure`/`Occurrence`/`Constraint`/`Field`), not values. Reflective members (`.params`, `.geometric_params`, `.material_params`, `.sub_entities`, `.ports`, `.constraints`) resolve per-param at elaboration time.
- **§9.4 Determinacy predicates** — `determined`/`constrained`/`undetermined`/`partially_determined` are compiler intrinsics (in prelude; verified resolved in `expr.rs:1119`). They compose with `forall`/`exists`/`and`/`or` and **participate in `where` guards** — the spec explicitly sanctions guards over determinacy state.
- **§9.5 Purposes** — "a named determinacy predicate"; activatable.
- **§11.1 / §11.3** — `std.determinacy.purposes` module promising `design_review`, `simulation_ready`.
- **Grammar (EBNF §appendix):** `purpose_member ::= constraint_line | sub_decl | let_decl | minimize_decl | maximize_decl`. `let_decl`, `minimize_decl`, `maximize_decl`, and guarded blocks are all in-grammar.

## §2 — Why deferred / activation status

Deferred only because the orchestrator is stopped for the `spec-gap-2026-05-27` authoring batch. No upstream substrate blocker:

- Grammar gate **passed** (G3, §7) — all four body shapes parse with exit 0 today.
- Determinacy predicate intrinsics exist.
- `Value::StructureInstance` / `StructureRegistry` (GR-001, tasks 3540/3542) **merged** — entity references resolve to real value cells.
- Reflective-aggregation expansion (`expand_purpose_reflective_placeholders`, tasks 2289/2544) **merged** — `.params` projection already works for the single-param case.

The one *non-substrate* pre-condition is that purpose **activation has no user-facing consumer today** (see §3). That gap is closed *inside this PRD* (task α), not deferred to another.

## §3 — Consumer (G1) — the load-bearing finding

**`Engine::activate_purpose(name, entity_ref)` has ZERO non-test consumers.** Verified: no call site in `crates/reify-cli/`, `gui/src-tauri/src/`, or any non-test path — only `crates/reify-eval/tests/*` exercise it. The full multi-ref/let/guard machinery would be a textbook **producer-orphan** (the C-02 / C-10 failure class) if shipped without a user surface. Therefore the named consumer is built **first**, in this batch:

**Primary consumer — `reify check --purpose <name>=<entity>[,<param>=<entity>…]` (CLI).** `cmd_check` (`reify-cli/src/main.rs:166`) already compiles → constructs `Engine` → calls `engine.check` → `report_eval_output` emits the constraint satisfied/violated/indeterminate report + exit code. The flag inserts an `activate_purpose` call *before* `check`; the purpose's injected constraints flow into the exact same report. This is a direct CLI-output + exit-code signal — the strongest signal class in the overlay vocabulary.

- Single-param: `--purpose simulation_ready=MyBracket`.
- Multi-param: `--purpose fits_within=part:MyPart,envelope:MyBox` (per-param `name:entity` pairs).

**Secondary consumer (declared, not owned here) — GUI auto-views.** `gui/src/stores/autoViewGenerator.ts` already generates one `auto:purpose:<name>` view per *active* purpose and `viewStateStore.ts` regenerates on purpose change — but **nothing populates `activePurposes` from the engine** (no Tauri command calls `activate_purpose`). Wiring a GUI activation command is **out of scope** (§10) and tracked as a follow-up; the CLI is the sufficient G1 consumer for this batch. Declared here so the orphan isn't reintroduced GUI-side.

**Consumer for the stdlib module:** the end user running `reify check --purpose simulation_ready=<their Structure>` — `simulation_ready` ships in `std.determinacy.purposes`, is in scope via the standard import path, and activates against a user structure with no per-structure opt-in (the universality property, §4.4).

## §4 — Contract: the multi-ref binding scheme (the core design)

The whole problem reduces to **per-param entity identity** surviving from compile-time stamp through activation-time remap into the eval graph. Today both halves assume one entity.

### 4.1 Compile-time stamp — `purpose::param` entity encoding

Today (`reify-compiler/src/expr.rs` ~1830-1900, the purpose-subject member-ref arm): a member ref `subject.mass` inside a purpose body stamps `ValueCellId { entity: scope.entity_name /* == purpose_name */, member: "mass" }`. For one param this is unambiguous; for N params it collides.

**Decision (binding scheme):** stamp the **per-param** entity as `format!("{purpose_name}::{param_name}")`. So in `purpose fits_within(part, envelope)`:
- `part.length` → `ValueCellId { entity: "fits_within::part", member: "length" }`
- `envelope.length` → `ValueCellId { entity: "fits_within::envelope", member: "length" }`

The `::` separator is chosen because it cannot appear in a user identifier (grammar `IDENT` excludes it) and does not collide with the existing `.`-based sub-scoping convention (`<entity>.<sub>`) nor the `purpose:<name>@<entity>` injected-constraint prefix used in `activate_purpose_constraints`. The single-param case stamps `"manufacturing_ready::subject"` — uniform, no special case. **Back-compat note:** the current single-param stamp is `purpose_name` (no `::param`); changing it to `purpose_name::param` is an internal encoding change with no on-disk or wire surface, so it is free to change provided the remap (4.2) and reflective-query plumbing (4.3) move in lockstep.

`scope.register` already records each param's `entity_kind`; the stamp site reads `scope.entity_name` today — it must instead resolve *which purpose param* a given identifier root binds to and stamp `purpose::that_param`. The scope must therefore expose a `purpose_param_root(ident) -> Option<&str>` lookup (the param-name set is known at `compile_purpose` time).

### 4.2 Activation-time remap — per-param mapping

`Engine::activate_purpose(name, entity_ref: &str)` and `activate_purpose_constraints` do a single `remap_entity(purpose_name, entity_ref)`. Replace with a **per-param binding map**:

```
// New signature (additive — see migration note)
pub fn activate_purpose_with_bindings(
    &mut self,
    purpose_name: &str,
    bindings: &[(String /* param_name */, String /* entity_ref */)],
)
// Existing single-entity API kept as a thin shim:
pub fn activate_purpose(&mut self, purpose_name: &str, entity_ref: &str) {
    // binds the purpose's sole param to entity_ref; panics/diagnostic if the
    // purpose has >1 param (callers must use the bindings form)
}
```

Activation then applies one remap per binding: for each `(param, entity)` pair, `remap_entity(format!("{purpose_name}::{param}"), entity)`. `remap_entity` (`reify-ir/src/expr.rs:1142`) already rewrites a single `from→to`; calling it N times over disjoint `from` stamps is safe because the `purpose::param` stamps are disjoint by construction (4.1). The injected-constraint prefix becomes `format!("purpose:{purpose_name}@{}", bindings-digest)` (a stable hash of the sorted binding pairs) so two activations of the same purpose against different entity sets don't collide in `active_purposes`.

**Reflective queries (`ResolvedSchemaQuery`)** already carry `param_name` (verified: `types.rs:127`, `expand_purpose_reflective_placeholders` filters by `param_name && query_kind`). The expansion is already per-param-correct; it only needs the multi-binding `entity_ref` lookup keyed by `param_name` instead of the single `entity_ref` argument. This is the lowest-risk part — the data model anticipated multi-param.

### 4.3 `CompiledPurpose` let-binding storage

`CompiledPurpose` (`types.rs:138`) has `constraints`, `objective`, `resolved_queries` — no let storage. Add:

```
pub struct CompiledPurposeLet {
    pub name: String,          // stamped entity "purpose::__let" or "purpose::param" scope
    pub cell_id: ValueCellId,  // ValueCellId { entity: "{purpose}::let", member: name }
    pub expr: CompiledExpr,    // compiled in purpose scope; may ref params + earlier lets
    pub span: Span,
}
pub struct CompiledPurpose { …; pub lets: Vec<CompiledPurposeLet>, }
```

Let exprs compile in the purpose scope (params + earlier lets visible; ordered, no forward refs — mirror entity-body let semantics). At activation, each let becomes a synthetic value-cell node in the eval graph stamped `ValueCellId { entity: "purpose:{name}@{digest}", member: "__let_{letname}" }`, with its expr `remap_entity`'d the same way as constraints. Constraint exprs referencing a let resolve to that synthetic cell (the let's stamp is remapped to the injected-constraint entity prefix, not to a user entity). Lets are **injected on activate, removed on deactivate** alongside the constraints (extend the `active_purposes` injected-id bookkeeping to also track injected let-cell ids).

### 4.4 Guarded blocks → implication constraints

A purpose guarded block `where <cond> { constraint A } else { constraint B }` lowers to **conditional constraint injection**, NOT runtime branching of the graph shape (purposes are graph-level; the activated graph must be deterministic). Lower each guarded member to an **implication**: `where C { constraint A }` injects `constraint C implies A` (Kleene three-valued per §9.2.3 — when `C` is `undef` the implication is `undef`, i.e. indeterminate, which `report_eval_output` already classifies as "indeterminate", not "violated"). `else { constraint B }` injects `constraint (not C) implies B`. Nested lets inside a guard are scoped to the guard's injected constraints. This reuses the existing implication/Kleene compile path (`determined` etc. already compose with logical ops) — no new eval primitive. The `else_members` already parsed by `lower_guarded_block` map to the `not C` arm.

### 4.5 Contract invariants

| # | Invariant |
|---|-----------|
| C1 | Per-param stamps are disjoint: distinct params of one purpose never share a `ValueCellId.entity`. |
| C2 | Activation with a binding map of size N applies exactly N remaps; an unbound param (param in purpose, missing from bindings) is a diagnostic, not a silent no-op. |
| C3 | A binding naming a param not in the purpose is a diagnostic. |
| C4 | Deactivation removes every injected node — constraints, objective, **and** let-cells — leaving the graph byte-identical to pre-activation (existing deactivate test invariant extended to lets). |
| C5 | A guard whose condition is `undef` yields indeterminate (not violated) for the guarded constraints (Kleene). |
| C6 | The single-param `activate_purpose(name, entity)` shim is behavior-identical to today for all existing single-param purposes (regression-locked). |
| C7 | Reflective queries (`.params` etc.) resolve against the **correct per-param** bound entity in a multi-param purpose. |

## §5 — `std.determinacy.purposes` stdlib module

New file `crates/reify-compiler/stdlib/determinacy_purposes.ri`, registered in `stdlib_loader.rs` as `("std.determinacy.purposes", include_str!("../stdlib/determinacy_purposes.ri"))` (one table entry — the loader is a flat `(path, source)` list; verified).

Contents (single-param `subject : Structure`, exercising let + guard so the module is a live consumer of §4):

```
pub purpose design_review(subject : Structure) {
    // all declared params present (the spec's canonical readiness check)
    constraint forall p in subject.params: constrained(p)
}

pub purpose simulation_ready(subject : Structure) {
    constraint forall p in subject.geometric_params: determined(p)
    where exists p in subject.material_params: constrained(p) {
        constraint forall p in subject.material_params: determined(p)
    }
}
```

(`design_review` = "every param has at least one constraint" — review-time intent; `simulation_ready` = "geometry fully determined, and *if* the structure carries material params then those are determined too" — guards the material check so a geometry-only part isn't spuriously blocked. These are spec-promised **example** purposes, §11.3 "example purposes"; exact constraint bodies are tunable, §11.) The module compiles only after features 1–3 land — intra-batch dep.

## §6 — Cross-PRD relationship (G4 seam ownership)

| Other PRD / cluster | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `constraint-solver-completion` (sibling cluster, authored in parallel) | produces (purposes emit objectives) → consumed by solver | `OptimizationObjective::{Minimize,Maximize}`, `Engine::active_objectives()` (`engine_purposes.rs:295`), `active_objective_map` | **constraint-solver-completion owns objective *solving*** | declared (cross-cluster dep) |
| `std.determinacy.purposes` (this PRD §5) | consumes (purpose body features) | purpose let/guard/multi-ref compile+activate | **this PRD** | intra-batch dep |
| `auto-binding-site-positions` (v0_6, partly landed: tasks 3808/3810 pending) | adjacent (both touch determinacy `Auto`→`Determined`) | none load-bearing — purposes read determinacy state, don't set `auto` | n/a | no seam |

**Objective seam detail (the one real cross-cluster seam).** Purposes already *carry* and *inject* objectives (`minimize`/`maximize` → `active_objective_map`); the eval crate exposes `active_objectives()`. Who *minimizes* them is the constraint solver. This PRD does **not** change objective injection (it already works for single-param; multi-ref objectives get the same per-param remap as constraints, §4.2 — covered). The open cross-cluster design question is **multi-objective composition** (a purpose with two `minimize` members, or two simultaneously-active purposes each with an objective): today `active_objective_map` is keyed by purpose-name (one objective per purpose; last-wins within a purpose body — verified `engine_purposes.rs:181`). Whether multiple objectives compose (weighted sum? lexicographic? Pareto?) is **owned by `constraint-solver-completion`**, not here. This PRD's contract: purposes *emit* objectives faithfully (per-param-correct); the solver cluster *decides how to combine* them. Declared as a real dependency edge at decompose time only if the constraint-solver tasks exist; otherwise recorded as a `## DESIGN FORK` + cross-cluster note (orchestrator stopped — sibling PRD not yet on disk).

No new in-engine seam is introduced — activation uses the existing constraint-injection path (overlay §3.5 ConstraintSolver / the demand-registry inject path in `activate_purpose_constraints`). No fourth contested-ownership pair created.

## §7 — Grammar gate (G3) — PASSED

`tree-sitter parse --quiet` from `tree-sitter-reify/`, all exit 0:

| Fixture | Shape | Result |
|---|---|---|
| `multiref.ri` | `purpose fits_within(part : Structure, envelope : Structure) { constraint determined(part.length) constraint determined(envelope.length) }` | exit 0 |
| `letbind.ri` | `purpose design_review(subject : Structure) { let margin = subject.length - subject.width  constraint margin > 0.0 }` | exit 0 |
| `guarded.ri` | `purpose simulation_ready(subject : Structure) { where determined(subject.material) { constraint determined(subject.youngs_modulus) } }` | exit 0 |
| `minmax.ri` | `purpose manufacturing_ready(subject : Structure) { constraint forall p in subject.geometric_params: determined(p)  minimize subject.cost }` | exit 0 |

The guarded fixture lowers to `(purpose_member (guarded_block (constraint_declaration …)))` and `lower_guarded_block` (`ts_parser.rs:1615`) already produces `MemberDecl::GuardedGroup`. **All rejections are at compile time, not grammar.** `grammar_confirmed = True` for every task.

## §8 — Approach (G5) and boundary-test sketch

**B + H.** The activation seam (compile-stamp ↔ activate-remap ↔ eval-graph inject) is the load-bearing interface; the two-way boundary tests below are the integration-gate signal (task ζ).

### 8.1 Boundary tests (facing both producer and consumer sides)

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| B1 | 2-param purpose activates, per-param identity preserved | `fits_within(part, envelope)` compiled; two distinct structures in graph; activate with `part:A, envelope:B` | `A.length` and `B.length` resolve to **different** value cells; constraint over `part.length > envelope.length` reads A's and B's actual values (not aliased) |
| B2 | single-param purpose unchanged (regression) | any existing single-param purpose (`manufacturing_ready`) | `activate_purpose(name, entity)` shim produces byte-identical injected constraints to pre-PRD (golden) |
| B3 | let-binding evaluates | `purpose p(s) { let m = s.a - s.b  constraint m > 0 }`; `s.a`, `s.b` determined | injected let-cell evaluates; constraint reads it; satisfied/violated matches `s.a - s.b > 0` |
| B4 | guard active arm | `where C { constraint A }`; C true | A injected as live constraint; participates in report |
| B5 | guard inactive arm (Kleene undef) | `where C { constraint A }`; C `undef` | A is **indeterminate**, not violated; `report_eval_output` returns `SomeIndeterminate` not `SomeViolated` |
| B6 | deactivate removes all (incl. lets) | activate then deactivate a let+guard+objective purpose | graph value_cells + constraints + objective map identical to pre-activate (extends existing deactivate invariant to lets) |
| B7 | CLI end-to-end | `reify check --purpose fits_within=part:A,envelope:B <file>` | exit code + report reflect the multi-ref purpose's constraints; unbound/unknown-param flag → clear CLI error + non-zero exit |
| B8 | stdlib activation | `reify check --purpose simulation_ready=<user Structure> <file>` | `simulation_ready` (imported from `std.determinacy.purposes`) activates; geometry-determined structure passes, geometry-undef structure reports indeterminate |

## §9 — Decomposition plan (the DAG)

Greek labels; real IDs assigned at decompose time. Modules: `reify-compiler` (compile/stamp), `reify-eval` (activate/inject), `reify-ir` (remap), `reify-cli` (consumer), `stdlib`.

### Phase 1 — Consumer surface first (close G1 before any producer)

- **α — `reify check --purpose <name>=<entity>` CLI flag (single-param path).** Parse `--purpose name=entity` (and the `name:entity,…` multi-pair form, even though multi-ref compile lands in β/γ — the flag parses both now; multi-ref *activation* is gated on γ). Wire `engine.activate_purpose` into `cmd_check` before `engine.check`. Unknown purpose / unbound param → clear CLI error + non-zero exit.
  - *Signal (leaf):* `reify check --purpose manufacturing_ready=<entity> <file>` activates an existing single-param purpose and its constraints appear in the satisfied/violated/indeterminate report + exit code (observable today against existing single-param purposes — proves the consumer before the producers).
  - *Modules:* reify-cli. *Prereqs:* none. *grammar_confirmed:* true.

### Phase 2 — Multi-ref binding scheme (the core)

- **β — Per-param `purpose::param` ValueCellId stamping (compile-time).** Replace the `scope.entity_name`-uniform stamp with per-param `format!("{purpose}::{param}")`; add `purpose_param_root` resolution to the purpose compile scope; thread param identity to reflective-query stamps. Remove the multi-param *compile* rejection at `traits.rs:286-301`. Lock single-param stamp regression (C6).
  - *Signal (intermediate):* unlocks γ. Observable proxy: a compile test pins that `part.length` and `envelope.length` stamp distinct `ValueCellId.entity` (`fits_within::part` vs `fits_within::envelope`) — and the multi-param reject diagnostic no longer fires.
  - *Modules:* reify-compiler. *Prereqs:* —. *grammar_confirmed:* true.
- **γ — Per-param activation remap (`activate_purpose_with_bindings`).** Add the bindings-map API; keep `activate_purpose` as the single-param shim (C6); apply one `remap_entity` per binding; binding-digest injected-constraint prefix; per-param reflective-query `entity_ref` lookup; validate unbound/unknown params (C2/C3). Wire α's multi-pair flag to this API.
  - *Signal (leaf):* `reify check --purpose fits_within=part:A,envelope:B <file>` — a 2-entity purpose activates and a constraint comparing `part.x` to `envelope.x` reads A's and B's distinct values (B1, B7). Exit code reflects satisfaction.
  - *Modules:* reify-eval, reify-cli. *Prereqs:* α, β. *grammar_confirmed:* true.

### Phase 3 — let bindings

- **δ — Purpose `let` storage + activation injection.** Add `CompiledPurposeLet` + `CompiledPurpose.lets`; compile let exprs in purpose scope (ordered, params + earlier lets visible); remove the `PurposeLetUnsupported` reject at `traits.rs:333-350`; at activate inject synthetic let-cells, at deactivate remove them (C4); constraint refs to lets resolve to the injected cell.
  - *Signal (leaf):* `reify check --purpose <p>=<entity>` on a fixture `purpose p(s){ let m = s.a - s.b  constraint m > 0 }` — the let evaluates and the constraint's satisfied/violated outcome matches `s.a - s.b > 0` (B3). Deactivate restores the graph (B6).
  - *Modules:* reify-compiler, reify-eval. *Prereqs:* γ. *grammar_confirmed:* true.

### Phase 4 — guarded blocks

- **ε — Purpose guarded blocks → implication injection.** Remove the guarded-block reject at `traits.rs:351-361`; lower `where C { A } else { B }` to `C implies A` / `(not C) implies B` injected constraints (Kleene three-valued, §9.2.3); guard-scoped lets nest into the guard's injected constraints.
  - *Signal (leaf):* `reify check --purpose <p>=<entity>` on `purpose p(s){ where determined(s.material){ constraint determined(s.youngs_modulus) } }` — active arm contributes a live constraint (B4); `undef` condition reports **indeterminate**, not violated (B5).
  - *Modules:* reify-compiler, reify-eval. *Prereqs:* γ (δ optional — ε can land before δ; wire ε after δ only if guard-scoped lets are in ε's scope, which they are → ε depends on δ).
  - *grammar_confirmed:* true.

### Phase 5 — stdlib module + integration gate

- **ζ — `std.determinacy.purposes` stdlib + B+H integration gate.** Author `crates/reify-compiler/stdlib/determinacy_purposes.ri` with `design_review` + `simulation_ready` (§5); register in `stdlib_loader.rs`. This is the **integration-gate task** — its signal is the §8.1 boundary-test suite (B1–B8) plus the stdlib end-to-end.
  - *Signal (leaf):* `reify check --purpose simulation_ready=<user Structure> <file>` — `simulation_ready` (from `std.determinacy.purposes`, no per-structure opt-in) activates: a geometry-determined structure passes, a geometry-`undef` one reports indeterminate (B8). The B1–B8 boundary tests are green.
  - *Modules:* stdlib (reify-compiler), reify-eval, reify-cli (tests). *Prereqs:* δ, ε (needs let + guards for `simulation_ready`'s body). *grammar_confirmed:* true.

### Phase 6 — spec/doc reconciliation

- **η — Spec + stdlib-reference update.** Update `reify-language-spec.md` §9.5 to show a multi-ref + let + guarded example (replacing the impression that purposes are constraint-only), and add `std.determinacy.purposes` entries to `reify-stdlib-reference.md`. Remove any "not yet supported" caveats now false.
  - *Signal (leaf):* the spec's §9.5 example block parses (`tree-sitter parse --quiet` exit 0) and matches the shipped stdlib bodies; stdlib-reference lists `design_review`/`simulation_ready`.
  - *Modules:* docs. *Prereqs:* ζ. *grammar_confirmed:* true.

### Dependency view

```
α (CLI consumer) ─┐
                  ├─► γ (activate multi-ref) ─► δ (let) ─► ε (guards) ─► ζ (stdlib + integration gate) ─► η (spec)
β (compile stamp)─┘
```

(`α` and `β` are independent roots; `γ` joins them. `δ`→`ε`→`ζ` is linear. Cross-cluster objective-composition edge to `constraint-solver-completion` declared at decompose time **only if** that cluster's tasks exist; else recorded as a fork.)

## §10 — Out of scope

- **GUI purpose-activation command.** `autoViewGenerator.ts` consumes `activePurposes` but no Tauri command populates it; wiring engine→frontend activation + a purpose-picker affordance is a separate GUI PRD. Declared in §3 so the orphan isn't reintroduced; not built here. **Follow-up:** file a `gui-purpose-activation` task post-batch.
- **Multi-objective composition** (weighted/lexicographic/Pareto). Owned by `constraint-solver-completion` (§6). Purposes emit objectives faithfully; combination is the solver's call.
- **The default purpose** (§9.5 "if no explicit purpose … a default purpose applies … robustness-oriented centrality"). A separate spec promise; not these four gaps.
- **Non-`Structure` entity-kind params at the binding scheme level** (`Occurrence`/`Constraint`/`Field` purpose params). The `::param` stamp + remap is entity-kind-agnostic by construction, but reflective-query resolution for non-Structure kinds is unverified; this batch ships `Structure` (the only kind with reflective-query plumbing today). Other kinds are a follow-up.
- **`type_params` on purposes** (`purpose p<T>(…)`). In-grammar but no use case in scope.

## §11 — Open (tactical) questions

1. **`::` vs another separator for the per-param stamp.** `::` chosen (cannot appear in `IDENT`; disjoint from `.`-sub-scoping and `@`-injection prefix). If a later grep-convention conflict surfaces, swap the constant — purely internal, no surface. **Decide during β** if conflict found; default `::`.
2. **Binding-digest hash for the injected-constraint prefix.** A stable hash of sorted `(param, entity)` pairs. `ContentHash::of_str` of the joined sorted pairs is the obvious default. **Decide during γ.**
3. **`design_review` / `simulation_ready` exact constraint bodies.** §5 gives defensible defaults (spec calls them "example purposes"). Tunable without re-architecting. **Decide during ζ.**
4. **CLI flag repeat vs comma-list for multiple active purposes.** `--purpose a=X --purpose b=Y` (repeatable) vs single flag. Default: repeatable flag, each value is one `name=binding-list`. **Decide during α.**
