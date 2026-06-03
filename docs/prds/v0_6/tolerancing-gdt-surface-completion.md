# PRD: `std.tolerancing` §7 GD&T / Surface / ISO-grade completion

**Status:** Draft · **Author session:** 2026-06-03 · **Milestone:** v0_6
**Closes:** gap-register `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` cluster **P13 tolerancing** (all 7 rows).
**Source doc:** `docs/reify-stdlib-reference.md` §7 (`std.tolerancing`).
**Substrate file under change:** `crates/reify-compiler/stdlib/tolerancing.ri` (+ a new `crates/reify-stdlib/src/tolerancing.rs` builtin module).

---

## §0 — Scope boundary: three "tolerance" subsystems, this PRD owns the *declarative GD&T surface*

Reify has **three** subsystems that share the word "tolerance". This PRD owns the third and
must not modify the first two. (The boundary table is lifted from
`tolerance-stackup-analysis.md` §0 and extended with the row this PRD owns.)

| Concern | Owner | This PRD? |
|---|---|---|
| **Kernel-realization tolerance budgeting** — how tightly the kernel must mesh/convert (`RepresentationWithin` extractor → per-stage budget → mesher) | `reify-eval/src/tolerance_{budget,combine,scope}.rs`, `engine_tolerance.rs`, `per-purpose-tolerance.md`, task **2874** | **NO — do not modify** |
| **Design dimensional stack-up** — does accumulated ±tol keep a gap/fit in spec | `reify-stdlib/src/stackup.rs`, `tolerance-stackup-analysis.md`, tasks **4004/4014** | **NO — read-only reuse** |
| **Declarative GD&T / dimensional / surface tolerancing surface** — the `std.tolerancing` §7 *types, lets, constraint-defs, and constructor fns* a designer writes (`Flatness`, `Position(MMC)`, `ISOToleranceGrade`, `require_finish`, `Conforms`) | **this PRD** + `stdlib/tolerancing.ri` + new `reify-stdlib/src/tolerancing.rs` | **YES** |

The new builtin module `reify-stdlib/src/tolerancing.rs` neither imports nor is imported by
the kernel-budget machinery or `stackup.rs`. It is a sibling of `stackup.rs`, wired into the
same `eval_builtin` dispatch chain (`reify-stdlib/src/lib.rs`).

---

## §1 — Consumer (G1)

**Named consumer: a mechanical designer authoring GD&T callouts in a `.ri` part model and
running `reify check` / `reify eval`.** The user-observable surface is the CLI, which prints
top-level value cells (`reify eval`) and reports constraint violations (`reify check`).

Every mechanism this PRD introduces names its consumer:

| Mechanism | Consumer |
|---|---|
| `iso_it_tolerance(grade, nominal_min, nominal_max)` builtin (new, `tolerancing.rs`) | `ISOToleranceGrade.tolerance_value` let → `reify eval` prints the IT-grade tolerance width (e.g. IT7@Ø50 = 25µm) |
| `effective_tolerance_zone(tol_value, material_condition, departure)` builtin (new) | `GeometricTolerance.nominal_zone` let **and** the redefined `Conforms` predicate |
| `GeometricTolerance.nominal_zone` derived let | A designer reads the material-condition-expanded zone width via `reify eval`; `Conforms` asserts on it |
| `Conforms` constraint-def (redefined, GD&T-aware) | A designer writes `constraint Conforms(tolerance: my_position, measured_deviation: 0.05mm)`; `reify check` passes/fails — and a deviation that **fails under RFS passes under MMC** because of the bonus, an observable expansion |
| `require_finish(feature, finish)` `.ri` free fn (new) | A designer writes `constraint require_finish(face, SurfaceFinish(...))`; `reify check` reports a violation when the finish spec is ill-formed |
| `symmetric_tolerance`/`limit_tolerance` → `DimensionalTolerance`, `Fit` nested members | A designer reads `.upper_limit`/`.tolerance_band`/`.max_clearance` off the returned structure via `reify eval` |

These are **not in-engine seams** (no kernel module / dispatcher / hook), so the
`engine-integration-norm.md` §3 sub-check does not apply. The consumer is the `reify
check`/`eval` user surface and the prelude's own derived lets — both first-class user-observable.

---

## §2 — Sketch of approach (the "what changes")

Two producer layers, mechanically minimal because `.ri` free functions and trait-level `let`s
already evaluate (verified — see §3):

