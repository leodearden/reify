# Autos floored by a derived cell — corpus survey and the defect it actually found

**Task #6146 | 2026-09-20 | branch `task/6146`, based on main `e7a886292e`**

Task #5954 left behind a claim: that `derive_param_intervals` mines only CONSTANT
operands, so a constraint like `a >= side` (with `side` a derived cell) yields no clamp
and a model floored only that way could report a false `Infeasible`. #5954 worked around
it by putting the floor on the `AutoParam` BOX instead.

This note records what a survey of the corpus and a first-hand measurement found:
**the shape does not occur**, **the mechanism claim is false**, and the real defect in
that code had the **opposite polarity** — a silent wrong answer, not a false
`Infeasible`. Steps 1–5 of #6146 closed it; this is the evidence half.

Code authority for the fixed behaviour is `constant_operand_value`'s doc comment in
`crates/reify-constraints/src/solver.rs`. This note is the survey and the measurements,
not a re-derivation of the rule.

---

## 1. The survey: ZERO hits

**Question.** Does any tracked `.ri` model contain an auto that is bounded ONLY by a
constraint against a derived cell, with no equivalent bound elsewhere?

**Method.** All tracked `.ri` files, re-measured at `e7a886292e`:

| Measurement | Command | Count |
|---|---|---|
| tracked `.ri` files | `git ls-files '*.ri'` | 715 |
| …mentioning `auto` | `… \| xargs grep -l '\bauto\b'` | 160 |
| …with BOTH a value-`auto` binding and a `constraint` | `… \| xargs grep -l '=\s*auto\b' \| xargs grep -l '^\s*constraint\b'` | 53 |
| …with a `constraint <ident> <cmp> <ident>` (ref-vs-ref) | `grep -lE '^\s*constraint\s+[\w.]+\s*(>=\|<=\|>\|<)\s*[\w.]+\s*$'` | 34 |

**Result: ZERO instances of the shape.** Two structural reasons, each sufficient alone.

### (a) There is no auto BOX syntax, so the qualifier cannot discriminate

`tree-sitter-reify/grammar.js` `auto_keyword` admits exactly three forms — `auto`,
`auto(free)`, and `auto(name = value, …)` — and the third is pose seeding, not bounds.
`docs/reify-language-spec.md:180` states the intent directly:

> To bound a solver-delegated value, attach a constraint to the binding rather than
> embedding `auto` in an expression.

Corroborated in-code: `compose_interval`'s doc records that `AutoParam.bounds` "is always
`None`" on every production path (all three construction sites in reify-eval hardcode it).

So "with no equivalent BOX bound" is trivially true of EVERY auto in the corpus and
cannot narrow anything — and **#5954's workaround (put the floor on the BOX) is available
only to hand-built Rust fixtures, never to a real `.ri` model.**

### (b) No auto is floored by a derived cell, in any file

The claim that autos and ref-vs-ref constraints are DISJOINT POPULATIONS at the FILE
level is **too strong, and this survey corrects it**: two files carry both.

- `examples/integration_corner_cases.ri` — autos `x`/`y` (`auto(free)`, :175–176, no
  constraint names either); the ref-vs-ref constraint is `constraint a < d` (:134) over
  `param a…d`, literal-defaulted params in a different scope.
- `examples/integration_full_v01.ri` — autos `load_auto` (:136, never constrained) and
  `load_free` (:137, constrained only against LITERALS at :295–296); the ref-vs-ref
  constraints `height > width` / `width > clearance` (:239–240) are CO-SCOPED with them
  but name no auto, and their operands are literal-defaulted `param`s.

The accurate statement — weaker, and still sufficient — is that **no ref-vs-ref
constraint in the corpus has an auto on one side and an auto-dependent derived cell on
the other.** Every auto that is constrained at all is constrained against literals.

### The two near-misses, and why each is decided correctly today

| Model | Shape | Verdict |
|---|---|---|
| `examples/fea_bracket_minimize_mass.ri` | `let yield_limit = 310MPa` (:133), used at `.max_von_mises < yield_limit` (:212) | A derived cell reading NO auto — a named alias for a constant. Mining a bound from it is CORRECT and must keep working. (The file's only auto, `thickness` (:87), is `auto(free)`, and the constraint's near side is a method access on a call result, which `derive_from_side` cannot read regardless.) |
| `docs/prds/v0_6/fixtures/discrete_mixed.ri` | `param up : Bool = auto` (:4) with `constraint t >= (if up then 3.0 else 5.0)` (:6) | A genuinely non-constant floor, but INLINE — `collect_value_refs` recurses into `Conditional`, so the pre-existing syntactic guard already sees `up` and rejects it. |

Both are pinned as regression cases in `solver.rs` `mod tests`
(`derive_intervals_still_mines_a_dependent_cell_that_reads_no_auto`,
`derive_intervals_still_rejects_an_inline_far_operand_naming_an_auto`) precisely because
a fix for the real defect could plausibly have broken either.

**Consequence:** no real model regresses to a false `Infeasible`, and #5954's premise
does not fire. There was nothing to fix on that side.

---

## 2. The mechanism claim is FALSE — measured, not inferred

#5954's stated mechanism ("`derive_param_intervals` only mines CONSTANT operands, so
`a >= side` yields no clamp") is wrong twice over. Both read first-hand at
`e7a886292e`:

1. **`constant_operand_value` never tested constant-ness** (`solver.rs`, the
   `collect_value_refs` guard). It rejected an operand only when the expression
   SYNTACTICALLY named an auto, and otherwise EVALUATED it against the `ValueMap`. A
   dependent cell is not an auto, so it sailed through and evaluated to a number.
   MEASURED with a throwaway probe: for `a >= side` with `side = 3*c` folded into the
   map, `derive_param_intervals` returned `a.lo = Some((0.0075, false))`. **A bound is
   DERIVED, not skipped** — the opposite of the claim.

