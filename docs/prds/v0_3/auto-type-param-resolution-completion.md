# `auto:` Type-Parameter Resolution — v0.2 Completion Contract

Status: contract completing `docs/prds/v0_2/auto-resolution-backtracking.md`
(authored 2026-04-28, decomposed into tasks 2659–2664 + 2387/2390/2391 of which
the DFS library + parser + compile-pipeline call-site all landed). Authored
2026-06-08 in interactive `/prd` session. B + H (contracts + two-way boundary
tests). Pending Leo approval before queueing tasks.

## §0 — Purpose and supersession

The v0.2 backtracking PRD and its v0.1 parent (`docs/prds/auto-type-param-resolution.md`)
shipped a DFS resolver library (M-001…M-012) that is **wired, tested, and now
invoked from the compile pipeline** — task **3558** (DONE, merged `8d1cf09598`)
landed the lowering + the resolver call-site
(`crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs`), populates
`CompiledModule.auto_type_substitution`, and rewrites the use-site placeholder
slot `SubComponentDecl.type_args[pos]` → `Type::StructureRef(candidate)` so the
**bound-check** sees the resolved candidate. `examples/bearing_auto_seal.ri`
resolves `Bearing<auto(free): Seal>()` → `GasketSeal` end-to-end today.

But the feature is still **inert from the user's view**, for one keystone reason
plus its dependents:

- **M-013 (the keystone — TODO):** the resolver picks a candidate and rewrites
  the *bound-check slot*, but **no production code applies the binding
  `Type::TypeParam(T) → Type::StructureRef(candidate)` to the instantiated
  sub-component's cells / IR**. So a template body that *uses* the type param
  (`param seal : T`) still types its cell `Type::TypeParam("T")` and evals to
  `Value::Undef`. The resolved type never reaches type-check, constraint
  evaluation, or eval.
- **Selection is constraint-blind.** The sole production caller uses the stub
  `CompileTimeIndeterminateChecker`
  (`auto_type_param_phase.rs:52-66`), which returns `Indeterminate`(=feasible)
  for every constraint. So `auto:` picks the lex-first **trait-conformer** and
  never uses the template's constraints to disambiguate — the entire point of
  v0.2 backtracking (pick the candidate whose constraints are satisfiable) is
  theatre.
- **The inert siblings stay inert.** M-007 backjumping (task 2660, "done" but
  never exercised from real source — its blame map only fires when constraint
  cells are typed `Type::TypeParam`, which only happens once substitution runs
  inside the search loop); the M-005/M-006 BFS-fallback **soundness hazard**
  (latent today, becomes a live silently-wrong-substitution bug the moment
  per-candidate substitution lands); and the 2562 incremental-binding
  optimization (unmeasurable until a real evaluator makes the DFS actually
  backtrack).

This PRD is the **completion contract**: it lands the substitution pass across
three layers (type-surface, constraint-aware selection, value population), makes
the BFS fallback sound, gives M-007 a real source-level exercise, and reconciles
the stale task graph. When its decomposition lands, v0.2 auto-resolution is done
and the v0.1 parent is formally superseded.

Supersession / completion links:
- **Completes** `docs/prds/v0_2/auto-resolution-backtracking.md`. The v0.2 file
  remains the design source-of-truth for the search algorithm; this PRD's
  §4–§8 supply the missing apply/evaluate/value/soundness contracts.
- **Supersedes** `docs/prds/auto-type-param-resolution.md` (v0.1 parent). §15
  names the supersession edit task.
- Audit evidence:
  - `docs/architecture-audit/findings/auto-resolution-backtracking.md` —
    mechanisms M-001…M-014; **M-013 is this PRD's keystone**. (NOTE: that
    finding predates 3558 and lists M-002/M-014 as PARTIAL/FICTION — both are
    **resolved by 3558**; §15 corrects the finding's state column.)
  - `docs/reify-language-spec.md` §3.9 — `auto` for type parameters.
  - `docs/reify-implementation-architecture.md` §6.2 — resolution algorithm.

**Workspace-rename caveat for implementers:** the audit and the SIR PRD cite
`crates/reify-types/...`; that crate was renamed — the live homes are
`crates/reify-ir` (e.g. `Value`, `ConstraintChecker`) and `crates/reify-core`
(`Type`). Cite the live paths.

## §1 — Goal and user-observable surface

A reify user who writes `auto:` / `auto(free):` on a sub-component type-arg gets
a **concretely-typed, constraint-selected, default-constructed** instance — not
a `Real`/`Undef` placeholder, and not a constraint-blind lex-first pick.
Concretely:

**CI-gate signals (hard acceptance):**

`cargo test -p reify-eval --test auto_type_param_completion_e2e` runs four
end-to-end fixtures and they all pass:

- **`examples/auto/bearing_resolved_value.ri`** (L1 + L3) — `Bearing<T: Seal>`
  with `param seal : T`, instantiated `sub b = Bearing<auto(free): Seal>()`, and
  `let seal_thickness = b.seal.thickness`. `reify eval` reports
  `seal_thickness = 2 mm` (the resolved `GasketSeal`'s own default — today this
  is `Undef`). The resolved cell carries `Type::StructureRef("GasketSeal")`, not
  `Type::TypeParam("T")`.
- **`examples/auto/bearing_constraint_select.ri`** (L2) — two `Seal` candidates,
  exactly one of which satisfies a `Bearing` top-level constraint that reads the
  candidate's resolved default (e.g. `constraint seal.thickness < bore_radius`).
  Under **strict** `auto: Seal`, resolution returns the **unique** constraint-
  satisfying candidate and `reify eval` shows it — where today the stub checker
  would call it `Ambiguous` (2 feasible) and emit an Error. The selection
  *changes* because constraints now matter.