### 2.1 New Rust builtin module `reify-stdlib/src/tolerancing.rs` (the producer)

Mirrors `stackup.rs`: a `pub fn eval_tolerancing(name, args) -> Option<Value>` added to the
`eval_builtin` dispatch chain in `lib.rs`, plus a `diagnose` classifier for the `Undef` path.

- **`iso_it_tolerance(grade: Int, nominal_min: Length, nominal_max: Length) -> Length`** —
  ISO 286-1 standard-tolerance-unit lookup. Computes the standard tolerance unit
  `i = 0.45·∛D + 0.001·D` (µm, `D` = geometric mean of the size-range bounds in mm) and
  multiplies by the IT-grade factor. **Supported envelope:** IT5–IT18, nominal sizes ≤ 500 mm
  (the standardised step ranges). Outside the envelope it returns `Value::Undef` + a diagnostic.
  The cube-root arithmetic is dimensionally awkward in `.ri` (∛Length is a fractional
  dimension), which is *why* the lookup is a Rust builtin, not a `.ri` table.
- **`effective_tolerance_zone(tolerance_value: Length, material_condition: MaterialCondition, bonus_departure: Length) -> Length`** —
  RFS → zone = `tolerance_value`; MMC/LMC → zone = `tolerance_value + bonus_departure`.
  Branches on the `Value::Enum { type_name: "MaterialCondition", variant }` (precedent:
  `reify-eval/src/compute_targets/buckling.rs` branches on an enum variant).

### 2.2 `crates/reify-compiler/stdlib/tolerancing.ri` (the declarative surface)

- `GeometricTolerance` trait: add `let nominal_zone = effective_tolerance_zone(tolerance_value, material_condition, 0mm)` and the documented default `material_condition : MaterialCondition = MaterialCondition.RFS`. The 18 GD&T structures **inherit** `nominal_zone` (precedent: `Physical.mass` is inherited by refining structures) — no per-structure re-declaration needed.
- `ISOToleranceGrade`: replace `param tolerance_value : Length` with `let tolerance_value = iso_it_tolerance(grade, nominal_min, nominal_max)` (derived, not a passthrough param).
- `Conforms` constraint-def: redefine from the trivial `tolerance_value > 0` to a GD&T-aware predicate `effective_tolerance_zone(tolerance.tolerance_value, tolerance.material_condition, feature_departure) >= measured_deviation` over a `param tolerance : GeometricTolerance` (trait-typed constraint param — compiler-accepted), with `measured_deviation : Length = 0mm` and `feature_departure : Length = 0mm`.
- `require_finish(feature: Real, finish: SurfaceFinish) -> Bool` `.ri` free fn: body `finish.value > 0mm` (well-formedness predicate usable in `constraint` position).
- `SurfaceFinish`: add `direction : SurfaceDirection = SurfaceDirection.Multidirectional` and `process : String = ""` defaults.
- §7.1 reshape: `symmetric_tolerance`/`limit_tolerance` return `DimensionalTolerance` (constructed in-body) instead of bare `Length`; `Fit` exposes nested `DimensionalTolerance` members per the doc.

### 2.3 Geometry typing (activates deferred task #3116)

A final, decoupled task registers the `Geometry` and `DatumRef` type names in the resolver and
flips all `feature : Real` / `datum_refs : Real` placeholder sites (incl. `require_finish`'s
`feature`) to `Geometry` / `DatumRef`. This is **task #3116, activated** — see §3 and §6.

---

## §3 — Pre-conditions / substrate verification (G3 + G6)

All novel syntax was parse-tested with `tree-sitter parse --quiet` **and** compiled with the
real `reify check` binary (memory: the tree-sitter CLI can drift from the compiler grammar, so
both were run). Fixtures live under `/tmp/prd-gate-fixtures/tolerancing-*.ri`.

