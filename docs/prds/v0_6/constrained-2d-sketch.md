# Constrained 2D sketching (`sketch` blocks)

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-25 · **Shape:** B+H (contracts + two-way boundary tests)

Provenance: 2026-07-24/25 language-review approval B10 (corrected subject — a prior session was mis-briefed with "affine transforms"; Leo confirmed B10 = 2D constrained sketching). Design session 2026-07-25 with Leo; all decisions in §5 resolved interactively.

## 1. Goal

A `.ri` author can declare a **2D constrained sketch** — named points/lines/arcs/circles plus geometric constraints and driving dimensions — inside a structure, have it **solved deterministically** at compile/eval time, and consume the solved closed profile in `extrude`/`revolve`/`sweep`/`loft` exactly like any other profile. Under- and over-constrained sketches fail **loudly** with typed diagnostics naming the free DOFs or the conflicting constraint set — visible from kernel-less `reify check`.

The vertical-slice consumer: a parametric **2020 aluminium extrusion cross-section** authored as a constrained sketch (symmetry, tangency, param-driven dimensions), extruded to a beam, committed under `examples/best_practices/` and compile-gated in CI. Changing a driving param observably changes the solved geometry.

## 2. Background

Today, 2D profiles are closed-form constructors (`rectangle`/`circle`/`ellipse`/`polygon`/…) lowering to a four-variant `ProfileKind` (`crates/reify-compiler/src/types.rs:1588`); `polygon` takes literal coordinates (`PolygonProfile{points: Vec<[f64;2]>}`, `crates/reify-ir/src/geometry.rs:1037`). There is **no** constrained-sketch solve — no way to say "these two lines tangent, this arc radius 1mm, solve". The standard-parts program (bookmark #5391) named aluminium extrusion profiles "painful without richer 2D sketching"; gear/pulley cross-sections and sheet-metal flat patterns queue behind the same gap.

Substrate reality (surveyed 2026-07-25, code-cited):

- **libslvs (SolveSpace) is linked and ~80% dormant.** `crates/reify-constraints/src/slvs_sys.rs` declares the full slvs vocabulary (38 constraint types, 9 entity types, workplanes); only 5 constraints and 3 entity builders are wired (`solvespace.rs:22-26`, `:266-301`). The 2D workplane mode works but is hard-coded XY-at-origin (`solvespace.rs:683-724`).
- **The existing `SolveSpaceSolver` path is dead on real `.ri` input.** Its `recognize_pattern` matches function names the compiler never emits (`solvespace.rs:105-160` vs `expr.rs:3178`); every test builds synthetic expression trees by hand; no `.ri` example reaches libslvs end-to-end. This PRD builds the first live path — via **typed direct lowering**, not pattern recognition.
- **Assembly relate-solve deliberately bypassed libslvs** for a hand-rolled 6-DOF Gauss–Newton because the bound C API has no rigid-group primitive (`relate_solve.rs:47-52`). That rationale does **not** apply to 2D sketching, where independent point coordinates are exactly slvs's native model — re-adopting libslvs for sketches is sound.
- **DOF data is computed and discarded.** libslvs returns the DOF count and the failing-constraint list; both are captured then dropped (`solvespace.rs:825`, `:828-835`, `:950-958`), and there is no `Slvs_hConstraint → constraint-id` map for attribution.
- **`tangent` is vocabulary-only** (ΔDOF published, no residual rows, no slvs mapping — `relate_solve.rs:479-480`). This PRD makes it real in 2D.
- **`Value::Plane` has no x-axis** (`value.rs:1176`) — sketch-on-plane placement is underdetermined by one DOF with today's datum; `Frame` has a full basis. Nothing today consumes a datum to place geometry. This is why v1 anchoring is deferred (§5 D2).
- **`aux let` parses today** (probed 2026-07-25) — construction geometry rides the existing `aux` marking (placement-relations-belt §4) with zero new grammar.

## 3. Consumers (G1)

| Consumer | Kind | What it consumes |
|---|---|---|
| `examples/best_practices/` 2020 extrusion exemplar (task ζ, this PRD) | user surface, CI-gated | the whole chain: sketch grammar → solve → profile → extrude |
| `reify check` DOF diagnostics | user surface | `E_SKETCH_UNDERCONSTRAINED` / `E_SKETCH_OVERCONSTRAINED` / ledger row, kernel-less |
| standard-parts program (bookmark #5391) | future PRD (task-tracked) | parametric extrusion/gear/pulley profiles as sketches |
| thread-hole-features (`docs/prds/v0_6/thread-hole-features.md` C4/C5) | landed PRD, future extension | sketch-derived cutter profiles as drop-ins for the `cutter : Geometry` derived-let idiom |
| future sketch-first GUI workflow | future PRD (named in §11) | the canonical-text contract (§5 D3/D4): sketches render to `.ri` text; edits round-trip |

Engine-integration sub-check (`docs/prds/v0_3/engine-integration-norm.md`): the solve enters via the **§3.5 ConstraintSolver** contract types with the invocation shaped as a per-scope pass exactly like the existing relate-solve (precedent: placement-relations-belt §6 — "no new seam is introduced"); the solved profile enters **§3.1 op-execute** as a new `GeometryOp`. The seam catalog is unchanged.

## 4. Sketch of approach

One new member-level block, one new solve pass, one new profile op:

```reify
structure def Extrusion2020 {
    param slot_w : Length = 6.2mm
    param length : Length = 100mm

    sketch profile {
        aux let cl = line(origin, point(0mm, 10mm))   // construction centerline
        let a  = point(0mm, 0mm)                       // literals = provisional seeds
        let b  = point(10mm, 0mm)
        let ab = line(a, b)
        let c1 = arc(point(9mm, 1mm), b, point(10mm, 2mm))

        fix(a)                       // absolute: grounds a at its seed
        horizontal(ab)
        tangent(ab, c1)
        distance(a, b, slot_w)       // driving dimension, param-driven
        symmetric(a, b, cl)
    }

    let body = extrude(profile, length)
}
```

- **Grammar** (novel — fixtures FAIL today; grammar task α is the prerequisite): the `sketch <name> { … }` block is a contextual-keyword member block copying the `relate` precedent (`tree-sitter-reify/grammar.js:714-748`). The body is **ordinary member grammar**: `let`-shaped entity declarations (with the existing `aux` modifier) plus bare constraint/dimension expressions (the `relation_member` shape). Also novel: per-scalar `auto(seed)` in entity-constructor argument position (§5 D6).
- **Compile**: the block lowers to a typed `SketchTemplate` (entities, constraints, dimension expressions, auto markers — §7 C2) carried on the structure template beside `relations`.
- **Solve**: a dedicated per-scope pass (sibling of relate-solve) hands the template to an extended libslvs `SystemBuilder` via a typed direct-build API (§7 C5) — per-sketch slvs group on the XY workplane — and maps the outcome to typed diagnostics + a check-ledger row (§7 C3). Pure numerics, **no kernel required**: runs at `check` as well as `eval`/`build`.
- **Profile**: non-aux curve entities are chained into closed loop(s); the sketch member binds a **Geometry realization** — a planar region face (outer loop minus contained hole loops) on local XY at z=0, satisfying the existing `Surface ∧ Closed ∧ Planar` profile precondition (`geometry.rs:727,748`) — consumed by the eight profile-consuming ops unchanged (§7 C4).

## 5. Resolved design decisions

- **D1 — Member sketch block is the single canonical surface.** `sketch <name> { … }` inside a structure; body = ordinary member grammar; no user-facing desugared `with {}` form. Rationale: named entities are load-bearing (constraints, diagnostics, GUI round-trip identity survive insertion; positional identity churns); construction geometry belongs to the solve unit but not the output wire and doesn't fit an expression form; a block gives sketch-local names a lexical scope; canonical text wants exactly one spelling. Internally the block lowers to a constrained-profile core, so downstream systems see "a profile". Structure params are the driving-dimension values — no separate sketch-param plumbing. Reuse unit = the enclosing structure.
- **D2 — v1 sketches live on local XY.** Authored in their own 2D frame; the region lands on XY at z=0 like every existing profile; placement uses existing idioms (`translate`/`rotate`, `at` on subs). `sketch <name> on <frame-expr> { … }` is a **named future seam** (§11): the block grammar is designed so adding `on` is non-breaking; if frame-composition proves awkward, better frame selectors/composers are the fallback. Face anchoring (topology selector → frame) is explicitly future — `Value::Plane` lacks an x-axis and no face→Frame projection exists.
- **D3 — Explicit seeds, canonical text.** Every entity coordinate literal is required and is the solver seed. The text is a complete superset of sketch state; the solve is a pure function of the file; solved positions are **never written back**. A future GUI writes click positions as the seed literals at entity-creation time and mutates those literals on drag — this is the round-trip contract the sketch-first GUI PRD will build on.
- **D4 — Provisional/absolute law.** Declaration literals are provisional (solver may move them). Absoluteness lives only in the constraint set: an implicit fixed `origin` construction point every sketch provides (the anchor for absolute dimensioning), `fix(entity)` (grounds an entity at its seed values; reuses the relate-vocabulary `fix`/`ground` word — `entity.rs:4952-4967` precedent), and driving dimensions (expression-valued, evaluated before solve). The whole DOF ledger derives from the constraint set.
- **D5 — Zero-uncovered-DOF policy.** After constraint application, every remaining DOF must be attributable to an `auto` declaration; otherwise `E_SKETCH_UNDERCONSTRAINED` (Error) naming the free DOFs per entity. Over-constrained/inconsistent → `E_SKETCH_OVERCONSTRAINED` naming the failing constraint set with source spans. One diagnostic per sketch naming the full affected set (consumption-accounting decision 7), never per-constraint spam.
- **D6 — `auto` in sketches** ("set to a value the solver chooses" — not "unconstrained"): per-scalar `auto(seed)` in entity argument position, seed mandatory (D3). Auto-covered DOFs solve **nearest-to-seed** (libslvs's native behavior), deterministically. The check ledger reports auto-resolved DOF counts. Designed now, shipped in-PRD (task η); the 2020 leaf does not depend on it.
- **D7 — Constraint vocabulary = existing relation names reused in 2D + 2D-specific additions + DRIVE forms.** Reused: `coincident`, `on`, `parallel`, `perpendicular`, `tangent`, `concentric`, and arity-3 `distance`/`angle` (the existing DRIVE-form precedent, `relation_signatures.rs:105-130`). New 2D-only: `horizontal`, `vertical`, `symmetric(a, b, about-line)`, `midpoint(p, line)`, `equal(a, b)` (length/radius), `radius(c, expr)`, `diameter(c, expr)`, `fix(e)`. All type to `Type::Relation` (DOF-removal directives, not Bool — existing law). Dimension value slots take arbitrary `Length`/`Angle` expressions.
- **D8 — Typed direct lowering to libslvs; no pattern recognition.** The sketch pass builds the slvs system from `SketchTemplate` data. The legacy `recognize_pattern` path is left untouched for the registry's auto-param route (with an impl-site breadcrumb noting the sketch path supersedes it for 2D; consolidation is out of scope §11). The stale claim at `examples/geometric_relations/bolt_plate.ri:16` ("hands the driving set to the SolveSpaceSolver") is corrected in task β's diff.
- **D9 — Dedicated solve pass, kernel-independent, default-ON.** Sibling of relate-solve in pass position, but **not** kernel-gated (sketch entities are self-contained; no feature-datum reads in v1) — so `reify check` reports sketch DOF diagnostics without a kernel. No env flag; landing order is the only control (discrete-cost deployment pattern, §3 decision 1 there). The registry's slots are untouched — sketch solves do not route through `SolverRegistry` component classification.
- **D10 — Profile handoff: loop assembly + one new IR op.** Non-aux curve entities are chained by solved-coincident endpoints into closed loops; open chain → `E_SKETCH_OPEN_PROFILE`; crossing loops → `E_SKETCH_LOOPS_CROSS`; containment classifies outer loop vs holes. New `GeometryOp::SketchProfile{loops}` (resolved segments: lines + arcs); OCCT builds wire(s) → face-with-holes (`BRepKind::Face`). Trait inference: `surface()` (bounded, closed, planar) — the existing `PROFILE_SLOT` precondition and `GeometryProfileRequired` discipline apply unchanged.
- **D11 — The sketch member binds the region Geometry directly.** `extrude(profile, length)` — no mandatory `.region` projection. Non-foreclosing: future solved-entity projections (`profile.a`, `profile.c1.center`) ride the uniform-member-access `Synthesized` extension point (its §7 M1 names exactly this pattern).
- **D12 — Construction geometry = existing `aux` marker.** `aux let` inside the sketch (parses today): participates in the solve, excluded from loop assembly and the region, excluded from body-set consumers per the existing belt `is_aux` rule.
- **D13 — Dimension-checked coordinates.** Sketch entity constructors enforce `Length` (and `Angle` where applicable) argument dimensions — unlike the legacy profile constructors (known gap, out of scope here). Sketch-context builtins (`point`, `line`, `arc`, `circle` inside the block) get real compiler signatures; the `point2`/`point3` compiler-phantom cleanup is task γ's scope where touched.
- **D14 — Loud-failure compliance (the INV-SF family, by design).** New diagnostics all carry codes (INV-SF-6); Error severity exits nonzero via the shared severity helper (INV-SF-2); unsolved sketch cells get `UndefCause::SketchSolveFailed{reason}` — never bare Undef (INV-SF-1); every sketch constraint is solved, verified, or diagnosed, and the check ledger gains a `sketches:` row derived from the same outcome entries (INV-SF-3, consumption-accounting decision 6); indeterminate outcomes (e.g. a dimension expression reading an Undef param) are typed `Transient` reasons, rendered as recorded (INV-SF-4); no placeholder types on the sketch surface — typed enums + newtype ids, PTYPE zero-baseline (INV-SF-5).

## 6. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `placement-relations-belt.md` | consumes | `aux`/`is_aux` marking for construction geometry; future `on <frame>` anchor = Phase-B `.world_frame` read (staging rules §7.1.3) | belt owns world-pose + aux; this PRD consumes | wired (landed) |
| `thread-hole-features.md` | produces (future) | sketch-derived cutter as a C4 drop-in (derived let, local frame, inert-until-consumed) | thread-hole owns the cutter idiom; this PRD produces profile values | future — no task filed; named in #5391's orbit |
| `uniform-member-access.md` | consumes | sketch member binds via the realization lane today; future entity projections ride `TerminalKind::Synthesized` (their §7 M1) | UMA owns the resolver mechanism; this PRD owns sketch member semantics | UMA batch in flight — v1 has no new resolver arm, no dep |
| `resolution-unification.md` / `stdlib-namespace.md` | norms only | no new definition kind, no new stdlib module in v1 (sketch builtins are compiler-registered, block-scoped) | n/a | landed; no seam |
| `eradicate-silent-undef.md` | consumes | additive `UndefCause::SketchSolveFailed`; shared severity helper; PDIAG discipline | silent-undef owns the machinery; additions here are additive | in flight (#5403 family) — additive, landing-order-independent (their §5 pattern) |
| `declared-intent-consumption-accounting.md` | consumes | check-ledger row for sketches; typed indeterminate reasons | DICA owns ledger machinery; this PRD adds its row per their decision 6 | in flight — additive row; no hard dep (ledger row lands with δ against whatever `finish_check` shape is on main) |
| `geometric-relations.md` task θ / #4388 | sibling split | **assembly relate** DOF ledger stays #4388; **sketch** DOF ledger is owned HERE (different solve, different scope) | split as stated | #4388 pending; no dep either way |
| `discrete-cost-minimisation.md` | pattern reuse only | deployment playbook (default-ON, wiring leaf = committed example); registry slots untouched by this PRD | n/a | no seam |
| `indexed-sub-instantiation.md` | none in v1 | sketch entity arrays out of scope | n/a | no seam |

No new contested-ownership pair is introduced (checked against the three known pairs in `phase-3-breadcrumb-map.md` §3).

## 7. Contracts (H)

### C1 — Surface grammar

- `sketch_block := 'sketch' identifier '{' sketch_member* '}'` as a structure/occurrence member. `sketch` is a contextual keyword token (the `relate` pattern, `grammar.js:714-719`). Reserved non-breaking extension point: an optional `on <expr>` clause between the name and `{` (future PRD).
- `sketch_member := ['aux'] let_declaration | expression` — entity declarations are ordinary `let`s; bare expressions are constraints/dimensions (the `relation_member` shape).
- Entity constructors (sketch-scoped): `point(x, y)`, `line(p, q)`, `arc(center, start, end)` (CCW start→end, slvs-native parametrization), `circle(center, r)`. Coordinate args: `Length`-dimensioned scalars or `auto(seed)`; point args: entity references or inline `point(…)`.
- Implicit binding `origin` : a fixed construction point at (0mm, 0mm), in scope inside every sketch block.
- Sketch-local names scope to the block; the block name binds one structure member (the region Geometry, D11). Name collisions with structure members follow existing shadowing rules; entity names are not visible outside the block in v1.
- **Grammar fixtures** (`tests/prd-gate/fixtures/sketch_*.ri` — authored + probed 2026-07-25, currently FAIL parse; **task α commits them** with its grammar diff; content reference: branch `task/5514` commit `3e850bab7e`): the block form, `auto(seed)` in call position, `aux let` inside a sketch, bare constraint members.

### C2 — Compile representation (`SketchTemplate`)

- Carried on the topology template beside `relations`: `SketchTemplate { name, entities: Vec<SketchEntityDecl>, constraints: Vec<SketchConstraintDecl> }`, declaration-ordered (determinism).
- `SketchEntityDecl { id: SketchEntityId, name: Ident, kind: SketchEntityKind (Point|Line|Arc|Circle), args: Vec<SketchScalar>, refs: Vec<SketchEntityId>, aux: bool }`; `SketchScalar = Literal(CompiledExpr) | Auto{seed: CompiledExpr}`.
- `SketchConstraintDecl { kind: SketchConstraintKind (typed enum — the D7 vocabulary), refs: Vec<SketchEntityId>, value: Option<CompiledExpr>, span }`.
- Compile-time rejections (all coded, all with spans): unknown entity reference → `E_SKETCH_UNKNOWN_ENTITY`; a member that is neither an entity `let` nor a relation-typed expression → `E_SKETCH_INVALID_MEMBER`; wrong entity kind for a constraint slot (e.g. `radius` on a line) → `E_SKETCH_CONSTRAINT_KIND`; dimension errors on coordinates (D13). 3D geometry constructors inside a sketch are rejected at the same point.
- No strings, no `Real` stand-ins for handles (INV-SF-5): typed enums + newtype `SketchEntityId`.

### C3 — Solve pass + outcome

- Per sketch, per scope instance; runs at `check`, `eval`, `build`; kernel-independent; one slvs group per sketch on the XY workplane; serialized behind the existing `SLVS_LOCK`.
- Inputs: dimension/seed expressions evaluated first. Any Undef input ⇒ outcome `Indeterminate(Transient::UndefInputs{cells})` — reason recorded and rendered, not an Error (INV-SF-4).
- `SketchSolveOutcome = Solved { positions, ledger: SketchDofLedger } | Underconstrained { free: Vec<(entity, dof-description)> } | Overconstrained { failing: Vec<(constraint, span)> } | Diverged { detail } | Indeterminate { reason }`. `SketchDofLedger { total_dof, constrained, auto_resolved, remaining = 0 }`.
- Mapping: `Underconstrained` → `E_SKETCH_UNDERCONSTRAINED` (lists uncovered per-entity DOFs; auto-covered DOFs excluded); `Overconstrained` → `E_SKETCH_OVERCONSTRAINED` (failing set with source spans via the C5 reverse map; redundant-but-consistent is v1-conservatively an error — refinement is an open question); `Diverged` → `E_SKETCH_SOLVE_DIVERGED`. One diagnostic per sketch. Errors exit nonzero via the shared severity helper.
- On any non-`Solved` outcome the sketch member's cells carry `UndefCause::SketchSolveFailed{reason}` (additive variant; formatted by the single `format_undef_cause`).
- `finish_check` ledger row: `sketches: N solved (D dof, A auto) / M failed(<reason class>)` — derived from the same outcome entries (no fork).
- Determinism (O9): entity order = declaration order; seeds from text; libslvs is deterministic given seeds; no RNG, no clock.

### C4 — Profile value

- Loop assembly over non-aux curve entities using solved positions + coincident topology: closed loops or `E_SKETCH_OPEN_PROFILE` (names the open chain's endpoints); crossing loops → `E_SKETCH_LOOPS_CROSS`; containment classification → one outer loop + zero or more hole loops per region.
- New IR op `GeometryOp::SketchProfile { loops: Vec<SketchLoop> }`, `SketchLoop = Vec<SketchSegment>`, `SketchSegment = Line{p0, p1} | Arc{center, start, end, ccw}` — resolved SI f64, local XY at z=0. Kernel: wires → face-with-holes, `BRepKind::Face`.
- The sketch member binds this realization directly (D11); `InferredTraits::surface()` (closed, planar) makes it `PROFILE_SLOT`-legal for `extrude`/`revolve`/`sweep`/`sweep_guided`/`loft`/`loft_guided`/`extrude_symmetric`/`extrude_infinite` with no consumer changes. Type-driven acceptance per uniform-member-access C2.

### C5 — Solver substrate (`reify-constraints`)

- `SystemBuilder` gains a **typed direct-build API**: `add_sketch(&SketchSystem) -> SketchHandleMap` — entities and constraints handed as data (no `CompiledExpr` pattern matching). `SketchSystem` is the crate-boundary twin of `SketchTemplate` with expressions pre-evaluated to f64.
- New entity builders: `circle`, `arc_of_circle`, `distance` (slvs radius carrier), `normal_in_2d` — all currently declared-only constants (`slvs_sys.rs:44-59`).
- New constraint mappings (from declared-only constants, `slvs_sys.rs:62-132`): `PT_ON_LINE`, `PT_ON_CIRCLE`, `DIAMETER`, `EQUAL_RADIUS`, `ARC_LINE_TANGENT`, `CURVE_CURVE_TANGENT`, `HORIZONTAL`, `VERTICAL`, `SYMMETRIC_LINE`, `AT_MIDPOINT`, `EQUAL_LENGTH_LINES`, `WHERE_DRAGGED` (backs `fix`).
- **Attribution map**: `Slvs_hConstraint → (SketchConstraintId, span)` kept in the builder; the failing-constraint readback (already implemented, `solvespace.rs:828-835`) resolves through it instead of collapsing to a count.
- **DOF surfaced**: `SlvsSolveResult::Ok{dof}` (already captured, `solvespace.rs:825`) is consumed by the pass's DOF accounting instead of being dead code.
- The legacy `recognize_pattern` route is untouched; breadcrumb comment at its head names this PRD and the supersession scope.

## 8. Boundary-test sketch

Facing both sides of each seam; these rows are task ζ's (and δ's) observable-signal source.

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| 1 | Fully-dimensioned sketch solves | committed fixture: rectangle-with-slot sketch, all DOF constrained | `reify check` exit 0; ledger row `sketches: 1 solved`; no sketch diagnostics |
| 2 | Solved profile extrudes | row-1 fixture + `extrude(profile, …)` | `reify eval`/`build` produce a solid; bbox matches driving dims within 1µm (equality-constraint convergence ≪ 1µm; not a method-floor bound) |
| 3 | Param drives geometry | row-2 fixture, `slot_w` changed | solved slot width and bbox change accordingly (the parametric payoff, observable) |
| 4 | Under-constrained is loud, kernel-less | fixture with one dimension removed, no kernel attached | `reify check` exits nonzero; `E_SKETCH_UNDERCONSTRAINED` names the free entity DOFs; cells carry `UndefCause::SketchSolveFailed` |
| 5 | Over-constrained is loud with attribution | fixture with two conflicting distances | `E_SKETCH_OVERCONSTRAINED` names both constraints by span; one diagnostic total |
| 6 | Tangency solves | line-arc tangent fixture (the vocabulary-only gap made real) | solved positions satisfy tangency; `reify eval` region is valid |
| 7 | `fix` + implicit `origin` anchor | fixture dimensioned off `origin`, `fix(a)` | solve reproducible; moving seeds of fixed entities is an over-constraint if inconsistent with dims |
| 8 | `aux` construction geometry | fixture with `aux let` centerline + `symmetric` | centerline participates in solve, absent from region/loop assembly |
| 9 | Open profile rejected | fixture with a gap in the loop | `E_SKETCH_OPEN_PROFILE` naming the open endpoints |
| 10 | Multi-loop region (bore) | outer loop + contained circle | face-with-hole; extrude yields a bored solid |
| 11 | `auto` covers DOF (task η) | fixture with `auto(seed)` scalar, otherwise under-constrained | solves nearest-to-seed; ledger reports `A auto`; twin fixture without `auto` errors per row 4 |
| 12 | Grammar fixtures parse (task α) | `tests/prd-gate/fixtures/sketch_*.ri` | `tree-sitter parse --quiet` exits 0, corpus CST tests pass |
| 13 | Undef dimension input | fixture whose dim reads an Undef param | outcome Indeterminate with recorded `Transient::UndefInputs`; rendered reason matches; not an Error |

## 9. Pre-conditions for activating

- None external. All assumed substrate verified on main 2026-07-25: libslvs links and solves (CI-run unit tests in `reify-constraints` exercise `Slvs_Solve` — FFI proven; `.ri`-reachability is what this PRD builds); `relate`-block grammar precedent; `aux let` parses (probe); profile-consumer precondition machinery; `stdlib-namespace` landed (no interaction in v1 regardless).
- Novel syntax (the `sketch` block, `auto(seed)` in call position) is **explicitly queued as task α**, upstream of everything — the grammar-gate resolution (b).

## 10. Decomposition plan

Intra-batch deps by letter; α is upstream of all compile-onward tasks. All tasks: `grammar_confirmed` true only for non-α tasks whose surface is α-delivered (they hard-dep α).

- **α — Grammar + AST: `sketch` block, `auto(seed)`, lowering.** `tree-sitter-reify` (grammar, corpus, fixture tests), `reify-syntax/ts_parser.rs`, `reify-ast` (`SketchDecl`). Signal: the `tests/prd-gate/fixtures/sketch_*.ri` fixtures (α commits them; content: branch `task/5514` @ `3e850bab7e`, mirrored untracked in the main checkout) parse with 0 ERROR nodes; corpus tests assert the CST; unlocks γ. (intermediate)
- **β — Solver substrate: direct-build API + entity/constraint builders + attribution + DOF surfacing.** `reify-constraints` per C5; corrects the stale `bolt_plate.ri:16` comment; breadcrumb at `recognize_pattern`. Signal: unlocks δ (crate-level tests exercise each new mapping through `Slvs_Solve`). (intermediate)
- **γ — Compile lowering: `SketchTemplate` + typed rejections.** `reify-compiler` per C2, incl. sketch-scoped builtin signatures and dimension checks (D13). Signal: committed malformed-sketch fixtures produce `E_SKETCH_UNKNOWN_ENTITY` / `E_SKETCH_INVALID_MEMBER` / `E_SKETCH_CONSTRAINT_KIND` at `reify check` with nonzero exit; unlocks δ/ε. Deps: α. (intermediate with observable rejections)
- **δ — Solve pass + DOF accounting + diagnostics + ledger.** `reify-eval` (pass), `reify-constraints` (outcome mapping), `reify-core` (codes), per C3. Signal: boundary rows 1, 4, 5, 7, 13 observable via `reify check` on committed fixtures (kernel-less). Deps: β, γ. (intermediate with observable diagnostics)
- **ε — Profile assembly + `SketchProfile` IR op + kernel face + trait inference.** `reify-ir`, `reify-eval`, `reify-kernel-occt`, `reify-compiler` (traits) per C4. Signal: boundary rows 2, 6, 8, 9, 10 via `reify eval`/`build` on committed fixtures. Deps: δ. (intermediate)
- **ζ — 2020 extrusion exemplar (integration-gate leaf).** `examples/best_practices/extrusion_2020.ri` + `INDEX.md` line; exercises symmetry, tangency, `fix`/`origin`, param-driven dims, multi-loop bore, extrude. Signal: compiles+builds in CI (`examples_smoke.rs`); boundary row 3 (param change observably moves geometry); the boundary-test table is this task's checklist. Deps: ε. (leaf)
- **η — `auto` in sketches.** Grammar already α-delivered; lowering (γ area), accounting + nearest-to-seed semantics + ledger (δ area) per D6. Signal: boundary row 11 fixture pair at `reify check`/`eval`. Deps: δ (and α, γ transitively). (leaf)
- **θ — Docs-truth: chunk + cheatsheet + discoverability.** New sketching section in `crates/reify-mcp/src/tools/chunks/` (signatures verified — every documented form compiles in a smoke `.ri`; `geometry_chunk_smoke.rs` fixture added), `.claude/skills/reify-design/SKILL.md` index line, intent-level findability ("draw a custom profile / constrain a sketch" finds the mechanism). Signal: chunk smoke test green; discoverability acceptance per the docs-truth gate. Deps: ε, ζ. (leaf)

Gate-test drift-guard check: new Rust tests are ordinary crate tests (no new `tests/infra/*.sh`, no new gate-resident standalone binary, no wall-clock assertions) — the α/γ/δ/ε diffs carry their fixtures and any `.config/nextest.toml` partition entries same-diff if a suite proves heavy. G7 walk: no detector/suppressor without escape, contracts are typed enums/schema (not prose), no log-scraping, no snapshot-action, shared loop-assembly and outcome-mapping helpers single-implementation (no-lockstep-duplication) — no waivers needed.

## 11. Out of scope (named future work)

- **Sketch anchoring** (`on <frame>`, face anchoring, re-anchor semantics) — future PRD; requires the datum→geometry-placement bridge and (for faces) a face→Frame projection. The block grammar reserves the slot non-breakingly.
- **Sketch-first GUI editing** (render sketches to canonical `.ri` text, bidirectional edit propagation, drag = seed mutation, `WHERE_DRAGGED` interactive solving) — future PRD; this PRD's D3/D4 canonical-text contract is its substrate.
- **Solved-entity projections** (`profile.a`, `profile.c1.center` as `Point2`) — future, via uniform-member-access `Synthesized`.
- **Solver consolidation** (three geometric solvers: relate-solve GN, sketch/libslvs, legacy pattern path) — future; breadcrumbs only here.
- Redundant-but-consistent constraint tolerance (v1: conservative error); sketch entity arrays / `forall` over entities; solver-driven counts (indexed-sub §3.5 discipline); objectives in sketch solves (`solvespace.rs` has no objective handling — constraint-solver-completion §11); drawings/callouts (thread-hole §10); dimension enforcement retrofit for legacy profile constructors; multi-sketch coupled solves.

## 12. Open questions (tactical)

1. **Arc parametrization ergonomics.** `arc(center, start, end)` is slvs-native; a `radius`-first convenience (`arc_r(start, end, r)`) may be worth adding. Decide during γ.
2. **`.region` alias.** D11 binds the member to the region directly; whether a `.region` synthesized alias should also exist (for symmetry with future `.dof` etc.) — decide during ε/θ.
3. **Containment algorithm detail** (even-odd vs winding for hole classification; tolerance for coincident-endpoint chaining). Decide during ε against OCCT wire-builder behavior.
4. **`fix` mapping**: `WHERE_DRAGGED` vs emitting the entity's params into `FIXED_GROUP`. Decide during β (whichever attributes better in the failing set).
5. **Redundant-consistent refinement**: post-solve residual check to downgrade consistent redundancy from error to warning — file as follow-up after v1 field experience.