- **`examples/auto/bearing_unsat.ri`** (L2 negative) — a `Bearing` whose
  constraint no candidate satisfies. `reify check` emits a clean
  `E_AUTO_TYPE_PARAM_NO_CANDIDATE` (current naming) naming the violated
  constraint — not a silent `Undef`.
- **`examples/auto/bounded_fallback_unsound.ri`** (γ soundness) — a declaration
  that forces the depth-bound or 100k-cap BFS fallback onto a per-param-feasible
  but jointly-infeasible assignment. `reify check` emits the hard
  `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE`; it **never** emits a cross-product-
  infeasible substitution.

**Soundness regression signal (hard acceptance):**

- `cargo test -p reify-compiler --test auto_fallback_soundness` asserts the
  invariant directly: for a generated family of depth/cap-exceeding declarations,
  every BFS-fallback result is either (a) jointly feasible under the real
  checker, or (b) a hard `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` — never a
  substitution that the joint recheck would reject.

**Diagnostic-stream signals:**

- `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` (NEW) and the existing
  `AutoTypeParamAmbiguous` / `AutoTypeParamNonUnique` /
  `AutoTypeParamNoCandidate` / `AutoTypeParamDepthBoundExceeded` /
  `AutoTypeParamCrossProductSizeExceeded` reach `EvalResult.diagnostics` and
  surface through every existing consumer (LSP hover, MCP `report_diagnostics`,
  CLI `reify check`).

**Substrate-invariant signal (M-007 real exercise):**

- `cargo test -p reify-compiler --test auto_backjumping_real_source` exercises
  the M-007 backjump path from **real `.ri` source** (constraint cells typed
  `Type::TypeParam` during the search), replacing the `MockConstraintChecker`
  scripting that is the only exercise today.

## §2 — Scope

### §2.1 — Residual mechanism table

The mechanisms this PRD owns, each linked to audit provenance and current state:

| # | Mechanism | Layer | Audit ref | State today | Owner |
|---|---|---|---|---|---|
| α | Monomorphize resolved generic sub-component (per-`(name, type-args)` clone) + apply `TypeParam→StructureRef` substitution into the clone's value-cells **and constraint-expr ref cells** | L1 | M-013 (keystone) | TODO — 3558 rewrites only the bound-check slot; body cells stay `TypeParam` | this PRD |
| β | Real per-candidate constraint feasibility: thread `&dyn ConstraintChecker` through `compile_*` into the resolution phase; substitute the candidate's type + defaults into a per-candidate `ValueMap` **inside** the search loop | L2 | M-007 (top concern 2/3), stub at `auto_type_param_phase.rs:52-66` | INERT — stub returns `Indeterminate` for all; DFS never backtracks | this PRD |
| γ | BFS-fallback soundness: joint-recheck of the fallback assignment + hard `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE`; revert the now-orphaned post-substitution hoists | L2 | M-005, M-006 | LATENT — sound only while β is deferred; markers at `auto_type_param.rs:777,1338,1383,2040` | this PRD |
| δ | Value population: auto-construct the resolved param from the candidate's own defaults → `Value::StructureInstance` (non-`Undef`) | L3 | M-013 (value half) | TODO — bare `param:T` has no `default_expr` → `Undef` (`unfold.rs:344`) | this PRD (SIR consumed) |
| ε | Task-graph reconciliation + M-007 real-source test + audit-finding state edits | — | M-002/M-007/M-013/M-014 | STALE — 3522 over-reaches; finding predates 3558 | this PRD |

A companion task `θ` formally supersedes the v0.1 parent PRD.

### §2.2 — What this PRD does NOT add

- **`auto:` outside sub-component type-args.** Resolution stays scoped to
  `sub x = Foo<auto: T>()` use-sites (the only position 3558 wired). `auto` on
  free-function type-params is explicitly deferred by
  `docs/prds/v0_6/generic-user-functions.md` §11; `auto` on enum variants is
  `generic-data-carrying-enums` territory. Both **reuse** the resolver Phase
  A/B/C unchanged (see §10).
- **Value-parameter `auto`** (the scope-level coupled solver, arch §11.4–§11.5).
  Carried forward from the v0.2 PRD out-of-scope.
- **Incremental-binding optimization** (task 2562). Stays a deferred bookmark;
  β makes its trigger *measurable* but does not action it (§10).
- **Candidates requiring non-defaulted constructor args.** δ auto-constructs only
  candidates that are zero-arg-constructible from their own defaults; a resolved
  candidate with a required (non-defaulted) param emits a clean diagnostic
  (`E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE`) rather than guessing args.
  Supplying args to an auto-resolved candidate is a future extension (§14.1).
- **SMT-style constraint propagation.** Search remains discrete over type
  choices; per-assignment feasibility is the existing checker (v0.2 out-of-scope,
  unchanged).
