# Stdlib surface-type substrate: blocked-surface-type tightenings (Vec3 / Range / Pose3 / LocationId / Part / ModalResult / loop-closure / force-velocity-acceleration) + the stdlib module-graph foundation

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-06-12 · **Approach: H** (hub-and-spoke contracts + two-way boundary tests — this PRD OWNS the genuinely-unowned surface-type families and the stdlib-prelude foundation, and ROUTES the already-owned families to their existing v0_6 PRD homes via a read-only cross-PRD seam table; blast radius spans reify-core / reify-compiler (stdlib) + reify-eval + examples)

Authored from the 2026-06-11 stdlib-placeholder-registry audit (tracking task 4548). This is the **assess-and-route** owner for the audit's *blocked-surface-type* bucket — the family of `pub type X = Real` aliases and `param X : Real`/`: String` placeholders whose tightening triggers have not fired because the target surface type does not yet exist (or is cross-module-unreachable, §7). It is the sibling of `docs/notes/stdlib-real-placeholder-audit.md`, which closed the *tightenable-now* / *blocked-composite* (#3115) / *blocked-geometry-type* (#3116) buckets; this PRD closes the residual *blocked-surface-type* bucket. All file:line citations re-verified 2026-06-12 against the working tree (post-task-4548 Phase-A: `Impulse`/`Momentum` registered, `ImpulseForce.impulse`→`Impulse`, `Mode.frequency`→`Frequency`).

## 1. Goal

After this PRD's decomposition lands, the stdlib carries **no `= Real` / `: Real` / `: String` surface-type placeholder** for a concept that has a real nominal or dimensioned type. Concretely:

- `pub type Vec3 = Real` (trajectory.ri:96) and every `param axis : Vec3` (kinematic.ri) become a **real 3-vector type** shared by `std.trajectory`, `std.kinematic`, `std.fea.multi_case`, and `std.fdm` — so assigning a scalar where a 3-vector is expected is a **compile error**, not a silent `Real`-alias collapse (the η-phase hazard documented at trajectory.ri:50-62).
- `param range : Real` (kinematic.ri SweepDim) becomes `Range<T>`, routed to the PRD that already owns `Range<T>`.
- `pub type Pose3 = Real` / `pub type LocationId = Real` (trajectory.ri:87/:106) become a real **pose** type and a **topology-selector** type (built on the *landed* Selector substrate, not a fresh alias).
- `ModalResult.part : String` / `ForcingTimeHistory.part : String` / `TransientResponse.part : String` (modal_analysis.ri) and the `feature`-style `Part` placeholders become a real **`Part`** structure_def.
- `EndEffectorTrack.modal_result : Real` (trajectory.ri:327) becomes the nominal **`ModalResult`** type; `Mechanism.joint_parents : Map<String, Real>` / `loop_closures : List<Real>` (kinematic.ri) become real **record** types.
- `JointLimit.max_force : Real`, `TOTSShaper.velocity_limit : Real`, `TOTSShaper.acceleration_limit : Real` (trajectory.ri) become `Scalar<Force>` / `Scalar<Velocity>` / `Scalar<Acceleration>` (the **dimensional-tightening sweep**; `Velocity` is a NEW named dimension).
- **The structural enabler is named and owned:** the stdlib prelude no longer hard-codes a single linear load order that makes a shared cross-module surface type unreachable (§7).
- **Every audit-listed marker cites a live owning task** (the 4548 closeout invariant): no `TODO(<label>)` / `FIXME(<label>)` in the blocked-surface-type family ends task 4548 uncited.

## 2. Background

