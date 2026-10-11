# PRD — Material & waste cost minimisation: geometry-coupled candidate sweep + rectangular nesting (M-WASTE)

> **Status:** active — expanded from the 2026-06-24 forward-stub on 2026-10-10/11, when milestone
> #4787 dispatched with all preconditions landed (esc-4787-3; Leo ruled option A, "expand now").
> Authored via `/prd` as an agent team (five investigation seats and an adversarial critic whose 24
> findings are folded in); the design forks were ruled by Leo on 2026-10-10 (§3, Q-A–Q-E).
> Decomposed 2026-10-11 by a sibling `/prd decompose` session into α #8487 … ω #8504 (ids in §8);
> capability manifest `material-waste-cost-minimisation.capability-manifest.{md,yaml}`, whose
> "Decompose-time corrections" section records the fixture/substrate refinements found at decompose.
>
> **Milestone:** v0_6. **Cluster:** `cost-optimisation` (PRD 1 → F-result → PRD 2 → M-WHOLE →
> M-UNITS → **M-WASTE**, the program's eventual goal). **Approach:** B + H — a new build-level
> resolution layer above the §3.5 ConstraintSolver seam, blast radius ≥ 5 crates plus the GUI,
> ≥ 8 mechanisms. Contract in §4, boundary tests in §5.
>
> **Code anchors** are cited by symbol. Substrate was read and probed on main `3bfae4d2e0`–`a030ee7528`
> (2026-10-10) with a debug build of `3bfae4d2e0`; any `path:line` below is a dated snapshot of that
> range unless attributed to another SHA. Main moves fast — re-locate lines at implementation time.
> Baseline evidence is committed as `tests/prd-gate/fixtures/mwaste_*.ri`.

---

## §0 — Purpose, provenance and supersession

A designer wants the *cheapest buildable* part: material is bought as stock (a sheet, a blank), cut,
and the offcut is paid for too. Every one of those quantities is a function of **realised geometry**
— kernel volume, bounding extents, how flat parts pack onto a sheet — and today none of them can
steer a solve. Worse, the failure is not uniform: a continuous auto behind a geometry objective
leaves `undef` and a misleading "objective expression evaluated to undefined"; a **finite** auto
behind the same objective is resolved *silently and wrongly* (CP-SAT finds the objective unscorable
and returns its first enumerated model — the larger part, exit 0, no diagnostic); and an objective
that reads geometry but no auto is handed to an unrelated component and breaks it (§2.3). This PRD
makes geometry-dependent cost a first-class objective and geometry-dependent predicates first-class
constraints for every auto whose domain is finite, and makes every other case loud.

**What ships.**

1. **Geometry-coupling classification** — a standalone pass, run on every eval path (cold, cached,
   warm, with or without solver or kernel), that finds the scope whose objective or constraints read
   a post-realisation value (`volume`, `area`, `bounding_box`, `centroid`, mass properties,
   `Physical.mass`, cross-sub geometry cells, the nesting result), computes the autos that reach it
   by a deterministic cone walk, removes that scope's objective from every residual solve, and either
   hands the scope to the sweep or refuses it with a coded diagnostic naming the auto, the read and
   the body.
2. **Auto pinning** — an engine overlay that fixes chosen autos to values and makes every auto-kind
   predicate, and every consumer of the value (per-instance sub overlays included), treat them as
   determined.
3. **The candidate sweep** — the outer layer of PRD 2's "outer enumeration wrapping an inner solve"
   (§3 Q-A): enumerate the coupled scope's finite domain with `CpSatSolver::solve_all` over its
   non-geometric constraints, evaluate each candidate as a pinned cold eval + realisation, score it
   by its objective over post-realisation values, reject it on any violated constraint, rank, and pin
   the winner into the final build — with P3-honest completeness and optimality through the landed
   F-result and P3 carriers and the existing optimality gate.
4. **Honest verdict diagnostics** — ties, partial sweeps and unevaluated candidates reported per
   P3 §3.4's ratified policy.
5. **Every solving path** — `reify eval` and the GUI (load, commit, Export); warm GUI edits keep the
   last winner and say so (§3 Q-D).
6. **Waste vocabulary** — exact (unpadded) bounding extents and stdlib **traits** carrying blank /
   offcut / extent lets, plus a `SheetStock` record (§3 Q-C).
7. **Rectangular 2D nesting** — a deterministic builtin that groups sheet parts by (material,
   thickness) and packs their flat footprints onto priced sheet stock (MaxRects, 0°/90°, kerf), with
   a descendants form mirroring `cost(self.descendants)` (§3 Q-E).
8. **The vertical slice** — committed CI examples for a catalog stock pick and a nesting-driven sheet
   choice, plus docs-truth.

**Supersession.** This document replaces the forward-stub (`ba1bb92551`, `0daca3902c`) wholesale.
Three stub claims were false on 2026-10-10 and are corrected, not carried:

- *"The realisation cache is flushed wholesale on every param edit"* (the stub's anchor
  `engine_edit.rs:746`, as of `ba1bb92551`). Keyed `RealizationCache::evict_family` has replaced that
  flush at the `edit_param` / `edit_source` sites since #6925 (`e596b62677`); only
  `reset_geometry_for_reload` still clears wholesale.
- *"F-cache (input-cone-keyed eviction) makes the outer loop tractable — candidates that differ only
  in non-geometric params reuse geometry."* **False premise (G6 branch 3).** `RealizationCache` is
  keyed `(ReprKind, entity, options_hash)` with one slot per entity — not content-addressed — so an
  A→B→A candidate sequence misses on the second A; it is consulted at all only when a tolerance is
  demanded (`execute_realization_ops` gates both `lookup` and `insert_terminal` on `demanded_tol`);
  and `input_cone_hash` is a staleness stamp, not a key. The sweep does **not** depend on the
  eviction surface (§2.2, §3 decision 4).
- The stub's decomposition (α outer-sweep mode · β material vocabulary · γ waste model · δ
  outer-loop ↔ F-cache incrementality · ε example) is replaced by §8; its δ is dropped (previous
  bullet). Its first deliverable — "a design doc choosing the outer-loop architecture (sweep
  granularity, cache-reuse strategy, ranked-result integration)" — is §3 + §4 of this document.

**Coordination with #6925 (eviction backstop, ruled 2026-10-10).** The sibling session's ruling
(option E, #8459, landed) reverts `edit_param`'s scoped classification fold to the full fold; the
keyed primitive (`evict_family`, `realization_cone_seeds` / `stale_realization_entities`, the
`input_cone_hash` stamp) is unchanged. The sweep consumes neither the realisation cache nor
`evict_family`, so neither #8459 nor the open #8460 (build-stamped cones over-evict geometry-query
consumers on the first post-build edit) is a dependency; #8460 is a trigger condition for the
deferred realisation memo (§9).

### §0.1 — What this is NOT

- **NOT continuous geometry-in-the-loop.** A continuous auto that reaches a geometry read of the
  coupled scope is refused with a coded Error suggesting a finite domain (`x in range(…)` / a
  catalog). The continuous inner layer — P2's `GeometryDispatch` (#6683) plus a re-realisation arm —
  is bookmark ι (§8), design-first and dep-gated (§3 Q-B).
- **NOT cross-template or multi-instance sweeps.** v1 sweeps one coupled scope per module,
  instantiated once, whose sweep set lies in its own autos; anything wider is refused with a code
  (§3 decision 2, Leo 2026-10-10). The two widenings are bookmarks μ (multi-instance) and ν
  (cross-template), gated on instantiation-value-flow.
- **NOT a second enumeration engine, objective fold or optimality gate** (§3 decision 3).
- **NOT the `solutions()` / alternatives surface.** The sweep ranks a candidate set, but the engine
  surfaces the winner only, exactly as every other solve path does. A user-facing alternatives list is
  PRD 2's bookmark θ / P3's ζ #6711 territory.
- **NOT true-profile nesting.** Irregular-profile (no-fit-polygon) nesting, arbitrary rotations and
  common-line cutting are bookmark λ.
- **NOT a BOM/waste report.** `io-lifecycle-bom-cost.md`'s `reify report --bom` roll-up of declared
  `Discard` occurrences is a separate, report-shaped surface; this PRD computes waste as a geometric
  quantity for optimisation and does not touch the report.
- **NOT a change to `reify check` / `reify build`'s solver-free posture** (§2.3): no auto resolves
  there today, coupled or not, and that is unchanged.
- **NOT MINLP, NOT a seeded global optimiser** (M-WHOLE §3.1, binding).
- **NOT a change to `SolveResult`, `ConstraintSolver::solve()`, or the F-result carrier** (I1). One
  additive `PartialReason` variant is added to P3's carrier (§4.3, §7).

---

## §1 — Goal and consumers (G1)

### §1.1 — What a user observes

