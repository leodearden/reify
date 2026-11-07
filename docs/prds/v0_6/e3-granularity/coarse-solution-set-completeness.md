# E3 coarse-arm re-decomposition — solution-set-completeness (P3)

Part of experiment **E3** (`docs/prds/v0_6/e3-decomposition-granularity-ab.md`). This PRD
randomized to the **coarse arm**: its standard decomposition (12 leaves, #6706–#6719, decomposed
2026-08-26) is retired to `deferred` and replaced by the 6 coarse tasks below. Coarse tasks carry
the full text of their constituents. The carrier leaf α (15 declared files — already above the
E3 ≤~12-file cap on its own) and the PRD-close leaf μ are preserved as singletons, reused in
place: cross-PRD prose seams cite #6706/#6711 by id (P4's #6751 stamps them), so re-filing copies
would break those bindings.

**Mapping:**

| Coarse task | Constituents | Reduction |
|---|---|---|
| P3-C1 (reused #6706) | α #6706 — the Completeness carrier, unchanged | 1→1 |
| P3-C2 | γ #6708 + ε #6710 + λ #6716 | 3→1 |
| P3-C3 | β #6707 + δ #6709 | 2→1 |
| P3-C4 | ζ #6711 + η #6712 + κ #6718 | 3→1 |
| P3-C5 | θ #6713 + ι #6715 | 2→1 |
| P3-C6 (reused #6719) | μ #6719 — PRD close, deps rewired to C1–C5 | 1→1 |

12 leaves → 6 tasks (2.0×). Out-of-PRD edges preserved: C2 ← #6655; C3 ← #6653; C4 ← #6691,
#6699; C5 ← #6659, #6677.

## P3-C1 = #6706 (α) — Completeness/SolutionSet carrier, reused unchanged

Deps unchanged (none). 15 declared files; root of the batch. The P2-μ #6680 ranked.rs seam note
and every invariant (C1–C4) apply as written.

## P3-C2 (γ+ε+λ): box branch-and-bound enumerator + refutation + envelope calibration

**Deps:** #6655 (hard — HC4 propagator), P3-C1. **Priority:** high (γ's).
**Files (3):** crates/reify-constraints/src/enumerate.rs, crates/reify-constraints/src/lib.rs,
crates/reify-core/src/diagnostics.rs

One coherent subsystem: the γ enumerator (one engine, three verdicts, C6/C7/C8 invariants, angle
canonicalisation), the ε refutation-by-subdivision surface (typed proven-infeasible naming the
narrowing constraint, zero solver iterations — including constructing the subdivision-refutation
fixture that root-box HC4 alone cannot decide), and the λ envelope calibration (measure the
dimension/node envelope, set TWO caps from data, commit the dated docs/notes/ measurement).
Internal order γ → (ε ∥ λ). C7's node-count-never-wall-clock rail applies to the whole task.

Combined signal: (γ) BT1 — the two-root fixture enumerates Exhaustive with exactly 2 solutions
in canonical box order, identical across two runs; (ε) BT5 — the newly constructed fixture emits
the typed refutation naming the narrowing constraint with zero Nelder-Mead iterations, plus the
companion root-box-undecided assertion; (λ) BT7 — a fixture at the measured boundary degrades to
Partial{DimensionAboveEnvelope} within the node budget, with the dated note committed.

## P3-C3 (β+δ): honesty floor + verdict policy + cluster attribution

**Deps:** P3-C1, P3-C2 (δ needs γ), #6653 (δ's toleranced-verdicts edge). **Priority:** high.
**Files (5):** crates/reify-constraints/src/solver.rs, crates/reify-constraints/src/registry.rs,
crates/reify-eval/src/engine_eval.rs, crates/reify-eval/src/engine_edit.rs,
crates/reify-core/src/diagnostics.rs

β (adopts #5388: stop asserting unproven uniqueness, typed cause, code the five free-auto sites;
scope rails a+b; coordinate-not-race with #5711) then δ (D1 verdict policy wiring γ's enumerator,
retire the unconditional free-auto warning, per-culprit cluster attribution, the F2/a2 ruling on
Partial{DomainUnbounded} anchored values). β's "#5388 acceptance (b) completes at δ" becomes an
intra-task completion criterion.

Combined signal: (β) `reify eval` on ssc_two_roots_strict.ri names the alternative root and a
typed non-uniqueness cause; (δ) ssc_two_roots_free.ri names BOTH roots and which was selected,
ssc_two_roots_strict.ri errors naming both, ssc_single_root_free.ri emits NO warning.

## P3-C4 (ζ+η+κ): basin identity — dedup + explain rendering, warm-re-solve stability, docs-truth

**Deps:** P3-C3 (ζ,η need δ), #6691 (P1-γ deterministic axis order — ζ's hard prereq), #6699
(P1-κ entry-point parity — η's hard prereq). **Priority:** medium.
**Files (7):** crates/reify-eval/src/engine_eval.rs, crates/reify-eval/src/engine_edit.rs,
crates/reify-cli/src/main.rs, crates/reify-mcp/src/tools/chunks/constraints.md,
examples/best_practices/discrete_choice.ri, examples/best_practices/INDEX.md,
.claude/skills/reify-design/SKILL.md

ζ (stop discarding the alternatives; dedup by basin box — D3, no tolerance constant; attach the
Completeness verdict; render in `reify explain`), η (continuation from the incumbent +
W_BASIN_CHANGED; close the cold-only emission hole on the warm path; scope boundary vs #6699
parity respected), κ (docs-truth: constraints.md multiplicity section, discrete_choice.ri
correction + INDEX row, cheatsheet line, discoverability). Internal order ζ → η → κ.

Combined signal: (ζ) BT10 — `reify explain` on a multi-basin objective fixture prints the
deduplicated set with its completeness verdict; (η) BT11 — a two-basin GUI/edit_param edit keeps
the incumbent basin or emits W_BASIN_CHANGED naming the move, cold and warm agree on the set;
(κ) the updated chunk's behaviours compile in a smoke .ri, the corrected exemplar passes the
corpus gate, INDEX matches, discoverability recorded.

## P3-C5 (θ+ι): C1 conformance sweep + the completeness composition law

**Deps:** P3-C1, P3-C3 (ι needs δ), #6659 (both sequence after it — in-progress on registry.rs),
#6677 (P2-ι UnifiedProblem — θ's hard prereq). **Priority:** medium.
**Files (3):** crates/reify-constraints/src/solvespace.rs,
crates/reify-constraints/src/relate_solve.rs, crates/reify-constraints/src/registry.rs

θ (no `unique` written from anything but a completeness verdict; sweep remaining write sites
after #6677; retire Partial{NotAttempted} from in-tree producers; expect small fixture fallout,
fix forward) and ι (the §3.5 Completeness meet replacing the boolean conjunction in
SolverRegistry::solve_inner, declined-component semantics per the #6681 seam note). Both
constituents' "sequence after #6659, do not race a live claimant" rule applies to the whole task.

Combined signal: (θ) BT9 — a geometric/relate fixture landing in one of several configurations
no longer reports fully determined; no `unique: true` literal outside the C1 helper; (ι) BT12 —
a two-component fixture (one Exhaustive, one Partial) reports Partial overall naming the
responsible component.

## P3-C6 = #6719 (μ) — PRD close + P4-co-tenancy manifest fix, reused unchanged

Deps rewired: was {all 11 siblings} → now {P3-C1, P3-C2, P3-C3, P3-C4, P3-C5}. Text untouched.