- **General monomorphization of non-`auto` generics.** The clone machinery α
  builds is keyed off the `auto`-resolution phase. Generic *user functions* use
  type **erasure** (no monomorphization, no value-side type-args — PRD
  `generic-user-functions.md` D1), a deliberately different model; α does not
  unify the two.

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status (2026-06-08) | Gate phase |
|---|---|---|---|
| Resolver library + DFS + backjumping wired | v0.1/v0.2 PRDs (tasks 2387/2659/2660…) | landed | substrate |
| Parser accepts `auto:`/`auto(free):` in `type_arg_list`; AST lowers to `TypeExprKind::Auto` | task 3526/3558 | landed (`a46e7d3888`) | substrate |
| Compile-pipeline call-site + `auto_type_substitution` populate + bound-check slot rewrite | task **3558** | **landed** (`8d1cf09598`) | hard prereq for α |
| **SIR** — `Value::StructureInstance` + `eval_structure_instance_ctor` + ctor-lowering precedence | `docs/prds/v0_3/structure-instance-runtime.md` task **3540** | **landed** (`3faa8373de`) | hard prereq for δ |
| `ConstraintChecker` trait object already a parameter of `resolve_auto_type_params_with_backtracking` | task 3558 | landed | substrate for β |
| Representability invariant (`assert_value_cell_types_representable` panics on a `Type::TypeParam` value cell; `Type::StructureRef` permitted Undef-only) | tasks 1867/2287 | landed | **forces** α before any `T`-in-body cell can reach eval |

No NEW substrate is invented: every mechanism either edits existing compiler/eval
code or consumes a landed capability (SIR, the resolver, the slot-rewrite). G3
grammar gate: **no novel syntax** — the demo fixtures use only
`structure def X<T: Trait>`, `param p : T`, `sub s = X<auto: Trait>()`, and
member access, all of which parse today (`auto:` since `a46e7d3888`; `param:T`
exercised in `trait_bounds_tests.rs`). Re-confirm the four example fixtures parse
0-ERROR at decompose.

## §4 — Contract: L1 substitution + monomorphization (α, the keystone)

### §4.1 — Why monomorphization is required

`EvaluationGraph::from_templates` (`crates/reify-eval/src/graph.rs:278-314`)
inserts **one shared template per name**, and both static and runtime child
binding look the child template up by `SubComponentDecl.structure_name`
(`graph.rs:365` → `find_template`, `crates/reify-compiler/src/types.rs:789`),
**ignoring `type_args`**. So a single shared `Bearing` template cannot carry two
different per-instantiation resolutions (`Bearing<auto: A>()` and
`Bearing<auto: B>()` in the same module). Correct resolution therefore requires a
**distinct monomorphized template per `(generic-name, ordered resolved
type-args)`**, with `structure_name` rewritten to point at it.

### §4.2 — Monomorphization contract

In `phase_auto_type_param_resolution` pass-2 (where the substitution map and
`&mut ctx.templates` are both in scope, `auto_type_param_phase.rs:180-187`), for
each resolved use-site:

1. Build the per-instantiation substitution map `Σ = {T_i → StructureRef(c_i)}`
   from `outcome.substitution`.
2. Compute a **synthesized monomorph name** `mono = mangle(generic_name, [c_i…])`
   — deterministic, order-stable (e.g. `Bearing$GasketSeal`). **Dedup:** two
   use-sites with identical `(generic_name, [c_i…])` share one monomorph (keyed
   in a `HashMap<MonoKey, String>` on the ctx), so the clone count is bounded by
   distinct instantiations, not use-sites.
3. If `mono` is new, **clone** the generic `TopologyTemplate`, apply `Σ` via the
   existing recursive walker `substitute_type_params`
   (`crates/reify-compiler/src/type_resolution.rs:1161`) over **every** cell type
   and **every constraint-expr `ValueRef` cell type** in the clone (not just the
   top-level type-arg slot), set `clone.name = mono`, strip its `type_params`,
   and push it into `ctx.templates`.
4. Rewrite the originating `SubComponentDecl.structure_name = mono` (in addition
   to the existing `type_args[pos]` slot rewrite, which stays for the
   bound-check).

**Invariants:**

1. After α, no value cell reachable from a resolved sub-component carries
   `Type::TypeParam`. The representability invariant
   (`engine_eval.rs:144`) is satisfied by construction — `StructureRef` is
   Undef-or-representable, `TypeParam` would panic. (α is *net-positive* for that
   invariant: it removes the only construct that could trip it.)
2. A module with **no** `auto:` type-args produces **zero** monomorphs and leaves
   `ctx.templates` byte-identical — preserving the load-bearing empty-substitution
   / topology-fingerprint stability `auto_type_param_phase.rs` documents.
3. The monomorph name is a pure function of `(generic_name, [resolved c_i…])`
   in lex order — deterministic across runs (mirrors the M-011 determinism pin).

### §4.3 — Generic templates with unresolved (non-`auto`) type-params

A generic `structure def` is only monomorphized **at an `auto:` use-site**. A
generic template that is never instantiated (or instantiated with explicit
concrete args, not `auto:`) is untouched — the explicit-arg path already rewrites
its slot to `StructureRef` and 3558's bound-check covers it. Templates that
remain abstract (declared, never instantiated) keep `TypeParam` cells but never
reach `from_templates` with a live instance, so the invariant never fires (the
status-quo, preserved).

## §5 — Contract: L2 real per-candidate constraint evaluation (β)

### §5.1 — Checker injection (the layering fix)

`reify-compiler` has **no** dependency on `reify-constraints` and cannot acquire
one (constraints dev-deps compiler → a prod edge cycles). `reify-eval` has
`reify-constraints` as a **dev-dependency only**. The real `SimpleConstraintChecker`
(`crates/reify-constraints/src/lib.rs:47`) is constructed by the **top-level
binary** and injected as `Box<dyn ConstraintChecker>` (`gui/src-tauri/src/main.rs:651`).

Contract: thread `checker: &dyn ConstraintChecker` (defaulting to the existing
stub) through the compile entry points into the phase:

```rust
// crates/reify-compiler/src/lib.rs — extended signatures (default-stub overload kept)
pub fn compile_with_prelude_context(
    parsed: &ParsedModule,
    prelude: &[&CompiledModule],
    checker: &dyn ConstraintChecker,   // NEW — was implicitly the stub
) -> CompiledModule;

// crates/reify-compiler/src/compile_builder/auto_type_param_phase.rs
pub(crate) fn phase_auto_type_param_resolution(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    checker: &dyn ConstraintChecker,   // NEW — replaces in-fn CompileTimeIndeterminateChecker
);
```

- The real checker value **must originate above reify-eval** (in the GUI/CLI
  binary that already links `reify-constraints`). reify-eval passes whatever its
  caller handed it; when none is supplied, the call defaults to
  `CompileTimeIndeterminateChecker` (kept, not deleted), so non-`auto` compiles
  and tests are unchanged.
- **Per-call-site, not global.** Only the structure sub-component resolution
  path opts into the real checker. The enum/fn-generics consumers of the shared
  resolver (`generic-data-carrying-enums` task 4031, etc.) keep passing the stub
  unless they separately opt in — bounding β's blast radius (see §10 G4).

### §5.2 — Per-candidate feasibility inside the search loop

Today Phase B's constraint list is **hoisted out** of the per-candidate / DFS-leaf
loop (`build_constraints_template`, `auto_type_param.rs:2045`, called at `:786`
and `:1377`) precisely because the verdict is candidate-independent (empty
`ValueMap`, unchanged constraints). β makes it candidate-dependent:

- For each candidate `c` at a search node, build a per-candidate `ValueMap`
  seeded with `c`'s resolved default field values (so a constraint reading
  `seal.thickness` sees `GasketSeal`'s `2mm`), and apply `Σ_partial` to the
  constraint exprs' ref-cell types so `build_constraint_blame_map`
  (`auto_type_param.rs:1902-1941`) finds the `Type::TypeParam`-typed refs that
  make M-007 backjumping fire.
- **Revert the hoist** at the three `NOTE(substitution-pass-trigger)` sites:
  move `build_constraints_template` back inside the loop with per-candidate
  `ValueMap` setup, as the markers instruct
  (`auto_type_param.rs:777-785,1383-1391,2033-2044`). This is part of γ's
  deliverable (§6) since the hoist + the soundness fix are the same code region.

**Invariant:** with the stub checker, β is a behavioural no-op — every check is
`Indeterminate`, so per-candidate `ValueMap`s don't change any verdict and the
DFS still never backtracks. β's effect is observable **only** under the real
checker. This keeps the change safe for every non-`auto` and stub-path caller.

## §6 — Contract: BFS-fallback soundness (γ)

### §6.1 — The hazard

The depth-bound (>`max_depth`, default 6) and 100k-cap branches fall back to v0.1
per-param-independent BFS (`auto_type_param.rs:1354-1375` and the cap mirror).
BFS picks each param's candidate independently. Once β makes Phase B
candidate-dependent, an assignment that is **per-param feasible** can be
**jointly infeasible** at the cross-product — and the fallback would emit it as a
substitution while the warning still reads "fell back to BFS." That is a
silently-wrong substitution.

### §6.2 — Joint-recheck contract

```
1. BFS fallback yields assignment A = (c_1, …, c_N).
2. Build a single ConstraintInput with ALL of A substituted (full per-A ValueMap)
   and run checker.check(&input) ONCE.
3. If no constraint is Violated  → A is jointly feasible:
      emit the existing AutoTypeParamDepthBoundExceeded /
      AutoTypeParamCrossProductSizeExceeded *Warning* (graceful degradation
      preserved) and accept A.
4. If any constraint is Violated → emit hard error
      E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE, naming the parameters considered,
      the bound/cap that fired, and the violated constraint(s). Produce NO
      substitution for the declaration.
```

- **Soundness:** the accepted A is always jointly feasible; an unsound A is never
  emitted. Cost is O(1) extra checks (one full-assignment check), not
  O(cross-product) — tractable precisely where exhaustive search was not.
- **Graceful degradation retained:** a bounded search whose fallback assignment
  *is* jointly feasible still compiles with a Warning (the v0.2 PRD's stated
  intent), unlike a blanket Warning→Error.
- Under the stub checker, step 3 always holds (no Violated), so γ is a no-op on
  the stub path — same safety property as β.

### §6.3 — Hoist reversion + 3637 reconciliation

The `NOTE(substitution-pass-trigger)` hoists (`auto_type_param.rs:777,1383,2040`)
and `build_constraints_template` (`:2045`) are reverted as part of β/γ. Task
**3637** ("revert post-substitution hoists when the substitution pass lands") is
marked DONE but delivered only the *diagnostic self-documentation* half — the
actual revert is **un-tracked**. ε re-homes the revert deliverable here (do not
re-open 3637; cite it).

### §6.4 — Diagnostic registration

`E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` is a NEW `DiagnosticCode` variant
registered in `crates/reify-ir/src/diagnostics.rs` (the home of the existing
`AutoTypeParam*` codes), with the standard severity/format contract, so it flows
to every diagnostic consumer with no per-consumer change.

## §7 — Contract: L3 value population (δ)

### §7.1 — Auto-construct semantics

