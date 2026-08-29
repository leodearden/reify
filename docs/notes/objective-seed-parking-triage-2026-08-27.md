# Objective seed-parking triage (task #6756)

Triage record for `docs/prds/v0_6/solution-set-completeness.md` §10 item 4:
*"`maximize` against a `<= 40mm` bound returns `24mm`; `minimize` against `>= 8mm`
returns `8.8mm`. Objectives look soft-penalised rather than bound-seeking. Needs triage
before it is called a bug."*

**Verdict: it is a bug — a correctness defect, not a numerics one.** Candidate (a), the
silent seed-fallback, is CONFIRMED; (b), (c) and (d) are ruled out. Nothing here changes
solver behaviour: the fix is already owned, three ways, and the disposition is
close-into-owner (§7). File:line anchors below are point-in-time — re-verify against
current `main` before building on one.

## Provenance

- **Measured at HEAD:** `9c1bed42a7cb949cfe15dcee67052c84d4d41ff3` (short `9c1bed42a7`,
  "Merge task/6341 into main", 2026-08-28T19:52:47+01:00), branch `task/6756`, base `main`.
- **Dates:** filed 2026-08-27 (the filename carries the filing date, matching the
  disposition line in the PRD's §10 header); **measured 2026-08-28**.
- **Task:** #6756 — "TRIAGE: minimize/maximize park at the seed instead of bound-seeking
  — mechanism verdict + discriminating probe set". Investigation only; deliverable is
  the probe set plus this verdict, explicitly **not** a fix.
- **PRD:** `docs/prds/v0_6/solution-set-completeness.md` §10 item 4.
- **Instrument (both files land with this task):**
  - `crates/reify-constraints/tests/objective_seed_parking_triage.rs` — probes P1–P6.
    `cargo test -p reify-constraints --test objective_seed_parking_triage`
  - `crates/reify-eval/tests/objective_seed_parking_e2e.rs` — probe P7, at the `.ri`
    driver level. `cargo test -p reify-eval --test objective_seed_parking_e2e`

## How to reproduce

No local patch is needed — both probe files are committed. Every number below comes from
running the two commands above at the HEAD named in the provenance block. Each probe
asserts a value **derived from a named in-tree constant** rather than a threshold tuned
to match an unknown output, so a divergence is a real signal, not a stale pin.

The probe set uses the **production** `AutoParam` shape throughout: `bounds: None` (always
`None` in production — `crates/reify-constraints/src/solver.rs:993-997` names all three
hardcoded construction sites) and `free: true` (keeps `verify_uniqueness` from confounding
a seed-parking probe with a uniqueness verdict). P6 is the single deliberate exception,
and that exception is the point of P6.

### Measured probe table

| Probe | problem | seed | measured | derivation | bit-identical |
|---|---|---|---|---|---|
| P1 | `min x` s.t. `x >= 8mm` | none | **8.800000 mm** (bits `0x3f8205bc01a36e2f`) | `8mm × 1.1` | yes |
| P2 | `max x` s.t. `8mm <= x <= 40mm` | none | **24.000000 mm** (bits `0x3f989374bc6a7efa`) | `(8mm + 40mm)/2` | yes |
| P3 | `max x` s.t. `x <= 40mm` | none | **36.000000 mm** (bits `0x3fa26e978d4fdf3c`) | `40mm − 0.1 × 40mm` | yes |
| P4a | `min x` s.t. `x >= 8mm` | 30mm | **30.000000 mm** (bits `0x3f9eb851eb851eb8`) | the seed | yes |
| P4b | `min x` s.t. `x >= 8mm` | 12mm | **12.000000 mm** (bits `0x3f889374bc6a7efa`) | the seed | yes |
| P4c | `max x` s.t. `8mm <= x <= 40mm` | 11mm | **11.000000 mm** (bits `0x3f86872b020c49ba`) | the seed | yes |
| P5 | `solve_ranked` on P1's problem | none | `BestFound { reason: ConvergedWithinBudget }`, 1 candidate, `objective_score: Some(0.0088)` | not `IterationLimit` | — |
| P6 | `min x` s.t. `2mm < x < 50mm`, **wall (5mm, 100mm)** | 25mm | **5.000000 mm** (bits `0x3f747ae147ae147b`) | the clamp floor | — |
| P7 | `.ri` driver, `min x` s.t. `8mm <= x <= 40mm` | none | **24.000000 mm** (bits `0x3f989374bc6a7efa`), **0 diagnostics** | the seed | yes (= P2) |

Every solve returned `SolveResult::Solved { unique: false }` — never `Infeasible`, never
`NoProgress`. The `unique: false` is expected and is **not** a divergence: the
`unique: true` the drift fallback constructs at `solver.rs:2029` is documented at
`solver.rs:1691-1693` as a *placeholder*, and `finalise_uniqueness`
(`solver.rs:2694-2730`) overwrites it — an all-`free` problem skips the uniqueness
re-solve entirely and reports `unique: false` (`solver.rs:2723-2728`).

---

## §1 — Candidate (a): silent seed-fallback — **CONFIRMED**

Both reported numbers are **exactly the seed**, returned by the silent seed-fallback. The
chain has five links, all in `crates/reify-constraints/src/solver.rs`:

1. **SEED.** `extract_initial_point` (`:420-440`, doc `:402-419`) resolves, per auto param,
   the first applicable of: (1) the current value; (2) an explicit `AutoParam::bounds`
   midpoint; (3) the **constraint-derived box** (task #5618) — the midpoint when BOTH sides
   were derived, else nudged inward from the single derived bound by
   `max(SEED_NUDGE_REL × |bound|, SEED_NUDGE_ABS)`, with `SEED_NUDGE_REL = 0.1` (`:239`)
   and `SEED_NUDGE_ABS = 1e-6` (`:244`); (4) the fixed `0.01` fallback.

   So a one-sided `x >= 8mm` seeds at `8mm × 1.1` = **8.8mm** (P1), a one-sided
   `x <= 40mm` seeds at `40mm − 0.1 × 40mm` = **36mm** (P3), and a two-sided
   `8mm..40mm` seeds at the midpoint **24mm** (P2). Arm 2 never fires in production
   because `AutoParam.bounds` is always `None` (`:993-997`).

2. **NO CLAMP WALL.** The clamp box handed to the optimiser is gated on `floor_applied`
   (`:1809-1825`): the constraint-derived clamp box is used **only** when the Money
   robustness floor fired. A `Length` objective is not Money (`objective_is_money` `:820`,
   gate `:1755-1760`), so the else-branch takes `effective_bounds` =
   `default_bounds_for(Length)` = `(1e-6, 10.0)` (`:1585-1594`). There is no wall anywhere
   near the user's bound.

3. **PENALTY UNDERSHOOT.** Cost is `obj + PENALTY_WEIGHT × violation + PENALTY_WEIGHT ×
   bound_penalty` (`:1539-1548`) with `PENALTY_WEIGHT = 1e6` (`:25`). Minimising
   `x + 1e6·(b − x)²` is stationary at `b − 1/(2 × PENALTY_WEIGHT)` = `b − 5e-7`, i.e.
   **5e-7 outside the active bound**. The solver's own comments state this verbatim
   (`:1017-1021`, `:1776-1781`) and `:1780-1781` already names the symptom this triage
   was filed for: *"Deriving from the RAW box instead yields a feasible-but-badly-suboptimal
   answer (the seed, returned via the drift fallback)."*

4. **FEASIBILITY REJECT.** The final check measures the LINEAR residual against
   `FEASIBILITY_THRESHOLD = 1e-12` (`:20`, `:1997`). `5e-7 >> 1e-12`, so the converged
   optimum is rejected as infeasible.

5. **SILENT SEED-FALLBACK.** Because the seed *is* feasible (`initially_feasible`), the
   rejected optimum is discarded and the **untouched initial point** is returned as
   `Solved` (`:1997-2031`). The objective is ignored. The only trace is a
   `tracing::debug!` — no diagnostic. `warm_start_fallback_returns_exact_initial_values`
   (`crates/reify-constraints/tests/solver_integration.rs:1483`) pins that the return is
   the EXACT initial, which is why every measurement above is bit-exact rather than "near".

**P4 is the decisive evidence.** Holding the constraints, the objective and its sense
fixed and moving only the seed, the answer tracks the seed bit-for-bit: 30mm → 30mm,
12mm → 12mm, and (two-sided, opposite sense) 11mm → 11mm. An output that is a bit-exact
function of the seed under those conditions can only be the seed being returned.

**P7 measures the silence.** At the `.ri` driver level (`compile_source_with_stdlib` →
`Engine::eval`), `minimize x` under `8mm <= x <= 40mm` returns 24.000000 mm — bit-identical
to the solver-level P2 result, so the driver adds nothing — and the eval emits **zero
diagnostics of any kind**. Not merely no `SolverOptimalityUnproven`: nothing at all. An
author is handed a number 16mm from the one they asked for, with no signal.

## §2 — Candidate (b): Nelder-Mead stalling / iteration exhaustion — **RULED OUT**

P5 reads the optimality signal through the public `solve_ranked` surface (`SolveMeta` at
`solver.rs:89` and `solve_with_meta` at `:2802` are private, so this is the only route)
and measures `OptimalityStatus::BestFound { reason: ConvergedWithinBudget }` — **not**
`IterationLimit`. Nelder-Mead converges fine; its answer is then discarded at link 4.

Corroborated in-tree, and predating this triage:
`crates/reify-eval/tests/solver_optimality_unproven.rs:123-127` documents the identical
1-param case — *"converges at the infeasible minimum (y ≈ 1mm − 500nm), triggers the
initially-feasible fallback to y=10mm, but iter_limited=false → BestFound{reason~'converged
within iteration budget'} → no warning"*. That 500nm **is** the 5e-7 of link 3.

This has a second consequence, which is why loudness is a separate deliverable:
`W_SOLVER_OPTIMALITY_UNPROVEN` is gated on the `IterationLimit` variant
(`crates/reify-eval/src/engine_eval.rs:6120-6136`), so it cannot fire here. P4 also rules
(b) out independently: a stalling optimizer does not produce an output that is a bit-exact
function of the seed.

## §3 — Candidate (c): Money robustness floor / centrality blend — **RULED OUT by construction**

No probe of its own; two existing anchors already settle it, and duplicating them into a
new fixture would add coverage without adding information.

- The robustness floor is **Money-gated** — `objective_is_money`
  (`solver.rs:820`), gate at `:1755-1760` — and
  `crates/reify-constraints/tests/robustness_floor.rs:397`
  (`non_money_objective_unchanged`) already pins that a non-Money objective is untouched.
  Every probe in this triage uses a `Length` objective.
- `build_centrality_objective` is synthesised **only** when `problem.objective.is_none()`
  (`solver.rs:1847`), so it cannot fire when the author wrote `minimize`/`maximize`.

## §4 — Candidate (d): objective-plumbing defect — **RULED OUT**

The objective reaches the solver and does drive the answer when it can:

- `reify-eval/src/engine_eval.rs` plumbs the `ObjectiveSet` (`GoverningObjective`
  `:3382-3433`, `:6003-6018`).
- `solver_integration.rs:498` (`optimize_with_feasible_initial_point`) shows the objective
  driving 25mm → ≈5mm when a wall exists — reproduced here as **P6**, measured at
  5.000000 mm.
- **P4** settles it directly: a mis-plumbed objective would not make the output a bit-exact
  function of the seed.

The differentiator is not plumbing; it is link 2.

## §5 — Correction to the PRD's item-4 wording

§10 item 4 reads *"`maximize` against a `<= 40mm` bound returns `24mm`"*, which implies a
**one-sided** shape. Measured (P3), a genuinely one-sided `x <= 40mm` returns **36mm**
(= `40mm − 0.1 × 40mm`, the one-sided seed nudge). The reported 24mm requires **both**
bounds — it is the two-sided derived-box midpoint seed.

This matters beyond pedantry: the filing presumed the 24mm was *not* explained by the
seed-fallback and was therefore a second, separate effect. It is the same mechanism, arm 3
of `extract_initial_point`, two-sided branch. There is one defect here, not two.

## §6 — Why the suite was blind

A real defect with fully green coverage, for a structural reason worth recording.

**Every in-tree objective fixture that asserts real progress sets an explicit
`AutoParam.bounds` wall strictly INSIDE the constraint region** —
`solver_integration.rs:498` says so in its own doc comment: *"Auto param bounds
(5mm–100mm) prevent the solver from overshooting the constraint boundary at 2mm, so the
optimizer converges at the bounds floor."* That wall is what supplies the clamp of link 2,
and **that shape never occurs in production**, where `bounds` is always `None`
(`solver.rs:993-997`).

P6 isolates exactly this. Same objective, same solver, wall inside the region:
25mm seed → 5.000000 mm, i.e. the objective moves the answer 20mm. Production shape,
no wall: it moves the answer 0mm.

| shape | wall inside the constraint region? | objective moves the answer |
|---|---|---|
| P1–P4, P7 (**production**, `bounds: None`) | no | **0mm** |
| P6 (the shape of `solver_integration.rs:498`) | yes (5mm–100mm) | 20mm |

Worse, the one fixture that puts the wall **outside** the region —
`solver_integration.rs:618`,
`warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible` (minimize `x` under
`x > 5mm ∧ x < 6mm`, returning exactly the 5.5mm midpoint seed) — encodes the symptom as
**intended behaviour**. So the corpus does not merely miss the defect; part of it pins the
defect in place. `solver.rs:1801-1809` records the same tension from the other side: making
the clamp unconditional breaks that fixture and one other, which is why the question is
bound to the `verify_uniqueness` contract (§7).

## §7 — Disposition: close into owner, split three ways

**No new fix task is filed by this triage.** Every arm already has an owner, and two of
them are bound to each other by a decision the solver source explicitly says must be taken
jointly. Filing a fourth task would fragment an already-owned decision.

| arm | owner | anchor |
|---|---|---|
| **Loudness** of the silent seed-fallback | **#6654 arm 3** (pending) | already names `solver.rs:1998-2032` explicitly in its own scope |
| **The clamp gate** (`floor_applied`, `solver.rs:1809-1825`) | **#5711** | `solver.rs:1801-1809` and `:2605-2612` |
| **The 5e-7 root cause** (`PENALTY_WEIGHT`) | **#6678** (P2 leaf κ) | squared-slack inequalities, retire `PENALTY_WEIGHT`, re-encode centrality |

Notes on each:

- **#6654 arm 3** makes the fallback *visible*; it does not make the answer right. P7
  measures why that is worth doing on its own: the wrong number currently returns with
  **zero** diagnostics, and `W_SOLVER_OPTIMALITY_UNPROVEN` structurally cannot fire here
  (§2).
- **#5711** owns the clamp gate *jointly with the `verify_uniqueness` contract*.
  `solver.rs:1801-1809` records that the unconditional-clamp form fails two named non-Money
  drift-fallback fixtures and produces `ConstraintNonUnique` via the flat-objective
  mechanism, and concludes: *"Revisit both together; neither is actionable in isolation."*
  This triage does not reopen that.
- **#6678** owns the 5e-7 trigger. **#6688 was CANCELLED-absorbed into #6678 on
  2026-08-27**, so any surviving `#6688` pointer (including the one in this task's own
  description) is stale and should be repointed at **#6678**.

One correction to how the 5e-7 has been characterised: it is not merely "three orders of
magnitude too small" relative to `FEASIBILITY_THRESHOLD`. It is the **trigger** that the
seed-fallback then **amplifies** — 5e-7 of undershoot is converted into a 0.8mm (P1) or
16mm (P2/P7) deviation, because the fallback discards the optimum wholesale rather than
projecting it back onto the bound.

Finally, on severity: P4 means this should not be dispositioned as a solution-quality or
tolerance issue. The answer is a bit-exact function of the seed, so the objective is not
being *approximately* honoured — it is not being honoured at all.

## §8 — The probes are a tripwire, not a specification

Both probe files characterise **current** behaviour and are **expected to go RED when
#5711 or #6678 lands**. Every assertion message says so. A RED here is the signal to
re-measure and update the tables — in the probe files and in this note — not a regression
to revert. No probe asserts the desired behaviour ("minimize reaches 8mm"), because that
would be a doomed RED that no in-scope change could turn GREEN.
