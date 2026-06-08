# PRD — Geometric Relations (underconstrained `at auto` + solved `relate`s) — core

**Milestone:** v0_6 · **Status:** active (foundation buildable now; end-to-end slice activation-gated — see Pre-conditions) · **Approach:** B + H (contract + two-way boundary tests) · **Authored:** 2026-06-08

**Design of record:** `docs/design/geometric-relations.md` (decisions A–O locked, §14, interactive 2026-06-08). This PRD is the **decomposition contract** for that design's *core* — the static assembly-mate system (design §11 steps 1–8). It does **not** re-open the locked decisions; read the design doc for the full ontology (§2 datum lattice), vocabulary (§3), and rationale. Durable record: `~/.claude/projects/-home-leo-src-reify/memory/project_geometric_constraint_relations_design.md`.

**Scope boundary (why "core"):** the **joint half** (design §8 / §11 step 9 — `joint…with`, the self-checking law, mount→FK offset, couplings-on-the-scalar-side) is **out of scope here** and ships as a companion PRD `geometric-joints.md`, hard-gated on **KIN-OFFSET-1** (`docs/prds/v0_6/kinematic-inter-joint-offsets.md`, task 4331, *deferred*) and co-designed with it per design §8.2. Geometry-in-the-loop solving is a separate future PRD (design §12). See **Out of scope**.

---

## 1. Goal & user-observable surface

Let a `.ri` author place geometry by declaring **underconstrained poses** (`at auto`) plus explicit **geometric relations**, and have a solver derive the coordinates — so design intent becomes the source and hand-computed SE(3) numbers disappear. The motivating before/after is design §0–§1:

```reify
sub bolt : Bolt at auto
relate {
    concentric(bolt.shank.axis, plate.hole.axis)   // removes 4
    flush(bolt.head.seat, plate.top.plane)          // removes 3 (2 redundant) → residual 1
}
```

**Consumer / user-observable signals** (the chain terminates at the `.ri` author via CLI + GUI — no orphan producers):
- `reify build` / `reify eval` on the above produces placed geometry with the bolt **coaxial + flush** to the plate (observable as GUI mesh position via the debug MCP, or a CI `.ri` example asserting the solved transform).
- `reify check` emits typed diagnostics for category errors (`angle(point, point, θ)` → datum-projection error; `angle(a, b, 5mm)` → unit error) **before** any solve.
- `reify explain` prints the **DOF ledger** (`spent 5 · free 1 → rotation about bolt.shank.axis`) and the GUI shows the DOF badge.
- A change to plate thickness **re-solves** the bolt's pose (no silent float-off).

---

## 2. Background

A full **SolveSpace/libslvs geometric constraint solver already exists and is dormant**: `crates/reify-constraints/src/solvespace.rs` defines `SolveSpaceSolver` with `impl ConstraintSolver` (solvespace.rs:~874) and recognizes Coincident / Parallel / Perpendicular / PtPtDistance / Angle patterns — but production installs **only** `DimensionalSolver` (`crates/reify-cli/src/main.rs:651`); the relation vocabulary is not callable from `.ri`. The classifier already routes `std::parallel/distance/angle_between/tangent` to `ConstraintDomain::Geometric` (`crates/reify-constraints/src/classifier.rs:46`). The typed **topology-selector value type** (`Type::Selector`/`SelectorKind`) is landed (4116/4117 done). `Frame`/`Transform`/`Plane`/`Axis` are first-class `Value`s; **`Direction` is not** (verified absent from `reify-types/src/ty.rs`) — it is the one genuinely new surface type. This is therefore **"activate and generalize," not green-field**; the bounded gaps are §11 / the decomposition below.

(Code anchors are hints as of authoring — main moves fast; **re-locate every symbol at implementation time**, per design.)

---

## 3. Sketch of approach

Map design §11's de-risked, dependency-ordered build sketch (steps 1–8) onto a task DAG (α–θ). Two layers:

- **Foundation (buildable now):** register `SolveSpaceSolver` (α); first-class `Direction` + datum projections (β); the relation vocabulary + `Relation` type + the three policing layers (γ); the `relate`/`at auto`/`auto(…)`/`where` **grammar production** (δ — the one novel-syntax prerequisite, see G3 below).
- **Integration (activation-gated):** the feature→datum trait bundle + dedup — *the real missing bridge* (ε, gated on topology-selector 4118–4120); the per-scope **relate-solve at the unified-DAG `Resolution` node** — the vertical slice (ζ, gated on the unified-DAG driver 4357–4362); `self`/grounding/construction datums (η); the DOF-ledger + geometric-residual diagnostics (θ).

The solve is the **datum-domain single-shot** case (design §5/§10): a sub's local datums realize independent of its assembly pose, so the relate-solve runs once and `ApplyTransform` (task 3901, done) places it — **not** the deferred geometry-in-the-loop cycle.

### G3 — grammar reality (verified 2026-06-08)

`tree-sitter parse` over extracted fixtures:

| Fragment | Parses today? | Disposition |
|---|---|---|
| `fn … -> Relation = …` (pure mate); `fn coincident<D: Datum>(…)` (kind-generic); `Direction` in a signature; `bolt.shank.axis` / `self.xy_plane` / `frame.z` projections; `midplane(…)` / `offset(plane, δ)` construction datums | ✅ exit 0 | use existing grammar (β, γ, η) |
| `at auto` + `relate { }`; `at auto where { }`; `auto(seed=…)` / `auto(free)` / `auto(x=…)`; `joint … with … = { }` | ❌ ERROR nodes | **grammar-producer task δ** (joint form deferred to the joint-half PRD) |

So δ is a real grammar prerequisite; every task that emits `relate`/`at auto` syntax `depends_on` δ. Fixtures live in `/tmp/prd-gate-fixtures/gr-*.ri` (regenerate from this table).

---

## 4. Resolved design decisions

These are **locked** (design §14) — listed so the decomposition isn't re-litigated:

- **`relate` mandated**, type-enforced; `Relation` is a type distinct from `Bool`; three verbs derive (→`Length`/query) / drive (→`Relation`) / check (→`Bool`, `is_`-prefix). `relate{}` accepts only `Relation`; `constraint` only `Bool`.
- **Datum lattice** fully typed (Direction/Point/Axis/Plane/Frame/Scalar); first-class `Direction` (dimensionless unit vector, distinct from `Vector3<Length>` and `Orientation`); member-access **projections** (total downward, a missing projection is the type-level rejection of nonsense); feature→datum **trait bundle** = analytic `GeomAbs_*` classification ∪ construction-history traits, **deduplicated by the same geometric equivalence the relation algebra uses**; `feature.axis : Axis | Axis?` refinement; dedup tolerance = **kernel representation tolerance**, unified with the assertion tolerance and dominating the solver convergence tolerance (the coherence law, `kernel_local ≤ solver_convergence ≤ assertion/dedup`).
- **`coincident(X, X)` removes `codim(X)`** is the one DOF law (no per-relation magic numbers); curation over dependent types; cross-kind only via explicit projection. Three policing layers: unit / kind-projection / curation-of-unconditional-signatures.
- **`at auto`** = a single 6-DOF unknown `Frame`; `auto(…)` parameterized (`free`/`seed`/component-fix); relations primary, value-fixes the escape hatch; residual DOF is an error unless `auto(free)` or (joint-half) a joint `with`.
- **Pure mate = `fn → Relation`** (DOF inferred + surfaced); kind-generic `coincident<D: Datum>` specializes to parallel/coaxial/coplanar/fasten.
- **Grounding:** `self` is the anchor/root datum source; **no global `world`** inside nested structures (encapsulation buys drop-in composability); construction datums are let-bound first-class values.
- **Over-constraint:** redundancy ≠ conflict; driving-set rank partition + redundant-as-assertion; **no `relate` precedence**.
- **Diagnostics** speak geometry, never solver internals; the DOF ledger; reuse undef-self-describing + `W_UNDERDETERMINED`.