For a resolved `param seal : T` (no explicit value, `T → GasketSeal`), δ
synthesizes a **zero-arg `StructureInstanceCtor` default_expr** on the
monomorphized clone's cell, so `elaborate_child_params_only`
(`crates/reify-eval/src/unfold.rs:291-378`) takes the default branch (`:338-342`)
and `eval_structure_instance_ctor` (`crates/reify-expr/src/lib.rs:910`) produces
`Value::StructureInstance(GasketSeal { thickness: 2mm, … })` from `GasketSeal`'s
own param defaults — replacing the `Undef` fallthrough (`unfold.rs:344`).

```
resolved param  seal : T          (T → GasketSeal, GasketSeal zero-arg constructible)
        ⇓ δ synthesizes default_expr
        seal : StructureRef("GasketSeal") = GasketSeal()   // zero-arg ctor over GasketSeal defaults
        ⇓ eval (unfold.rs:338 → eval_structure_instance_ctor)
        Value::StructureInstance(GasketSeal { thickness: 2mm, outer_diameter: 30mm })
```

### §7.2 — Invariants & edges

1. **Member access through the resolved instance works** — `b.seal.thickness`
   evaluates via SIR's landed member-access path (task 4342). The L1
   `StructureRef` type + the L3 value together make `seal.thickness : Length =
   2mm` both type-check and eval.
2. **An explicit value wins.** If the use-site supplies the param
   (`Bearing<auto: Seal>(seal: GasketSeal(thickness: 1mm))`), δ does not
   override it — the synthesized default only fills an otherwise-empty cell
   (mirrors normal default-vs-arg precedence at `unfold.rs:336-345`).
3. **Non-constructible candidate.** If the resolved candidate has a required
   (non-defaulted) param, δ cannot zero-arg-construct it → emit
   `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE` (NEW) naming the missing
   param, rather than producing a partially-`Undef` instance. (§14.1 tracks the
   future "supply args to the auto-resolved candidate" extension.)
4. δ depends on α's monomorphized clone existing — a shared template cannot carry
   two different per-instantiation ctor defaults.

## §8 — Contract: reconciliation, M-007 activation, audit edits (ε)

### §8.1 — Task 3522 reconciliation

Task **3522** ("invoke orchestrator from compile pipeline; populate
`CompiledModule.auto_type_substitution`; downstream eval yields correctly-typed
value") is **stale**: its named deliverables (a) the call-site and (b) the
populate were **both landed by 3558**; its over-reaching clause "downstream eval
yields correctly-typed value (not Real, not Undef)" **is M-013** — α + δ here.

Reconciliation (decided 2026-06-08):
- Close 3522 as **superseded-by-3558** (populate + call-site are on main;
  `done_provenance` kind=merged, commit `8d1cf09598`).
- The over-reaching eval signal is **re-homed** to this PRD's α/δ leaves.
- **Repoint task 3751** (`Type::Unknown`, currently `depends_on 3522`) → depend
  on this PRD's α leaf (the substitution infrastructure it actually needs).
- The brief's "M-013 `depends_on` 3522" edge is **dropped** — there is no pending
  populate step to depend on (it would be a permanently-satisfiable open dep).

### §8.2 — M-007 real-source activation

M-007 backjumping (task 2660, "done") has never run from real source — the e2e
tests script verdicts via `MockConstraintChecker::with_call_queue`. With β
substituting candidate types into constraint-expr ref cells inside the loop, the
blame map (`auto_type_param.rs:1902-1941`) finally fires from real `.ri`
constraints. ε adds `auto_backjumping_real_source` (§1) proving the backjump path
executes against the real checker — the first real exercise of task 2660's core
claim.

### §8.3 — Audit-finding state edits

Update `docs/architecture-audit/findings/auto-resolution-backtracking.md`:
M-002/M-014 → **WIRED** (resolved by 3558); M-013 → **WIRED** (this PRD);
M-005/M-006 → **WIRED** (γ soundness fix); M-007 → **WIRED** (β real exercise).

## §9 — Resolved design decisions

**(9.1) Substitution is a compile-time IR rewrite over monomorphized clones, not
an eval-time lookup.** `CompiledModule.auto_type_substitution` is keyed by
param-name only and is a documented lossy debug/audit aggregate
(`auto_type_param_phase.rs:193-205`); the eval layer has no per-cell `T`-identity
to look up. Rewriting `Type::TypeParam → Type::StructureRef` over the clone's
cells (via the existing `substitute_type_params` walker) is the clean mechanism
and is net-positive for the representability invariant (removes the only
panic-trigger).

**(9.2) Full per-`(name, type-args)` monomorphization, not in-place rewrite.**
In-place rewrite of the shared template is correct only for single-instantiation
and silently wrong on the second differing instantiation — exactly the audit's
dominant failure shape. The synthesized-name clone (deduped per distinct
instantiation) is correct by construction; the `from_templates` lookup-by-name
(`graph.rs:365`) forces the synthesized name + the `structure_name` rewrite.

**(9.3) Real constraint evaluator injected as `&dyn ConstraintChecker` from the
binary, defaulting to the stub.** The crate DAG forbids reify-compiler (and
reify-eval, prod) from constructing `SimpleConstraintChecker`; the only
non-cycling path is threading the trait object the GUI/CLI already owns. Keeping
the stub as the default makes β safe for every non-`auto`/test caller, and
per-call-site opt-in bounds the blast radius away from the enum/fn-generics
consumers of the shared resolver.

**(9.4) BFS-fallback soundness via single joint-recheck + hard error, not blanket
Warning→Error.** Sound (never emits an unsound substitution), O(1) extra cost,
and preserves the v0.2 PRD's explicit graceful-degradation intent for bounded
searches whose fallback assignment is in fact jointly feasible. Blanket
Warning→Error would reject currently-working compiles.

**(9.5) Auto-construct the resolved param from the candidate's own defaults.**
Matches spec §3.9 ("system, give me one"). A bare `param : T` becoming a usable
instance is the compelling user story; a non-constructible candidate emits a
clean diagnostic rather than a partial-`Undef`.

**(9.6) β/γ/δ are behavioural no-ops on the stub path.** Each new mechanism is
gated on the real checker (β, γ) or on a resolved `auto:` use-site (α, δ). A
module without `auto:`, or compiled with the default stub, is byte-identical to
today — the safety property that lets this land incrementally.

**(9.7) 3522 closed-superseded, not re-scoped in place.** 3558 already satisfied
3522's named deliverables; re-homing the residual (b) to fresh α/δ leaves
reflects ground truth and keeps the dependency direction honest (no M-013→3522
inversion).

## §10 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR, task 3540) | consumes | `Value::StructureInstance` + `eval_structure_instance_ctor` (`reify-expr/src/lib.rs:910`); δ synthesizes a zero-arg ctor over it | SIR | **landed** (`3faa8373de`) |
| `docs/prds/v0_2/auto-resolution-backtracking.md` (v0.2 parent) | completes | resolver Phase A/B/C/DFS + the M-013 apply pass | this PRD | this PRD's decomposition landing = v0.2 done |
| `docs/prds/auto-type-param-resolution.md` (v0.1 parent) | supersedes | — | this PRD | task θ adds §0 supersession line |
| task **3522** (M-014 populate) | reconciles | call-site + populate already on main (3558) | this PRD | ε closes 3522 superseded-by-3558 |
| task **3751** (`Type::Unknown`) | produces | the `TypeParam→StructureRef` substitution infra it consumes | this PRD | ε repoints 3751 `depends_on 3522` → α |
| task **2562** (incremental-binding optimization) | enables | β makes its >50ms-on-real-models trigger *measurable* (stub made per-leaf cost ≈0) | 2562 (deferred bookmark) | β does not action it; stays deferred |
| `docs/prds/v0_6/generic-user-functions.md` (fn generics) | independent | reuses `substitute_type_params` + Phase A/B/C, but uses **type erasure** (no monomorph, no value-side type-args, D1) | that PRD | no contest — different model; β's real-checker is per-call-site opt-in, fn path keeps stub |
| `docs/prds/v0_6/generic-data-carrying-enums.md` (DCE, task 4031) | independent | plugs `EnumDef.type_params` into the **existing** resolver Phase A | that PRD | no contest — DCE consumes Phase A unchanged; keeps the stub checker |
| task **3637** (hoist-revert tracker, DONE) | reconciles | the post-substitution hoist revert it tracked but didn't deliver | this PRD | γ delivers the revert; ε cites 3637 (don't re-open) |