| Construct | Verdict | Evidence |
|---|---|---|
| Trait-level derived `let nominal_zone = builtin(...)` | **parses + compiles** | precedent `trait Physical { let mass = volume(geometry)*material.density }` (`structural_physical.ri:40`); `trait Costed { let line_cost : Money = ... }` (`io.ri:110`); fixture `np-1.ri` → `reify check` exit 0 |
| `ISOToleranceGrade { let tolerance_value = iso_it_tolerance(...) }` | **parses + compiles** | fixture `np-1.ri` exit 0 |
| `constraint def Conforms { param tolerance : GeometricTolerance; … member-access predicate }` | **parses + compiles** | `compile_constraint_def` accepts trait/structure/enum-typed params (`defs_phase.rs:88-112`); member-access-in-constraint precedent `constraint material.density > 0` (`structural_physical.ri:44`); fixture `np-2.ri` exit 0 |
| `require_finish` `.ri` fn w/ member access + Bool comparison | **parses + compiles + EVALUATES** | fixture `req-clean.ri` → `reify eval` prints `UseIt.ok = true` |
| enum default on a structure param (`= SurfaceDirection.Multidirectional`) | **works** | already used in current `tolerancing.ri:70` etc. |
| `.ri` free fns require **no** Rust registration | **confirmed** | `eval_user_function_call` (`reify-expr/lib.rs:1064`) looks up `.ri` fns by name/arity; a new `require_finish` Just Works |
| new builtins wired into dispatch | **available seam** | `if let Some(v) = stackup::eval_stackup(...)` chain in `reify-stdlib/src/lib.rs:164`; add a `tolerancing::eval_tolerancing` arm |

**Substrate finding — `= undef` is trait-only.** `param x : Real = undef` compiles on a
**trait** param but **fails on a `structure def` param** (`error: unresolved name: undef`, verified
on both `Real` and `String`, prelude on). The task-#3918 `= undef` precedents are all on traits.
`SurfaceFinish` is a structure, so its `process` default uses the **string sentinel `""`**, not
`= undef` (the current `.ri` comment already suggests `""`). This is recorded as a doc-reconcile,
not a grammar-work prerequisite.

**G6 numeric premise — ISO 286-1 formula reproduces the published table.** Hand-checked against
standard cells (validated during authoring): IT6@Ø18–30 = 13 µm (computed 13.07), IT7@Ø30–50 =
25 µm (24.97), IT8@Ø6–10 = 22 µm (22.5) — all round to the published values. The cube-root
formula matches the ISO 286-1 tables to the standard's own rounding for **IT5–IT18 over 3–500 mm**;
IT01–IT4 use different (linear/interpolated) definitions and are **out of the supported envelope**
(return `Undef`). Task α's signal asserts these cells within rounding — the numeric floor.