---

## 5. Pre-conditions for activating

| Prerequisite | For | Status |
|---|---|---|
| Topology-selector 4118 / 4119 / 4120 (predicate-ctors + resolve, composition + `face()/edge()/body()`, gate) | ε feature→datum bridge | **pending** |
| Unified-DAG driver — `Resolution`-node executor (4357–4362; substrate 4354/4356 done) | ζ relate-solve at the Resolution node | **pending** |
| `ApplyTransform` (task 3901) | ζ placement | **done** ✅ |
| Generics 4232 (done) / 4235 (completion) | kind-generic `coincident<D>` (γ — named specializations don't need 4235) | 4232 done; 4235 **pending** |
| Grammar producer δ (this batch) | δ ← all `relate`/`at auto`-emitting tasks | internal |

**Foundation tasks α, β, γ (specializations), δ are buildable now**; ε/ζ/η/θ stage behind the above via real `add_dependency` edges, so the scheduler runs the foundation immediately and sequences the integration as substrate lands.

---

## 6. Cross-PRD relationship (G4)

| Other PRD / substrate | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/engine-unified-build-dag.md` | consumes | relate-solve runs at the per-scope `Resolution` node (`solve → realize-placed`); declines geometry-in-the-loop with `E_EVAL_UNRESOLVED` | **this PRD** owns the relate-solve executor (ζ); unified-DAG owns the Resolution-node machinery | queued (driver 4357–4362 pending) |
| topology-selector value type (4116–4120) | consumes | feature→datum projection rides on `Type::Selector` / `SelectorKind` | topology-selector PRD owns the substrate; this PRD consumes | blocked-on 4118/4119/4120 |
| `v0_6/kinematic-inter-joint-offsets.md` (KIN-OFFSET-1 / 4331) | produces (joint half) | relate solves the **mount frame/axis**; KIN-OFFSET-1 threads the offset through FK / loop / dynamics | **`geometric-joints.md`** (companion) owns the seam, **co-designed** with KIN-OFFSET-1 (design §8.2) | deferred — **out of scope here**; 4331 to be promoted (stub → full PRD) |
| `v0_6/constraint-solver-completion.md` | extends | `W_UNDERDETERMINED` (§3.6) extended with geometric residual naming | this PRD (θ) | queued |
| undef-self-describing (4321–4327) | extends | `UndefCause::{SolveFailed, DatumProjectionUnavailable}` + `reify explain` | this PRD (θ) | queued |
| `geometry-transforms-frames-projection.md` (P6) | adjacent | `Frame`/`Transform`/`Plane`/`Axis` surface types | this PRD owns **`Direction`** (new, distinct from `Vector3<Length>`/`Orientation`); geometry-transforms owns Frame/Transform/Plane/Axis | wired (Frame/Plane/Axis exist) |
| `generic-user-functions.md` (4232 done / 4235 pending) | consumes | kind-generic `coincident<D: Datum>`; `codim(D)` at monomorphization | generics PRD owns generics; this PRD consumes | 4232 done; kind-generic gated on 4235 |
| mechanism-completion / mechanism subsystem | produces (joint half) | joint = relate-defined mount + mechanism-owned motion variable | `geometric-joints.md` | **out of scope here** |

No new contested-ownership seam introduced (checked against the overlay's known trio). The one reciprocal-risk seam — relate↔KIN-OFFSET-1 (each could claim "the other threads the offset") — is resolved by **co-design under the joint-half PRD**, which is why 4331 should be promoted first.

---

## 7. Contract section (H) — load-bearing seams

### 7.1 The relate-solve seam at the `Resolution` node

Per structure scope, given (i) the set of `Relation`s, (ii) the `at auto` `Frame` unknowns, (iii) the grounded anchor (`self` / a grounded sub), the relate-solve **MUST**:

1. **Realize each sub's local datums** (independent of assembly pose — the single-shot property).
2. **Rank-partition** the relations at the witness config into a **maximal independent driving set** + a **redundant remainder** (design §10).
3. Hand **only the driving set** to `SolveSpaceSolver` → `SolveResult::Solved { values, unique }` (`reify-ir/src/constraint.rs:148`). `auto(free)` ⇒ skip the uniqueness check and accept `unique:false` with a concrete seed value (never a free variable — design §8.1).
4. **`ApplyTransform`** (3901) places each sub from the solved `Frame`.
5. **Verify the redundant remainder** as post-solve geometry-backed predicate assertions (the unified-DAG predicate path), not as solver constraints.

**Invariants:**
- Solve is **per-structure**, hierarchical inside-out; cross-structure-level relations are out of scope (v1).
- Over-constraint is **order-independent** — no `relate` precedence; a consistent redundant set is silent (opt-in lint), an inconsistent set is a loud minimal-conflict diagnostic.
- **Tolerance hierarchy** (the coherence law): `kernel_local ≤ solver_convergence ≤ assertion/dedup`. The *same* tolerance governs dedup, as-built assertion satisfaction, and (dominating) the solver convergence — exposed as one `Length` knob, kernel-defaulted.
- **Grounding:** every `at auto` sub must trace transitively to ground (`self` or a grounded sub); an all-mutually-related auto set with no ground path is a **global-float** error.

**Error semantics:** under-constrained → `W_UNDERDETERMINED` + DOF ledger naming the residual twist (unless `auto(free)`); conflicting → minimal conflict set + geometric explanation + newest-member-as-primary; non-convergent → "try a `seed:` nearer the config"; wrong-root (`unique:false`) → "set `seed:` to choose". Diagnostics **speak geometry, never libslvs internals**.

### 7.2 Feature→datum projection contract

`feature.<projection> : Datum`, total downward per the datum lattice (design §2.1–§2.2): `Planar→Plane`, `Cylindrical→Axis + radius:Length`, `Conical→Axis + apex:Point + half_angle:Angle`, `Spherical→Point(centre)+radius`, `Linear→Axis`, `ArcBounded→Axis+Point(centre)+radius`, `Revolute→Axis`, `Extruded→Direction`, `Vertex→Point`. Provenance = analytic `BRepAdaptor_*`→`GeomAbs_*`→`.Axis()` classification **∪** construction-history datum-traits (covers the non-analytic B-spline tail). The bundle is the **deduplicated union** canonicalized by the relation algebra's own geometric equivalence (coaxial axes merge, coplanar planes merge, coincident points merge within `tol = max(confusion_floor ≈ 0.1µm, localTol(A), localTol(B))`); a genuine disagreement **correctly fails to merge** (signals "not axial"). `feature.axis : Axis | Axis?` — refined to `Axis` for unambiguous features, `Axis?` (or a select-a-subfeature diagnostic) when ambiguous.

### 7.3 The `Relation` type + `relate`-block typing contract

The relation vocabulary returns `Relation` (a DOF-removal directive, **no truth value**). `relate { }` accepts **only** `Relation`; `constraint` accepts **only** `Bool`; cross-placement is a type error. Three policing layers: **(a) unit** (the metric arg's dimension is in the signature — `angle(a,b,5mm)` / `distance(a,b,30°)` are type errors); **(b) kind/projection** (operands must project to the named datum type; implicit projection allowed *iff unique* — `Axis→Direction` via `.dir` auto-lifts, `Frame→Direction` is ambiguous → write `frame.z`); **(c) curation** (ship only unconditionally-well-defined signatures; conditional cases are compounds bundling their precondition — e.g. no bare `distance(Plane,Plane)`; `offset(a,b,δ) = parallel(a,b) & on(a.point,b,δ)`). DOF inference: each overload publishes a nominal ΔDOF (design §3.4); a compound infers ΔDOF by summing its body and surfaces it as its contract (hover: `offset(Plane,Plane,Length) -> Relation removes 3`). Home: `relation_signatures.rs`, mirroring `math_signatures.rs` / `joint_signatures.rs`.

---

## 8. Boundary-test sketch (H) — facing both the solve engine and the `.ri` author

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| B1 | concentric + flush bolt (the §1 example) | bolt + plate realized; 2 relations; 1 residual | solved pose coaxial within tol, seat coplanar within tol; ledger `spent 5 / free 1 → rotation about bolt.shank.axis`; `reify build` emits placed mesh |
| B2 | redundant-consistent over-constraint | 3 relations, rank 2 | driving-set = 2 to SolveSpace; the 1 redundant verified as a **passing assertion**; **silent** by default; no error |
| B3 | conflicting relations | `concentric` (0mm) + `at_distance(…, 5mm)` | minimal conflict set; geometric explanation ("concentric forces 0mm; at_distance forces 5mm"); newest member flagged; `build` fails loud |
| B4 | under-constrained, no `auto(free)` | 1 relation, 1 residual | `W_UNDERDETERMINED` + ledger names the residual twist; error |
| B5 | under-constrained **with** `auto(free)` | same + `auto(free)` | solves; `unique:false`; residual seeded; no error |
| B6 | global float | auto subs relate only to each other, no ground path | global-float diagnostic "6 DOF — the assembly floats in `self`: ground a part" |
| B7 | as-built degradation | fully-determined operands (no `auto`), 1 relation | relation degrades to **assertion**: satisfied → silent, violated → conflict diagnostic |
| B8 | datum dedup | revolved-rectangle cylinder | `cylinder.axis` resolves to **one** `Axis` (side-face + 2 end-arc + revolute-history sources all merge within tol) |
| B9 | projection category error (static) | `angle(p1, p2, θ)`; `distance(some_direction, …)` | typed `E_DATUM_*` error at `reify check`, **before** any solve |
| B10 | unit category error (static) | `angle(a, b, 5mm)` | unit type error at `reify check` |
| B11 | wrong-root | multi-root config, no seed | `unique:false` + "set `seed:` to choose" + visualize/re-seed loop |

The integration-gate task **ζ** names B1–B3 + B5 as its observable signal (a CI `.ri` example suite); these face both the producer (solve engine + datum bridge) and the consumer (`reify build`/`check`/`explain`).

---

## 9. Decomposition plan (α–θ; G2 signal per task)

Greek labels here; task IDs assigned at decompose time. **Phase 1 = foundation** (α–δ, buildable now); **Phase 2 = vertical slice** (ε + ζ — the end-to-end concentric+flush example, the consumer signal); **Phase 3 = surfaces** (η, θ).

- **α — Register `SolveSpaceSolver` in the production `SolverRegistry` (CLI + GUI).** Modules: `reify-constraints/src/{registry.rs,solvespace.rs}`, `reify-cli/src/main.rs`, GUI engine. *Signal (intermediate → unlocks ζ):* a `.ri` fixture with a geometric-classified constraint (`std::parallel`/`std::distance`) that produced no geometric solve before now reaches `SolveSpaceSolver` and solves — asserted by a `reify check`/`eval` integration test; SolveSpace present in the registry the CLI/GUI engines install. *Prereqs:* none. `grammar_confirmed=true`.

- **β — First-class `Direction` type + datum-projection member access** (`.dir/.normal/.origin/.x/.y/.z/.xy_plane` on Axis/Plane/Frame/Direction; total-downward; missing projection = typed error). Modules: `reify-types/src/ty.rs`, `reify-*/value.rs`, geometry value variants. *Signal (intermediate → γ,ε,ζ,η,θ):* `reify check` accepts `let d : Direction = axis.dir`, rejects `point.dir` with `E_DATUM_PROJECTION_UNAVAILABLE`, and rejects the ambiguous bare `frame.dir`; a CI `.ri` example exercises projections. *Prereqs:* none. `grammar_confirmed=true` (projections parse).

- **γ — Relation vocabulary (`relation_signatures.rs`) + the `Relation` type + the three policing layers.** Primitives (incidence / angle / distance), `coincident<D: Datum>` + named compounds (`concentric/flush/offset/tangent/parallel/perpendicular/antiparallel`), ΔDOF inference + surfacing. Modules: `reify-compiler/src/relation_signatures.rs` (NEW), classifier routing, generics. *Signal (intermediate → δ,ζ,η,θ):* `reify check` types `concentric(a: Axis, b: Axis) -> Relation`; rejects B9/B10 category errors with typed diagnostics; hover shows `offset(Plane,Plane,Length) -> Relation removes 3`. *Prereqs:* β; **kind-generic `coincident<D>` gated on 4235** (named specializations don't need it). `grammar_confirmed=true` (`fn -> Relation`, generics parse).

- **δ — Grammar production: `relate { }` + `at auto` / `auto(…)` pose-binding + `at … where { }` + `Relation`-vs-`Bool` enforcement + the 3-verb routing.** tree-sitter rule + parser tests + lowering. Modules: `tree-sitter-reify/`, `reify-compiler` lowering, auto-binding-sites framework, constraint dispatch. *Signal (intermediate → ζ,η):* fixtures `gr-01/02/03` parse (`tree-sitter parse --quiet` exit 0) with parser tests in `tree-sitter-reify/tests/`; `relate { }` rejects a `Bool` body member (`E_RELATE_EXPECTS_RELATION`) and `constraint` rejects a `Relation`. *Prereqs:* β, γ (the `Relation` type must exist for enforcement). **`grammar_confirmed=false` — this is the grammar producer.**

- **ε — Feature→datum trait bundle (analytic `GeomAbs_*` ∪ construction history) + dedup by geometric equivalence + `feature.axis : Axis|Axis?` refinement.** *The real missing bridge.* Modules: `reify-compiler/src/geometry_traits_inference.rs`, topology-selector value type (4118–4120), OCCT `BRepAdaptor_*` FFI. *Signal (intermediate → ζ,θ):* a `.ri` example where `cylinder.axis` / `hole.axis` resolves to a concrete `Axis` over a realized feature (`reify eval` prints the datum / a relation over it type-checks against the realized solid); **B8 dedup** (revolved-rectangle cylinder → one axis) passes. *Prereqs:* β; **4118 / 4119 / 4120**.

- **ζ — Per-scope relate-solve at the `Resolution` node: driving-set rank partition → `SolveSpaceSolver` → `ApplyTransform`; redundant-remainder as geometry-backed assertions.** *The vertical slice / integration-gate leaf.* Modules: unified-DAG `Resolution` node, `reify-constraints`, `ApplyTransform` (3901). *Signal (leaf — the consumer signal):* the §1 example builds — `reify build` places the bolt coaxial + flush (GUI mesh position via debug MCP / CI example asserting the solved transform); **boundary tests B1–B3 + B5** pass. *Prereqs:* α, β, γ, δ, ε; **unified-DAG driver 4357–4362**.

- **η — `self` datums + grounding + construction-datum constructors.** Implicit-ground, trace-to-ground, the global-float diagnostic, `ground`/`fix` sugar; `midplane/axis_through/plane_through/offset/frame_at`. Modules: `self` handling, `frame3`/`plane_*`/`axis_*`. *Signal (leaf):* a `.ri` example binds a construction datum (`let mid = midplane(...)`) and mates to it (builds); an ungrounded auto-assembly emits the **B6 global-float** diagnostic; `ground(sub)` sugar resolves to `fasten(sub.frame, self.frame)`. *Prereqs:* β, γ, δ, ζ. `grammar_confirmed=true` (constructors + `self` projections parse).

- **θ — DOF ledger + geometric residual naming + conflict sets; plug into undef-self-describing + `W_UNDERDETERMINED` + determinacy.** Add `UndefCause::{SolveFailed{under/over/diverged}, DatumProjectionUnavailable}`; null-space twist naming at the witness config. Modules: undef-self-describing tracer, `reify explain`, `W_UNDERDETERMINED` (constraint-solver-completion §3.6), determinacy, GUI badge. *Signal (leaf):* `reify explain` on an under-constrained `at auto` sub prints the DOF ledger ("spent 5 · free 1 → rotation about bolt.shank.axis"); a conflicting `relate` (**B3**) emits the minimal conflict set with geometric explanation; GUI DOF badge updates. *Prereqs:* γ, ε, ζ.

**DAG:** α→ζ · β→{γ,ε,ζ,η,θ} · γ→{δ,ζ,η,θ} · δ→{ζ,η} · ε→{ζ,θ} · ζ→{η,θ}. Out-of-batch: ε←{4118,4119,4120}; ζ←{4357–4362}; γ(kind-generic)←4235; ζ←3901(done). **Companion correction-task** (decompose-time): a prose-update task pointing `geometric-joints.md` + KIN-OFFSET-1 at design §8.2's co-design seam.

---

## 10. Out of scope for this PRD

- **The joint half** (design §8 / §11 step 9): `joint…with`, the self-checking law, mount→FK offset threading, couplings-on-the-scalar-side → companion **`geometric-joints.md`**, hard-gated on **KIN-OFFSET-1** (task 4331, deferred) and **co-designed with it** (design §8.2). Promote 4331 (stub → full B+H PRD) before authoring the joint half. The `joint … with` grammar (fixtures `gr-05a/05b` FAIL today) is that PRD's grammar producer, not δ.
- **Geometry-in-the-loop solving** (a relation whose datum depends on the very pose it constrains) — `E_EVAL_UNRESOLVED`; a future PRD reintroducing a bounded fixpoint scoped to the `Resolution` node (design §12).
- **Closed kinematic loops with mobility** via the geometric solver — owned by the existing loop-closure Newton solver (design §8.3).
- **First-class partial application** of relations (depends on closure/function-value support; relations are still first-class bindable/passable/conjoinable values).
- **Cross-structure-level relations** — solves are per-structure in v1.

---

## 11. Open questions (tactical — surfaced, not blocking; design §13)

1. **Standard mate library enumeration.** Pin the exact mate set (concentric / flush / offset / tangent / centered / symmetric) and definitions. *Decide during γ.*
2. **Interop / migration.** Coexistence with explicit `at <pose>` and the raw-selector escape hatch; a migration story for existing `.ri` files. *Decide during δ / η.*
3. **Final keyword spelling.** `relate` / `at auto` / `self` / `ground` are working names — confirm before δ's grammar lands. *Decide before δ.*
4. **Dedup tolerance knob surface.** Per-model vs per-`relate`; the exact default expression (design §2.3). *Decide during ε.*
5. **`reify explain` ledger format.** Table vs `--format json` (mirror `reify doc`). *Decide during θ.*

---

## 12. Notes for decompose mode

- File α–θ with `planning_mode=True`; wire **all** deps (intra-batch per §9 DAG + out-of-batch 4118/4119/4120, 4357–4362, 4235, 3901) while deferred; flip the whole batch to `pending` in one bulk call. The scheduler runs α/β/γ/δ now and sequences ε/ζ/η/θ as substrate lands (blocked-vs-pending semantics).
- Build the **capability manifest** beside this PRD (`geometric-relations.capability-manifest.md`): grammar-fixture bindings for δ (the `gr-0*` fixtures), wired-on-main bindings for α (registry), field-population N/A (no result-field sampling), numeric-floor N/A (the DOF numbers are exact codimension counts from the `codim(X)` law — not error-floor bounds, so G6 branches 1/2 don't fire; branch 3 traces each ζ capability to α/β/γ/δ/ε + the unified-DAG dep). Any FAIL blocks the batch.
- The `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata fields are substrate for future tracking infra — the orchestrator does not read them yet.