2. **In #5954's own fixture the decisive gate was never operand constant-ness.** It was
   `floor_applied`: the CLAMP box is built only when
   `apply_robustness_floor && objective_is_money(objective)`; otherwise the code takes
   `effective_bounds` wholesale. That fixture minimises a dimensionless quantity, so
   `floor_applied == false` and NO derived clamp is applied for ANY operand — a
   constant-operand constraint would have yielded no clamp either. The fixture worked
   because its explicit `bounds: Some(…)` IS `effective_bounds`.

Nothing on main carried the false claim into source: #5954's deliverable landed under
#5467 (`e3fe52a339`), its `A_FLOOR`/`COUPLING_COEFF` fixture constants are not on main,
and the claim survives only in #5954's task description and on the stale `task/5954`
branch. No doc correction was owed anywhere in the tree.

---

## 3. What the survey actually found: the same gap, opposite sign

`CompiledExpr::collect_value_refs` (`crates/reify-ir/src/expr.rs`) is a purely SYNTACTIC
walk — it recurses into sub-expressions but never expands `dependent_cells`. A derived
cell that transitively reads an auto names no auto itself, so it was invisible to the
guard, while `build_trial_values` had already folded it into the map as a finite number.

The result was **not** a false `Infeasible`. It was a **silent wrong answer**: a quantity
that MOVES with the solve became a HARD clamp, and the auto was held to it while the
value that produced it walked away. Blast radius by consumer:

| Consumer | Derived against | Effect of a bogus bound |
|---|---|---|
| `extract_initial_point` | `current_values` | a bad SEED — recoverable |
| `derived_seed_box` | `current_values` | a bad SEED — recoverable |
| the `floor_applied` CLAMP box | `trial_values` | **a wrong RETURNED VALUE — silent and severe** |
| `verify_uniqueness` | `current_values` | a param counted as constraint-bracketed on a bound the user never wrote — a PRD §11.6 uniqueness false-negative, reachable on the γ path with no Money objective |

This is the same expansion gap #5720 / #5467-LAYER-2 closed on the DECOMPOSITION side,
still open one module over in the derivation family. Because the survey found no model
that triggers it, it was LATENT — the cheapest possible moment to close it.

**The fix** (#6146 steps 2 and 4) reuses `decompose::dependent_cell_auto_reads` verbatim
and widens the guard from "is this ref an auto?" to "does this ref MOVE when the solver
moves?" — an auto, a cell with a non-empty transitive auto set, or a cell absent from
that map (cycle-tainted, auto dependence unknown). decompose.rs is untouched; its
omission-on-cycle semantics remain correct for its own drop-side consumer.

**The one user-visible verdict change**, measured both ways on the same fixture by
flipping only the predicate:

| | `a`'s interval | abstention set | γ verdict |
|---|---|---|---|
| BEFORE | `lo = Some((0.0075, false))`, `hi = None` | `{}` | `ConstraintNonUnique` |
| AFTER | `lo = None`, `hi = None` | `{0}` | `Solved` |

The direction is MONOTONE — the fix can only GROW the abstention set, and
`strict_autos_constraint_bracketed` is monotone in it — so no previously-`Solved` γ model
can newly fail. Pinned by
`gamma_strict_auto_floored_only_by_a_derived_cell_abstains_not_errors`.

**That monotonicity argument is scoped to γ.** Three other things move for this shape and
are NOT covered by it, all latent for the same reason (no corpus model has the shape):

- the SEED paths (`extract_initial_point`, `derived_seed_box`/`multistart_points`) give up
  their #5618 derived start point and fall back to `0.01` / `default_bounds_for`. This is a
  priced trade, not an oversight — the reasoning is on `constant_operand_value`'s doc, and
  `extract_initial_point_derived_cell_floor_falls_through_to_fixed_default` pins it;
- the NON-γ path reuses the same intervals for its PERTURBATION ANCHORS, so a widened box
  re-anchors the confirming re-solve and can move that verdict in either direction;
- the ABSTENTION side's MENTIONS test stays syntactic, so an auto a constraint reaches only
  through a derived cell (`constraint side >= 5` with `side = 3*c`) is still neither
  bracketed nor abstaining — the same §11.6 false negative in the mirror direction.
  Pre-dating this task and unchanged by it; recorded on
  `params_in_underivable_constraints`' doc and tracked as task #7727.

---

## Cross-references

- **#5711** (done) — already re-ran the UNCONDITIONAL clamp and ruled the `floor_applied`
  gate STAYS. "Relax the feasibility handling for penalty-active constraints" is therefore
  **adjudicated, not open**; #6146 did not reopen it.
- **#5720 / #5467 LAYER 2** (`e3fe52a339`) — the same dependent-cell expansion gap, closed
  on the decomposition side. `dependent_cell_auto_reads` is that work, reused here.
- **#6465** — the separate, accepted γ gap (a blend FLAT over its bracket). Untouched.
- `constant_operand_value` in `crates/reify-constraints/src/solver.rs` — the normative
  statement of the rule and of the residual it resolves.
- **#7727** (filed by #6146) — widen the ABSTENTION side's MENTIONS test through dependent
  cells, closing the mirror-direction §11.6 false negative described above.
- **#7728** (filed by #6146) — hoist `DerivationCtx`' `auto_reads`/`cell_ids` pair to once
  per resolution instead of once per entry-point call.