**Geometry-typing substrate (for the #3116 task).** `Solid` already resolves to `Type::Geometry`
(`type_resolution.rs:563`); `Geometry`/`Surface`/`Curve`/`DatumRef` do **not**. Registering
`Geometry` is ≈ 1 resolver arm; `DatumRef` aliases `Geometry` (or reuses the existing `Datum`
structure). The real cost is the *cascade*: `feature : Geometry` invalidates the `= 0.0` default,
so `feature` becomes **required**, rippling to every construction site in `examples/`/`tests/`.

---

## §4 — Resolved design decisions

1. **`nominal_zone` is a scalar effective-zone-SIZE (`Length`), not a geometric region — and it
   does NOT depend on #3116.** A region-valued zone needs a geometry-kernel *zone-construction*
   op (offset/sweep a zone solid around the feature) that does not exist and is **out of #3116's
   scope** (#3116 only registers the geometry *type*). The material-condition-expanded zone *width*
   is fully computable from `tolerance_value` + `material_condition` + departure, geometry-free.
   The geometric-region form is documented as deferred. **Answers the author-session question:
   yes, there is a reason nominal_zone does not depend on #3116 — #3116 doesn't unblock it.**

2. **#3116 IS activated and wired** (set `pending`, folded into this PRD's graph as task δ) — but
   the value-semantics chain does **not** depend on it; δ depends on the value-semantics tasks and
   rebases onto the final `.ri`. Rationale: the geometry typing is the last tolerancing
   Real-placeholder debt and belongs to "completing §7", its resolver lift is trivial, and doing it
   in the same pass avoids a second sweep over `tolerancing.ri`. Decoupling keeps the HIGH-priority
   `require_finish` off the resolver cascade. `require_finish` and all GD&T structures ship with
   `feature : Real` (consistent with today's 16 sites); δ flips all 17 in one pass.

3. **ISO-grade lookup is a Rust builtin via the ISO 286-1 formula, not a `.ri` data table.**
   Cube-root dimensional arithmetic forces Rust; the formula avoids embedding ~234 table cells.

4. **MMC/LMC/RFS branching is in Rust** (`effective_tolerance_zone`), not `.ri` `match` — it
   co-locates with the ISO builtin in the new module, is unit-testable, and matches the
   `buckling.rs` enum-branch precedent. (`.ri` `match` exists but is not needed here.)

5. **`Conforms` becomes a real, non-tautological conformance check.** Default (`measured_deviation
   = 0mm`) always passes (well-formed); a user-supplied `measured_deviation` is checked against the
   material-condition-expanded zone, so the MMC bonus is *observable* (a deviation that fails under
   RFS passes under MMC). `feature_departure`/`measured_deviation` are user scalars now; #3116 +
   a future geometry-measurement path can derive them.

6. **`Range<Length>` is NOT added.** `ISOToleranceGrade` keeps `nominal_min`/`nominal_max`
   (the doc's `Range<Length>` field is a doc-reconcile — `Range<T>` params are unrealizable, per
   the P11/P15 gap rows). Recorded in the §7 doc-reconcile (task ε).

7. **Scope = full P13 cluster (all 7 rows)**, including the §7.1 `symmetric/limit/Fit` return-shape
   reshape (isolated in task γ because it is a breaking return-type change).

---

## §5 — Out of scope

- The kernel-realization tolerance budget (`tolerance_budget/combine/scope.rs`, `RepresentationWithin`) — §0 row 1. **Do not modify.**
- The stack-up subsystem (`stackup.rs`, `stackup_rss`/`worst_case`/`monte_carlo`) — §0 row 2. **Read-only reuse.**
- A geometric *region*-valued `nominal_zone` (needs a zone-construction kernel op; deferred).
- Actual surface-finish *verification* (we cannot measure a manufactured surface at design time; `require_finish` is a well-formedness/requirement predicate, not a metrology check).
- A surface-finish → DFM/manufacturing-export pipeline (std.process is itself trait-surface-only, P14 — separate cluster).
- IT01–IT4 grades and nominal sizes > 500 mm (out of the supported ISO-formula envelope; `Undef` + diagnostic).

---

## §6 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | Resolution |
|---|---|---|
| `stdlib/tolerancing.ri` GD&T/dimensional/surface types | **this PRD** | — |
| Kernel-realization tolerance budget / `RepresentationWithin` | `per-purpose-tolerance.md` + task 2874 | Orthogonal; this PRD does not touch it (§0/§5). |
| Stack-up subsystem (`stackup.rs`, Contributor/StackupResult) | `tolerance-stackup-analysis.md` (4004/4014) | Read-only reuse. γ's `DimensionalTolerance`/`Fit` reshape must not break stackup (Contributor uses flat fields, not `symmetric_tolerance`); task γ/ε run the stackup example to confirm. |
| `Geometry`/`DatumRef` resolver typing (feature/datum_refs Real → typed) | **this PRD** (activates task **#3116**) | #3116 set `pending`, folded as task δ, depends on ε. No contested ownership — resolved by activation, not a reciprocal "the other owns it". |
| `Datum` structure feature reference | this PRD (δ) | δ may reuse the existing `Datum` structure as the `DatumRef` value-form rather than inventing a new opaque type (tactical, §8). |

No new in-engine seam is introduced, so no `engine-integration-norm.md` extension is needed.

---

## §7 — Decomposition plan (one bullet per task → observable signal)

Spine: **α (builtins) → β (.ri core) → γ (.ri §7.1 reshape) → ε (CI gate + docs)**; **δ (#3116
geometry typing)** hangs off ε. The single user-observable leaf is **ε**. All `.ri` structural
work is serialized on the one file (`tolerancing.ri`) via the β→γ→(ε)→δ chain to respect the
orchestrator's narrow file locks.

- **α — `reify-stdlib/src/tolerancing.rs` builtins + dispatch wiring.** Implement
  `iso_it_tolerance` (ISO 286-1 formula, IT5–IT18 ≤500 mm envelope, `Undef`+diagnostic outside)
  and `effective_tolerance_zone` (RFS/MMC/LMC branch on the `MaterialCondition` enum); add the
  `tolerancing::eval_tolerancing` arm to the `eval_builtin` chain in `lib.rs` and a `diagnose`
  classifier.
  **Signal:** `cargo test -p reify-stdlib` asserts `iso_it_tolerance` matches published cells
  (IT6@Ø18–30=13µm, IT7@Ø30–50=25µm, IT8@Ø6–10=22µm) within rounding, the envelope edges return
  `Undef`, and `effective_tolerance_zone` yields `tol` for RFS and `tol+departure` for MMC/LMC;
  `grep` shows the new arm in `lib.rs` dispatch (anti-orphan). *Deps: none.*

- **β — `tolerancing.ri` core surface (the 4 named gaps + SurfaceFinish defaults).** Add
  `GeometricTolerance.nominal_zone` let + `material_condition` RFS default; convert
  `ISOToleranceGrade.tolerance_value` to a derived let; redefine `Conforms` (GD&T-aware predicate);
  add the `require_finish` `.ri` fn; add `SurfaceFinish` `direction` enum default + `process = ""`.
  **Signal:** a `reify eval` conformance test prints `ISOToleranceGrade(...).tolerance_value` =
  the IT-grade width and a `Flatness(...).nominal_zone` = the expanded width; `reify check` on a
  `Conforms(measured_deviation:…)` callout passes under MMC where it fails under RFS;
  `require_finish` returns a Bool usable in a constraint. *Deps: α.*

- **γ — `tolerancing.ri` §7.1 return-shape reshape.** `symmetric_tolerance`/`limit_tolerance`
  return a constructed `DimensionalTolerance`; `Fit` exposes nested `DimensionalTolerance` members
  (hole/shaft) with the documented derived `max_clearance`/`min_clearance`.
  **Signal:** `reify eval` reads `symmetric_tolerance(10mm, 0.1mm).upper_limit` and
  `Fit(...).hole_tolerance.upper_limit` off the returned structures; the stack-up example
  (`examples/tolerance-stackup-3part.ri`) and task-336 integration test stay green. *Deps: β.*

- **ε — end-to-end CI gate + §7 doc reconcile (the user-observable leaf, B integration gate).**
  Commit `examples/tolerancing/std_tolerancing_surface.ri` exercising every §7 symbol end-to-end
  (require_finish, ISO lookup, `Conforms` with an MMC bonus that flips a pass/fail, `nominal_zone`,
  the reshaped `symmetric_tolerance`/`Fit`, SurfaceFinish defaults), run green in CI via
  `reify check`/`reify eval`. Reconcile `docs/reify-stdlib-reference.md` §7 (`Range<Length>` →
  `nominal_min`/`nominal_max`; `process = undef` → `= ""`; `nominal_zone` = scalar effective
  zone-size) and mark the 7 P13 gap-register rows closed.
  **Signal:** `reify eval examples/tolerancing/std_tolerancing_surface.ri` (CI) prints the
  expected IT widths / expanded zones / fit clearances and the MMC-vs-RFS pass/fail flip; the 7
  P13 rows show closed in the gap register. *Deps: β, γ.*

- **δ — geometry typing (task #3116, activated).** Register `Geometry` + `DatumRef` in
  `type_resolution.rs` (+ `reify-types`), flip all 17 `feature : Real` → `Geometry` (incl.
  `require_finish` + remove the `= 0.0` feature defaults → feature required) and 8
  `datum_refs : Real` → `DatumRef`, and update the §7 example + any affected examples/tests.
  **Signal:** `param feature : Geometry` resolves (was `unresolved type: Geometry`); `cargo test
  -p reify-compiler` green; the §7 example constructs a tolerance with a real geometry feature;
  the Real-placeholder audit doc task-F column is closed. *Deps: ε (rebase onto final `.ri` +
  example). #3116 set `pending`, description updated (17 feature sites; rebase note).*

---

## §8 — Open (tactical / implementation-time) questions

1. **`DatumRef` representation (δ):** opaque alias to `Type::Geometry`, or reuse the existing
   `Datum` structure (label + feature) as a `List<Datum>` for `datum_refs`? Tactical — decide at
   δ implementation; both satisfy the resolver-registration signal.
2. **`iso_it_tolerance` IT01–IT4 / >500 mm:** ship `Undef`+diagnostic (current plan) or extend the
   formula. Envelope extension is additive and can be a later follow-up.
3. **`require_finish` strength (β):** the v1 body is `finish.value > 0mm` (well-formedness). A
   later enhancement could validate `value` against the chosen `SurfaceParameter`'s achievable
   range — out of this PRD's scope but noted.
4. **`Conforms` back-compat:** the old `Conforms { param tolerance_value : Length }` callers (if
   any in examples/tests) need migration to the new `param tolerance : GeometricTolerance` shape;
   β/ε must grep and migrate. The default-`measured_deviation` path keeps the common case passing.