A flat bracket chooses its stock and finish (illustrative — reachable once #5470 and the
catalog-membership batch land; η's examples are the binding fixtures):

```
module bracket_stock
type MassCostRate = Money / Mass
type ArealCostRate = Money / Area

enum WidthClass { Narrow, Wide }

structure def Bracket : Stocked {                         // Stocked: §4.5 (Physical + extent/blank lets)
    param thickness : Length = auto
    constraint thickness in standard_sheet_thicknesses()  // catalog membership (#8355–#8357)
    param width_class : WidthClass = auto
    let width = if width_class == WidthClass.Wide then 80mm else 50mm
    param geometry : Solid = box(width, 120mm, thickness)
    param material : Material = Material(name: "Steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
    param steel_price : MassCostRate = 3.2USD / 1kg
    param coating_price : ArealCostRate = 40USD / 1m^2
    constraint volume(geometry) >= 30000mm^3               // enough material
    minimize blank_volume * material.density * steel_price + area(geometry) * coating_price
}
```

`reify eval` enumerates every (thickness, width class) pair, realises each, discards those below the
material floor, and resolves the cheapest — a thin wide plate when steel is dear relative to
coating, a thick narrow one when coating is dear — printing one `I_GEOMETRY_SWEEP` line (candidates
enumerated / evaluated / feasible / rejected by the volume floor) and staying silent on optimality
because the enumeration was complete (`ProvenOptimal`). Raising the floor until nothing qualifies
gives `E_GEOMETRY_SWEEP_INFEASIBLE` naming the floor. Declaring `thickness` a plain continuous
`Length = auto` gives `E_GEOMETRY_COUPLED_CONTINUOUS` naming `thickness`, `volume`, `area` and
`geometry`, with the `in range(…)` hint — on `reify eval`, `reify check` and in the editor alike. In
the GUI the winner is what renders; dragging `steel_price` keeps the rendered winner and shows
`I_GEOMETRY_SWEEP_STALE` until the edit is committed. A three-plate assembly picks its sheet stock by
minimising `nest_sheet_parts(...).cost`.

### §1.2 — Consumers

| Mechanism | Consumer (G1) |
|---|---|
| Classification pass + coded refusal (α) | Every eval path. `reify eval` / GUI: the refusal or the not-resolved cause replaces today's NoProgress, residual-1.0, silent first-model pick and objective mis-routing (§2.3). `reify check` / LSP / `reify test` / reify-cli MCP: the continuous refusal (Error) and, for finite scopes, `W_GEOMETRY_COUPLED_NOT_RESOLVED` with its reason. Consumed in-engine by the sweep (γ) as its work-list. |
| Auto pinning (β) | The sweep (γ) — sole writer of pins; the warm-edit path (δ) and Export read them. Not user-surfaced. |
| Candidate sweep (γ) | `reify eval` (γ) and the GUI load / commit / Export (δ) — the paths that run a solver with a kernel. User surface = resolved values + `I_GEOMETRY_SWEEP` + the existing `W_SOLVER_OPTIMALITY_UNPROVEN` gate. |
| Verdict diagnostics (γ2) | `reify eval` / GUI: strict-tie Error, free-tie Warning, partial-sweep Warning per P3 §3.4. |
| GUI paths + warm honesty (δ) | GUI load / commit / Export run the sweep; slider drag (`preview_parameter` → `edit_check` → `edit_param`) keeps the winner and surfaces `I_GEOMETRY_SWEEP_STALE` through the eval-diagnostics channel #6724 adds. The GUI debug-MCP `reify_set_parameter` is a commit (`commit_parameter` → `update_source`), so it re-sweeps. |
| Exact extents + `Stocked` / `SheetPart` traits + `SheetStock` (ε) | `.ri` authors writing blank / offcut / stock-fit cost; consumed by ζ (footprints) and the η examples. |
| Nesting builtin + status pass (ζ) | `.ri` objectives (`minimize nest_rectangles(...).cost`); the η nesting example. |
| Descendants nesting form (ζ2) | `.ri` objectives (`nest_sheet_parts(filter(self.descendants, SheetPart), …)`), mirroring `cost(self.descendants)`. |
| CI examples + harness e2e (η) | CI (examples smoke + `harness_engine.rs`); the integration gate for §5. |

**Engine-integration sub-check (G1).** Classification (α) and pinning (β) extend problem
construction inside the **§3.5 ConstraintSolver** seam's engine-side callers (`build_solver_problem`,
`build_merged_solver_problem`, the warm builders in `engine_edit.rs`) and add no solver. The sweep
(γ) is a **new seam**: a build-level resolution layer that re-enters the engine's own eval +
realisation per candidate. Per `engine-integration-norm.md` §3.10 ("new seams added by the PRD that
introduces them, in the same commit as that PRD's resolution"), it is catalogued as **§3.12 —
build-level geometry-coupled resolution seam** in this PRD's own landing commit, its call site named
as γ's entry. The stock lets (ε) and the nesting builtin (ζ) are value expressions over geometry
queries that ride the existing **§3.1 op-execute / `GeometryKernel` query** path, as
`fits_build_volume` rides `BoundingBox` — no seam change. No orphan-producible `pub fn` in a kernel
crate.

---

## §2 — Background & substrate (verified 2026-10-10)

### §2.1 — The objective and the solver never see kernel output

- **Objective path.** `minimize` lowers to an `ObjectiveSet` of `ObjectiveTerm{sense, expr, weight,
  priority}`; `build_solver_problem` (`crates/reify-eval/src/engine_eval.rs`) places it in the
  owned, lifetime-free `ResolutionProblem`; `eval_objective_set`
  (`crates/reify-constraints/src/solver.rs`) evaluates it with `reify_expr::eval_expr` over a
  `ValueMap` — the single objective fold, shared by the Nelder-Mead cost surface and both
  `solve_ranked` scoring paths (P3 preserves this; so does this PRD). The only engine→solver
  callback is `reify_ir::ComputeDispatch` (`OptimizedComputeDispatcher`, value-only, empty
  `realization_inputs`).
- **Order of operations.** `Engine::eval` resolves every auto (resolve order → per-template or
  merged-cluster solve → write-back) **before any kernel call**, then mints symbolic geometry
  handles. Realisation, geometry-query post-processing and the post-geometry constraint re-check
  come later in `build_with_geometry_output` (`execute_realization_ops` → `run_post_processes` →
  `check_constraints_post_geometry`). Geometry queries are therefore evaluated **only after** the
  autos are frozen.
- **What the kernel gives.** `volume` → OCCT `BRepGProp::VolumeProperties` (exact B-rep
  integration): smooth within a fixed topology, kinked where topology changes, **bit-identical run
  to run** (5/5 identical over a 20-body fixture; the analytic `3000 − 25π mm³` matched to every
  printed digit).

### §2.2 — The realisation cache, eviction, and why the sweep does not use them

- `RealizationCache` (`crates/reify-eval/src/realization_cache.rs`) is keyed
  `(ReprKind, entity_id, options_hash)` with a tolerance bucket — **one slot per entity**, not
  content-addressed. `execute_realization_ops` probes and inserts it **only when `demanded_tol` is
  `Some`**, which `compute_demanded_tols` yields only for a declared representation bound or an
  active purpose tolerance.
- `RealizationCache::evict_family` (keyed eviction, #6925) runs at the `edit_param` / `edit_source`
  compare sites and in `refresh_and_gate_demanded_realizations`; `clear_realization_cache` has one
  production caller, `reset_geometry_for_reload`. #8459 (2026-10-10) reverts `edit_param`'s scoped
  classification fold to the full fold; the primitive is unchanged.
- `evict_family` frees cache entries only; native OCCT shapes are freed only by
  `GeometryKernel::reset`, which `reset_geometry_for_reload` calls (together with clearing the
  cache) — the existing, correct per-candidate memory bound.
- The engine's one content-addressed cache is the persistent **ComputeNode** cache
  (`Engine::persistent_cache_key`), which does not cover realisations.

Consequence: a per-candidate cold eval + realisation gets no cross-candidate reuse from any
existing cache, and needs none to be correct. Reuse is a deferred optimisation (§9).

### §2.3 — Empirical baseline (probes 2026-10-10, debug build of `3bfae4d2e0`)

Committed fixtures under `tests/prd-gate/fixtures/`; each row is `reify eval` unless stated.

| Fixture / probe | Today |
|---|---|
| `mwaste_coupled_continuous.ri` — `w : Length = auto` in `[20,60]mm`, `minimize volume(box(w,30mm,10mm))` | rc 1: `warning: Constraint solver made no progress: objective expression evaluated to undefined at solution point`; `w = undef`; `error: failed to compile geometry operation: argument 'width' for box is unresolved (Undef)`. `reify check` rc **0** with the same `error:` lines (INV-SF-2; the geometry-awaiting-solve path is owned by #5404, the residual inventory by #7308). |
| same with `let v = volume(body); minimize v` | same, plus `note: … v is undef (because: op contract failed (OpContractViolation))` — a wrong cause (INV-SF-1; owned by #5197). |
| Money objective `volume(body) * rho * price`, plain and via `Physical.mass` | rc 1, `undef`, NoProgress; the robustness-floor info line still prints. |
| `mwaste_coupled_bool.ri` — strict `wide : Bool = auto` choosing a 40 vs 20 mm width, `minimize volume(body)` | rc **0**, `wide = true` — the **larger** volume — with **zero** diagnostics (`auto(free)`: only the generic not-uniquely-determined warning). CP-SAT finds the objective unscorable and returns its first enumerated model as feasibility-only. A silent wrong answer (INV-SF-3). |
| `mwaste_let_constraint.ri` — `let v = volume(body); constraint v >= 6000mm^3` | rc 1: `error: constraints could not be satisfied (max absolute residual: 1.00e0)`, and **no** `E_EVAL_UNRESOLVED` — the δ trace predicate (`constraints_reaching_auto`) sees only inline queries over a direct `ValueRef`. The inline form `constraint volume(body) >= …` does get `E_EVAL_UNRESOLVED`. |
| `mwaste_objective_fallback.ri` — `wide` concrete, `minimize volume(body)`, an independent `x : Length = auto` in `[2,9]mm` | rc 0 but `x = undef` with NoProgress: an objective whose reads reach no auto is attached to component 0 (`ObjectiveConsumption::FallbackComponentZero`) — an unrelated component — which then fails. |
| A sub-object objective (`minimize self.a.mass`, the sub sized by a parent Bool auto through a ctor arg) plus an independent auto, run 6× | alternates between a solved result and all-`undef` (3 of 6): the objective→component assignment iterates a `HashMap` (`DecompositionPrelude`) — the hash-order leak #6691 owns. |
| `mwaste_parent_auto_to_sub.ri` — parent `thin : Bool = auto` → `let thick` → `sub a = Plate(t: thick)` | `Panel.thin = true`, `Panel.thick = 0.002 m`, but `Panel.a.t = undef` and `error: per-instance re-realization compile error for Panel.a.geometry: argument 'depth' for box is unresolved (Undef)`: a **solved** auto never reaches a sub's ctor-arg overlay (instantiation-value-flow #6631 / #6657). A **concrete** parent param does reach it. |
| `mwaste_fn_solid_no_fold.ri` — `fn vol_of(g : Solid) -> Volume { volume(g) }` | `v_fn = undef` ("op contract failed") while inline `volume(body) = 0.000012 m^3`: a geometry query inside a user `fn` never folds post-realisation (`geometry_ops::rewrite_geometry_queries` does not descend into fn bodies). |
| Same models with every auto concrete | correct values (`mass = 0.0942 kg`, `cost = 0.1884 USD` for 40×30×10 mm steel at 2 USD/kg); concrete geometry constraints are enforced post-hoc (`VIOLATED`). |
| `bounding_box` of a 40×30×10 mm box; extents via `dot(bbox_size(bb), vec3(1,0,0))` | evaluates; every extent padded by 2e-7 m (`ex = 0.0400002 m`) — `query_bbox` uses `BRepBndLib::Add`, which adds shape tolerance — so a perfectly fitting blank shows `offcut ≈ 3.2e-5 × volume`. `bbox_size(bb).x` → `member access not yet supported` (#6892). |
| `constraint t in standard_sheet_thicknesses()` | grammar gate rc 1 (3 ERROR nodes at `in`) — #8355 (pending). |
| `@solver_hint("discrete_set", standard_sheet_thicknesses)` on `t`, closed-form cost | resolves `t = 2.244 mm` — **not a catalog member**; identical with no hint (until #8356/#8357). |

Concrete-parameter geometry cost is already correct; the gap is that no resolution path evaluates the
model *at a candidate* before choosing — and that the failure modes around it are silent or
mis-attributed.

**Which paths solve at all.** `reify eval` (geometry modules: `configured_eval_engine` +
`realize_for_check`) and the GUI (load / commit: `check_with_solve_slot` then `tessellate_snapshot`;
Export: `engine.build`) run a solver with a kernel. `reify check` and `reify build` run **without** a
solver by design (`cmd_build`'s documented posture), so every auto is unresolved there today;
`reify test`, the LSP and the reify-cli MCP (`ensure_engine`) run with neither solver nor kernel. The
GUI load path never reaches `build_with_geometry_output`, and the GUI surfaces **no** eval-time
diagnostic today (#6724).

### §2.4 — Landed and pending substrate this PRD builds on

- **F-result carrier (landed, #4801/#4803/#4804)** — as on main, which differs from
  `ranked-solve-result.md` §3.1: `RankedSolveResult::Ranked { candidates, optimality,
  completeness }`; `BestFoundReason ∈ {IterationLimit, EnumerationBudget, ConvergedWithinBudget,
  Unreported}`; `W_SOLVER_OPTIMALITY_UNPROVEN` fires iff `BestFound{reason}` with
  `reason.stopped_at_budget()` (`optimality_unproven_finding` in `engine_eval.rs`). Both engine
  seams surface `candidates[0]` only.
- **P3 completeness carrier (landed, #6706)** — `Completeness::{Exhaustive, Partial{reason},
  Refuted}`, C1 (`unique` derived only via `Completeness::derived_unique`), C2 (`ProvenOptimal`
  requires `Exhaustive`, `permits_proven_optimal`). P3 maps a capped CP-SAT enumeration to
  `Partial { BoxBudgetExhausted }` (P3 §3.6 table); `NotAttempted` is a migration state the sweep
  must never emit. The `Refuted`-in-`Ranked` seam is open (P3 #6900).
- **PRD 2 (CP-SAT, landed in part)** — `CpSatSolver::solve_all(problem, cap) -> SolveAllResult
  { solutions, complete }` and the `solve_ranked` argmin override (#5468) and default-ON wiring
  (#5469) are landed; `AutoParam.domain` + `Int` bound-mining (δ #5470), the integration gate
  (#5471) and the mixed outer loop (ζ #5472) are pending.
- **Catalog membership** — `constraint x in C` (#8355 α), membership domains in the solver layer
  (#8356 β), `discrete_set` as its sugar (#8357 γ): all pending. `AutoParam` has **no** `domain`
  field at HEAD (`id, param_type, bounds, free`), `build_auto_param_list` sets `bounds: None`, and
  CP-SAT mines an Enum domain from variant literals *in the constraints it is passed* — so today only
  `Bool` enumerates as-is; an Enum enumerated over a stripped constraint set would narrow. #5470 is
  therefore a hard prerequisite of the sweep (§6).
- **Auto pinning precedent** — `connector_pin_if_determined` (`engine_eval.rs`) excludes a
  determined connector from `auto_params` and writes it as determined — but only inside
  `build_solver_problem` and only under `has_active_solver`, and the merged builder errors on pins.
  There is **no** API to pin an *auto* to a chosen value: `param_overrides` covers `Param` cells only
  (both solvers overwrite an override on an auto), and some two dozen sites test `kind.is_auto()`
  directly (§4.2).
- **No sweep / DoE / optimise machinery exists** (`reify --help`; the `sweep()` builtins are
  kinematic snapshot sweeps; `reify report --bom` is a kernel-free value roll-up).

### §2.5 — The P2 relationship (why the inner layer waits)

`geometry-algebra-solver-unification.md` (P2) §7.6 designs the kernel-query arm of the unified
continuous core: a `GeometryDispatch` trait (declared in `reify-ir`, implemented in `reify-eval`,
consumed by `reify-constraints` — `ComputeDispatch`'s shape) answering `reify_ir::GeometryQuery`
over already-realised bodies whose **pose** varies ("arm A only"). It names per-trial
**re-realisation** — "M-WASTE's regime" — out of scope with this PRD as successor, and calls its
trait "that loop's inner half". P2 is almost entirely pending (only ε #6672 landed); leaf ο #6683
waits on most of P2 plus #6251, and its wired hard dep #6583 is **cancelled** as a duplicate of the
still-pending #5436, so ο's real gate is #5436 and the wired edge is stale (reported to Leo
2026-10-10; not this PRD's to fix). This PRD therefore builds the **outer** layer now and leaves
the continuous inner layer (arm B on P2's trait) as bookmark ι.

### §2.6 — Grammar and language substrate (G3)

No novel syntax. Every fixture shape is ordinary member/expression grammar: `param … = auto`,
`constraint`, `minimize`, `let`, `enum`, structure constructors, function calls, `type` aliases for
compound units (`type MassCostRate = Money / Mass` — a direct `param x : Money / Mass` is a parse
error, by design). A not-yet-registered builtin such as `nest_rectangles(...)` parses; note it then
yields only a **Warning** (`unresolved function`) and an `undef` let (#5997 owns the Error flip), so
no RED test may assert a hard error for an unregistered name. The `in` operator the catalog examples
use is #8355's grammar, a **hard upstream dependency** of η, never assumed. `grammar_confirmed =
true` for every leaf.

Language facts the design is built around (all measured 2026-10-10):

- A geometry query inside a user `fn` taking a `Solid` never folds post-realisation
  (`mwaste_fn_solid_no_fold.ri`) — so every stock helper in this PRD is a **trait `let`** with its
  own geometry leaf, never a `fn`, and a part's geometry must be a structure `param`/`let`, never a
  `fn` returning a `Solid`.
- Post-realisation, a child `let` folds at its scoped id only if it contains a geometry leaf itself;
  a pure child `let` chained over sibling child lets stays `undef` (a parent-level `let` over the
  same values folds via `post_process_derived_lets`). Every per-part extent let therefore carries its
  own `bounding_box(geometry)` leaf.
- `self.descendants` evaluates to entity-path strings; members cannot be read through it (silently
  `undef`). Only the `cost(self.descendants)` intercept and its `apply_cost_aggregation` rewrite turn
  it into per-child reads — the precedent ζ2 clones.
- Lists of structure records work (`[SheetStock(...), …]`, `cat[i].price`, `generate(n, |i| …)`,
  `.sum`, `.count`); `map` / `fold` / lambda `filter` / `max(list)` do not (#5393). A `USD` literal
  inside a free `fn` body does not resolve (#8173), so example catalogs are built in structure `let`s
  or take a `Money` parameter.

---

## §3 — Resolved design decisions

**Leo's rulings, 2026-10-10** (asked as menus after the investigation):

- **Q-A — Architecture: the build-level candidate sweep, shipped now as the outer layer.** Chosen
  over "geometry oracle inside the solver" and over "wait for P2 ο and add a re-realisation arm",
  after a ramifications discussion. The two are not alternatives but the two layers of PRD 2's
  binding shape (M-WHOLE §3.1: outer enumeration wrapping an inner solve). The **outer** layer
  carries anything a build computes — including non-smooth and combinatorial quantities such as
  nesting yield, which a finite-difference Jacobian sees as identically zero — and gives honest,
  provable optimality over a declared finite domain. The **inner** layer (P2's `GeometryDispatch`
  plus a re-realisation arm) gives local continuous shape optimisation, polynomial in the number of
  continuous dims but never provable and never able to carry nesting. Shipping the outer layer
  forecloses nothing: the v1 continuous refusal becomes a routing decision when the inner layer
  lands (bookmark ι). The one long-term discipline is in the contract: both layers report through the
  same carriers and composition law, and never duplicate the enumeration engine.
- **Q-B — Finite domains only in v1.** A continuous auto in the sweep set gets the coded refusal of
  §4.4, naming the auto and suggesting `x in range(…)` or a catalog. Continuous search is bookmark ι.
- **Q-C — Waste model = blank + catalog fit + 2D nesting.** Blank / offcut lets over exact extents;
  stock fitting is a catalog auto with a containment constraint (`stock_t >= extent_z`), chosen by the
  same sweep; plus nesting (Q-E).
- **Q-D — Warm edits keep the winner.** A warm GUI edit does not re-sweep: swept autos keep the last
  full-build winner and a coded info diagnostic says the optimum is stale; a full rebuild re-sweeps.
- **Q-E — Nesting = rectangular footprints, grouped.** Deterministic rectangle bin-packing (MaxRects,
  0°/90° rotation, kerf) of each sheet part's flat footprint, grouped by (material, thickness),
  against a priced sheet-size catalog; true-profile nesting is bookmark λ.

**Derived decisions** (analysis converged; each is pinned by a §4 clause and a §5 row; the critic's
findings that forced them are folded in):

1. **Classification is a standalone pass over post-realisation reads, on every path.** A
   *post-realisation read* is any expression node with no value until realisation has run (§4.1). A
   template is *geometry-coupled* iff its objective or a constraint contains one after `let`
   expansion. The pass runs inside `eval` / `eval_cached` and inside the warm builders of
   `edit_param` / `edit_source` — **with or without a solver** — so the same classification answers
   on `reify eval`, `reify check`, the LSP, `reify test`, the MCP and GUI drags. It never uses the
   solver's component decomposition: that iterates a `HashMap` (#6691) and cannot see sub ctor-arg
   edges (§2.3); the pass walks the read cone deterministically instead.
2. **Sweep set and v1 scope.** For the coupled template *T*, the sweep set *S* = every auto reached
   (through `let`s, dependent cells, sub ctor-arg edges and realisation inputs) by *T*'s objective
   **or** by a post-realisation-reading constraint of *T*. v1 requires: exactly one coupled template
   in the module; *T* instantiated once (the root structure or a single sub instance); *S* ⊆ *T*'s own
   autos. Otherwise `E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED` names the templates / instances / foreign
   autos. **Ruled by Leo (2026-10-10): the conservative rule, plus a bookmark for each widening** —
   μ (a coupled template instantiated more than once: per-instance pins and scoring) and ν (child autos
   joining a parent's sweep set), both gated on instantiation-value-flow #6592 / #6657. **On every eval, pinned or not, *T*'s
   objective is removed from every residual `ResolutionProblem`** — it is the sweep's, and is never
   routed through `FallbackComponentZero`. Autos outside *S* solve per candidate exactly as today,
   with no part of *T*'s objective.
3. **One machinery.** Assignments come from `CpSatSolver::solve_all` over *S* with *T*'s
   post-realisation-reading constraints removed (non-prunable at enumeration time, judged per
   candidate — PRD 2 §4.3's role for continuous-reading constraints in ζ) and no objective; the
   candidate score is *T*'s expanded `ObjectiveSet` through `eval_objective_set`; the optimality
   warning is the existing `optimality_unproven_finding`; ranking reuses CP-SAT's. No second
   backtracker, fold, gate or ranking (`no-lockstep-duplication`).
4. **Candidate = whole-model pinned cold eval + realisation.** Correct by construction (it is what
   `reify eval` does for a concrete model). No dependency on the realisation cache or keyed eviction
   (§2.2); OCCT memory is bounded by `reset_geometry_for_reload` between candidates. Cheaper candidate
   evaluation is deferred with trigger conditions (§9).
5. **Candidate outcomes are three, not two.** *Feasible*: every constraint of *T* is `Satisfied` in
   the candidate's post-geometry check and its score is defined. *Rejected*: some constraint is
   `Violated` (evidence: which). *Unevaluated*: a constraint is `Indeterminate`, realisation failed,
   the score is `Undef`, or a residual solve in *T*'s scope did not reach `Solved` — cause recorded
   and counted, never selected, never confused with infeasibility.
6. **Completeness and optimality honesty** (P3 law; the sweep is the outer enumeration, each
   candidate a leaf whose *S*-autos are exact):
   - `Completeness::Exhaustive` iff the enumeration completed within budget, no candidate was
     *unevaluated*, and *T* has no residual auto constrained jointly with *S* (whose per-candidate
     solve is not a proof). Then `ProvenOptimal` (C2): argmin over the declared domain **of the
     objective as computed** (for a nesting objective, the deterministic packing's cost — §4.6).
   - Capped ⇒ `Partial { BoxBudgetExhausted }` (P3's mapping for capped CP-SAT) +
     `BestFound { EnumerationBudget }` ⇒ the existing `W_SOLVER_OPTIMALITY_UNPROVEN` fires once.
   - Any unevaluated candidate ⇒ `Partial { CandidatesUnevaluated { count } }` — a new, additive
     `PartialReason` variant γ adds to P3's carrier (§7) — with `BestFound { ConvergedWithinBudget }`
     (no budget stop: honest, and silent on the optimality gate by design — the Partial is surfaced
     by decision 7's policy instead). A residual-coupled scope ⇒ `Partial { InnerSolveUnproven }`, same
     reason and surfacing.
   - No feasible candidate, none unevaluated, enumeration complete ⇒ `E_GEOMETRY_SWEEP_INFEASIBLE`
     naming the constraint(s) that rejected the most candidates (worded "none found within budget"
     when capped). No feasible candidate with any unevaluated ⇒ `E_GEOMETRY_SWEEP_UNEVALUATED`,
     naming the causes — never reported as infeasibility. Neither rides the `Ranked` carrier, whose
     `Refuted` seam is still open (P3 #6900).
   F-result §0.1 granted `ProvenOptimal` to PRD 2 alone because PRD 2 was the only complete
   enumerator when it was written; P3 C2 since made `Exhaustive` the criterion. This PRD's landing
   commit adds a note to F-result §0.1 accordingly.
7. **Verdict → diagnostic follows P3 §3.4 (Leo, 2026-08-26), through one policy function.** Strict
   auto with several tied minimisers ⇒ Error naming them; `auto(free)` with ties ⇒ coded Warning naming
   n and which was taken; `Partial` ⇒ coded Warning carrying the reason. #6901 owns that policy for
   the solver paths; whichever of #6901 and γ2 lands first implements the §3.4 policy function, and the
   other adopts it (the precedent #6901's own record sets with #6678). No second copy. Ties are exact
   `f64` equality of scores; tie order is enumeration order.
8. **Determinism.** The sweep builds its `AutoParam` list in declaration order (never from a
   `HashSet`), values in domain-construction order; ties broken by that order; budgets are candidate
   counts, never wall-clock (P3 C7); `solve_all` is called with `cap = budget + 1` so a domain of
   exactly *budget* candidates is reported complete; OCCT volume is bit-identical run to run (§2.1).
   Two identical runs produce bit-identical values, winner and summary.
9. **Candidate diagnostics are pruning evidence, not user output.** Per-candidate builds' diagnostics
   are dropped; the user sees the winner build's diagnostics plus one `I_GEOMETRY_SWEEP` summary.
   Mirrors PRD 2 §4.3.
10. **The winner is what is built and exported.** After a sweep, `sweep_pins` holds the winner; every
    later build on that engine (values, GUI mesh, STEP export) builds the pinned winner. Export does
    **not** re-sweep; a source change or whole-file reload clears the pins and re-sweeps.
11. **Where the sweep runs, and what everywhere else says.** The sweep runs on paths with a solver, a
    kernel and the `UnifiedDag` scheduler (`reify eval`, GUI). The classification pass reports
    everywhere: a continuous auto in *S* is `E_GEOMETRY_COUPLED_CONTINUOUS` (Error) **on every path**
    — it is structurally unresolvable in v1, so INV-SF-4 makes it an error, not a transient
    Indeterminate; a finite *S* on a path where the sweep cannot run is
    `W_GEOMETRY_COUPLED_NOT_RESOLVED` with a typed reason (no solver / no kernel / legacy scheduler /
    sweep cancelled / sweep not yet available) and an `UndefCause` on each auto. An engine-side
    `SweepState` (not attempted / swept / infeasible / unevaluated / not run {reason}) is the single
    record the pass consults, so nothing is double-reported. For constraints it classifies, the pass
    supersedes the δ predicate's `E_EVAL_UNRESOLVED` (one code per condition).
12. **Pins behave as concrete values on every consumer** — every auto-kind predicate, both problem
    builders, the warm builders, and the per-instance sub ctor-arg overlay — so a pinned parent auto
    sizes a sub's geometry exactly as a concrete parent param does (`mwaste_parent_auto_to_sub.ri`).
    Coordinated with instantiation-value-flow (#6631 / #6657), which owns the *solved*-auto overlay.
13. **Stock data is a type, not stdlib prices.** The stdlib gains a `SheetStock` record and the
    `Stocked` / `SheetPart` traits; it ships **no price list** (prices are supplier data).
14. **Exact extents.** The `bounding_box` padding is removed at its source (`AddOptimal` without shape
    tolerance), so `stock_t >= extent_z` admits a 2 mm part on 2 mm sheet; every existing consumer is
    re-verified (ε).
15. **The sweep's score is never floored.** Swept scopes are excluded from
    `detect_robustness_floor_applied` (its "floor applied" info would be false); residual inner solves
    keep PRD 1's floor exactly as today.
16. **Cancellation.** The GUI's per-solve `CancellationHandle` is honoured between candidates; a
    cancelled sweep pins nothing and reports `W_GEOMETRY_COUPLED_NOT_RESOLVED { cancelled }`.

---

## §4 — Contract (B + H)

### §4.1 — Geometry-coupling classification (α)

**Post-realisation read.** An expression node with no value until realisation has run: a
geometry-query call (`geometry_ops::is_geometry_query_call` — `volume`, `area`, `centroid`,
`bounding_box`), a geometry-consumer call (`is_geometry_consumer_call` — `center_of_mass`,
`moment_of_inertia`, topology helpers), a `body_mass_props` target, a cross-sub cell folded by
`post_process_cross_sub_value_cells` (`self.part.mass`, `self.part.extent_x`), a `Stocked` /
`SheetPart` extent / blank let (§4.5), a nesting builtin call (§4.6), or a `let` / dependent cell
whose value derives from any of these. The classifier is **not** a call-name list: mass properties
and folded cross-sub cells are not call-shaped.

**The pass.** One helper, `classify_geometry_coupling(template, …) -> CouplingReport`, called from
`Engine::eval` and `eval_cached` **outside** the `has_active_solver` gate, and from the
`edit_param` / `edit_source` warm entity-group builders (the same "one shared helper, every AutoParam
builder" shape #5470's ruling M uses). Over the template's expanded constraints, objective and
`dependent_cells` it finds post-realisation reads, then walks their read cone **deterministically**
(declaration order; through `let`s, dependent cells, sub ctor-arg edges and realisation inputs) to
the autos they reach — never through `decompose_into_components` / `decompose_and_classify`, which
iterate a `HashMap` and cannot see ctor-arg edges.

**The report.** `CouplingReport { template, sweep_set S (declaration order), reads (with bodies),
continuous: Vec<auto>, scope_violations }`, and the actions:
- *T*'s objective is removed from the residual `ResolutionProblem` in both builders
  (`build_solver_problem`, `build_merged_solver_problem`) and in the warm builders — always.
- *S*'s autos are excluded from residual solves (they are pinned by the sweep or reported).
- *T*'s post-realisation-reading constraints are excluded from residual solves and from the δ
  predicate's `declined` set (the sweep judges them; the δ predicate's `E_EVAL_UNRESOLVED` is
  superseded for them).
- Diagnostics per `SweepState` (§3 decision 11, §4.4).

Templates with no post-realisation read are untouched (W1).

### §4.2 — Auto pinning (β)

- **State.** `Engine.sweep_pins : BTreeMap<ValueCellId, Value>` and `Engine.sweep_state :
  SweepState`, MUST-SURVIVE rows in `reset_per_build_state`'s exhaustive classification
  (compile-enforced), distinct from `param_overrides` (which covers `Param` cells only and which both
  solvers overwrite for autos). Written only by the sweep (γ); cleared on source change / whole-file
  reload; never persisted.
- **One predicate.** A single pin-aware "is this an unresolved auto?" predicate. β's diff classifies
  **every** `kind.is_auto()` / `is_auto_cell` site in `crates/reify-eval/src` as pin-aware or
  deliberately not, in its own description. Known sites (2026-10-10): `EvaluationGraph::is_auto_cell`
  (→ `constraints_reaching_auto`), the auto pre-seeds (`evaluate_params_and_lets_unified`, the
  guarded-group arms in `eval` / `eval_cached`, `Snapshot::from_compiled_module`), both problem
  builders, `build_auto_param_list`, `build_combined_param_let_graph`, `CellReadIndex::build`,
  `scope_qualifies_for_centrality`, `detect_underdetermined`, `resolve_order`'s auto counting, the
  guard-deactivation arm and `engine_edit.rs::deactivate_if_not_auto`, `deps.rs`'s auto traces,
  `engine_admin.rs::module_has_auto_cells`, `invariants.rs`, the
  `re_eval_consumers_of_in_walk_mints*` family, and the `edit_param` / `edit_source` warm groups.
- **Applied as a concrete value at the pre-seed**, outside the `has_active_solver` gate (where the
  connector-pin precedent writes, which would leave pins inert on solver-less paths) and before child
  elaboration, so a pinned parent auto reaches a sub's per-instance ctor-arg overlay exactly as a
  concrete parent param does (§3 decision 12). The merged builder excludes pins too.
- **Warm edits never re-solve or override a pinned cell** (the mechanism behind Q-D).

### §4.3 — The sweep (γ)

**Entry.** An explicit engine entry — working name `Engine::resolve_geometry_coupled(&module) ->
SweepReport` — called by the caller **before** its normal path, never from inside
`build_with_geometry_output` (which `relate_solve::realize_structures` re-enters for sub-modules; a
sweep there would recurse). Callers: `cmd_eval`'s geometry branch, before `realize_for_check` (γ);
the GUI's `check_with_solve_slot` on load / commit (δ). Export builds the current pins (decision 10).
It runs iff a solver is active, a default kernel is registered and the scheduler is `UnifiedDag`;
otherwise it records `SweepState::NotRun{reason}` and returns.

**Steps.**
1. **Classify** (§4.1). No coupled template ⇒ return (no-op, W1). *S* empty ⇒ no sweep: the
   objective governs nothing and gets the existing `E_OBJECTIVE_INERT` from the pass. A scope violation ⇒
   `E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED`. Any continuous auto in *S* ⇒ `E_GEOMETRY_COUPLED_CONTINUOUS`
   (already emitted by the pass; the sweep does not run).
2. **Domains.** Every auto in *S* is finite through `AutoParam.domain` / `is_discrete()` (PRD 2 δ
   #5470: `Bool` intrinsic, `Enum` **declared** domain, `Int` mined, catalogs via #8356). An Enum
   domain is never mined from the stripped constraint set. An auto with no declared finite domain
   (including a data-carrying Enum that would fall back to literal mining) ⇒
   `E_GEOMETRY_COUPLED_CONTINUOUS` with reason "no declared finite domain".
3. **Enumerate** with `CpSatSolver::solve_all(problem, budget + 1)` over *S* (declaration order),
   with *T*'s post-realisation-reading constraints removed and no objective. An empty enumeration
   over a catalog-membership domain is #8356's `NoCatalogMemberFeasible`, not this PRD's error.
4. **Evaluate each candidate** in `solve_all` order, up to the candidate budget (§11 Q1), checking the
   cancellation handle between candidates: set `sweep_pins`; `reset_geometry_for_reload` (its "once
   per reload" doc contract is restated); run an internal `build_with_geometry_output(emit = false)`
   variant that, where `module_named_steps` and the kernel are live, (a) scores *T*'s expanded
   `ObjectiveSet` by folding each term with `geometry_ops::rewrite_geometry_queries` and evaluating
   through `eval_objective_set` (made `pub` — the single objective fold), (b) returns the post-geometry
   verdict of every constraint of *T*, and (c) returns whether each residual solve in *T*'s scope
   reached `Solved`. Classify the candidate feasible / rejected / unevaluated (§3 decision 5).
5. **Rank** feasible candidates ascending by score, factoring CP-SAT's ranking (`ScoredModel`, tie
   counting, `verdict_from_enumeration` in `cpsat.rs`) rather than copying it.
6. **Verdict** per §3 decision 6, including the additive `PartialReason::CandidatesUnevaluated`.
   `unique` = `Completeness::derived_unique` over the minimiser set.
7. **Commit.** Set `sweep_pins` to the winner and `sweep_state = Swept{…}`, or record
   `Infeasible` / `Unevaluated` / `NotRun{cancelled}` and pin nothing. Return `SweepReport { S,
   winner, verdict, counts, rejections and unevaluations by cause }`. The caller's normal path then
   builds the winner; on that build `optimality_unproven_finding` (made `pub(crate)`) is called
   **once** with the sweep's `OptimalityStatus`, the §3.4 policy (γ2) emits tie / Partial diagnostics,
   `I_GEOMETRY_SWEEP` is emitted once, and *T* is excluded from `detect_robustness_floor_applied`.

### §4.4 — Diagnostic catalogue (coded; working names, §11 Q2)

| Code | Severity | When |
|---|---|---|
| `E_GEOMETRY_COUPLED_CONTINUOUS` | Error, every path | an auto in *S* has no declared finite domain; names autos, reads, bodies; hint `x in range(…)` / catalog |
| `E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED` | Error, every path | more than one coupled template; *T* instantiated more than once; *S* reaches another template's autos |
| `W_GEOMETRY_COUPLED_NOT_RESOLVED` (+ `UndefCause` per auto) | Warning | finite *S* on a path where the sweep did not run: no solver, no kernel, legacy scheduler, cancelled, or (before γ lands) sweep unavailable |
| `I_GEOMETRY_SWEEP` | Info | once per sweep: *S*, candidates enumerated / evaluated / feasible / rejected-by-constraint / unevaluated-by-cause, budget, verdict |
| `W_SOLVER_OPTIMALITY_UNPROVEN` (existing) | Warning | capped sweep, via the existing gate, once |
| §3.4 policy codes (with #6901) | Error / Warning | strict tie; free tie; `Partial{…}` (§3 decision 7) |
| `E_GEOMETRY_SWEEP_INFEASIBLE` | Error | complete, nothing unevaluated, nothing feasible; names the most-rejecting constraint(s) and counts |
| `E_GEOMETRY_SWEEP_UNEVALUATED` | Error | nothing feasible and at least one candidate unevaluated; names the causes and counts |
| `I_GEOMETRY_SWEEP_STALE` | Info | a warm edit touched an input of *T*; the winner is kept until the next full build (Q-D) |
| `E_NEST_NO_STOCK`, `E_NEST_PART_TOO_LARGE`, `E_NEST_INVALID_INPUT` | Error | §4.6 status pass |
| `W_NEST_SUB_NOT_NESTED` | Warning | ζ2: a collection sub or a deeper descendant conforming to `SheetPart` was not nested |

### §4.5 — Exact extents and stock vocabulary (ε)

- **Tight bounding box.** OCCT `query_bbox` (`crates/reify-kernel-occt/cpp/occt_wrapper.cpp`) calls
  `BRepBndLib::Add(shape, box)`, which enlarges the box by sub-shape tolerances (1e-7 m per face). It
  switches to `BRepBndLib::AddOptimal(shape, box, /*useTriangulation=*/false,
  /*useShapeTolerance=*/false)` — declared in the installed OCCT 7.8.1 header, and measured exact on a
  box (gap 0, 4.7e-15 relative on a cut solid). The Manifold kernel already returns a tight box. Every
  `GeometryQuery::BoundingBox` consumer is re-verified in the same diff: `dispatch_bounding_box`,
  `fits_build_volume` (DFM's exact compare), the `engine_build.rs` cap-vertex and new-edge-midpoint
  uses, `primitive_attribute_seed` extents, the topology-selector extremal z
  (`topology_selectors.rs`, `selector_vocabulary_v2.rs`), and the tests carrying a padding tolerance
  (`centered_primitives_e2e.rs`, the `harness_occt_measurement` extrude/loft tests). ε also times
  `AddOptimal` against `Add` on a BSpline-heavy example and records the ratio in its description
  (selectors call it). `is_bounded`'s own `Add` is out of scope (a predicate, not a measurement).
- **Stdlib `stock.ri`** (new module beside `standard_stock.ri`, registered in
  `crates/reify-compiler/src/stdlib_loader.rs` **before** `std.determinacy.purposes`, which must stay
  last):
  - `structure def SheetStock { param material : String; param thickness : Length; param width :
    Length; param length : Length; param price : Money }` — a priced sheet; `material` matches
    `Material.name`.
  - `trait Stocked : Physical { let extent_x : Length; let extent_y : Length; let extent_z : Length;
    let blank_volume : Volume; let offcut_volume : Volume }` — every let carries **its own** geometry
    leaf (`dot(bbox_size(bounding_box(geometry)), vec3(1,0,0))`, …; `blank_volume` is the product of
    three inline extents; `offcut_volume` is that product minus `volume(geometry)`), because a chained
    pure child let does not fold at its scoped id and a `fn` taking a `Solid` does not fold at all
    (§2.6). Extents are in the part's **own** frame, so the #5436 frame rule (member reads compose
    through the carried frame) cannot inflate them.
  - `trait SheetPart : Stocked { param quantity : Int = 1 }` — a flat part nested by ζ / ζ2.
  - No price list ships (§3 decision 13).

### §4.6 — Rectangular nesting (ζ, ζ2)

**Binding (ζ).** `nest_rectangles` is an `EvalBuiltin` row (`crates/reify-builtins/src/registry.rs`,
arm in `crates/reify-stdlib/src/registry_dispatch.rs`, kernel `crates/reify-stdlib/src/nesting.rs`) —
the only binding kind the builtin-signature registry wires today. An `EvalBuiltin` is `args -> Value`
with no diagnostics sink, so the builtin returns its outcome **in the value**, and a small engine pass
(beside `run_post_processes`' existing passes, ζ's own) turns a non-`Ok` status in any evaluated cell
into the coded Error and records the `UndefCause` on that cell's numeric fields. It needs no new
post-process for its *inputs*: post-realisation, parent lets that call it are re-evaluated by
`post_process_derived_lets` once the child extent lets fold.

```
enum NestStatus {
    Ok,
    NoStock { material: String, thickness: Length },
    PartTooLarge { part: Int, largest_width: Length, largest_length: Length },
    InvalidInput { reason: String }
}
structure def PartFootprint { param extent_x : Length; param extent_y : Length; param extent_z : Length;
                              param quantity : Int; param material : String }
structure def NestGroup     { param material : String; param thickness : Length; param sheet : SheetStock;
                              param sheets_used : Int; param utilisation : Real }
structure def NestResult    { param status : NestStatus; param cost : Money; param sheets_used : Int;
                              param utilisation : Real; param waste_area : Area; param groups : List<NestGroup> }

nest_rectangles(parts : List<PartFootprint>, sheets : List<SheetStock>, kerf : Length) -> NestResult
```

Semantics (normative — fully pinned so any implementation gives the same packing):

1. **Footprint.** Per part, `thickness` = the smallest extent; the footprint is the other two. Parts
   are modelled flat and axis-aligned in their own frame; a part rotated within its frame gets a
   conservative (larger) footprint — documented, not detected.
2. **Lattice.** All lengths are rounded to the nearest integer micrometre before packing; packing
   arithmetic is exact integer arithmetic.
3. **Grouping.** Parts group by `(material, thickness)` on the lattice; a group packs only onto sheets
   with equal `material` and lattice-equal `thickness`.
4. **Packing.** Per group and per candidate sheet size: quantities expand to instances, sorted by
   (longer side desc, area desc, input index asc). Each instance is placed by MaxRects over all open
   sheets in opening order: choose the free rectangle and orientation minimising the short-side
   leftover, then the long-side leftover, then the lower y, then the lower x, then the earlier sheet,
   then 0° before 90°. Every part dimension is inflated by `kerf`, and each sheet dimension by `kerf`
   (so edge parts lose no kerf). A new sheet opens when nothing fits. The group's sheet size is the one
   with least `sheets_used × price`, then fewer sheets, then catalog order.
5. **Result.** `cost` = Σ over groups of `sheets_used × price`; `utilisation` = Σ part area / Σ used
   sheet area; `waste_area` = Σ used sheet area − Σ part area; `groups` populated per group.
6. **Refusals.** No matching stock for a group ⇒ `NoStock`; a part fitting no candidate sheet in either
   orientation ⇒ `PartTooLarge`; negative `kerf` or non-positive quantity ⇒ `InvalidInput`. On any
   non-`Ok` status the numeric fields are `Undef`, and ζ's pass emits `E_NEST_*` and records the
   cause (INV-SF-1, INV-SF-6). Under the sweep, such a candidate's score is `Undef` ⇒ *unevaluated*
   (§3 decision 5).
7. **Honesty.** MaxRects is a heuristic: `sheets_used` is the count of *this* deterministic packing,
   an upper bound on the true minimum. A sweep's `ProvenOptimal` over a nesting objective is
   optimality of the objective *as computed*, stated in the doc chunk, never claimed as a minimum
   sheet count.

**Descendants form (ζ2).** `nest_sheet_parts(filter(self.descendants, SheetPart), sheets, kerf)` is
rewritten into `nest_rectangles` over one `PartFootprint` per conforming child, built from that
child's scoped `extent_*`, `quantity` and `material.name` cells — a clone of the
`cost(self.descendants)` intercept (`crates/reify-compiler/src/expr.rs`) and its
`apply_cost_aggregation` eval rewrite (`crates/reify-eval/src/structural_query.rs`), which ζ2 gives a
diagnostics sink. Same limits as `cost(...)`: depth-1, non-collection subs; a collection sub or a
deeper descendant that conforms to `SheetPart` gets `W_NEST_SUB_NOT_NESTED` naming it — never a silent
drop.

### §4.7 — Invariants

- **W1 — no regression.** A module with no geometry-coupled template resolves byte-identically
  (values, diagnostics) to today, on every path.
- **W2 — never silent, exactly one outcome.** Every coupled template ends in exactly one of: swept
  (`I_GEOMETRY_SWEEP`), refused (`E_GEOMETRY_COUPLED_CONTINUOUS` / `E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED`),
  not run on this path (`W_GEOMETRY_COUPLED_NOT_RESOLVED` + `UndefCause`), infeasible
  (`E_GEOMETRY_SWEEP_INFEASIBLE`) or unevaluated (`E_GEOMETRY_SWEEP_UNEVALUATED`) — on cold, cached
  and warm paths alike. The silent CP-SAT first-model pick and the `FallbackComponentZero` objective
  mis-routing (§2.3) are gone.
- **W3 — honesty.** `ProvenOptimal` ⇔ `Exhaustive` per §3 decision 6; capped ⇒
  `BestFound{EnumerationBudget}` and the existing warning; never a fabricated budget reason;
  unevaluated is never infeasible; infeasibility never rides the `Ranked` carrier.
- **W4 — determinism.** Bit-identical values, winner and `I_GEOMETRY_SWEEP` counts across runs;
  classification independent of hash order; budgets in candidate counts.
- **W5 — single machinery.** One enumeration engine (`solve_all`), one objective fold
  (`eval_objective_set`), one optimality gate (`optimality_unproven_finding`), one ranking, one §3.4
  policy function, one pin predicate, one classification helper.
- **W6 — pins never leak.** `sweep_pins` holds only *S*, is cleared on source change, is never copied
  into `param_overrides`, and never changes a cell outside *S*.
- **W7 — the winner is what is built.** Values, GUI mesh and STEP export after a sweep come from the
  pinned winner; Export does not re-sweep.

---

## §5 — Boundary-test sketch (B + H, two-way)

The integration gate η names every row not owned by a GUI leaf. Fixtures are `tests/prd-gate/fixtures/`
paths; a new fixture a compiled test reads is registered in `_RUST_COUPLED_RI_FIXTURES` in the same
diff (§8 test layout).

| # | Side | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|---|
| B1 | producer (α) | continuous coupling refused on every path | `mwaste_coupled_continuous.ri` | `reify eval` **and** `reify check` exit nonzero with `E_GEOMETRY_COUPLED_CONTINUOUS` naming `w`, `volume`, `body`; no "objective expression evaluated to undefined"; `w` carries that cause. α asserts nothing about the downstream geometry-op lines (#5404's). Baseline §2.3 |
| B2 | producer (α) | the silent first-model pick is gone | `mwaste_coupled_bool.ri` | before γ: `W_GEOMETRY_COUPLED_NOT_RESOLVED{sweep not yet available}` and `wide` undef with that cause — not `wide = true` silently; after γ: B5 |
| B3 | producer (α) | let-indirected coupling is classified | `mwaste_let_constraint.ri` and its `let v …; minimize v` sibling | classified coupled (continuous ⇒ B1's refusal), no residual-1.0 error |
| B4 | back-compat (α, β) | no coupling ⇒ no change | an existing objective fixture with no post-realisation read (e.g. `objective_set_weighted.ri`) and a merged-cluster fixture | byte-identical values + diagnostics (W1) |
| B4b | producer (α) | the objective never mis-routes | `mwaste_objective_fallback.ri` | `x` resolves exactly as it does with the objective deleted from the file (no NoProgress); the objective, whose reads reach no auto (*S* empty), gets the existing `E_OBJECTIVE_INERT` (`DiagnosticCode::ObjectiveInert`) instead of breaking an unrelated component; no sweep runs |
| B5 | consumer (γ) | finite sweep picks the true argmin, provably | an `Enum` width class × `Bool` option, Money objective = material term (inline `volume(geometry) * density * price`) + coating term (`area(geometry) * coating_price`) | `reify eval` picks the cheapest candidate; **swapping the two prices' relative size flips the choice**; `ProvenOptimal`, no optimality warning; one `I_GEOMETRY_SWEEP` line with exact counts. (Inline queries only — B5 does not need ε.) |
| B6 | consumer (γ) | geometry constraints steer the choice | B5 plus `constraint volume(geometry) >= v_min` that the cheapest candidate violates | winner = cheapest *feasible*; summary counts one rejection by that constraint |
| B7 | consumer (γ) | infeasibility is a coded error | B6 with `v_min` above every candidate | `E_GEOMETRY_SWEEP_INFEASIBLE` naming the constraint and counts; exit nonzero |
| B8 | producer (γ) | capped sweep stays honest | budget below the domain size (`--sweep-budget`); and a domain of exactly *budget* candidates | below: `BestFound{EnumerationBudget}`, `W_SOLVER_OPTIMALITY_UNPROVEN` exactly once, winner = best of the evaluated prefix; exactly-budget: complete, `ProvenOptimal` |
| B9 | producer (γ) | Enum domain is the declared one | an Enum variant appearing only in the objective or a geometric constraint | still enumerated |
| B10 | consumer (γ) | unevaluated ≠ infeasible ≠ proven | a fixture where one candidate's realisation fails (a boolean that empties at one parameter value) | that candidate is counted *unevaluated* with its cause; verdict `Partial{CandidatesUnevaluated}`, never `ProvenOptimal`; if every candidate fails, `E_GEOMETRY_SWEEP_UNEVALUATED`, not infeasible |
| B11 | consumer (γ2) | P3 §3.4 verdict policy | two candidates with exactly equal scores, strict and `auto(free)` variants | strict ⇒ Error naming both; free ⇒ coded Warning naming 2 and the one taken |
| B12 | consumer (α, γ) | scope rule | two templates with geometry objectives; a coupled template instantiated twice | `E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED` naming them, on every path |
| B13 | consumer (δ) | GUI shows the winner; drags keep it | GUI load of B5's fixture via the debug-MCP harness; a slider drag on a price; then commit | after load, `engine_state` values = the winner; after the drag, values unchanged and `I_GEOMETRY_SWEEP_STALE` present in the eval-diagnostics field #6724 adds; after commit, the re-swept winner |
| B14 | producer (β) | pins act as concrete values, through subs | `mwaste_parent_auto_to_sub.ri` with `thin` pinned | `Panel.a.t = 0.002 m`, `Panel.a.ez` and `Panel.ma` evaluate — identical to the same file with `thin` a concrete `true` — and a geometry constraint over a pinned auto gets a definite post-geometry verdict |
| B15 | consumer (α) | kernel-less paths say why | B5's fixture under the reify-cli MCP / LSP / `reify test` (no kernel) and `reify check` (no solver) | `W_GEOMETRY_COUPLED_NOT_RESOLVED` with the matching reason and an `UndefCause` per auto; continuous fixtures still give B1's Error |
| B16 | consumer (α) | warm paths classify too | an *unswept* coupled template edited through `edit_param` (e.g. after a cancelled or not-run sweep) | no silent first-model pick, no NoProgress: the warm builder applies the same classification |
| B17 | producer (ε) | exact extents and blank lets | `blank_volume` of a 40×30×10 mm box (a `Stocked` structure); a pocketed block | `|offcut_volume| ≤ 1e-10 × volume` for the box (today ≈ 3.2e-5 × volume from the padding); `offcut_volume / (π r² h) − 1` within 1e-9 for the pocket; every pre-existing `bounding_box` consumer test green |
| B18 | consumer (ζ, ζ2) | nesting | four 500×500 mm squares on a 1000×1000 mm sheet, kerf 0, then kerf 2 mm; a group with no matching stock; the descendants form over three `SheetPart` subs and a collection sub | kerf 0: `sheets_used = 1`, `utilisation = 1.0`, `cost` = one sheet; kerf 2 mm: the hand-computed count under §4.6 item 4's kerf rule; no-stock: `E_NEST_NO_STOCK` naming material + thickness and `cost` undef with that cause; ζ2 equals the explicit list; the collection sub gets `W_NEST_SUB_NOT_NESTED` |
| B19 | back-compat (γ) | determinism | B5/B6 and a two-template-free sub-object fixture (§2.3 row 7 shape, finite) run twice | bit-identical values and summary (W4) |

**G6 bases for the numeric rows.** B5/B6 compare exact OCCT volumes and areas (bit-deterministic,
§2.1) — an ordering, not a tolerance. B17's box case rests on `AddOptimal` without shape tolerance
returning the B-rep's planar extents (measured exact); the 1e-10 bound is ~10⁶ ulps of slack yet five
orders below the 3.2e-5 padding signal it must distinguish; the pocket case rests on GProp matching
the analytic volume to 1.4e-16 relative. B18 uses **squares**, so orientation is irrelevant and §4.6
item 4's fully pinned order makes the packing hand-computable; the kerf variant's count is computed by
ζ from §4.6's rule, not taken from a run.

---

## §6 — Pre-conditions for activating

All met — this PRD is the activation of milestone #4787 (deps #4795, #4804, #5125, #6925 done). Hard
upstream edges on **pending** tasks, wired at decompose:

- **#5470** (PRD 2 δ — `AutoParam.domain`, `is_discrete()`, Enum declared domain, `Int` mining) → γ.
  Without it only `Bool` enumerates and Enum domains narrow.
- **#6724** (P4 γ — route eval-time diagnostics into the GUI) → δ. Without it the GUI cannot show
  `I_GEOMETRY_SWEEP_STALE` or any sweep diagnostic.
- **#8357** (and through it #8355 / #8356 — catalog membership) → η (the catalog stock-pick example).

Not prerequisites, by design: #6925 / #8459 / #8460 (eviction); P2 ο #6683 (gates bookmark ι only);
P3 #6900 / #6903 (the sweep uses landed carrier API plus one additive variant); #6901 (coordinated,
whoever-first, §3 decision 7); PRD 2 ζ #5472 (sibling); #6631 / #6657 (β delivers pin flow through
the overlay itself and coordinates).

---

## §7 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `discrete-cost-minimisation.md` (PRD 2) | consumes | `CpSatSolver::solve_all`, CP-SAT ranking helpers; `AutoParam.domain` / `is_discrete()` (δ #5470) | PRD 2 owns enumeration + domain channel; **this PRD** owns the build-level leaf evaluator | `solve_all` landed; #5470 pending — hard upstream of γ |
| PRD 2 ζ #5472 (mixed outer loop) | sibling outer loop | both are outer enumerations; ζ's leaf = a continuous residual solve in the registry, the sweep's leaf = a pinned whole build above eval | each owns its leaf evaluator; **shared**: enumeration engine, carriers, P3 law | pending; no edge (disjoint files). PRD 2 §7's "independent, no code seam" row is corrected by θ3 |
| `ranked-solve-result.md` (F-result) | consumes | `RankedSolveResult` / `OptimalityStatus` / `BestFoundReason::{EnumerationBudget, ConvergedWithinBudget}`; `optimality_unproven_finding` | F-result owns carrier + gate; **this PRD** owns its producer | landed. §0.1's `ProvenOptimal` grant gets a note **in this PRD's landing commit**; θ3 fixes the stale F-cache names, 4787 rows and the §3.1 carrier listing |
| `solution-set-completeness.md` (P3) | consumes + extends | `Completeness`, C1 `derived_unique`, C2 `permits_proven_optimal`, `Partial{BoxBudgetExhausted, InnerSolveUnproven}`; **adds** `PartialReason::CandidatesUnevaluated { count }` (γ, additive) | P3 owns the carrier; γ adds one variant with a P3 cross-reference (θ3) | carrier landed (#6706). The open `Refuted`-in-`Ranked` seam (#6900) is not used: infeasibility is a coded error, never a carrier value |
| #6901 (P3 E3C3 — §3.4 verdict policy) | coordinates | the single §3.4 verdict → diagnostic policy function | #6901 for solver paths; γ2 for the sweep; **whoever lands first implements it, the other adopts** (#6901's own #6678 precedent) | pending; no edge (either order works) |
| #6554 (strict CP-SAT non-uniqueness silent) | adjacent | strict-auto ties | #6554 / #6901 on the solver path; the sweep's ties go through §3.4 (γ2) | pending |
| #6691 (P1 γ — no hash order reaches `ResolutionProblem`) | adjacent | the `DecompositionPrelude` `HashMap` iteration behind §2.3's nondeterminism | #6691 | pending; the classification pass avoids decomposition entirely, so no edge |
| `catalog-membership-constraint.md` | consumes | `constraint x in C` (#8355), membership domains (#8356), `discrete_set` sugar (#8357); `NoCatalogMemberFeasible` (#8356 D8) owns an **empty pre-geometry** enumeration over a catalog | that PRD | pending — #8357 hard upstream of η. `E_GEOMETRY_SWEEP_INFEASIBLE` is only for candidates that exist and are all rejected post-geometry |
| `instantiation-value-flow.md` (#6631 pending, #6657 deferred) | coordinates | per-instance sub ctor-arg overlay for *solved* autos | that PRD owns solved-auto flow; **β** owns pin flow (pins are pre-seeded as concrete values, the path that already works) | no edge; β's B14 is the guard |
| #6724 (P4 γ — eval-time diagnostics into the GUI) | consumes | a GUI eval-diagnostics field on `GuiState` | `solver-legibility-telemetry.md` | pending — hard upstream of δ |
| `geometry-algebra-solver-unification.md` (P2) | produces-for (future); adjacent | `GeometryDispatch` (#6683) + a re-realisation arm = the inner layer; P2 δ #6671 / #6659 own typed per-constraint refusals | P2 owns the trait and the refusal enum; bookmark ι owns arm B's design; α's `E_GEOMETRY_COUPLED_CONTINUOUS` becomes this regime's cause in P2's refusal enum when #6671 lands | P2 pending; ι dep-gated on #6683. θ3 updates P2 §5 row and §10 successor line. (Reported to Leo, not fixed here: P2 ο's wired hard dep #6583 is cancelled as a duplicate of pending #5436, so that edge is stale.) |
| #5404 / #7308 / #5197 | adjacent | geometry-awaiting-solve diagnostics; `reify check` `error:`-at-exit-0 inventory; `OpContractViolation` mis-cause | those tasks | α asserts nothing they deliver |
| `whole-model-objective-coupling.md` (M-WHOLE) | consumes (per candidate) | merged-cluster solves run unchanged inside each candidate; the classification pass runs in `build_merged_solver_problem` too | M-WHOLE | landed; θ3 adds the M-WASTE row status |
| `continuous-cost-minimisation.md` (PRD 1) | consumes (per candidate) | the Money robustness floor composes in every candidate's residual solves; swept scopes are excluded from `detect_robustness_floor_applied` | PRD 1 | landed; θ3 corrects §5 row 3 / §10 (the non-existent `cache-input-cone-rekey.md`) |
| `multi-aspect-objective-units-coherence.md` (M-UNITS) | consumes | `E_OBJECTIVE_MIXED_DIMENSION` — blank, coating and nesting costs are all `Money` | M-UNITS | landed |
| `selective-realization-eviction.md` (#6925) | none (by design) | the sweep uses neither `RealizationCache` nor `evict_family` (§2.2) | eviction PRD | landed; #8459 / #8460 noted (§0) |
| `io-lifecycle-bom-cost.md` | adjacent | `Discard` / `DiscardReason.Offcut` report roll-up vs this PRD's geometric waste | that PRD | θ3 adds a one-line boundary note |
| `builtin-signature-registry.md` | consumes | an `EvalBuiltin` row for `nest_rectangles` (the only wired kind) | that PRD | landed seed |
| `engine-integration-norm.md` | extends | new §3.12 build-level geometry-coupled resolution seam | **this PRD** | entry lands in this PRD's commit |
| PRD 2 bookmark θ / P3 ζ #6711 | produces-for (future) | the sweep's ranked candidate set is a solution set of auto-assignment records | those bookmarks | no edge |

No new contested-ownership pair (checked against `phase-3-breadcrumb-map.md` §3). P2 calls its trait
"that loop's inner half" while this PRD's outer loop does not use it — resolved by Q-A's layering: the
outer layer is this PRD's, the inner layer (arm B) is bookmark ι's, and P2 keeps the trait.

---

## §8 — Decomposition plan

B+H shape. Greek labels with the task ids filed at decompose (2026-10-11). Decompose-time adjustments —
θ1's reify-design index line moved to θ2, B10's failing-candidate fixture shape, the `AddOptimal` scope for
curved faces, the one-line record shorthand — are recorded in the capability manifest's "Decompose-time
corrections" and in each task's description, not here.
Spine α → β → γ → {γ2, δ}; ε → ζ → ζ2 in parallel; η the integration gate; θ1–θ3 docs; ι / κ / λ
bookmarks; ω close.

**Test layout and drift guards.** Eval-side end-to-end tests extend the existing
`crates/reify-eval/tests/harness_engine.rs` harness (no new standalone `crates/*/tests/*.rs` binary —
harness-layout ratchet, Leo 2026-07-22); solver- and stdlib-level tests are in-src `#[cfg(test)]`
units; GUI assertions use the existing debug-server harness under `--features gui`. No leaf adds a
`tests/infra/*.sh` or a wall-clock assertion (budgets are candidate counts, P3 C7). **Fixture rule:**
every `tests/prd-gate/fixtures/*.ri` a compiled test reads is added to `_RUST_COUPLED_RI_FIXTURES` in
`scripts/verify.sh` in the same diff (PG-DRIFT in `tests/infra/test_verify_scope.sh` enforces it); a
leaf departing from this layout carries its own registration in the same diff.

- **α #8487 — Geometry-coupling classification pass + coded refusals, on every path.**
  Modules: `crates/reify-eval/src/engine_eval.rs` (`classify_geometry_coupling`; called from `eval` /
  `eval_cached` outside the solver gate and from both problem builders; objective removal; δ-predicate
  supersession; `SweepState` read; `E_OBJECTIVE_INERT` for an empty *S*),
  `crates/reify-eval/src/engine_edit.rs` (the warm builders call the same helper),
  `crates/reify-eval/src/lib.rs` (`SweepState` type), `crates/reify-core/src/diagnostics.rs` (codes),
  `crates/reify-ir/src/value.rs` (`UndefCause` variant for the not-resolved cause).
  **LEAF signal:** B1 (`reify eval` and `reify check`), B2 (pre-γ form), B3, B4, B4b, B12, B15, B16.
  Prereqs: none. `grammar_confirmed = true`.

- **β #8488 — Auto-pin overlay.**
  Modules: `crates/reify-eval/src/{engine_eval.rs, engine_build.rs (reset_per_build_state rows),
  graph.rs, snapshot.rs, engine_edit.rs, resolve_order.rs, deps.rs, engine_admin.rs, invariants.rs}`.
  §4.2 in full; the site classification is written into the task's own description.
  **Intermediate → γ, δ** (consumer: the sweep). In-process signal γ builds on: B14, plus a pinned
  auto reading `Determined` on eval / check / build paths, surviving `reset_per_build_state`, and
  skipped by the warm re-solve.
  Prereqs: α. `grammar_confirmed = true`.

- **γ #8489 — The sweep: engine entry, enumeration, candidate evaluation, ranking, honesty; `reify eval` wiring.**
  Modules: `crates/reify-eval/src/` (new sweep module; `engine_build.rs` non-emitting candidate build
  returning score + verdicts + residual status; `engine_eval.rs` `optimality_unproven_finding`
  `pub(crate)`, robustness-floor exclusion), `crates/reify-constraints/src/{solver.rs
  (`eval_objective_set` pub), cpsat.rs (ranking factored for reuse)}`, `crates/reify-ir/src/completeness.rs`
  (`PartialReason::CandidatesUnevaluated`), `crates/reify-cli/src/main.rs` (`cmd_eval` calls the entry;
  `--sweep-budget`), `crates/reify-core/src/diagnostics.rs` (codes).
  **LEAF signal:** B5, B6, B7, B8, B9, B10, B19 under `reify eval`; B2's fixture now swept (`wide`
  resolves to the 20 mm width).
  Prereqs: α, β, **#5470** (out-of-batch, hard). `grammar_confirmed = true`.

- **γ2 #8490 — Sweep verdict diagnostics per P3 §3.4.**
  Modules: the §3.4 policy function (wherever #6901 places it, or created here if γ2 lands first —
  §3 decision 7), the sweep's commit step.
  **LEAF signal:** B11.
  Prereqs: γ. Coordinates with #6901 (no edge). `grammar_confirmed = true`.

- **δ #8491 — GUI: load / commit / Export see the sweep; drags keep the winner; cancellation.**
  Modules: `gui/src-tauri/src/engine.rs` (`check_with_solve_slot` calls the entry; Export builds the
  current pins; the per-solve `CancellationHandle` reaches the sweep),
  `crates/reify-eval/src/engine_edit.rs` (`I_GEOMETRY_SWEEP_STALE` when an edit touches *T*'s inputs;
  pins cleared on `update_source` / reload).
  **LEAF signal:** B13 through the debug-server harness, asserting the eval-diagnostics field #6724
  adds.
  Prereqs: γ, **#6724** (out-of-batch, hard). `grammar_confirmed = true`.

- **ε #8492 — Exact extents + stock vocabulary.**
  Modules: `crates/reify-kernel-occt/cpp/occt_wrapper.cpp` (`query_bbox` → `AddOptimal`), the
  `bounding_box` consumers and tests listed in §4.5, `crates/reify-compiler/stdlib/stock.ri` (new:
  `SheetStock`, `Stocked`, `SheetPart`), `crates/reify-compiler/src/stdlib_loader.rs` (registration).
  **LEAF signal:** B17 under `reify eval`; a `SheetStock` catalog built in a structure `let` reads
  back its fields; the `AddOptimal` timing ratio is recorded.
  Prereqs: none (parallel to α–δ). `grammar_confirmed = true`.

- **ζ #8493 — Rectangular nesting builtin + status pass.**
  Modules: `crates/reify-builtins/src/registry.rs` (row), `crates/reify-stdlib/src/{nesting.rs (new),
  registry_dispatch.rs, lib.rs}`, `crates/reify-compiler/stdlib/stock.ri` (`NestStatus`,
  `PartFootprint`, `NestGroup`, `NestResult`), `crates/reify-eval/src/engine_build.rs` (the status
  pass). §4.6 items 1–7.
  **LEAF signal:** B18's explicit-list half under `reify eval`.
  Prereqs: ε. `grammar_confirmed = true`.

- **ζ2 #8494 — `nest_sheet_parts(filter(self.descendants, SheetPart), …)`.**
  Modules: `crates/reify-compiler/src/expr.rs` (intercept beside `cost(...)`),
  `crates/reify-eval/src/structural_query.rs` (rewrite beside `apply_cost_aggregation`, with a
  diagnostics sink).
  **LEAF signal:** B18's descendants half.
  Prereqs: ζ. `grammar_confirmed = true`.

- **η #8495 — Integration gate: CI examples + harness e2e.**
  Modules: `examples/material_waste_stock_pick.ri` (catalog `in` thickness + Enum width class, a
  volume floor, material + coating Money objective over `Stocked` blank lets),
  `examples/material_waste_nesting.ri` (three `SheetPart` plates sized by one swept catalog thickness
  in the parent, `nest_sheet_parts` cost objective, sheet size chosen by the nest),
  `crates/reify-eval/tests/harness_engine.rs` (every §5 row not owned by δ, as end-to-end assertions).
  **LEAF signal (integration gate):** both examples run under `reify eval` in CI with the winning
  values, `ProvenOptimal` silence and exact `I_GEOMETRY_SWEEP` counts; §5 rows asserted end to end.
  Prereqs: γ, γ2, δ, ε, ζ2, **#8357**, **#5470**. `grammar_confirmed = true` (the `in` grammar is #8355's).

- **θ1 #8496 — Doc chunks, registry-verified + reify-design index + discoverability.**
  Modules: `crates/reify-mcp/src/tools/chunks/` (`measurement.md`: exact extents, `Stocked`;
  `constraints.md`: geometry-coupled objectives / constraints, the finite-domain rule, the sweep
  summary and budget, the v1 scope rule; a stock / nesting section, including §4.6 item 7's honesty
  note), `.claude/skills/reify-design/SKILL.md` (one index line).
  **LEAF signal:** each documented signature compiles as written in a smoke `.ri`; an author who knows
  only the goal ("pick the cheapest sheet", "minimise material cost", "nest parts on a sheet") finds
  the mechanism from the chunk / index wording.
  Prereqs: η. `grammar_confirmed = true`.

- **θ2 #8497 — Exemplar corpus.**
  Modules: `examples/best_practices/<geometry-cost exemplar>.ri` + its `INDEX.md` row in the same
  commit (`best_practices_index_matches_corpus_directory`).
  **LEAF signal:** the exemplar compiles clean; its swept and geometric constraints are listed in
  `best_practices_constraint_gate.rs`'s expected-`Indeterminate` set (that gate is kernel-less and
  solver-less by design); a separate `reify eval` assertion pins the swept value. The refused
  continuous form is shown in the θ1 chunk, never in the gated corpus file.
  Prereqs: η. `grammar_confirmed = true`.

- **θ3 #8498 — Companion corrections.**
  Modules (docs only): `docs/prds/v0_6/ranked-solve-result.md` (§0 / §6 / §7 stale F-cache names and
  4787 rows; §3.1 carrier listing), `continuous-cost-minimisation.md` (§5 row 3, §10),
  `discrete-cost-minimisation.md` (§7 M-WASTE row), `geometry-algebra-solver-unification.md` (§5 row,
  §10 successor line → this PRD + bookmark ι), `whole-model-objective-coupling.md` (§8 / §10 rows),
  `solution-set-completeness.md` (the `CandidatesUnevaluated` variant), `io-lifecycle-bom-cost.md`
  (boundary note), `docs/reify-language-spec.md` (§10.8 deferred successor → shipped capability; the
  feature-table row). (The norm §3.12 entry and the F-result §0.1 note land with this PRD.)
  **LEAF signal (docs):** committed prose; no sibling PRD or the spec still calls M-WASTE a stub or
  cites a non-existent F-cache PRD.
  Prereqs: η. `grammar_confirmed = true`.

- **ι #8499 — `[MILESTONE]` Bookmark: continuous geometry-in-the-loop (the inner layer).**
  PENDING milestone; ON DISPATCH escalate for a `/prd` design session — do NOT implement. Charter: arm
  B (per-trial re-realisation) on P2's `GeometryDispatch`, routed from α's continuous refusal;
  local-optimum honesty (`BestFound`); content-addressed realisation memo; native-memory policy;
  finite-difference budget. `execution_class: decision`.
  Prereqs: **#6683** (P2 ο), γ.

- **κ #8500 — `[MILESTONE]` Deferred-optimisation decision (§9).**
  PENDING milestone; ON DISPATCH measure per-candidate in-process cost on η's examples (and any
  printer_v01-scale coupled model), re-read §9's trigger table, and either file the justified items or
  record why none is. `execution_class: decision`.
  Prereqs: η.

- **μ #8501 — `[MILESTONE]` Bookmark: multi-instance coupled templates.**
  PENDING milestone; ON DISPATCH escalate for `/prd` design — do NOT implement. Charter: lift §3
  decision 2's "instantiated once" limit — per-instance pins (keyed by instance, not template cell),
  per-instance scoring of the template's objective, how instances' objectives combine, and the
  separable-vs-product enumeration question (§9). `execution_class: decision`.
  Prereqs: η, **#6592** and **#6657** (instantiation-value-flow β / κ — per-instance overlays and
  solved-value flow).

- **ν #8502 — `[MILESTONE]` Bookmark: cross-template sweep sets.**
  PENDING milestone; ON DISPATCH escalate for `/prd` design — do NOT implement. Charter: lift §3
  decision 2's "*S* ⊆ *T*'s own autos" and "one coupled template" limits — child autos joining a
  parent's geometry-objective sweep, several coupled templates in one module, and how that composes
  with M-WHOLE's merged clusters. `execution_class: decision`.
  Prereqs: η, **#6592**, **#6657**.

- **λ #8503 — `[MILESTONE]` Bookmark: true-profile nesting.**
  PENDING milestone; ON DISPATCH escalate for `/prd` design. Charter: irregular profiles (a flat-profile
  extraction query + a no-fit-polygon engine), arbitrary rotations, common-line cutting.
  `execution_class: decision`.
  Prereqs: η.

- **ω #8504 — PRD close.** Status → terminal token, landed leaf ids, AS-AUTHORED freeze paragraph and
  LIVE / AS-AUTHORED map, matching header on the capability manifest (overlay "PRD terminal status").
  **LEAF signal:** the committed header. Prereqs: **every** other leaf, ι / κ / λ / μ / ν included (overlay
  rule; a `cancelled` sibling counts as satisfied). The PRD stays non-terminal until each bookmark is
  dispositioned — expanded into its own PRD (bookmark `done`) or dropped (`cancelled`) — which is the
  honest state.

**Dependency view**
```
α → β → γ → γ2 ─────┐
          ↘ δ ──────┤
     #5470 → γ      ├→ η → {θ1, θ2, θ3, κ, λ, μ, ν}
     #6724 → δ      │    ↑
ε → ζ → ζ2 ─────────┘  #8357 (#8355, #8356)
γ + #6683 → ι     #6592 + #6657 → {μ, ν}     {α … ν} → ω
```

---

## §9 — Deferred optimisations and widenings (breadcrumbs with trigger conditions)

Each is ruled **out of v1**, with the condition that would justify it and the condition that would
rule it out. κ (§8) is the dep-gated decision task that re-reads this section once η's examples give
real measurements.

| Item | Justified when | Ruled out when |
|---|---|---|
| **Coupled-cone-only realisation** — realise only bodies in *S*'s read cone per candidate, the rest once | κ's measurement shows realisation outside that cone is > 50 % of per-candidate time on a representative model | per-candidate time is dominated by the coupled bodies themselves |
| **Content-addressed realisation memo** keyed by input-cone hash (modelled on `persistent_cache_key`) | candidates revisit identical sub-geometry **and** #8460 has landed so build-stamped cones are trustworthy | cone-only realisation already removes the repeated work |
| **Warm sweep reuse** (re-sweep on edit only when the edit touches *T*'s inputs) | GUI users report stale optima as a problem **and** a sweep is under ~1 s on representative models | sweeps remain multi-second |
| **Parallel candidate evaluation** (several engines) | a representative sweep exceeds ~10 s single-threaded **and** the target is a user machine | — never in CI / verify paths: CPU is the scarce resource there |
| **Separable sweeps** (enumerate independent coupled sub-sets separately instead of as a product) | μ / ν have landed multi-scope sweeps **and** a product exceeds the budget while each factor does not | — |

The two scope widenings are not optimisations and are not here: they are bookmarks μ and ν (§8),
dep-gated on instantiation-value-flow.

---

## §10 — Out of scope (named successors)

- Continuous geometry-driving autos → bookmark **ι** (inner layer on P2 `GeometryDispatch` #6683).
- True-profile / irregular nesting, arbitrary rotations, common-line cutting → bookmark **λ**.
- Multi-instance coupled templates → bookmark **μ**; cross-template sweep sets → bookmark **ν**.
- A user-facing alternatives / `solutions()` surface → PRD 2 θ, P3 ζ #6711.
- BOM/waste **reports** (`reify report --bom`) → `io-lifecycle-bom-cost.md`.
- Supplier price data in the stdlib (§3 decision 13).
- Solving autos on `reify check` / `reify build` (their solver-free posture is unchanged).
- MINLP / seeded global optimisation (M-WHOLE §3.1).
- Any change to `SolveResult`, `ConstraintSolver::solve()` or the F-result carrier; the only P3 carrier
  change is the additive `CandidatesUnevaluated` variant.
- `reify check`'s exit code on non-constraint `error:` lines (INV-SF-2) → #5404 / #7308; the
  `OpContractViolation` mis-cause → #5197; hash-order leaks in decomposition → #6691; `USD` in `fn`
  bodies → #8173; `Vector3` component access → #6892.

---

## §11 — Open questions (tactical — decide at implementation time)

1. **Default candidate budget.** A candidate is a full build (tens of ms to seconds), not a CP-SAT node.
   **Suggested:** 256 evaluated candidates, a CLI override `--sweep-budget N`, no `.ri` surface in v1
   (mirrors P2 §11 Q3); `solve_all`'s cap is `budget + 1` (§3 decision 8). Decide in γ.
2. **Diagnostic code names** in §4.4 are working names; follow `reify-core` `DiagnosticCode`
   conventions. Decide in α / γ / ζ.
3. **`I_GEOMETRY_SWEEP` verbosity** — always one line (suggested: it is the only evidence a sweep ran)
   or only under `--verbose` when `ProvenOptimal`. Decide in γ.
4. **Partial-reason precedence** when a sweep is both capped and has unevaluated candidates.
   **Suggested:** `BoxBudgetExhausted` (it is the one that fires the warning); the summary still
   counts the unevaluated. Decide in γ.
5. **Lattice resolution for nesting.** 1 µm suggested (below any kerf, above OCCT tolerance). Decide
   in ζ against the fixtures.
6. **Where the §3.4 policy function lives** if γ2 lands before #6901 — `engine_eval.rs` beside the
   other verdict findings is suggested. Decide in γ2 (or adopt #6901's).
7. **`AddOptimal` cost** on BSpline-heavy shapes — ε records the ratio; if a topology selector becomes
   materially slower, ε keeps `Add` for that selector's internal use and `AddOptimal` for the
   user-visible query. Decide in ε.
