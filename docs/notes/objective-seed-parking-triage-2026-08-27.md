# Objective seed-parking triage (task #6756)

Triage record for `docs/prds/v0_6/solution-set-completeness.md` §10 item 4:
*"`maximize` against a `<= 40mm` bound returns `24mm`; `minimize` against `>= 8mm`
returns `8.8mm`. Objectives look soft-penalised rather than bound-seeking. Needs triage
before it is called a bug."*

**Verdict: it is a bug — a correctness defect, not a numerics one** (§5: `minimize` and
`maximize` return bit-identical answers). Candidate (a), the silent seed-fallback, is
CONFIRMED; (b), (c) and (d) are ruled out. Nothing here changes
solver behaviour: the fix is already owned, three ways, and the disposition is
close-into-owner (§7).

**Citation convention.** Anchors below lead with the **symbol** and carry the line range
only as a parenthetical hint. That is the house convention this PRD states in its own
header (`docs/prds/v0_6/solution-set-completeness.md:5` — *"Main moves fast —
cite-by-symbol; re-locate lines at implementation time"*), and it binds here because this
note is written to be read **later**, by the owners of #5711 / #6678 / #6654 — i.e.
exactly when the line numbers will have rotted. Grep the symbol first; every line range is
point-in-time, valid only at the HEAD in the provenance block, and one that no longer
matches its symbol is stale rather than a finding.

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
  - `crates/reify-constraints/tests/objective_seed_parking_triage.rs` — probes P1–P6, P8.
    `cargo test -p reify-constraints --test objective_seed_parking_triage`
  - `crates/reify-eval/tests/harness_engine/objective_seed_parking_e2e.rs` — probe P7, at the `.ri`
    driver level. `cargo test -p reify-eval --test harness_engine objective_seed_parking_e2e`

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
| P8a | `min x` s.t. `8mm <= x <= 40mm` | none | **24.000000 mm** (bits `0x3f989374bc6a7efa`) | the seed — **bit-identical to P2's `max`** | yes |
| P8b | `max x` s.t. `x >= 8mm` | none | **10000.000000 mm** = 10 m (bits `0x4024000000000000`) | `default_bounds_for(Length)` upper corner | yes |
| P8c | `min x` s.t. `x <= 40mm` | none | **0.001000 mm** = 1e-6 m (bits `0x3eb0c6f7a0b5ed8d`) | `default_bounds_for(Length)` lower corner | yes |

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

1. **SEED.** `extract_initial_point` (body `:420-440`, doc `:402-419`) resolves, per auto param,
   the first applicable of: (1) the current value; (2) an explicit `AutoParam::bounds`
   midpoint; (3) the **constraint-derived box** (task #5618) — the midpoint when BOTH sides
   were derived, else nudged inward from the single derived bound by
   `max(SEED_NUDGE_REL × |bound|, SEED_NUDGE_ABS)`, with `SEED_NUDGE_REL = 0.1` (`:239`)
   and `SEED_NUDGE_ABS = 1e-6` (`:244`); (4) the fixed `0.01` fallback.

   So a one-sided `x >= 8mm` seeds at `8mm × 1.1` = **8.8mm** (P1), a one-sided
   `x <= 40mm` seeds at `40mm − 0.1 × 40mm` = **36mm** (P3), and a two-sided
   `8mm..40mm` seeds at the midpoint **24mm** (P2). Arm 2 never fires in production
   because `AutoParam.bounds` is always `None` — the *"Constraint-derived parameter
   bounds (task #5618)"* header comment above `default_bounds_for` (`:993-997`) names all
   three construction sites.

2. **NO CLAMP WALL.** The clamp box handed to the optimiser is gated on `floor_applied` —
   the `let bounds = if floor_applied` gate in `solve_core_with_sd_tolerance`
   (`:1809-1825`): the constraint-derived clamp box is used **only** when the Money
   robustness floor fired. A `Length` objective is not Money (`objective_is_money` `:820`,
   its gate in `solve_core_with_sd_tolerance` `:1755-1760`), so the else-branch takes
   `effective_bounds` =
   `default_bounds_for(Length)` = `(1e-6, 10.0)` (`:1585-1594`). There is no wall anywhere
   near the user's bound.

3. **PENALTY UNDERSHOOT.** Cost is `obj + PENALTY_WEIGHT × violation + PENALTY_WEIGHT ×
   bound_penalty` — `ConstraintCostFunction::cost` (`:1539-1548`) — with
   `PENALTY_WEIGHT = 1e6` (`:25`). Minimising
   `x + 1e6·(b − x)²` is stationary at `b − 1/(2 × PENALTY_WEIGHT)` = `b − 5e-7`, i.e.
   **5e-7 outside the active bound**. The solver's own comments state this verbatim — the
   `#5618` header comment above `default_bounds_for` (`:1017-1021`) and
   `solve_core_with_sd_tolerance`'s penalty-undershoot note (`:1776-1781`) — and the
   latter (`:1780-1781`) already names the symptom this triage
   was filed for: *"Deriving from the RAW box instead yields a feasible-but-badly-suboptimal
   answer (the seed, returned via the drift fallback)."*

4. **FEASIBILITY REJECT.** The final check measures the LINEAR residual against
   `FEASIBILITY_THRESHOLD` = 1e-12 (the const at `:20`; the
   `final_max_residual > FEASIBILITY_THRESHOLD` check in `solve_core_with_sd_tolerance`
   at `:1997`). `5e-7 >> 1e-12`, so the converged
   optimum is rejected as infeasible.

5. **SILENT SEED-FALLBACK.** Because the seed *is* feasible, the `initially_feasible`
   drift fallback in `solve_core_with_sd_tolerance` (`:1997-2031`) discards the rejected
   optimum and returns the **untouched initial point** as `Solved`. The objective is ignored. The only trace is a
   `tracing::debug!` — no diagnostic. `warm_start_fallback_returns_exact_initial_values`
   (`crates/reify-constraints/tests/solver_integration.rs:1483`) pins that the return is
   the EXACT initial, which is why every measurement above is bit-exact rather than "near".

**P4 is the decisive evidence.** Holding the constraints, the objective and its sense
fixed and moving only the seed, the answer tracks the seed bit-for-bit: 30mm → 30mm,
12mm → 12mm, and (two-sided, opposite sense) 11mm → 11mm. An output that is a bit-exact
function of the seed under those conditions can only be the seed being returned.

**P8 is the closing evidence — see §5.** Minimise and maximise return the *identical*
answer, bit for bit, on the same two-sided problem. An objective whose *sense* has no
effect on the answer is not a soft-penalty artefact, a partial-progress artefact, or a
seed coincidence. Nothing else in the candidate set survives it.

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
the `SMALL_MM_SOURCE` doc comment ("B6 source") in
`crates/reify-eval/tests/solver_optimality_unproven.rs:123-127` documents the identical
1-param case — *"converges at the infeasible minimum (y ≈ 1mm − 500nm), triggers the
initially-feasible fallback to y=10mm, but iter_limited=false → BestFound{reason~'converged
within iteration budget'} → no warning"*. That 500nm **is** the 5e-7 of link 3.

This has a second consequence, which is why loudness is a separate deliverable:
`W_SOLVER_OPTIMALITY_UNPROVEN` is gated on the `IterationLimit` variant
(the γ-gate in `Engine::eval` — `crates/reify-eval/src/engine_eval.rs:6120-6136`, the
*"γ (task #4804)"* comment), so it cannot fire here. P4 also rules
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
  — its call site in `solve_core_with_sd_tolerance` (`solver.rs:1847`) — so it cannot fire
  when the author wrote `minimize`/`maximize`.

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

## §5 — Sense-invariance, and the sense × shape grid

Not anticipated in the original filing; found by a throwaway probe run during planning
and pinned as **P8**. On the **same** two-sided problem (`8mm <= x <= 40mm`, production
shape, no seed):

| sense | measured | bits |
|---|---|---|
| `Minimize` | 24.000000 mm | `0x3f989374bc6a7efa` |
| `Maximize` | 24.000000 mm | `0x3f989374bc6a7efa` |

**Bit-identical.** On this shape the objective's sense is ignored outright. That is what
takes the finding out of the "tolerance / solution quality" category: no soft-penalty
story, no partial-progress story and no seed-coincidence story survives an answer that is
invariant under negating the objective.

### The full grid — and a corrected prediction

The one-sided controls **contradicted this task's planned prediction** (that a one-sided
shape returns its nudged seed under *either* sense). They are pinned as MEASURED, and the
divergence is filed as **`esc-6756-1`** rather than retuned:

| | `x >= 8mm` | `x <= 40mm` | `8mm <= x <= 40mm` |
|---|---|---|---|
| `Minimize` | **8.800000 mm** — seed | **0.001000 mm** = 1e-6 m — default corner | **24.000000 mm** — seed |
| `Maximize` | **10000.000000 mm** = 10 m — default corner | **36.000000 mm** — seed | **24.000000 mm** — seed |

Both corner values are exactly `default_bounds_for(Length)` = `(1e-6, 10.0)`
(`solver.rs:1585-1594`), so the pinned numbers remain *derived* — just from a different
constant than planned.

This **sharpens** §1's verdict rather than contradicting it. The seed is returned exactly
when the objective points **toward** a derived bound: that is when the penalty term is
active, so the optimum lands 5e-7 outside the bound (link 3), is rejected (link 4), and
the fallback fires (link 5). When the objective points **away**, the optimum is feasible,
nothing is rejected, no fallback fires, and the optimiser simply runs to the default-box
corner. **Both reported numbers (8.8mm and 24mm) are the points-toward case.**

Sense-invariance on the two-sided shape is therefore not a general claim that the sense is
never read — on one-sided shapes the sense decides *which* of two silent failure modes you
get. Neither is the optimum the author asked for.

### Overlap with §10 item 3 — recorded, not taken into scope

The 10 m corner independently reproduces this PRD's §10 **item 3** — *"An inequality-only
strict `auto` with no upper bound parks at the `10 m` default-box corner with no
warning"* — owned by **#6655** / P1-ε **#6692**. The `minimize` mirror image, the **1e-6 m
lower corner**, is *not* named in item 3's wording; it is recorded here for that item's
owner. This triage files nothing for it and does not widen its own scope to cover it.

## §5.1 — Correction to the PRD's item-4 wording

§10 item 4 reads *"`maximize` against a `<= 40mm` bound returns `24mm`"*, which implies a
**one-sided** shape. Measured (P3), a genuinely one-sided `x <= 40mm` returns **36mm**
(= `40mm − 0.1 × 40mm`, the one-sided seed nudge). The reported 24mm requires **both**
bounds — it is the two-sided derived-box midpoint seed.

This matters beyond pedantry: the filing presumed the 24mm was *not* explained by the
seed-fallback and was therefore a second, separate effect. It is the same mechanism, arm 3
of `extract_initial_point`, two-sided branch. There is one defect here, not two.

**Where the verdict lives.** The PRD's §10 item-4 annotation deliberately carries only the
*disposition* — is-it-a-bug, the wording correction, the three owners — plus a pointer
here. **This note is the single source of truth** for the mechanism chain, the probe
table, the severity call and the item-3 boundary. Keep it that way: a second copy of the
verdict in the PRD would drift the moment #5711 or #6678 lands and one of the two copies
is updated. When that happens, re-measure and update *this* file, and touch the PRD line
only if the disposition itself changed.

## §6 — Why the suite was blind

A real defect with fully green coverage, for a structural reason worth recording.

**Every in-tree objective fixture that asserts real progress sets an explicit
`AutoParam.bounds` wall strictly INSIDE the constraint region** —
`solver_integration.rs:498` says so in its own doc comment: *"Auto param bounds
(5mm–100mm) prevent the solver from overshooting the constraint boundary at 2mm, so the
optimizer converges at the bounds floor."* That wall is what supplies the clamp of link 2,
and **that shape never occurs in production**, where `bounds` is always `None`
(the `#5618` header comment above `default_bounds_for`, `solver.rs:993-997`).

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
defect in place. `solve_core_with_sd_tolerance`'s gate note (`solver.rs:1801-1809`)
records the same tension from the other side: making
the clamp unconditional breaks that fixture and one other, which is why the question is
bound to the `verify_uniqueness` contract (§7).

## §7 — Disposition: close into owner, split three ways

**No new fix task is filed by this triage.** Every arm already has an owner, and two of
them are bound to each other by a decision the solver source explicitly says must be taken
jointly. Filing a fourth task would fragment an already-owned decision.

| arm | owner | anchor |
|---|---|---|
| **Loudness** of the silent seed-fallback | **#6654 arm 3** (pending) | already names the `initially_feasible` drift fallback in `solve_core_with_sd_tolerance` (`solver.rs:1998-2032`) explicitly in its own scope |
| **The clamp gate** (`floor_applied` in `solve_core_with_sd_tolerance`, `solver.rs:1809-1825`) | **#5711** | that function's gate note (`:1801-1809`) and `verify_uniqueness`'s doc (`:2605-2612`) |
| **The 5e-7 root cause** (`PENALTY_WEIGHT`) | **#6678** (P2 leaf κ) | squared-slack inequalities, retire `PENALTY_WEIGHT`, re-encode centrality |

Notes on each:

- **#6654 arm 3** makes the fallback *visible*; it does not make the answer right. P7
  measures why that is worth doing on its own: the wrong number currently returns with
  **zero** diagnostics, and `W_SOLVER_OPTIMALITY_UNPROVEN` structurally cannot fire here
  (§2).
- **#5711** owns the clamp gate *jointly with the `verify_uniqueness` contract*.
  `solve_core_with_sd_tolerance`'s gate note (`solver.rs:1801-1809`) records that the
  unconditional-clamp form fails two named non-Money
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

Finally, on severity: **§5 means this must not be dispositioned as a solution-quality or
tolerance issue.** An objective whose sense has no effect on the answer is a
**correctness** defect. P4 says the same thing from the other side — the answer is a
bit-exact function of the seed, so the objective is not being *approximately* honoured, it
is not being honoured at all. Neither reading leaves room for "the numbers are a bit off".

## §8 — The probes are a tripwire, not a specification

Both probe files characterise **current** behaviour and are **expected to go RED when
#5711 or #6678 lands**. Every assertion message says so. A RED here is the signal to
re-measure and update the tables — in the probe files and in this note — not a regression
to revert. No probe asserts the desired behaviour ("minimize reaches 8mm"), because that
would be a doomed RED that no in-scope change could turn GREEN.
