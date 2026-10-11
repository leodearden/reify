# PRD — Catalog membership: `constraint x in C`, a hard domain, with `discrete_set` as its sugar

> **Status:** active — authored 2026-10-07 via `/prd` (Leo + Claude, agent team), from Leo's
> rulings on esc-5470-5 (2026-10-06/07; recorded in task #5470 `metadata.x_ruling`) and four more
> on this draft (Q-S–Q-V, 2026-10-08; §10). An adversarial review of the draft is folded in.
> Decomposed 2026-10-08: leaf ids are in §9; the capability manifest and its stamped sidecar sit
> beside this file. Landed by docs carrier #8353.
>
> **Milestone:** v0.6. **Approach:** B + H — the grammar/parser is a load-bearing seam and the
> work spans eight crates plus the GUI editor grammar. Contract in §5, boundary tests in §6.
>
> **Code anchors** are cited by symbol. Substrate was read on main `b0e15c946f`–`90b846861f`
> (2026-10-07); any `path:line` below is a dated snapshot of that range. Main moves fast —
> re-locate lines at implementation time.

---

## 1. Goal and consumers (G1)

A designer picks a standard size by saying so:

```
structure BoltedJoint {
    param bolt_length : Length = auto
    param plate_a : Length = 6mm
    param plate_b : Length = 8mm
    param nut_h   : Length = 6.5mm
    constraint bolt_length in standard_bolt_lengths()
    constraint bolt_length >= plate_a + plate_b + nut_h + 2mm   // thread engagement
    constraint bolt_length <= plate_a + plate_b + nut_h + 5mm   // protrusion
}
```

`reify eval` resolves `bolt_length = 25mm`: the one stock length that fits. If no stock length
fits, the designer gets an Error that names the catalog, the two members either side of the gap
and the constraint each one breaks — never a silent non-stock value. If several fit and the auto
is strict, the designer is told which ones (and can add an objective or write `auto(free)`).
`reify check` flags an overridden `bolt_length = 22mm` as a violated constraint, because
membership is part of what a valid design is. The catalog can be a stdlib function, a user
function, a local `let` list, a `param` list passed down from an assembly, or a range
(`n in 1..8`). `@solver_hint("discrete_set", C)` keeps working as a shorthand for exactly this
constraint.

**Consumers (each one runs in CI or is a user surface):**

- **Designers**, through `reify eval` / `reify check` / the GUI on any model that writes
  `x in C` or the `discrete_set` hint (CLI output, coded diagnostics).
- **PRD 2 ε (#5471)** — `examples/discrete_stock_cost.ri`, the cost-argmin stock pick
  (`docs/prds/v0_6/discrete-cost-minimisation.md` §5 B8, §8 ε). It depends on this PRD's γ.
- **The MCP doc chunks, the best-practices corpus and the reify-design skill index** (δ), so an
  author who wants "pick from standard sizes" finds the mechanism.
- **Engine-integration seam:** §3.5 ConstraintSolver (`docs/prds/v0_3/engine-integration-norm.md`).
  The domain extractor and the refusal live inside that seam; no new seam is introduced.

## 2. Background

### 2.1 What exists today (measured on main, fresh debug build, 2026-10-06/07)

| Probe | Result today |
|---|---|
| `@solver_hint("discrete_set", standard_bolt_lengths)` on a Length auto, window [22.5, 25.5] mm | `24mm` — a non-stock length, the window midpoint; no diagnostic, exit 0 |
| same, window [22.5, 24.5] mm (no member fits) | `23.5mm`, silent |
| same, `constraint bolt_length == 22mm` | `22mm`, silent |
| `@solver_hint("discrete_set", sizes)` where `sizes` is a sibling `Length` param | compiles clean; hint ignored |
| `constraint x in [22mm, 24mm]` | parse error (`in` is not a binary operator) |
| `@solver_hint("discrete_set", standard_bolt_lengths())` (a call) | warning "requires a collection reference", hint dropped |
| `constraint a + b == 0.3m`, a = 0.1m, b = 0.2m (`reify check`) | VIOLATED, exit 1 — `==` is exact f64 |

`@solver_hint` is compiled and stored on `ValueCellDecl.solver_hints`
(`crates/reify-compiler/src/types.rs`, `SolverHint { kind, collection: String, span }`) and read
by nothing outside the compiler and the doc renderer (audit finding M-008,
`docs/architecture-audit/findings/solver-hint-payloads.md`).

### 2.2 The "local catalogs are compile-rejected" premise was a parse defect

PRD 2 §2.3 (probe 2026-07-24) recorded that a local-`let` catalog is compile-rejected
(`unknown selector kind '@solver_hint'`) and scoped catalogs to stdlib functions. The rejection
was never a catalog rule. `@` is also the ad-hoc port-selector operator
(`tree-sitter-reify/grammar.js::ad_hoc_selector`, `expr @ ident(args)`), so an annotation on the
line after a member that ends in a value parses as `<that value> @solver_hint(...)`. With the
annotation placed first, a `let` or `param` catalog compiles clean today: the validator
(`annotations.rs::validate_solver_hint_collections`) accepts any in-scope name. The parse defect
is task #8300 (high). Leo ruled the restriction lifted (esc-5470-5).

### 2.3 Why a constraint, not a hint (Leo, esc-5470-5, ruling J)

Spec §12.1 says hints change *how* the solver searches, not *what* is valid, and "may" be ignored.
A stock-size guarantee is a statement about validity, so it belongs in a constraint:

- `reify check` and `@test` verify it on determined values, including overrides.
- A catalog is an ordinary expression with an ordinary read-set, so `let`/`param` catalogs,
  dependency tracking and the warm edit path come for free (the reverse index registers a
  constraint under every `ValueRef` it reads, `reify-eval/src/deps.rs`).
- "No member fits" is ordinary infeasibility with the membership constraint named.
- Hints stay advisory; `discrete_set` is defined as shorthand for the constraint.

The alternative — making the hint itself a hard domain channel — would have needed a second
validity channel threaded through every AutoParam builder, a catalog resolver with its own
dependency edges, and a spec exception. Leo ruled the pivot.

## 3. Rulings this PRD implements (esc-5470-5, Leo 2026-10-06/07)

| Ruling | Content |
|---|---|
| A | A resolved catalog is a **hard** domain: the solver returns only a member, or an Error. |
| B | A distinct Error when no member satisfies the constraints, plus a test. |
| C | An unresolvable catalog is an **Error**, never a warning-and-solve-without-it. |
| J / K | Membership is a constraint, spelled `x in C`; `@solver_hint("discrete_set", C)` compiles to it. |
| 2 | Catalogs may be any in-scope list: stdlib fn, user fn, `let`, `param`, constructor argument. |
| L | Domain extraction from membership lives in the **solver layer**, so the cold and warm paths both get it from the constraints they already pass. |
| N | This PRD's docs leaf owns the spec `in` entry, §12.1's last paragraph and §10.7's smart-defaults paragraph. PRD 2 η (#5473) is unchanged. |
| O | Membership compares members with the **same Scalar-equality verdict as `==`** (D3). Re-confirmed 2026-10-08 (Q-S) after the original premise — that `==` is toleranced — was found false. |
| P | A membership the solver cannot turn into a domain, or one that reaches the continuous solver, gets a **typed refusal**. |
| Q | A range right-hand side (`n in 1..8`) is in v1. |
| R | One PRD. PRD 2 is amended in the same docs landing. ε (#5471) depends on this PRD's γ. |

## 4. Resolved design decisions

**D1 — Precedence, associativity, parse hazards.** `in` binds looser than ranges (`..`, `..<`,
the prefix forms), comparisons, arithmetic and the symbol logicals `&&`/`||`, and tighter than the
keyword logicals `not`, `and`, `or`, `implies`. So `n in 1..8` is `n in (1..8)`, `x + 2mm in C`
is `(x + 2mm) in C`, `a in A and b in B` is `(a in A) and (b in B)`, `not x in C` is
`not (x in C)`. `in` does **not** chain: `a in B in C` is a compile Error (and so is
`x in C || y in D`, which parses as a chain because `||` binds tighter; the diagnostic suggests
`or`). `in` takes no part in the chained-comparison desugaring (`a < b < c`).
- `grammar.js`: `prec.left(-1, …)` in `binary_expression`. Measured on a scratch copy: no new
  conflicts; 261/261 corpus tests pass; every one of the 770 tracked `.ri` files that parses
  cleanly today parses to an identical tree; the `forall … in`, `[i in …]`, `joint … in` and
  `port … : in` positions are unaffected (their `in` follows an identifier, type or keyword slot).
  At comparison precedence (4), `n in 1..8` silently parses as `(n in 1)..8` — the reason for -1.
- A quantifier, conditional or lambda body is shorter than `in`: `forall v in xs: v in C` parses
  as `(forall v in xs: v) in C` (the same pre-existing shape as `forall v in xs: v > 0 and v < 3`).
  α emits a coded syntax diagnostic when the left operand of `in` is an unparenthesised
  quantifier, conditional or lambda, telling the author to parenthesise the body. The general
  quantifier-body precedence issue (it affects `and` today) is filed separately.
- GUI editor grammar (`gui/src/editor/reify.grammar`): `range` sits looser than every keyword
  logical today, so D1 needs `range` moved above `kwNot` in the `@precedence` block plus an `in`
  level between them — a visible editor-parse change, pinned in
  `gui/src/__tests__/reifyGrammarCorpus.test.ts`.
- Spec §16 gets an `in` level between comparison (11) and `not` (12).

**D2 — Operand typing.** `e in C` type-checks when `C : List<T>`, `Set<T>` or `Range<T>` and
`e : T` (same dimension for Scalars; the same enum for Enums; Int, Bool, String also allowed). The
result is `Bool`. Anything else is a coded compile Error: a non-collection right operand, a
dimension mismatch (`DimensionMismatch`), an element-kind mismatch. The six-op comparison guard
(`expr.rs::emit_comparison_operand_diagnostics`) rejects a List operand, so `in` gets its own guard.

**D3 — Equality (ruling O).** Membership in a List or Set holds when some member is equal to `e`
under the **same Scalar-equality verdict `==` uses** — one implementation, called from both.
Today that verdict is exact f64 (`reify_expr::eval_eq`; `SimpleConstraintChecker` adds nothing).
Task #6653 (pending, ruled 2026-08-26) replaces it with a single-sourced relative tolerance plus a
dimension-aware absolute floor for **all** Scalar equality verdicts; membership is one of them and
picks the policy up from the same place, never through a second epsilon. On the solve path the
question is moot: CP-SAT assigns a member's own `Value`. The exposure is check-time membership of
a *derived* value (`constraint plate_a + plate_b in standard_sheet_thicknesses()`), identical to
`==`'s exposure today.

**D4 — Evaluation.** Kleene, matching spec §9.2.2 for comparisons:
- an Undef `e` or Undef `C` gives Undef (the strict-Undef precheck in `reify_expr::eval_binop`);
- List/Set: `true` if some member is equal; otherwise `Undef` if some member comparison is Undef;
  otherwise `false`. An empty List/Set gives `false` (as `exists` over an empty collection);
- Range value: interval test honouring `lower_inclusive`/`upper_inclusive` and absent bounds.
`In` returns from `eval_binop` before the post-result `OpContractViolation` push, as
`And`/`Or`/`Implies` do, so a Kleene Undef is not reported as an operator contract failure. At
check time an Undef result is Indeterminate attributed through the operand's existing undef
provenance (INV-SF-1/4); the solve path turns an Undef catalog member into an Error (D6).

**D5 — Range right-hand sides.**
- **Range literal** (`e in lo..hi`, `e in lo..<hi`, `e in >=x` …): the compiler lowers it to the
  equivalent comparison conjunction (`lo <= e and e <= hi`; `..<` gives `e < hi`; `>=x` gives
  `e >= x`, and so on), keeping the source spans. Range membership then has one meaning — two
  comparisons — and every comparison path handles it: DimensionalSolver residuals, interval
  derivation, and Int bound-mining.
- **Int bound-mining with solve-invariant bounds.** #5470 mines direct comparisons of the bare auto
  against compile-time constants only, so `n in 1..max_n` (with `param max_n : Int = 8`) would have
  no mined upper bound. β extends the mining to bounds that are **solve-invariant** (D6's
  predicate), evaluated from `current_values`. Editing `max_n` dirties the constraint (it reads
  `max_n`), so the re-solve sees the new bound.
- **Range-valued right operand that is not a literal** (`let w = 22.5mm..25.5mm; t in w`): stays
  `BinOp::In`. On an Int auto with a solve-invariant `w`, D6 turns it into the finite domain
  `lo..=hi`; a domain wider than `MAX_INT_DOMAIN` is **rejected** with #5470's typed
  `AutoNoFiniteDomain` (never truncated). On a Scalar auto it is refused by D7 with a message to
  write the range inline — a documented v1 limitation (δ).

**D6 — Domain extraction (ruling L).** One solver-layer function turns a problem's membership
constraints into auto domains. Inputs: `ResolutionProblem.current_values`, `.functions`, the
problem's autos and its dependent-cell auto reads — all present in the solver layer on both paths.
- **Solve-invariance.** One predicate decides whether an expression can change during this solve:
  it reads an auto of the problem, a dependent cell that reads one, or a cycle-tainted cell. It is
  today's `solver.rs::DerivationCtx::varies_with_solve`, moved to a shared home in
  `reify-constraints` and used by the extractor, the Int-mining extension (D5) and the refusal (D7)
  — not copied.
- **Shape.** Only a top-level conjunct (a constraint, or a member of a top-level `and`) of the form
  `ValueRef(a) in C`, `a` an auto of the problem and `C` solve-invariant, contributes a domain.
  Everything else stays an ordinary predicate (CP-SAT evaluates it during forward checking) or is
  refused by D7.
- **Evaluation.** `C` is evaluated once with `EvalContext::new(current_values, functions)`. A List
  or Set gives its members; an Int Range gives `lo..=hi`; a Scalar Range gives no domain (D5).
- **Catalog validity (ruling C).** For **every** `In` node in the problem whose catalog is
  solve-invariant — extractable or not — the catalog is evaluated before search: Undef, an empty
  collection, or a collection containing Undef is a coded Error (`CatalogUnresolvable`) naming the
  auto(s), the constraint and the catalog. This also closes the CP-SAT forward-check gap where an
  Undef membership result is "don't prune" and would accept a non-member. Duplicates are removed
  (first occurrence kept, order preserved — `DiscreteDomain::new`, #5470).
- **Several sources.** Several membership conjuncts on one auto intersect (order of the first); a
  membership domain also intersects a declared domain (#5470's Enum declared variants). An empty
  intersection is reported by the extractor itself as `NoCatalogMemberFeasible` (D8) — an empty
  `DiscreteDomain` cannot be constructed, so it must never reach CP-SAT.
- **Where it runs.** At the start of `registry.rs::decompose_prelude`, so `solve_inner` and
  `objective_consumption` keep sharing one prelude body, and its domains are written into
  `AutoParam.domain` before decompose — so decompose (#5470's discrete widening),
  `DiscreteFirstFallback::route`, ζ's mixed path (#5472) and the per-component `sub_auto_params`
  all see the same domains. `CpSatSolver`'s domain construction calls the same function when used
  directly. One implementation; re-applying it is idempotent.
- **Known gap with a live owner.** The warm edit builders pass `dependent_cells: Vec::new()`, so on
  the warm path a catalog that reads an auto **through a `let`** (`let cat = [a, a + 2mm]`, `a`
  auto) looks solve-invariant and would be extracted from `cat`'s stale value. #6690
  (solver-driver-parity P1-β, pending, high) deletes the warm builder and routes both edit paths
  through `build_solver_problem`, which populates `dependent_cells`; that closes it. β pins the cold
  behaviour (B9) and leaves a note on #6690. A direct auto reference in `C` is caught on both paths.

**D7 — Typed refusal (ruling P).** An `In` node is unenforceable by `DimensionalSolver` exactly
when an operand subtree is **not** solve-invariant (D6's predicate). A constraint containing such a
node is refused with a coded Error (`MembershipNotSolvable`) naming the constraint and the auto(s)
it reads; the hint text depends on the shape (an extractable top-level `x in C` gets no rewrite
advice; `x + 2mm in C` is told to restate it as `x in C'`; a Scalar Range value is told to inline
the range). An `In` whose operands are all solve-invariant — a determined param, a catalog
function, or a discrete auto that ζ has already fixed into `current_values` for its inner solve —
is a constant during the solve, evaluates normally, and is never refused.
- **Placement:** one guard function, shared with #5470 item 5's "an auto with a domain must not
  reach a domain-ignoring solver" refusal, called by `DimensionalSolver` before any solve work on
  **both** entry paths: `solve_with_meta` and `solve_ranked_impl` (before its multistart loop,
  which calls `solve_core` directly and would otherwise bypass a `solve_with_meta`-only check).
  Placing it in the solver (not the registry) covers `SolverRegistry::new(single)` and direct
  callers.
- Without it, `BinOp::In` falls into the flat 0/1 `_` arm of `constraint_residual` /
  `constraint_violation` and returns either an accidental pass or a misleading "max absolute
  residual" (measured).
- On a component CP-SAT solves (all autos finite from any source), `in` is an ordinary predicate
  during forward checking — no refusal.
- The engine records the refused autos' undef cause from the refusing diagnostic's code, not the
  generic `SolveFailed { "infeasible" }` (`engine_eval.rs::record_failed_autos`, cold and merged
  paths) — INV-SF-1.

**D8 — "No catalog member feasible" (ruling B).** When CP-SAT's enumeration completes with no
feasible assignment and at least one auto of the component carries a membership-derived domain
(CP-SAT re-runs the extractor on this failure path to recover which autos and catalogs those are,
since `AutoParam.domain` itself is producer-agnostic), the verdict is a coded Error
(`NoCatalogMemberFeasible`) instead of the generic `ConstraintUnsatisfiable`. Content:
- the auto(s), the catalog(s) and their member counts;
- for a **single** catalog auto with an orderable (numeric) domain: sort the members, record each
  member's first violated constraint (a post-hoc pass on the failure path, `|members| ×
  |constraints|` evaluations with the forward-check context), and report every adjacent pair whose
  violated constraints differ — the members either side of a gap — each with its constraint. For
  the bolt example: "20mm violates `bolt_length >= plate_a + plate_b + nut_h + 2mm`; 25mm violates
  `bolt_length <= plate_a + plate_b + nut_h + 4mm`". If every member violates the same constraint,
  report that constraint and the member count;
- for several catalog autos, or a non-orderable domain: "no combination of members satisfies the
  constraints", naming the autos and catalogs (per-member attribution is unavailable there —
  forward checking prunes prefixes).
The refused/infeasible autos' undef cause carries this code (as D7).

**D9 — `discrete_set` is sugar (ruling J).** `@solver_hint("discrete_set", C)` on a cell `x`
compiles to the constraint `x in C`:
- a bare name that resolves to a zero-argument function compiles to a call of it, using the
  function's declared return type (today's spelling `standard_bolt_lengths` keeps working); a name
  that resolves to a value cell compiles to a reference; any other expression argument
  (`[1mm, 2mm]`, `standard_bolt_lengths()`, `1..8`) compiles as an ordinary expression in the
  cell's scope (`AnnotationArgValue::Expr` already carries the AST) instead of being dropped;
- the synthesized constraint is appended **after** the structure's own constraints, so existing
  `S#constraint[N]` ids and labels do not shift; it carries a label naming its origin
  (`discrete_set on bolt_length`) and the annotation's span;
- a hinted cell inside a guarded group gets its constraint in that guarded group (see I1's scope);
- every declaration site that builds a cell honours the hint — the param and `let` arms
  (`entity.rs`, `guards.rs`) and the sites that today write `solver_hints: vec![]`
  (`compile_builder/entities_phase.rs` auto sub-override cells, `connect.rs`, the port-member param
  site named in `docs/prds/v0_6/compiler-type-hygiene.md`) — or rejects it with a code (INV-SF-3),
  through the shared decl-construction helper where one exists, not a fourth copy;
- on an auto it restricts the solve; on a determined param or a `let` it is checked like any other
  constraint (the declaration is consumed either way).

**D10 — `prefer_stock` and `preferred_strategy`.** Not wired (their meaning is a soft preference —
an objective term — not validity). **γ makes them loud:** each emits a coded Warning
(`SolverHintNotConsumed`) saying the hint is not yet consumed, so a declared intent is never
silently inert (umbrella principle, INV-SF-3). Ruled 2026-10-08 (Q-T): the Warning ships in γ
#8357, and the real semantics are a human decision gate, `[MILESTONE]` #8360, gated on γ.

**D11 — Diagnostic codes** (names tactical; one code per distinct user remedy, each with an
`E_`/`W_` mnemonic in its doc comment per `reify-core/src/diagnostics.rs` convention):
`MembershipOperandKind` (E, compile), `MembershipChained` (E, compile), `MembershipBodyNeedsParens`
(E, syntax — may fold into the chain code), `CatalogUnresolvable` (E, solve), `NoCatalogMemberFeasible`
(E, solve), `MembershipNotSolvable` (E, solve), `SolverHintNotConsumed` (W, compile). `DimensionMismatch` is reused for dimension errors. Every Error-severity path exits non-zero
(INV-SF-2; `reify eval` and `reify check` both exit 1 on an Error today, measured).

**D12 — Several feasible members.** A catalog auto follows its declaration (P3 §3.4 verdict policy,
`docs/prds/v0_6/solution-set-completeness.md`):
- strict `auto`: one feasible member → resolves; more than one → the strict-auto non-uniqueness
  **Error naming the solutions** (#6554 implements it for CP-SAT; β depends on it). Without #6554
  CP-SAT returns the first member with `unique: false` and no diagnostic;
- `auto(free)`: the first feasible member in catalog order (PRD 2 D4 determinism), with the
  existing non-unique Warning;
- with an objective: the argmin over the complete enumeration (PRD 2 β, `ProvenOptimal`).
The spec §10.7 default (robustness / centrality) does not apply to a catalog auto: CP-SAT builds
no centrality objective. δ's §10.7 text says so, and β makes `reify explain` stop reporting
`synthetic-centrality` for an auto whose domain comes from membership
(`engine_eval.rs::scope_qualifies_for_centrality` is a type-plus-inequality test today).

## 5. Contract

### 5.1 Surface (α)

- Grammar: `binary_expression` gains `prec.left(-1, seq(field('left', $._expression),
  field('op', 'in'), field('right', $._expression)))`. `in` stays unreserved (no `word:` rule).
  `tree-sitter-reify/src/` is gitignored and regenerated, not committed. The GUI editor grammar
  changes per D1. #8300 also edits `grammar.js`/`scanner.c`; α follows it (dependency).
- `reify-syntax` needs no change for the operator (`ts_parser.rs::lower_binary_expr` carries the op
  as text); D1's parenthesisation diagnostic is emitted where the CST still shows parentheses.
- IR: `BinOp::In` appended **after** `Implies` (`CompiledExpr::binop` hashes `op as u8`; inserting
  mid-enum would change every cache key holding a later variant).
- Exhaustive `BinOp` matches that get an arm: `type_compat.rs::infer_binop_type` (→ `Bool`),
  `reify_expr::eval_binop` (→ D4, early return), `dual_eval.rs::eval_dual_binop` (an opaque Bool with
  a comparison-class kink — never the arithmetic `{}` arm, whose second match ends in
  `unreachable!`), `gui/src-tauri/src/engine.rs::format_expr`, `reify-test-support/src/builders/expr.rs`,
  and the test-only `type_compat.rs::_exhaustive_binop_check` + `ops` table. Non-exhaustive sites
  that must list `In` explicitly: `dual_eval.rs::subtree_has_kink` (a `false` there is acted on as a
  proof) and `type_compat.rs::resolve_binop` ("in" → `In`). `is_comparison_op` /
  `flatten_comparison_chain` must not include it.
- Range-literal lowering (D5) happens in the compiler, so `BinOp::In` never carries a range-literal
  right operand.
- Classifier (`classifier.rs::ConstraintClassifier::collect_flags`): a `BinOp::In` node sets
  `has_logical`, so a component holding a membership routes to the Logical or CrossDomain slot
  (CP-SAT / `DiscreteFirstFallback`). #6681 replaces name-based classification with capability
  routing; whichever of α / #6681 lands second carries the membership routing across (note on #6681).
- Refusal (D7): the shared guard in `DimensionalSolver` on both entry paths, plus the engine's
  undef-cause recording (`engine_eval.rs`).

### 5.2 Domains (β)

- One function, in the enumeration-domain module #5470 creates
  (`crates/reify-constraints/src/enumeration_domain.rs`), with the full problem as input and
  `Result<per-auto DiscreteDomain, Vec<Diagnostic>>` as output (exact shape tactical). Rules: D6.
- Called at the start of `decompose_prelude`; its domains are written into `AutoParam.domain`
  before decompose. `CpSatSolver` calls it when `domain` is `None` on a direct call.
- `AutoParam.domain` and `AutoParam::is_discrete()` are #5470's (hard, producer-agnostic). This
  PRD adds a producer; it does not redefine either.
- Failure verdicts travel as `SolveResult::Infeasible { diagnostics }` with a code (the engine
  passes them through unchanged); never `NoProgress` (an uncoded Warning in the engine).
- D8's diagnostic replaces CP-SAT's generic verdict in `cpsat.rs::verdict_from_enumeration`'s
  "none found, search complete" arm when a membership domain is present.
- The solve-invariance predicate moves out of `solver.rs` to a shared module; D5's Int-mining
  extension, D6 and D7 call it.

### 5.3 Sugar (γ)

- Lowering per D9, through the shared decl-construction helper where one exists.
- Type errors come from α's typing (D2) on the synthesized constraint, reported at the hint's span.

### 5.4 Invariants

- **I1 — No silent non-member.** On every solve path (cold eval, merged cluster, warm
  `edit_param`, warm `edit_source`, direct solver use), an auto whose membership constraint reaches
  the solve resolves to a member, or the run reports a coded Error. *Scope:* a membership inside a
  `where` group is honoured exactly when guarded constraints are — today they never reach the cold
  solve (#7489, #6572 gap 1) and the warm paths include every guarded arm unconditionally (#8045).
  This PRD does not fix guarded constraints; notes go on #7489 and #8045 so their tests cover
  membership. The warm `let`-catalog case is D6's #6690 gap.
- **I2 — One meaning.** The hint form and the `in` form produce identical values and identical
  diagnostics apart from the constraint label.
- **I3 — No second epsilon.** Membership equality is `==`'s verdict (D3).
- **I4 — Byte-identity for models without `in`/`discrete_set`/`prefer_stock`/`preferred_strategy`**
  (PRD 2 D1). Models whose `discrete_set` hint was silently ignored change behaviour by design
  (B5); models carrying the other two hint kinds gain D10's Warning.

## 6. Boundary-test sketch (two-way)

Fixtures live in `tests/prd-gate/fixtures/` (cited by full path); annotated members come first in a
structure body until #8300 lands. A compiled test that reads a fixture registers its basename in
`scripts/verify.sh`'s `_RUST_COUPLED_RI_FIXTURES` **in the same diff** (PG-DRIFT reds otherwise);
e2e tests extend `crates/reify-eval/tests/harness_engine.rs`, not a new test binary.

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | parse and precedence (α) | `n in 1..8`, `x + 2mm in C`, `a in A and b in B`, `not x in C`, `x in C \|\| y in D`, `forall v in xs: v in C` | CST shapes per D1; `a in B in C` and `x in C \|\| y in D` → `MembershipChained` (suggesting `or`); unparenthesised quantifier body → the parenthesise diagnostic; corpus regressions for `forall v in`, `[i in …]`, `joint … in`, `port … : in` unchanged; GUI corpus pins the same shapes; an `in` line at or left of the member column is reported by the member-continuation guard (INV-SF-7) |
| B2 | check-time membership (α) | `tests/prd-gate/fixtures/cmc_membership_check.ri`: `param b1 : Length = 25mm`, `param b2 : Length = 22mm`, both `in standard_bolt_lengths()` | `reify check`: b1 OK, b2 VIOLATED, exit 1 |
| B3 | operand typing (α) | Mass in a Length catalog; Length in a `List<Int>`; `x in 5mm` | coded compile Errors (D2), exit 1 |
| B4 | refusal (α, stays true after β) | `tests/prd-gate/fixtures/cmc_unextractable.ri`: Length auto, `constraint x + 2mm in standard_bolt_lengths()` | `reify eval`: `MembershipNotSolvable` naming the constraint and `x`; `x` undef with that code as its cause; exit 1 |
| B4b | refusal on the ranked multistart path (α) | two Length autos, a `minimize`, and B4's unextractable `in` | `MembershipNotSolvable`, exit 1 (the multistart loop never runs) |
| B4c | fixed-operand `in` inside a continuous constraint (α) | `enum Metal { Steel, Iron, Copper }`, `param material : Metal = Metal.Steel` (determined), `gap : Length = auto`, `constraint gap >= (if material in [Metal.Steel, Metal.Iron] then 1mm else 2mm)`, `constraint gap <= 3mm` | solves with no refusal; `gap` lies in [1mm, 3mm] (the continuous default puts it at the window centre) |
| B5 | stock pick (β) | `tests/prd-gate/fixtures/cmc_bolt_feasible.ri` (§1's model) | `reify eval`: `BoltedJoint.bolt_length = 0.025 m`, no diagnostic, exit 0 (baseline today: `0.024 m`, silent) |
| B5b | several members fit (β + #6554) | §1's model with upper bound `+ 10mm` (window [22.5, 30.5] mm): 25mm and 30mm fit | strict auto: the #6554 non-uniqueness Error naming 25mm and 30mm, exit 1; the same model with `auto(free)`: `0.025 m` plus the non-unique Warning |
| B6 | no member fits (β) | `tests/prd-gate/fixtures/cmc_bolt_gap.ri` (upper bound `+ 4mm`, window [22.5, 24.5] mm) | `NoCatalogMemberFeasible` naming `standard_bolt_lengths`, 20mm with the lower-bound constraint and 25mm with the upper-bound constraint; `bolt_length` undef with that code; exit 1 (baseline: `0.0235 m`, silent) |
| B7 | local catalogs (β) | `tests/prd-gate/fixtures/cmc_local_catalogs.ri`: `let sizes = [22mm, 24mm, 26mm]`, and separately a `param sizes : List<Length>`, window [22.5, 25.5] mm | both resolve to 24mm, exit 0 |
| B8 | Int range literal (β + #5470) | `tests/prd-gate/fixtures/cmc_int_range.ri`: `n : Int = auto`, `n in 1..8`, `n * 3 >= 10`, `n * n <= 17` | `n = 4`, no diagnostic |
| B8b | Int range with a param bound (β) | B8 with `param max_n : Int = 8` and `n in 1..max_n` | `n = 4`; editing `max_n` to 3 through the warm path gives a coded infeasibility Error (`ConstraintUnsatisfiable` — the range literal lowered to comparisons, so this is Int-mined, not a catalog), never a value above 3 |
| B9 | catalog validity (β) | an empty user list; a list with an Undef member; a catalog reading another auto through a `let` | `CatalogUnresolvable` for the first two (cold and warm); the auto-dependent one is not extracted and reaches B4's refusal on a Length auto (cold; warm per D6's #6690 gap) |
| B10 | warm path (β) | production registry; eval B7's param-catalog model, then change the catalog to `[23mm, 27mm]` through the warm path (`edit_param` if it accepts a List value, else `edit_source`) | the auto re-solves to 23mm through the warm path; a non-member is never returned (I1) |
| B11 | sugar equivalence (γ) | `tests/prd-gate/fixtures/cmc_sugar_equivalence.ri`: the same model twice, once with `@solver_hint("discrete_set", standard_bolt_lengths)` (bare name, annotation first), once with `in` | identical resolved values; the hint form's constraint label names its origin (I2) |
| B12 | sugar on a determined param (γ) | `@solver_hint("discrete_set", standard_bolt_lengths)` on `param b : Length = 22mm` | `reify check`: VIOLATED with the hint-origin label, exit 1 |
| B13 | back-compat (all) | the existing constraint/solver suites, the `examples/` corpus | byte-identical values and diagnostics for models without `in` / hints (I4) |

## 7. Pre-conditions

- **#5470** (PRD 2 δ, re-scoped 2026-10-07): `AutoParam.domain: Option<DiscreteDomain>` (hard),
  `AutoParam::is_discrete()`, `enumeration_domain.rs`, Int bound-mining, typed domain rejection,
  discrete re-widening in decompose, and item 5's domain-loss refusal (D7 shares its guard). β
  depends on it; #5470 depends on **#6967**.
- **#6554** (pending, low, dispatchable): CP-SAT strict-auto non-uniqueness Error (D12). β depends
  on it; priority inheritance lifts it.
- **#8300** (in progress, high): the `@` annotation parse defect; α depends on it (both edit
  `grammar.js`/`scanner.c`, and realistic examples need it).
- **#6653** (pending): toleranced Scalar equality — a seam (§8), not a dependency.

## 8. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner |
|---|---|---|---|
| PRD 2 δ #5470 | this consumes | `AutoParam.domain`, `is_discrete()`, `enumeration_domain.rs`, the domain-loss guard | #5470 owns the channel, type and guard; this PRD (β) adds the membership producer and the `In` refusal to the same guard |
| PRD 2 ε #5471 / B8 | this produces for | γ gates ε (dependency edge); `discrete_stock_cost.ri` uses `in` or the sugar | ε keeps its examples; this PRD owns the mechanism |
| PRD 2 ζ #5472 mixed path | this produces for | discrete autos fixed into `current_values` make an `In` over them solve-invariant, so the inner continuous solve evaluates it (D7); an `In` reading a continuous auto is refused | contract stated here; note on #5472 |
| PRD 2 η #5473 | none | spec "Deferred capabilities" row | η, unchanged (ruling N) |
| #6554 CP-SAT strict-auto non-uniqueness | prerequisite | D12 strict-auto Error | #6554 |
| P3 #6901 (coarse) | adjacent | retypes undef causes and the non-uniqueness message | #6901; D7/D8's coded undef causes are compatible |
| #6653 toleranced equality | shared policy | membership's per-member equality is a Scalar equality verdict (D3) | whichever of #6653 / α lands second routes `in` through the single-sourced policy; note on #6653 |
| #6690 resolution-problem builder unification | this relies on | warm edit paths pass `dependent_cells` | #6690; D6's warm gap closes when it lands (note on #6690) |
| #7489, #6572, #8045 guarded constraints | adjacent | guarded constraints on the cold solve (#7489/#6572) and warm guard flips (#8045) | those tasks; I1 is scoped to unguarded membership until they land (notes on #7489, #8045) |
| #6681 capability routing | adjacent | replaces name-based classification, which α extends with an `In` arm | whichever lands second ports membership routing (note on #6681) |
| #8300 annotation parse | prerequisite | `@` after a valued member; shares `grammar.js` | #8300 |
| #8210 warm re-solves ignore `discrete_set` / Enum domains | superseded | its `discrete_set` half dissolves (D6), its Enum half is #5470 ruling M | cancelled with pointers (Q-V, 2026-10-08) |
| DIC β #5416 capability envelope | adjacent | registry pre-dispatch screen vs D7's solver-head guard | #5416 owns the screen API; D7's guard stays in the solver (covers direct callers) |
| solution-set-completeness #6710 / #6718 | adjacent | Refuted-verdict naming vs D8; κ edits `constraints.md`, `discrete_choice.ri`, INDEX, cheatsheet | P3 owns the generic Refuted verdict; this PRD owns D8; δ edits only its own sections and leaves `discrete_choice.ri` to #6718 |
| #6684 relation-vocabulary chunks | adjacent | also edits `constraints.md` | separate sections; no edge |
| #7984 `contains` documented but uncallable | adjacent | `in` is the working membership surface | δ documents `in` and removes `collections.md`'s claim that `contains` is callable (Q-U, 2026-10-08); #7984 keeps the call-form question |
| `docs/prds/solver-hint-payloads.md` (+ audit M-008) | superseded in part | stored-only hints; its "no warnings under `reify check`" acceptance for `m11_annotations.ri` | amended in this docs landing |
| `solution-set-completeness.md`, `declared-intent-consumption-accounting.md`, `constraint-solver-completion.md` | references | name PRD 2 δ as owner of `discrete_set`, or call `@solver_hint` orthogonal | repointed in this docs landing |

## 9. Decomposition plan

Task ids stamped at decompose (2026-10-08): **α #8355, β #8356, γ #8357, δ #8358, ω #8359**;
decision gate `[MILESTONE]` #8360 (D10); docs carrier #8353. Out-of-batch edges: #8300 → α;
#5470, #6554 → β; γ → ε #5471.

**G7 walk** (reify INV-SF-1..7 + umbrella, every leaf): no unwaived hit. The one residual — a
membership inside a `where` group is ignored as every guarded constraint is today — is owned by
#7489 / #6572 / #8045 (I1's scope), not waived.

- **α #8355 — The `in` operator: grammar, typing, evaluation, check, refusal.**
  Modules: `tree-sitter-reify/grammar.js` + `test/corpus`, `gui/src/editor/reify.grammar` +
  `gui/src/__tests__/reifyGrammarCorpus.test.ts`, `reify-syntax` (parenthesise diagnostic),
  `reify-ir` (`BinOp::In`), `reify-compiler` (`resolve_binop`, typing guard, range-literal
  lowering, no chaining), `reify-expr` (`eval_binop`, `dual_eval`), `reify-constraints` (classifier
  arm; the shared D7 guard in `solver.rs`; the solve-invariance predicate moved to a shared module),
  `reify-eval` (`engine_eval.rs` undef cause), `reify-core` (codes), `gui/src-tauri/src/engine.rs`
  (`format_expr`), `reify-test-support` (builders), `reify-lsp` (`EXPR_KEYWORDS`, optional).
  **Signal (LEAF):** B1–B4c — `reify check tests/prd-gate/fixtures/cmc_membership_check.ri` reports
  b2 VIOLATED and exits 1; `reify eval tests/prd-gate/fixtures/cmc_unextractable.ri` reports
  `MembershipNotSolvable` and exits 1. Prereqs: #8300. `grammar_confirmed = false` (α is the
  grammar producer).
- **β #8356 — Membership domains in the solver layer.** Modules: `reify-constraints`
  (`enumeration_domain.rs`, `registry.rs` `decompose_prelude`, `cpsat.rs`), `reify-eval`
  (`engine_eval.rs` undef causes, `scope_qualifies_for_centrality`; `tests/harness_engine.rs`),
  `reify-core` (codes), `scripts/verify.sh` (`_RUST_COUPLED_RI_FIXTURES`). **Signal (LEAF):** B5–B10
  — `reify eval tests/prd-gate/fixtures/cmc_bolt_feasible.ri` prints `BoltedJoint.bolt_length =
  0.025 m` and exits 0; `cmc_bolt_gap.ri` reports `NoCatalogMemberFeasible` naming 20mm and 25mm
  and exits 1; B5b's strict-auto Error; the warm-path test. Prereqs: α, #5470, #6554.
- **γ #8357 — `@solver_hint("discrete_set", C)` compiles to `x in C`; the other hint kinds made loud.**
  Modules: `reify-compiler` (`annotations.rs`, `types.rs`, the cell decl sites of D9 incl.
  `entity.rs`, `guards.rs`, `compile_builder/entities_phase.rs`, `connect.rs`), `reify-core`,
  `examples/m11_annotations.ri` (Feature 7: give `BoltedPanel` constraints so it resolves, and
  update the stale "does not yet consume these hints" comment) and its test
  `crates/reify-compiler/tests/harness_result_annotation/m11_annotations_solver_hint_tests.rs` (it
  asserts no warnings and pins `SolverHint.collection`'s string shape). **Signal (LEAF):** B11, B12
  — `reify eval tests/prd-gate/fixtures/cmc_sugar_equivalence.ri` prints identical values for both
  forms; `reify check` flags B12 VIOLATED with the hint-origin label. Prereqs: β.
  **Integration gate for ε #5471.**
- **δ #8358 — Docs truth.** Modules: `crates/reify-mcp/src/tools/chunks/{constraints,syntax,parameters,
  collections,stdlib}.md`, `examples/best_practices/catalog_membership.ri` + `INDEX.md` row (same
  commit — the index/corpus bidirectional test), `.claude/skills/reify-design/SKILL.md` index line
  after "Discrete choices", `docs/reify-language-spec.md` (§5 new "Membership" subsection incl. the
  quantifier-parenthesis rule and the Scalar-Range-value limitation, §9.2 undef rule, §16 `in` row
  and the range row reconciled to the grammar, §12.1 last paragraph and its `standard_bolt_lengths`
  example, §10.7 per D12), `crates/reify-compiler/stdlib/standard_stock.ri` doc comments.
  **Signal (LEAF):** every documented signature compiles in the chunk fence gate
  (`crates/reify-compiler/tests/harness_doc_chunks/`); the new exemplar passes `examples_smoke` and
  the best-practices constraint gate; the membership section and the INDEX line say "pick from a
  catalog / standard sizes / stock" in intent words, so an author who knows the goal but not `in`
  finds it. Prereqs: γ. Note: `crates/**/*.md` is not on the docs fast path; δ is an ordinary
  code-path task.
- **ω #8359 — Close this PRD.** Stamp the terminal Status (`SHIPPED` + landed leaf ids), the AS-AUTHORED
  freeze paragraph and LIVE/AS-AUTHORED map, here and on the capability manifest. Signal: the
  committed header. Prereqs: α, β, γ, δ.

Dependency view: `#8300 → α`; `#6967 → #5470 → β`; `#6554 → β`; `α → β → γ → δ → ω`;
`γ → ε #5471`.

## 10. Open questions

Design questions raised on this draft — all **ruled by Leo, 2026-10-08**:

- **Q-S — D3 equality (re-confirms ruling O).** Membership uses `==`'s verdict: exact today,
  #6653's single-sourced tolerance when it lands. **Yes.**
- **Q-T — D10.** A coded "not yet consumed" Warning for `prefer_stock` / `preferred_strategy` in γ,
  plus a decision gate for real `prefer_stock` semantics. **Both** (#8357, #8360).
- **Q-U — `contains` (#7984).** δ removes `collections.md`'s claim that `contains` is callable and
  points at `in`; #7984 keeps the call-form question. **Yes.**
- **Q-V — #8210.** Cancel as superseded, with pointers. **Yes.**

Tactical — decided in the named leaf:

1. **Constraint labels on solver diagnostics (β).** The solve path carries `ConstraintNodeId`
   only; the check path applies user labels through `engine_constraints.rs::labeled_diagnostics`.
   Pass the label through when the engine holds one, else print the constraint's source text.
2. **Code names (α/β/γ).** D11's names follow `reify-core` convention; final names are the
   implementer's.
3. **The shared solve-invariance module's name and home (α).**