**Seam-ownership resolution.** The two real seams are unambiguous: SIR owns
`StructureInstance`/the ctor (landed; this PRD consumes); this PRD owns the
`compile_*` checker-injection signature change (no other PRD touches it). The
**shared-resolver** seam (fn-generics / DCE / this PRD all call
`resolve_auto_type_params*`) is resolved by **per-call-site checker choice**: β's
real evaluator is opt-in at the structure sub-component call-site only; the
fn-generics and DCE call-sites keep the stub, so changing the resolver's
*available* feasibility power does not change *their* behaviour. This is a new
shared seam — recorded here so a later PRD doesn't assume the resolver globally
gained constraint-aware feasibility.

## §11 — Boundary test sketch (cross-crate; facing both ways)

### §11.1 — Producer-side (reify-compiler, reify-eval look outward at consumers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Monomorph carries concrete type.** Compile `sub b = Bearing<auto(free): Seal>()` with `param seal : T`. | α landed. | `ctx.templates` contains `Bearing$GasketSeal` whose `seal` cell type is `StructureRef("GasketSeal")`; `SubComponentDecl.structure_name == "Bearing$GasketSeal"`; **no** value cell types `Type::TypeParam`. |
| **Two instantiations, two monomorphs.** `Bearing<auto: A>()` and `Bearing<auto: B>()` resolving to different candidates in one module. | α full-monomorph. | two distinct monomorph templates; identical-instantiation use-sites dedupe to one. |
| **Constraint-aware unique selection.** Strict `auto: Seal` where one of two candidates violates a `Bearing` constraint reading the candidate default. | β real checker wired. | the violating candidate is filtered; result is `Selected(survivor)`, **not** `Ambiguous`. With the stub, result is `Ambiguous` (regression-pins the stub-vs-real difference). |
| **Bounded fallback, jointly feasible.** >6 auto-params whose BFS assignment passes the joint recheck. | γ joint-recheck. | `AutoTypeParamDepthBoundExceeded` **Warning**; substitution accepted. |
| **Bounded fallback, jointly infeasible.** BFS assignment fails the joint recheck. | γ joint-recheck. | hard `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE`; **no** substitution emitted; soundness invariant holds. |
| **Value population.** Resolved `seal : T` (T→GasketSeal, zero-arg constructible). | δ + SIR. | eval yields `Value::StructureInstance(GasketSeal{thickness:2mm,…})`; `b.seal.thickness == 2mm`. |
| **Non-constructible candidate.** Resolved candidate has a required non-defaulted param. | δ edge. | `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE` naming the param; no partial-Undef instance. |
| **M-007 real backjump.** Real-source declaration whose constraint blames an earlier param. | β substitutes constraint-cell types. | blame map non-empty; `DfsControl::BackjumpTo(J)` fires; result matches the exhaustive-search baseline. |