The stdlib-real-placeholder audit (`docs/notes/stdlib-real-placeholder-audit.md`) classified every `param X : Real` into six buckets and drove three tightening waves: *tightenable-now* (#3111/#3112/#3113), *blocked-composite* → named-dimension aliases (#3115), *blocked-geometry-type* → `Geometry`/`DatumRef` (#3116). What remains is a seventh, implicit bucket the audit did not have a row for: **blocked-surface-type** — placeholders whose target is a *new nominal/record/parametric surface type* (not a dimension alias, not `Geometry`), and which in several cases is **cross-module-shared** and therefore blocked by the stdlib loader's linear-order model (§7), not merely by a missing type.

These placeholders were introduced deliberately, with an in-file convention (modal_analysis.ri header §1; trajectory.ri:535-540): *all* dimensioned scalars and not-yet-existing surface types are encoded as `Real` (or `String` for selector/name placeholders) with a `TODO(<label>)`/`FIXME(<label>)` naming the future-tightening target, so a single sweep can retarget each family in one pass. The convention worked — but the triggers (the owning surface types) never landed, so the markers accumulated uncited. The 2026-06-11 audit flagged the family; task 4548 is its tracking owner.

**The families** (full enumeration in §9):

| Family | Placeholder today | Target | Owner |
|---|---|---|---|
| `vec3-type` | `pub type Vec3 = Real`; `param axis : Vec3` | real 3-vector | **routed → V #4575** (existing PRD homes) |
| `range-type` / `range-angle-type` | `param range : Real`; `prb_validity_range : Real` | `Range<T>` / `Range<Angle>` | **routed → R #4576** (numeric-and-range PRD) |
| `pose3-type` | `pub type Pose3 = Real` | rigid pose/frame | **owned → P #4577** |
| `location-id-type` | `pub type LocationId = Real`; `at : String` | topology selector | **owned → P #4577** (threads landed Selector substrate, task 4116) |
| `part-structdef` | `part : String` | `Part` structure_def | **owned → Pt #4578** |
| `modal-result-type` | `modal_result : Real` | nominal `ModalResult` | **owned → M #4579** |
| `loop-closure-record-type` / `Map<BodyId,JointParent>` | `loop_closures : List<Real>`; `joint_parents : Map<String, Real>` | record types | **owned → M #4579** |
| `force-velocity-acceleration-scalar` | `max_force/velocity_limit/acceleration_limit : Real` | `Scalar<Force/Velocity/Acceleration>` | **owned → S #4580** (Velocity = new named dim) |
| stdlib module-graph **FOUNDATION** | linear `include_str!` prelude | DAG-loaded / forward-ref-resolved prelude | **owned → F #4574** |

Two families (`vec3-type`, `range-type`) already have natural homes on main (the markers self-document this — "retarget to the geometry/pose PRD's Vec3"). This PRD does **not** re-own them; it routes them via §6's seam table and files their tightening tasks as dependents of the owning PRDs. Everything else is genuinely unowned and owned here.

## 3. Resolved design decisions

1. **Hub-and-spoke, not monolith.** This PRD owns only the genuinely-unowned families (Pose3/LocationId, Part, ModalResult/loop-closure/Map, the force/velocity/acceleration sweep) plus the stdlib-prelude FOUNDATION. The already-owned families (`Vec3`, `Range<T>`) are routed to their existing v0_6 PRD homes (§6), referenced **read-only** — their PRD docs are not edited and their lock set is not touched. Rationale: a monolith re-owning `Vec3`/`Range` would collide with active decompositions (`affine-map-type`, `math-linalg-n-generality`, `kinematic-inter-joint-offsets`, `numeric-and-range-literal-forms`); singular ownership is preserved by routing rather than absorbing.

2. **`LocationId` rides the LANDED Selector substrate (task 4116), not a new `= Real` alias.** modal_analysis.ri:441-442 explicitly calls `LocationId` "the future `LocationId` topology-selector type". Task 4116 already introduced `Value::Selector` / `Type::Selector` / `SelectorKind{Face,Edge,Body}` (the topology-selector value/type substrate). A `LocationId` that selects a location on a `Part` **is** a topology selector, so the P-task threads the 4116 machinery rather than re-inventing selection as a parallel alias-to-nominal. The `at : String` placeholders (modal_analysis.ri StepForce/ImpulseForce/HarmonicForce/SampledForce) migrate to the same selector type.

3. **`Pose3` is genuinely new and pairs with `affine-map-type`'s `Transform3`.** `affine-map-type.md` §4.4 owns the rigid `Transform3 { rotation: Orientation, translation: Vector3<Length> }` but does **not** own a `Pose3` surface type. A 3-D rigid-body pose (position + orientation at one instant) is the same algebra as `Transform3`; the P-task introduces `Pose3` as a real type adjacent to (and convertible with) `Transform3`, and the PRD notes that adjacency so the two do not diverge. `Pose3` stays owned here (it is unowned on main) while `Vec3`/`Vector3` route out (owned).

4. **`Velocity` is a NEW named dimension; `Force` and `Acceleration` already exist.** Verified 2026-06-12 in `crates/reify-core/src/dimension.rs`: `FORCE` (:491) and `ACCELERATION` (:513) are registered; there is **no** `Velocity` entry (only `AngularVelocity` :509), confirming trajectory.ri:551's in-file note ("Velocity NOT in NAMED_DIMENSIONS"). The S-task adds `pub const VELOCITY = from_exps(&[(0,1),(2,-1)])` (m·s⁻¹) + a `(VELOCITY, "Velocity")` table entry — mirroring exactly the `IMPULSE`/`Momentum` registration task 4548 landed in Phase A (dimension.rs:555-556) — then retargets the three trajectory placeholders to `Scalar<Force>`/`Scalar<Velocity>`/`Scalar<Acceleration>`. The dimensioned-zero constraint convention (`constraint max_force > 0 * 1N`, the in-file precedent at modal_analysis.ri HarmonicForce `frequency > 0Hz` and the Phase-A `impulse > 0 * 1N * 1s`) is used; polymorphic-zero has NOT landed.

5. **The FOUNDATION is the NARROWER stdlib-prelude migration, not "build DAG loading from scratch".** `crates/reify-compiler/src/module_dag.rs` already exists (DFS, cycle detection, post-order topological sort, embedded-stdlib fallback) — but it serves **user-project** imports. The stdlib prelude is still a hardcoded linear `include_str!` sequence (`stdlib_loader.rs::load_stdlib`). The F-task migrates the stdlib prelude onto the existing `ModuleDag` (add `import std.*` decls to the stdlib `.ri` files + topo-load) **OR** adds forward-reference resolution to the growing-prelude compile. See §7.

6. **Re-citation is canonical-form, verified with the reify-audit PTODO detector.** Every routed marker is rewritten `TODO(<label>, #NNNN):` / `FIXME(<label>, #NNNN):` — the human-readable label is preserved and `#NNNN` (1..=5 ASCII digits, non-zero) anywhere on the marker line satisfies the structural lane (`crates/reify-audit/src/ptodo.rs` `has_canonical_cite`/`extract_cites` scan the whole line). The liveness lane (β) additionally requires the cited id to resolve to a LIVE non-terminal task, so the owning child tasks (§10) are filed (decompose) **before** re-citation. The land-now markers `FIXME(impulse-dim)` and printer `TODO(frequency-scalar)` are **deleted** (resolved by 4548 Phase A), not re-cited.

7. **Scope boundary — do not touch out-of-family markers.** The cited stdlib files also carry markers owned by *other* lines: `joint-value-type`, `mechanism-type`, `joint-type`, `body-id-type`, `body-type` (kinematic-completion PRD), `beta-3816` (B-spline evaluator), `trait-coerce` (trait coercion), `stiffness-type` (flexures α), `β-phase` build-direction. These are **not** in this PRD's families and are left to their owners. Only the families in §9 are touched.

## 4. Out of scope (named)

- **`Vec3`/`Vector3` surface type itself** — owned by `affine-map-type.md` (`Vector3<Length>`, `vec3()` in affine context), `math-linalg-n-generality-and-signatures.md` (`vec`/`vec2`/`vec3` construction signatures, §2/§3), and `kinematic-inter-joint-offsets.md` (`point3`/`vec3` joint authoring, §3/§7.1). This PRD routes the *stdlib placeholder tightening* to those homes (§6, task V); it does not design the vector type.
- **`Range<T>` surface type itself** — owned by `numeric-and-range-literal-forms.md` (§1/§2: `Range<T>` + `.contains`/`.lower`/`.upper`). `tolerancing-gdt-surface-completion.md` §4 decision 6 explicitly scopes `Range<Length>` OUT to that PRD. This PRD routes the stdlib placeholder tightening there (§6, task R).
- **`module` path-declaration grammar + path-vs-location enforcement + `priv` visibility** — owned by `module-and-visibility-hardening.md` (§0/§1 spec §7.1, §8 tasks α/γ/δ). That PRD owns `module_dag.rs`'s module-PATH semantics; it does **not** own the stdlib prelude LOAD-ORDER or forward-reference resolution (its §5 out-of-scope is silent on both). The FOUNDATION (task F) is therefore unowned and lives here, cross-referencing that sibling (§6/§7).
- **The dimensioned-scalar tightenings landed by 4548 Phase A** — `Impulse`/`Momentum` named dimension, `ImpulseForce.impulse`→`Impulse`, `Mode.frequency`→`Frequency`. Done; their markers are deleted, not re-cited.
- **Polymorphic literal zero** — owned by `type-hygiene.md` (β). The S-task uses the dimensioned-zero RHS convention in the interim.
- **The other surface-type placeholders owned elsewhere** (decision 7): `joint-value-type`/`mechanism-type`/`joint-type`/`body-id-type`/`body-type` (kinematic-completion), `beta-3816`, `trait-coerce`, `stiffness-type`.

## 5. Pre-conditions

- **F (FOUNDATION) gates V, P, M, and cross-module R.** A shared `Vec3`/`Pose3`/`ModalResult`/record type used by both an early-loaded module (`std.fea.multi_case`, loaded before `std.trajectory`) and a late-loaded one (`std.kinematic`, loaded after) is **unreachable** under the current linear loader (§7). The tightening tasks that introduce a shared cross-module type therefore depend on F (real dep edges added at decompose).
- **Selector substrate (task 4116) is LANDED** — `Value::Selector`/`Type::Selector`/`SelectorKind` present on main. P (LocationId) consumes it; no new substrate needed.
- **`Force`/`Acceleration` named dimensions present; `from_exps` const helper + NAMED_DIMENSIONS table-driven resolution present** (dimension.rs) — S adds only `Velocity` + retargets. The `Scalar<Q>` parametric resolver arm + `Vector3<Q>`/`Point3<Q>` arms are present (`type_resolution.rs`, audit note §"Resolver capability reference").
- **No novel grammar for the owned tasks** beyond what the routed PRDs introduce. `Part`/`Pose3`/`ModalResult`/record types are `structure def`s in existing grammar. The FOUNDATION (F) may introduce `import std.*` decls if it takes the ModuleDag-migration branch — that grammar is owned by `module-and-visibility-hardening.md` and consumed here (G4 seam).

## 6. Cross-PRD relationships (G4 seam table)

Existing PRDs referenced **read-only** — not edited; routed tasks filed as their dependents to bound this task's lock set to its own files.

| Seam | Direction | Mechanism | Owner |
|---|---|---|---|
| `numeric-and-range-literal-forms.md` | routes-to | `Range<T>` + `.contains`/`.lower`/`.upper` (§1/§2) | **R #4576** filed as dependent; markers `range-type`/`range-angle-type` cite #4576. tolerancing-gdt §4 decision 6 scopes `Range<Length>` here. |
| `affine-map-type.md` | routes-to / adjacent | `Vector3<Length>`, `vec3()` (§4.2); `Transform3` rigid pose (§4.4) | **V #4575** (Vec3) filed as dependent. **P #4577** (Pose3) stays owned here but is designed adjacent to `Transform3` so they converge. |
| `math-linalg-n-generality-and-signatures.md` | routes-to | `vec`/`vec2`/`vec3`/`matrix` construction + compiler signatures (§2/§3) | **V #4575** filed as dependent. |
| `kinematic-inter-joint-offsets.md` | routes-to | `point3`/`vec3` joint authoring (§3/§7.1) | **V #4575** filed as dependent; the `param axis : Vec3` kinematic.ri sites are this line's consumers. |
| `module-and-visibility-hardening.md` | complements | `module` path semantics + `import` grammar (§7.1, §8 α/γ); `module_dag.rs` user-project loading | **F #4574** owns the stdlib-prelude load-ORDER / forward-refs (that PRD's §5 leaves it out); F consumes the `import` grammar if it takes the ModuleDag branch. Zero doc edit to that PRD. |
| Selector substrate (task 4116) | consumes (landed) | `Value::Selector`/`Type::Selector`/`SelectorKind` | **P #4577** threads it for `LocationId`; no new substrate. |
| `type-hygiene.md` β (polymorphic zero) | consumes-interim | dimensioned-zero RHS convention | **S** uses `> 0 * 1N` in the interim; β's sweep may later simplify. |
| `docs/notes/stdlib-real-placeholder-audit.md` | sibling | blocked-surface-type is the residual bucket | this PRD; audit's other buckets closed by #3111/#3115/#3116. |

## 7. Structural blocker (the FOUNDATION — task F)

**The placeholder is not the only blocker; the loader is.** `crates/reify-compiler/src/stdlib_loader.rs::load_stdlib()` compiles the stdlib as a **hardcoded linear `include_str!` sequence with a growing prelude**: each module is compiled against all *previously-listed* modules (the `for (module_name, source) in &sources` loop, `&modules` growing one per iteration). Dependency is **enforced by position**, documented in the ordering comments throughout the `sources` vec.

The crux for a *shared cross-module surface type*:

- `std.fea.multi_case` is listed **early** (loader `sources` index ≈ `std.fea.multi_case`, before `std.analysis`/`std.modal.analysis`).
- `std.trajectory` is listed **later** and is where `pub type Vec3 = Real` (and `Pose3`/`LocationId`/`JointValue`) actually live (trajectory.ri:87-106).
- `std.kinematic` is listed **last-ish**, and its ordering comment states verbatim: *"Depends on std.trajectory (Vec3 and JointValue aliases) and std.units"* — a **backward** dependency satisfied only because trajectory precedes kinematic in the vec.

So `Vec3` is used by `std.fea.multi_case` (early), `std.trajectory` (mid), `std.kinematic` (late), and `std.fdm` — but it is *defined* mid-sequence in trajectory. Under the growing-prelude model, `std.fea.multi_case` (compiled **before** trajectory) **cannot see** trajectory's `Vec3` alias; the only reason the markers compile today is that each module independently aliases `Vec3 = Real` (or uses bare `Real`). The moment `Vec3` becomes a single *real* nominal type, it must be defined in **one** module that is loaded **before every consumer** — impossible to retrofit into the current linear order without either (a) hoisting a new foundational geometry module to the top of the vec and rewiring every consumer through the growing prelude, or (b) giving the loader real dependency resolution.

`module_dag.rs` **already provides (b) for user projects**: a full module-DAG loader (DFS + cycle detection + post-order topological sort + embedded-stdlib fallback). But the **stdlib prelude does not route through it** — `load_stdlib()` predates it and still uses the linear vec. `module-and-visibility-hardening.md` owns `module_dag.rs`'s module-PATH semantics (file-location match, §7.1; grammar/enforcement, §8 α/γ) but **not** the stdlib load-ORDER (its §5 out-of-scope does not mention prelude order or forward-refs) — so the FOUNDATION is genuinely unowned.

**F's two candidate approaches (decided in the F-task, not here):**
1. **Migrate the stdlib prelude onto `ModuleDag`** — add `module std.*` + `import std.*` decls to the stdlib `.ri` files and topo-load them through the existing DAG, replacing the position-enforced vec. Reuses landed machinery; depends on the `import` grammar (`module-and-visibility-hardening.md`, G4 seam).
2. **Add forward-reference resolution** to the growing-prelude compile — a pre-pass that registers all stdlib type *names* before bodies compile (mirrors the same-module skeleton pre-pass, task 3895, already used so a `structure_def` is visible to an accessor fn in the same module).

Either unblocks a single shared `Vec3`/`Pose3`/`ModalResult`/record type. **Without F, the V/P/M families are permanently stuck** (the explicit warning in task 4548). F is therefore the spine of the decomposition (§10).

## 8. Contract section (H) + two-way boundary tests

Per owned family. Each contract is pinned by a two-way boundary test (positive: the tightened type accepts the right value; negative: the wrong value is now a loud error where it was silently `Real`-accepted before).

### 8.1 FOUNDATION (F)
A stdlib module loaded at any position can reference a surface type defined in any other stdlib module, regardless of vec order. **Boundary test:** define a real `Vec3` in one foundational module; reference it from both `std.fea.multi_case` (early) and `std.kinematic` (late); `load_stdlib()` compiles with zero Error diagnostics (today: the early consumer fails name resolution). Negative: a genuine cyclic stdlib import is rejected with a cycle diagnostic (module_dag's cycle detection).

### 8.2 Vec3 (V, routed)
`param axis : Vec3` resolves to the real 3-vector type (per the owning PRD). **Boundary test:** `revolute(axis: <a real Vec3>)` type-checks; `revolute(axis: 1.0)` (bare scalar) is a **compile error** (today: silently accepted — the η-phase alias-collapse hazard, trajectory.ri:50-62). Wiring lives in the V-task under its owning PRD.

### 8.3 Range (R, routed)
`param range : Range<T>` (SweepDim) / `prb_validity_range : Range<Angle>` (flexures) resolve to `Range<T>`. **Boundary test:** a `Range` literal with `.contains`/`.lower` is accepted; a bare scalar is rejected. Wiring lives in the R-task under `numeric-and-range-literal-forms.md`.

### 8.4 Pose3 + LocationId (P)
`pub type Pose3` is a real pose type; `pub type LocationId` / `at : <selector>` resolve to the Selector substrate (task 4116). **Boundary test:** a `StepForce.at` selecting a face on a `Part` type-checks; a `Pose3` passed where a `LocationId` is expected is a **compile error** (today: both are `Real`, silently interchangeable). Pose3 round-trips with `Transform3` (affine-map adjacency).

### 8.5 Part (Pt)
`ModalResult.part` / `ForcingTimeHistory.part` / `TransientResponse.part` resolve to a real `Part` structure_def (no longer `String`). **Boundary test:** a `Part` value is accepted; a bare string literal is rejected where `Part` is required (today: `String` placeholder accepts any string). The producer (`modal_ops.rs`) echo path migrates from `""`/string to a real `Part` handle.

### 8.6 ModalResult + loop-closure/Map records (M)
`EndEffectorTrack.modal_result` resolves to the nominal `ModalResult` type; `Mechanism.joint_parents : Map<BodyId, JointParent>` and `loop_closures : List<LoopClosure>` resolve to real record types. **Boundary test:** an `EndEffectorTrack` built with a real `ModalResult` type-checks; a bare `Real` is rejected. Depends on F (cross-module reach: `ModalResult` is defined in `std.modal.analysis`, consumed in `std.trajectory`).

### 8.7 force/velocity/acceleration sweep (S)
`Velocity` is registered in `NAMED_DIMENSIONS`; `JointLimit.max_force : Scalar<Force>`, `TOTSShaper.velocity_limit : Scalar<Velocity>`, `TOTSShaper.acceleration_limit : Scalar<Acceleration>`. **Boundary test (mirrors 4548 Phase-A impulse):** a dimensioned `<Force>` value is accepted for `max_force`; a `Scalar<Velocity>` passed for `max_force` is a **compile/constraint error** (today: both bare `Real`); `constraint max_force > 0 * 1N` reports OK/VIOLATED. Reads go through tolerant `si_value` extractors (non-breaking on the read side, per the 4548 Phase-A precedent).

## 9. Reference inventory (marker → family → owning task)

Every task-4548-listed marker, by family. Line numbers re-verified 2026-06-12 post-Phase-A (drifted from the plan's pre-Phase-A numbers; re-cite by content). **Re-citation targets (V/R/P/Pt/M/S) are assigned real IDs at decompose (§10).**

| Family → task | Marker sites (file:line, current label) |
|---|---|
| `vec3-type` → **V #4575** | `fea_multi_case.ri` TODO(vec3-type) ×2 (≈:305, :433); `kinematic.ri` TODO(vec3-type) ×5 (axis ≈:99/:113/:122, axis_x/axis_y ≈:129/:130); `trajectory.ri` TODO(vec3-type) (alias ≈:94); `fdm.ri` ≈:105 (β-phase build_direction normalization that rides Vec3) |
| `range-type` / `range-angle-type` → **R #4576** | `kinematic.ri` TODO(range-type) ×2 (SweepDim.range ≈:211/:216); `flexures.ri` TODO(range-angle-type) ×2 (≈:127/:145) |
| `pose3-type` + `location-id-type` → **P #4577** | `trajectory.ri` TODO(pose3-type) (≈:85), TODO(location-id-type) (≈:104); `modal_analysis.ri` FIXME(location-id-type) ×4 (StepForce/ImpulseForce/HarmonicForce/SampledForce `at` ≈:441/:509/:556/:621) |
| `part-structdef` → **Pt #4578** | `modal_analysis.ri` FIXME(part-structdef) ×3 (ModalResult.part ≈:216, ForcingTimeHistory.part ≈:682, TransientResponse.part ≈:754) |
| `modal-result-type` + `loop-closure-record-type` / `Map<BodyId,JointParent>` → **M #4579** | `trajectory.ri` TODO(modal-result-type) (EndEffectorTrack ≈:322, continuation ≈:324); `kinematic.ri` Mechanism: `TODO: Map<BodyId,JointParent>` ×2 (≈:190/:194), TODO(loop-closure-record-type) ×2 (≈:191/:195) |
| `force-velocity-acceleration-scalar` → **S #4580** | `trajectory.ri` TODO(force-scalar) (JointLimit.max_force ≈:490), TODO(velocity-scalar) (≈:550), TODO(acceleration-scalar) (≈:553), convention prose (≈:536) |

**Deleted by 4548 Phase A (not re-cited):** `FIXME(impulse-dim)` (modal_analysis.ri, resolved by Impulse registration + `impulse : Impulse`); `TODO(frequency-scalar)` ×2 (printer_print_envelope.ri:37/:133, resolved by `Mode.frequency : Frequency`).

**Out-of-family in these files — DO NOT TOUCH (decision 7):** `joint-value-type`, `mechanism-type`, `joint-type`, `body-id-type`, `body-type`, `beta-3816`, `trait-coerce`, `stiffness-type`.

## 10. Decomposition plan

Letters match task-4548's plan labels. **Filed 2026-06-12 (task 4548 step-8)** via fused-memory `submit_task(planning_mode=True)` → `commit_planning(pending)`; dependency edges V/R/P/M → F added via `add_dependency`. **Spine: F → {V, P, M}; R depends on F only where cross-module; Pt, S independent of F.** Owned tasks (P/Pt/M/S/F) are children of this hub PRD; routed tasks (V/R) are filed as dependents of their owning PRDs (§6).

**Filed task IDs (re-citation targets for task 4548 steps 9-14):**

| Leaf | Task ID | Family | Status | Depends on |
|---|---|---|---|---|
| F | **#4574** | stdlib-module-graph foundation | pending | — (spine) |
| V | **#4575** | vec3-type (routed) | pending | #4574 |
| R | **#4576** | range-type / range-angle-type (routed) | pending | #4574 |
| P | **#4577** | pose3-type + location-id-type | pending | #4574, task 4116 (landed) |
| Pt | **#4578** | part-structdef | pending | — |
| M | **#4579** | modal-result-type + loop-closure-record-type | pending | #4574 |
| S | **#4580** | force/velocity/acceleration-scalar sweep | pending | — |

- **F (#4574) — stdlib module-graph / forward-reference FOUNDATION.** Migrate `stdlib_loader.rs::load_stdlib` off the linear `include_str!` vec onto `module_dag.rs`'s topo sort (add `import std.*` decls) OR add forward-reference name-registration to the growing-prelude compile. Modules: reify-compiler (stdlib_loader.rs, stdlib `.ri` headers). Deps: none (consumes landed `module_dag.rs` + `import` grammar). Signal: a shared real `Vec3` defined once is reachable from both an early- and a late-loaded stdlib module; `load_stdlib()` green. **CRITICAL — spine.**
- **V (#4575) — Vec3/Vector3 surface type (routed → affine-map / math-linalg / kinematic-offsets).** Tighten `pub type Vec3 = Real` + all `param axis : Vec3` to the real vector type owned by those PRDs; cascade examples. Modules: reify-compiler (stdlib trajectory.ri/kinematic.ri/fea_multi_case.ri/fdm.ri), examples. Deps: **F**. Filed as dependent of the Vec3-owning PRD line. Signal: §8.2 boundary test.
- **R (#4576) — Range<T> (routed → numeric-and-range-literal-forms).** Tighten `SweepDim.range` + `prb_validity_range` to `Range<T>`/`Range<Angle>`. Modules: reify-compiler (stdlib kinematic.ri/flexures.ri), examples. Deps: **F** where the range type is cross-module-shared. Filed as dependent of `numeric-and-range-literal-forms.md`. Signal: §8.3.
- **P (#4577) — Pose3 + LocationId.** Introduce `Pose3` (adjacent to `Transform3`); route `LocationId` + `at : String` to the Selector substrate (task 4116). Modules: reify-compiler (stdlib trajectory.ri/modal_analysis.ri), reify-eval (selector echo), examples. Deps: **F** (Pose3 cross-module), task 4116 (landed). Signal: §8.4.
- **Pt (#4578) — Part structure_def.** Replace the `part : String` placeholders with a real `Part` structure_def; migrate the producer echo. Modules: reify-compiler (stdlib modal_analysis.ri/fea_multi_case.ri), reify-eval (modal_ops.rs), examples. Deps: none hard (Part is single-module-definable, but a cross-module `Part` rides **F**). Signal: §8.5.
- **M (#4579) — ModalResult + loop-closure/Map records.** Tighten `EndEffectorTrack.modal_result` to nominal `ModalResult`; introduce `LoopClosure` record + `JointParent` record + `Map<BodyId, JointParent>`. Modules: reify-compiler (stdlib trajectory.ri/kinematic.ri), examples. Deps: **F** (ModalResult defined in std.modal.analysis, consumed in std.trajectory — the canonical cross-module case). Signal: §8.6.
- **S (#4580) — force/velocity/acceleration scalar sweep (+ new Velocity dimension).** Add `VELOCITY` to `NAMED_DIMENSIONS` (mirror 4548 Phase-A IMPULSE); retarget `max_force`/`velocity_limit`/`acceleration_limit` to `Scalar<Force/Velocity/Acceleration>`; dimensioned-zero constraints. Modules: reify-core (dimension.rs), reify-compiler (stdlib trajectory.ri), examples. Deps: none. Signal: §8.7.

Dependency edges (filed): #4575→#4574, #4577→#4574, #4579→#4574, #4576→#4574 (R cross-module). Pt (#4578) / S (#4580) independent unless their tightened type is cross-module-shared (then →#4574).

## 11. Boundary-test sketch (two-way)

| # | Scenario | Pre | Post |
|---|---|---|---|
| 1 | shared `Vec3` reachable cross-module | early consumer (fea_multi_case) compiled before trajectory | F: defined-once `Vec3` resolves in both; `load_stdlib()` zero Error (today: name-resolution failure if not self-aliased) |
| 2 | cyclic stdlib import | introduce a real cycle | F: cycle diagnostic (module_dag detection) |
| 3 | `revolute(axis: 1.0)` bare scalar | `axis : Vec3` | V: compile error (today: silent `Real`-collapse) |
| 4 | `Range` literal vs bare scalar to `SweepDim.range` | `range : Range<T>` | R: literal accepted, bare scalar rejected |
| 5 | `Pose3` where `LocationId` expected | both `= Real` today | P: compile error (nominal separation) |
| 6 | `StepForce.at` selects a face on a `Part` | selector value | P: accepted; bare string rejected |
| 7 | `Part` value vs bare string to `ModalResult.part` | `part : String` today | Pt: `Part` accepted, string rejected |
| 8 | `EndEffectorTrack` with real `ModalResult` | `modal_result : Real` today | M: nominal accepted, bare `Real` rejected |
| 9 | `Scalar<Velocity>` passed for `max_force` | `max_force : Scalar<Force>` | S: rejected (today: both bare `Real`) |
| 10 | `constraint max_force > 0 * 1N` | dimensioned force | S: OK/VIOLATED (today: bare `> 0`, INDETERMINATE risk) |
| 11 | reify-audit PTODO over touched stdlib | post-re-citation | every in-family marker CITED to a live task; land-now markers DELETED (4548 §15 invariant) |

## 12. Open questions (tactical)

1. **F approach** — ModuleDag migration (`import std.*` decls) vs forward-reference name pre-pass. Suggested: forward-reference pre-pass first (smaller blast radius, reuses task-3895 skeleton pre-pass) with ModuleDag migration as the durable follow-up. Decide in F.
2. **Pose3 ↔ Transform3 relationship** — alias, conversion, or distinct types with `to_transform3()`. Decide in P, coordinating read-only with `affine-map-type.md`.
3. **`Part` shape** — opaque handle vs structured (geometry + material + topology index). Suggested: minimal opaque structure_def first, matching the `String`-selector semantics, then grow. Decide in Pt.
4. **Velocity dimension exponents** — `m·s⁻¹` = `from_exps(&[(0,1),(2,-1)])` (length idx 0, time idx 2). Confirm index convention against `ACCELERATION` (:513) at S authoring.
5. **Where `Vec3`/`Pose3`/`ModalResult` physically live post-F** — a new foundational `std.geometry.types` module hoisted to the top of the prelude, vs distributed definitions resolved by forward-refs. Decide jointly in F + V/P/M.