### §11.2 — Consumer-side (downstream PRDs / user `.ri` look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **`Type::Unknown` resolves via the substitution infra.** 3751's empty-collection element type participates in `auto`-style resolution. | α substitution infra; 3751 repointed. | 3751's positive case resolves; no `List<Real>` cascade (3751's own gate). |
| **Stub-path callers unchanged.** Compile any non-`auto` module, or an `auto` module with the default stub checker. | β/γ default-stub overload. | byte-identical diagnostics + substitution to pre-PRD `main` (no-op invariant). |
| **fn-generics / DCE unaffected.** Compile a generic user fn / generic enum that reuses the resolver. | per-call-site stub retained on those paths. | their existing tests pass unchanged; resolver behaviour on their call-sites is identical to today. |
| **LSP / MCP / CLI surface the new diagnostic.** A `.ri` triggering `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE`. | γ registers the code. | the code appears in LSP hover, MCP `report_diagnostics`, and `reify check` with no per-consumer change. |

## §12 — Decomposition plan

Vertical-slice B+H. α is the foundation (monomorph + L1 substitution); β/γ/δ each
ship one residual layer; ε reconciles the task graph + activates M-007; ζ is the
integration gate; θ supersedes v0.1.

### Phase 1 — Foundation: L1 substitution + monomorphization

- **α — Monomorphize resolved generic sub-components and apply `TypeParam→StructureRef` into the clone's cells + constraint-expr refs.**
  - Crates: `reify-compiler/src/compile_builder/auto_type_param_phase.rs`, `reify-compiler/src/type_resolution.rs` (reuse `substitute_type_params`), `reify-compiler/src/types.rs` (monomorph name/dedup map), `reify-compiler/tests/`.
  - Observable signal (intermediate; unlocks β, δ, ε): `reify-compiler` integration test asserts a resolved `Bearing<auto: Seal>()` produces a monomorph template `Bearing$<cand>` whose `seal` cell is `StructureRef`, the `SubComponentDecl.structure_name` is rewritten, and a debug-build smoke confirms **no** `Type::TypeParam` value cell reaches `assert_value_cell_types_representable`. Dedup + determinism pinned.
  - Prereqs: none beyond landed 3558/SIR.

### Phase 2 — L2 constraint-aware selection (β)

- **β-inject — Thread `&dyn ConstraintChecker` through `compile_*` into the phase (default = existing stub).**
  - Crates: `reify-compiler/src/lib.rs` (+ `compile`/`compile_with_prelude` overloads), `reify-compiler/src/compile_builder/auto_type_param_phase.rs`, `reify-eval/src/lib.rs` + call-sites (pass-through), `gui/src-tauri/src/main.rs` + `crates/reify-cli/src/main.rs` (supply `SimpleConstraintChecker`).
  - Observable signal (intermediate; unlocks β, γ): all `compile_*` call-sites compile; stub default keeps every existing test green (no-op invariant test).
  - Prereqs: α.

- **β — Per-candidate `ValueMap` substitution inside the search loop + real feasibility.**
  - Crates: `reify-compiler/src/auto_type_param.rs` (per-candidate `ValueMap`; substitute candidate defaults + constraint-cell types in the loop).
  - Observable signal (LEAF): `examples/auto/bearing_constraint_select.ri` — strict `auto: Seal`, two candidates, one constraint-violating; `reify eval` selects the unique survivor (today: `Ambiguous` Error). Regression test pins stub→`Ambiguous`, real→`Selected`.
  - Prereqs: α, β-inject.

### Phase 3 — BFS-fallback soundness (γ)

- **γ — Joint-recheck + `E_AUTO_TYPE_PARAM_BOUNDED_INFEASIBLE` + revert the post-substitution hoists.**
  - Crates: `reify-compiler/src/auto_type_param.rs` (fallback recheck; revert hoists at `:777,1383,2045`), `reify-ir/src/diagnostics.rs` (new code).
  - Observable signal (LEAF): `examples/auto/bounded_fallback_unsound.ri` emits the hard error; `cargo test -p reify-compiler --test auto_fallback_soundness` proves the invariant (fallback never emits a joint-infeasible substitution); a jointly-feasible bounded fixture still compiles with a Warning.
  - Prereqs: β.

### Phase 4 — L3 value population (δ)

- **δ — Auto-construct the resolved param from candidate defaults (zero-arg `StructureInstanceCtor` default_expr).**
  - Crates: `reify-compiler/src/compile_builder/auto_type_param_phase.rs` (synthesize default_expr on the monomorph cell), `reify-eval` (verify `elaborate_child_params_only` takes the default branch), `reify-eval/tests/`.
  - Observable signal (LEAF): `examples/auto/bearing_resolved_value.ri` — `reify eval` reports `b.seal.thickness == 2mm` (non-`Undef`); non-constructible-candidate fixture emits `E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE`.
  - Prereqs: α (monomorph clone), SIR (landed, cross-PRD).

### Phase 5 — Reconciliation + M-007 activation (ε)

- **ε — Close 3522 superseded-by-3558; repoint 3751; M-007 real-source test; audit-finding state edits.**
  - Files: task graph (3522 status, 3751 dep), `docs/architecture-audit/findings/auto-resolution-backtracking.md` (M-002/M-007/M-013/M-014 → WIRED), `reify-compiler/tests/auto_backjumping_real_source.rs` (NEW).
  - Observable signal (LEAF): `auto_backjumping_real_source` passes (real-source backjump, no `MockConstraintChecker`); `grep` shows the finding's M-013 row marked WIRED with this PRD's task IDs; 3751 `depends_on` α (not 3522).
  - Prereqs: α, β (M-007 needs real-checker substitution).

### Phase 6 — Integration gate + supersession

- **ζ — Integration acceptance.**
  - All four `examples/auto/*.ri` pass `cargo test -p reify-eval --test auto_type_param_completion_e2e`; the producer + consumer boundary tables (§11) are realized; `cargo test --workspace` green; the GUI/CLI binary actually injects `SimpleConstraintChecker` (smoke that an `auto:` constraint-selecting fixture resolves under the real binary, not just the test harness).
  - Observable signal (LEAF — the B+H integration gate): full CI green; §11 boundary tests pass both ways.
  - Prereqs: α, β, γ, δ, ε.

- **θ — Supersede v0.1 parent PRD.**
  - Files: `docs/prds/auto-type-param-resolution.md` (add `## §0 — Superseded` → this PRD); `docs/prds/v0_2/auto-resolution-backtracking.md` (completion-status note).
  - Observable signal (LEAF): `grep -E '^Status:.*[Ss]uperseded' docs/prds/auto-type-param-resolution.md` returns the marker; supersession commit in `git log`.
  - Prereqs: α, β, γ, δ (residuals actually resolved before retiring v0.1).

### §12.1 — Dependency view

```
α ──┬──────────────► δ ──┐
    │                    │
    ├── β-inject ── β ──┬─┼──► ζ ──► (θ)
    │                   │ │
    │              γ ◄──┘ │
    │              │      │
    └──────────────┴──ε ──┘
            (ε needs α,β; ζ needs α,β,γ,δ,ε; θ needs α,β,γ,δ)
```

8 in-batch tasks (α, β-inject, β, γ, δ, ε, ζ, θ). Cross-PRD edges: δ→SIR
(task 3540, landed); ε edits 3522 (close) + 3751 (repoint). No edge inverts onto
3522.

## §13 — Out of scope for this PRD

- `auto:` outside sub-component type-args (fn type-params, enum variants) — §2.2.
- Value-parameter `auto` (scope-level coupled solver, arch §11.4–§11.5).
- Incremental-binding optimization (task 2562) — β makes it *measurable*, does
  not action it.
- Supplying explicit constructor args to an auto-resolved candidate — §14.1.
- Unifying the monomorphization model with fn-generics type-erasure.
- SMT-style constraint propagation.
- `auto` re-resolution on registry change (Phase D / SchemaNode topology trigger,
  task 2388) — separate deferral, unchanged.

## §14 — Open questions (tactical; surfaced, not blocking)

1. **Args to an auto-resolved candidate.** §7.2 emits a diagnostic for
   non-constructible candidates. A future `Bearing<auto: Seal>(seal_thickness =
   1mm)` forwarding form may be wanted. **Suggested resolution:** defer to a
   focused follow-up once dogfood shows demand; the diagnostic is the honest v0.3
   behaviour. Decide at δ impl time.

2. **Monomorph name mangling collisions.** `mangle(generic, args)` must avoid
   colliding with a user `structure def` named `Bearing$GasketSeal`.
   **Suggested resolution:** use a sigil illegal in source identifiers (`$` is not
   a valid `.ri` identifier char — confirm against the grammar at α impl) or a
   reserved prefix; emit a build error on the (impossible-from-source) collision.
   Decide at α impl time.

3. **Per-candidate `ValueMap` seeding depth.** β seeds the candidate's *own*
   default field values. A constraint reading a *nested* resolved field
   (`seal.gasket.thickness`) needs the nested default chain. **Suggested
   resolution:** seed one level (the candidate's direct defaults) for v0.3;
   nested-default seeding is a measured follow-up if a real constraint needs it.
   Decide at β impl time.

4. **Where the binary supplies the checker for batch/headless compiles.** The
   GUI/CLI inject `SimpleConstraintChecker`; reconciliation/CI compile paths may
   run without it (defaulting to the stub → constraint-blind selection in those
   contexts). **Suggested resolution:** acceptable for v0.3 (the stub path is a
   sound subset — it never picks an *infeasible* candidate, only a less-
   disambiguated one); document the contexts where real selection is active.
   Decide at β-inject impl time.

## §15 — Audit-finding + task-graph companion edits

To be applied as part of ε / θ:

- `docs/architecture-audit/findings/auto-resolution-backtracking.md`:
  - **M-002, M-014** → state **WIRED** (resolved by task 3558, `8d1cf09598`) —
    correct the stale PARTIAL/FICTION rows; strip the "no task owns the
    orchestrator call-site" note.
  - **M-013** → state **WIRED** with this PRD's α/δ task IDs.
  - **M-005, M-006** → state **WIRED** (γ joint-recheck soundness fix; hoists
    reverted).
  - **M-007** → state **WIRED** (β real-source exercise; `auto_backjumping_real_source`).
- Task **3522** → `done`, `done_provenance {kind: merged, commit: 8d1cf09598}`,
  note "superseded by 3558; residual eval signal re-homed to
  auto-type-param-resolution-completion α/δ."
- Task **3751** → `remove_dependency 3522` + `add_dependency α`.
- Task **3637** → cite as the diagnostic-self-documentation predecessor; the
  hoist-revert it tracked is delivered by γ (do not re-open).
- `docs/prds/auto-type-param-resolution.md` → `## §0 — Superseded by
  docs/prds/v0_3/auto-type-param-resolution-completion.md`.

End of PRD.
