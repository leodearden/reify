# Kinematics: typed joint and relation kinds, one normalized loop-closure record, one codec

**Milestone:** v0_6 · **Status:** active (authored 2026-10-13; decomposed 2026-10-13: α #8828 … ω₂ #8856, χ #8851 held on S8; leaf ids in §10 and the sidecar; gap-fill 2026-10-16: φ₂ #9281, follow-on #9283) · **Date:** 2026-10-13 · **Approach:** B + H (contracts §4, two-way boundary tests §5). Blast radius is seven crates (reify-ir, reify-compiler, reify-constraints, reify-stdlib, reify-eval, reify-core, reify-gui), with more than 8 mechanisms and seven cross-PRD consumers.

**Program:** `plans/review-all-program-reify-20261012.md`, stream **S6** (wave 1, spawn, size L, risk medium). The stream's remedy and the reify-ir home for the kind enums were ruled in that document's §Resolved decisions ("S6 Kinematics"). This PRD does not reopen them. Leo's rulings on this PRD's own open questions (R1–R8) are recorded in §13.

**Code anchors** were verified against main `e217c0f30b` and re-verified at `b1a7be2982` (2026-10-13). Every S6 path is byte-identical between the review's pinned tree `a030ee7528` and `b1a7be2982`. Main moves fast, and several in-flight branches rewrite these files (§8). Cite by symbol, and re-locate each symbol at implementation time.

**Normative substrate:** `docs/legibility/design-invariants.md`. This PRD:
- establishes **INV candidate 5** (an extracted primitive ships with an adoption ratchet) for its own primitives, via C10;
- complies with **INV-SF-1** (`undef-has-provenance`), **INV-SF-2**, **INV-SF-6** (`diagnostics-carry-codes`), **INV-PD-2** (`result-fields-populated-or-owned`: the `kinematic.ri` record declarations match what the runtime writes) and the umbrella `nothing-vacuous-and-unowned`.

Quality heuristics are cited by name from `/home/leo/src/dark-factory/docs/code-quality.md`.

---

## 1. Goal

A joint's kind, a geometric relation's kind and a loop closure each become a typed value. Each is parsed **once**, and its consumers dispatch on it **exhaustively**:

- **[Amended 2026-10-20, retro-critic R6]** `JointKind` lives in reify-ir and `RelationKind` lives in reify-builtins, below every consumer.
- A typed `Joint` view in reify-stdlib carries the per-kind behaviour as exhaustive methods: transform, Jacobian columns, motion subspace, value encode/decode, midpoint, force shape.
- `append_body` builds a loop-closure record **normalized** to the loop proper, read by one parser.
- The solver treats **every unbound joint in a loop** as an unknown, whichever side of the loop the `body()` order put it on.
- The solved joint values a snapshot reports are keyed by joint and typed by a `.ri` enum (`JointPosition`). That enum is the only value `bind` accepts.

After this PRD, a user or agent can observe the following. Each item names its before-image fixture under `tests/prd-gate/fixtures/`.

1. **Correct closed-chain answers where today's are silently wrong.**
   - Two arms on one unbound carriage, closed by a tie, close. Today the loop is left open (0.8 m vs 1.001 m) with a false "kinematic singularity" warning (`s6_shared_carriage_tie.ri`).
   - A brace solves as a loop. Today joint j1 is freed a second time on the closing side, body B sits at 1.0005 m where the brace demands 0.5 m, and the result is flagged singular (`s6_brace_through_snapshot.ri`).
   - A CoreXY gantry closes whichever way its tie is declared. Today the natural orientation leaves the carriage at its range midpoints (0.0005 m, 0 m) with no warning (`s6_corexy_tie_orientation.ri`).
   - An open body appended after a closing body gets its own inverse-dynamics force. Today it is dropped and the closing body is reported in its place (`s6_idyn_open_after_closing.ri`).
   - A sprung flexure inside a loop keeps its spring torque (`s6_idyn_closed_flexure.ri`).
2. **No silent Undefs where a value or a reason exists.**
   - An open-chain snapshot over an unbound planar, cylindrical or spherical joint returns the range-midpoint pose (`s6_open_planar_midpoint.ri`).
   - A closed-chain sweep with a multi-DOF free joint stays defined at every step (`s6_closed_planar_sweep_warmstart.ri`; the width fix itself is #8545).
   - `world_parented_closure` and the new self-loop rejection carry coded errors (`s6_mechanism_errors.ri`).
   - An infeasible TOTS shaper makes `reify eval` exit 1 with `E_TrajectoryConstraintInfeasible` (`s6_tots_infeasible.ri`).
   - Inverse dynamics over a coupling body is a coded refusal (`s6_idyn_coupling.ri`).
3. **One meaning per name.**
   - `reify check` and `reify eval` run the same `input_shape` solve. Today check solves a stand-in and prints "All constraints satisfied" on a problem eval finds infeasible.
   - `s.free_values[j]` returns `RevolutePosition { angle: … }` for the named joint, in units.
   - `bind(j, RevolutePosition { angle: a })` is the one binding form. GUI sliders keep writing back through it.
4. **Adding a joint or relation kind is one variant.** reify-ir, the compiler's joint tables, the stdlib `Joint` methods, reify-constraints' residual dispatch and reify-eval's relation phrase, joint-cell and mechanism-error tables, plus the GUI joint descriptor, all fail to compile until the new variant is handled. Today it means about 20 string tables, 12 of which silently return `None`/`Undef`, plus test loops over `JOINT_KINDS`.

**Consumers (G1).** Each consumer below plugs into an existing seam:

- The `snapshot` / `sweep` / `inverse_dynamics` / `joint_jacobian` / `bind` builtins on the value-eval path: `reify eval`, `reify check`, the LSP and the GUI.
- The ComputeNode-dispatch seam (`docs/prds/v0_3/engine-integration-norm.md` §3.4) for `input_shape` and inverse dynamics.
- The ConstraintSolver seam (§3.5) for relation residuals.
- The GUI slider write-back (`gui/src-tauri/src/engine.rs::collect_snapshot_bind_pairs`).
- **S9 λ #8741**, **#6669**, **S15 ο #8718**, **#7188**, **#7185** and **S8**'s typed diagnose-hook column (held leaf χ).
- `.ri` authors, through the docs-truth leaf ω₁.

## 2. Background (verified on main 2026-10-13)

Seat maps and probe outputs: `tests/prd-gate/fixtures/s6_*.ri` are the committed before-images. Their outputs are tabulated in the capability manifest.

- **Joint kind is a string in a `Value::Map`.**
  - 20 production functions or consts branch on ≥2 joint-kind names. That count is the record's 18, plus `reify-eval relate_solve::is_motion_joint_cell_type`, plus `gui/src-tauri/src/engine.rs::extract_joint_descriptor`, which silently maps cylindrical, planar and spherical to `FixedNoMotion`.
  - The `joints::eval_joints` constructor arms are among the 20.
  - 12 of these end in a catch-all `_ => None` / `Value::Undef` / `Type::dimensionless_scalar()`.
  - A typed `loop_closure_value::JointKind` exists but is re-derived from strings at 3 production sites.
  - "Driving" is defined three times: `joints::DRIVING_JOINT_KINDS`, the `kinematic.ri` `DrivingJoint` trait bounds, and the sweep helpers.
  - 11 test loops iterate `JOINT_KINDS` / `DRIVING_JOINT_KINDS` / `MULTI_DOF_KINDS`.
- **The joint Map is the joint's identity.**
  - Bindings and loop-Jacobian targets match joints by structural equality on the whole Map. The Map carries the raw axis, `origin`, flexure extras and a copy of a coupling's parent.
  - Promoting joints to `StructureInstance` is a rejected ruling ("SIR-α"). So the typed `Joint` (C3) is a **parsed view**, and the `Value` keeps flowing for identity.
  - Identical constructions alias (#7243, out of scope).
- **The crate graph.**
  - reify-ir (deps: reify-core, reify-ast) sits below the compiler, constraints, stdlib, eval, expr, LSP and the GUI. The compiler does not depend on reify-stdlib.
  - The orphan rule puts kind-level facts on `JointKind` in reify-ir.
  - The math needs stdlib-private helpers (`compose_transforms`, `normalize_quaternion`, `trig_input`, `validate_dimensionless_unit_axis_vec3`, `SpatialVector6`), so `Joint` lives in reify-stdlib.
- **Loop-closure records.** `mechanism.rs::append_body` records three shapes with one Map shape and two different tails:
  1. **Tie** (parent conflict): `path_b` ends in the pose link.
  2. **Brace** (the ancestor case of the parent-conflict branch): same shape as a tie.
  3. **Ring** (`cycle_introduced`, where `at` is unmounted but already an ancestor of `parent`): `path_b` ends in `at` itself, so the closing joint's own motion is in the loop. A self-loop is the degenerate ring.
- **Parsers and FK.**
  - `loop_closure_solver::mechanism_loop_closure_chains` re-derives a class and has **zero** production consumers.
  - `loop_closure::extract_loop_closure_chains` feeds the solver, with its own `strip_world_sentinel`.
  - `snapshot::parent_conflict_closing_body_ids` decides FK placement.
- **The defect beneath the class question.**
  - The solver holds every `path_a` joint at its bound or midpoint value, and frees only `path_b`'s unbound joints, keyed by chain position.
  - The world→split prefix is on both paths, so a shared unbound joint becomes a second unknown (carriage and brace fixtures).
  - Tree-side unbound joints are never solved at all, so the result depends on `body()` order (CoreXY fixture).
  - Across loops, every loop resolves against the user's bindings only. FK keeps the first synthesized binding, so a later loop can be silently open (#7207).
  - The shared prefix cancels **exactly** from the residual: `inv(P·A)·(P·B) = inv(A)·B` for rigid transforms.
- **Closed-chain inverse dynamics.**
  - It repeats the open chain's marshalling.
  - It hard-codes `compliance: None` under a false "mirrors the open-chain literal" comment.
  - It selects tree bodies positionally (`bodies[..n_total − n_lc]`, plus a second copy in `dynamics::eval::diagnose`).
- **The diagnostics wrapper has no production effect.**
  - snapshot discards the wrapper's diagnostics and re-runs the plain solve whenever its count-against-6 rule fires.
  - Singularity reaches users through an `is_singular` key that is *absent* when false, plus a recursive value scan in `engine_eval` that duplicates the wrapper's message.
- **`input_shape` has two implementations.**
  - `reify eval` and the GUI take the trampoline. `reify check` and the LSP take a stand-in solve that echoes the input profile. Measured on `examples/trajectory/tots_optimal_ptp.ri`: 26.7 s on check vs 7.5 s on eval.
  - `TotsOutcome::code_str` names codes that `DiagnosticCode` lacks.
- **`kinematic.ri` declarations do not match the runtime.**
  - `Snapshot.free_values` is declared `List<JointValue = Real>`; the runtime holds per-loop flat Newton storage.
  - `is_singular` is absent when false.
  - `LoopClosure` declares four params that no runtime key carries: `.joint` reads undef and `.closing_joint` is a compile error.
- **Data-carrying enum payloads must be compile-time literals today.**
  - `variant_construct.rs::compile_variant_construct` rejects `RevolutePosition { angle: theta }` and even `-30deg` with "non-constant payload value … not yet supported".
  - #7456 (pending, high) lifts this. 21 of the 34 `bind(` lines in tracked `.ri` pass non-literal values (`prj/printer_v01/printer.ri` ×19).

## 3. Sketch of approach

1. **Typed kinds** (β, γ, δ). **[Amended 2026-10-20, retro-critic R6]** `JointKind` in reify-ir with an exhaustive name map; `RelationKind` is the builtin registry's generated `Relation` sub-enum (S8-ψ #9418), which γ binds. The compiler, constraints, eval and GUI tables derive from them.
2. **Typed joint view, typed joint value** (ε, ζ, η, ι, λ₀, λ, μ).
   - Typed transform primitives.
   - `Joint` with `Joint::from_value` as the single parser, plus the codec.
   - The `.ri` `JointPosition` enum as the one bound and solved value.
   - One `bindings` module.
   - One resolver.
   - The GUI slider walker learns the enum form before `bind` drops the raw forms.
3. **One loop-closure record** (θ, κ, ν, ξ, π).
   - Normalized at construction; read by one parser.
   - Symmetric unknowns; one value per joint across loops, re-measured on final values.
   - Coded mechanism and loop-input errors.
   - A typed solve status with one diagnostics producer.
   - Per-joint `free_values` with a Snapshot-based warm start.
4. **Dynamics and trajectory** (ρ, σ₀, σ, τ, υ, φ, χ).
   - `MechanismTree`.
   - Dynamics on `Joint`, after σ₀ moves the spatial math to the crate root and the center-of-mass query into dynamics, so the module edges run one way.
   - One `input_shape` implementation over a typed `Shaper`.
   - Coded TOTS outcomes, plus a held leaf for the check/LSP path.
5. **Retirements, partitions, docs, close** (ψ₁, ψ₂, ψ₃, ω₁, ω₂).

The ratchet member α lands first, with one row per retired pattern. Each leaf lowers its rows.

## 4. Contract (H)

### C1: `reify_ir::JointKind`

`pub enum JointKind { Prismatic, Revolute, Coupling, Fixed, Cylindrical, Planar, Spherical }` uses the same variant order as today's stdlib enum, so the re-export is order-identical. It is `Copy + Eq + Hash`. Every method is an exhaustive match:

| Method | Meaning |
|---|---|
| `ALL` | every variant |
| `as_str` / `from_str` | the runtime `kind` tag |
| `nominal_type_name` / `from_nominal_type_name` | `kinematic.ri` structure names |
| `from_ctor_name` | the 10 builtin constructors; `couple`, `gear`, `screw` and `rack_and_pinion` map to `Coupling` |
| `is_driving` | the `DrivingJoint` set |
| `motion_dof` | Prismatic 1, Revolute 1, Coupling 1, Fixed 0, Cylindrical 2, Planar 3, Spherical 3 |
| `storage_width` | Newton slots: 1, 1, 1, 1, 2, 3, 4 in variant order. Fixed keeps its 1-slot sentinel; Spherical uses 4 quaternion slots |
| `position_variant_name` | the C4 variant name: `Some("PrismaticPosition")` … for driving kinds, `None` for Coupling and Fixed |
| `accepts_relate_mount` | true for every kind except Coupling |

`reify_stdlib::loop_closure_value::JointKind` becomes `pub use reify_ir::JointKind` (ruling). `flat_len` callers move to `storage_width`.

### C2: `reify_builtins::RelationKind`

~~`pub enum RelationKind` has 12 variants: `Coincident, On, Parallel, Antiparallel, Perpendicular, Concentric, Flush, Offset, Tangent, Fasten, Angle, Distance`. It provides `ALL`, `as_str()` (the DSL names) and `from_name(&str)`. It carries **no** relation semantics; those stay with S9 λ's `RelationSig` (R4).~~ (superseded by retro-critic R6)

**[Amended 2026-10-20, retro-critic R6]** `pub enum RelationKind` is `reify_builtins::RelationKind`: the registry's generated `Relation` binding-kind sub-enum, minted by `registry!` from the 12 relation rows of S8-ψ #9418 (`Coincident, On, Parallel, Antiparallel, Perpendicular, Concentric, Flush, Offset, Tangent, Fasten, Angle, Distance`). γ does not hand-write it and creates no `relation_kind.rs` in reify-ir. `ALL` is `RelationKind::iter()`; `as_str()` is the row's name (`row_for(kind.into()).name`); `from_name` does not exist (I-REG-1: `lookup(name, argc)` is the one string→builtin parser). Name, arity and result type live on the row; operand policy and metric slot are S9 λ's `relation_sig(RelationKind)`. reify-constraints and reify-eval take reify-builtins as a normal dependency (strata-legal).

What changes:
- **Instance and rank carry the kind.** `reify_constraints::RelationInstance.name` and `RelationRank.name` become `kind: RelationKind`. The only construction site, `reify_eval::relate_solve::relation_instance`, sets it — **[Amended 2026-10-20, retro-critic R6]** via `lookup(function.name, args.len()).and_then(BuiltinId::as_relation)`, or the `BuiltinId` the `CompiledExpr` carries once registry β #6002 lands. R4's parse point stands; only the parser is the registry's.
- **Unknown names stop being silent.** A relation-typed call whose `(name, argc)` resolves to no `Relation` row takes the existing `skipped` path and its `unconsumable_relation_diagnostic`. Today it yields zero residual rows and says nothing.
- **Exhaustive matches.** `residual_dispatch` and `describe_demand` match exhaustively over the generated enum (I-REG-2 for the `Relation` group), and `fasten` gets an explicit phrase.
- **One metric accessor.** `RelationInstance::metric()` replaces the two independent single-metric derivations.
- **Qualified-name parsers go through `RelationKind`.** reify-constraints' `solvespace.rs` relation dispatch (`qn.contains("parallel")`, which also matches `antiparallel`) and `classifier.rs::is_geometry_qualified_name` both strip the `std::` prefix and resolve through the registry's `lookup` to a `RelationKind` (**[Amended 2026-10-20, retro-critic R6]**; no `from_name`). Whether `std::fasten` (absent today) belongs in the classifier's set is decided in γ by reading the classifier's consumer (§12.6).

### C3: `reify_stdlib::joints::Joint` (a parsed view, never a replacement value)

```rust
pub(crate) enum Joint {
    Prismatic   { axis: UnitVec3, range: Range<f64>, origin: Option<Transform> },
    Revolute    { axis: UnitVec3, range: Range<f64>, origin: Option<Transform> },
    Coupling    { parent: Box<Joint>, ratio: f64, offset: f64, origin: Option<Transform> },
    Fixed       { origin: Option<Transform> },
    Cylindrical { axis: UnitVec3, translation: Range<f64>, rotation: Range<f64>, origin: Option<Transform> },
    Planar      { axis_x: UnitVec3, axis_y: UnitVec3, range_x: Range<f64>, range_y: Range<f64>, range_theta: Range<f64>, origin: Option<Transform> },
    Spherical   { range_angle: Range<f64>, origin: Option<Transform> },
}
impl Joint {
    fn from_value(v: &Value) -> Result<Joint, JointParseError>;   // the ONE parser
    fn kind(&self) -> JointKind;
    fn transform(&self, q: &JointValue) -> Result<Transform, JointError>;
    fn jacobian_columns(&self, q: &JointValue) -> Result<Vec<SpatialVector6>, JointError>;
    fn motion_subspace(&self) -> Result<Vec<SpatialVector6>, JointError>; // Coupling: Err(Unsupported)
    fn midpoint(&self) -> Option<JointValue>;                             // Fixed: None
    fn encode_position(&self, q: &JointValue) -> Result<Value, JointError>; // -> the C4 variant
    fn decode_position(&self, v: &Value) -> Result<JointValue, JointError>; // C4 only; variant must match kind
    fn force_value(&self, tau: &[f64]) -> Result<Value, JointError>;     // Coupling: Err(Unsupported)
}
```

The rules:
- **Parse once.** Every consumer parses at its entry and carries `(Value, Joint)`.
- **Uniform rejection.** A Map that `from_value` rejects is rejected identically by every builtin.
- **Today's coupling semantics.** A same-kind follower with `ratio × parent + offset`. #7188 re-expresses cross-kind couplings on this variant afterwards (R2).
- **The codec belongs to ι.** C4 variant names come from `JointKind::position_variant_name`, so no second name table exists.
- **Constructors go through the kind table.** The `eval_joints` constructor arms build through `JointKind::from_ctor_name` / `as_str`.
- **Shape and kept code.** The multi-DOF `joint_jacobian` List shape is user-visible and kept. Where #7188's branch already parses couplings (`CouplingSpec::from_map`), ι reads it before writing its own.

### C4: `JointPosition`, the one `.ri` joint value (R6, R7)

`kinematic.ri` declares:

```
pub enum JointPosition {
    PrismaticPosition   { distance: Length },
    RevolutePosition    { angle: Angle },
    CylindricalPosition { distance: Length, angle: Angle },
    PlanarPosition      { x: Length, y: Length, angle: Angle },
    SphericalPosition   { orientation: Orientation },
}
```

Two parse constraints shape the variant names, both verified by probe:
- Construction uses bare variant heads; `JointPosition::X { … }` does not parse.
- Prelude variant heads capture same-named local variants, so the names avoid the existing `Prismatic` / `Revolute` / … structures.

Payloads that are not literals need **#7456** (`CompiledExprKind::VariantCtor`). That is an upstream edge of λ and λ₀.

**The `.ri` binding tier.** `bind(j, p)` accepts **only** a `JointPosition` whose variant matches `j`'s kind (R7).
- A raw scalar, a list or a bare `Orientation` is a coded `BindPositionMismatch` error, and so is a variant mismatch.
- **The static check** lives in conformance's mechanism-joint bound check (the "joint bound" partition after S9 ο #8745). It fires only for an inline variant construction. The compiler types a construction as `Type::Enum`, not as the variant, so a `ValueRef` or `free_values[j]` argument is checked at run time.
- **No double diagnostic.** It must not double-fire with `E_MECHANISM_NONDRIVING_JOINT` (`mechanism_nondriving_joint_compile.rs` pins exactly one diagnostic).
- **Dimensions.** Payload dimensions are compiler-checked.

**The internal tier.** Rust-built bindings carry a decoded `JointValue`, never a `JointPosition`. These are snapshot's synthesized bindings for solved joints, `dynamics::eval::snapshot_for_sample` (which binds every body, couplings and fixed joints included), and `sweep`'s per-step bindings. They are built through `bindings::bind_internal(joint, JointValue)`. The `.ri` `bind` arm decodes the `JointPosition` once into the same internal form.

λ also extracts the `snapshot` builtin's body as a typed `pub(crate) fn snapshot::snapshot_of(mech, bindings, warm) -> Value`. `sweep` calls it directly in λ (no `eval_builtin("bind")` / `eval_builtin("snapshot")` left in sweep), and dynamics calls it in σ, after σ₀.

**Display.** `Value::Enum` prints named payload fields after the existing `Type::Variant` head: `JointPosition::RevolutePosition { angle: 0.785 rad }`. Unit variants print exactly as today. All three formatters change together: `value.rs` `Display`, the LSP `format_hover`, and `format_display`.

### C5: the normalized loop-closure record (R1, R8)

**The record.** `append_body` writes a closing edge as `{body_id, closing_joint, kind: "loop_closure", chain_a, chain_b}`. The two chains are the paths **below their last shared node**, because the shared prefix cancels exactly. Each chain keeps its tail as today:
- for a tie or brace, `chain_b` ends in the pose link (when the pose is not identity);
- for a ring, `chain_b` ends in `at`.

The record carries no class field. Within one record the two chains share no joint, an invariant asserted at construction.

**Build-time rejections.**
- A self-loop (`at == parent`) is rejected with a coded `MechanismError::SelfLoopClosure`. The guard runs **before** both closing branches: on a mounted `at` a self-loop takes the parent-conflict branch, where it would normalize to an empty loop.
- `world_parented_closure` stays a build-time rejection.

**Implicit roots.** A joint with no recorded parent is world-rooted, in the record walk (as today) **and** in FK. Today FK's `joint_world_transform` returns `None` for it, making the whole snapshot an uncoded Undef.

**FK placement.** FK places each closing body at the world frame its `chain_b` ends at, composed with the body's own `pose` unless `chain_b` already carries it:
- for a tie or brace, `chain_b` ends in the pose link, so the body sits at `T(parent) ∘ pose` (#7186's rigid-tie rule);
- for a ring, `chain_b` ends in `at`, so the body sits at `T(chain_b end) ∘ pose`, which equals `T(at) ∘ pose` at convergence.

There is one rule and no class. For a fully bound, violated loop the body sits where its declared closing edge puts it; #7185 flags the violation.

**One reader.** `reify_stdlib::loop_closure_record` (a pub module) provides:
- `LoopClosure::from_value`, which **requires** `chain_a` / `chain_b` with no structural fallback;
- `loop_closures(&Value)`, `closing_body_ids()` and `is_closed_chain(&Value)`;
- the one `strip_world_sentinel`, built on `mechanism::is_world`;
- `LoopClosureSolverInputs::build`, specified in C6.

Deleted:
- `extract_loop_closure_chains`, `mechanism_loop_closure_chains`, `LoopClosureChain`, `closing_side_contains_closing_joint`, `parent_conflict_closing_body_ids` and both `strip_world_sentinel` copies;
- the record-key literals in `reify-eval modal_ops` (its two `"loop_closures"` reads become `is_closed_chain`) and `dynamics/eval.rs`.

**Unknowns are every unbound joint in the loop, on both chains (R8).** The free set no longer depends on the order of `body()` calls. A tree-side joint enters the residual through its effect on `chain_a`. The FD Jacobian perturbs it on that chain, and its solved value feeds FK through the synthesized bindings.

**One value per joint across loops.** Snapshot solves loops in record order.
- Each loop resolves against the user's bindings **plus every joint value fixed by earlier loops**, whether those joints were solved or held.
- After the last loop, **every** loop's closure residual is re-measured on the final joint values. A loop that the final values leave open reports `NotConverged` (C6), and #7185 surfaces it. This absorbs #7207.
- A stacked simultaneous solve is out of scope (§11).

**Mechanism errors.** `MechanismError { DuplicateSolid, WorldParentedClosure, SelfLoopClosure }` lives in `mechanism.rs` with `discriminator()` and `code()`. `engine_eval::detect_mechanism_errors` routes through `MechanismError::from_value`, with no string literals in reify-eval.

### C6: solver inputs, solve status and the one diagnostics producer

**Solver inputs.**
- `LoopClosureSolverInputs { chain_a: Vec<Slot>, chain_b: Vec<Slot> }` with `Slot { joint: Value, parsed: Joint, value: JointValue, free: bool }`.
- `free_slots()` and `newton_width()` (#8545's `flat_width`) are methods. The pairing makes the equal-length invariant structural, and the three ad hoc length checks are deleted.

**Input errors.** `LoopClosureInputError` has four variants:
- `RecordShape { field }`
- `PathNotWorldRooted { path }`
- `UnresolvableJoint { chain, index, joint }`
- `JointParse(JointParseError)`

A snapshot whose inputs fail returns a snapshot **error Map**: `{kind: "snapshot", error: <discriminator>, error_message}`. This is the shape mechanism errors already use. `engine_eval`'s existing error-Map scan, the one θ routes `MechanismError` through, turns it into a coded diagnostic.
- ν makes that scan recurse into `List` / `Map` values, as the singularity scan already does, so a failing snapshot inside a `sweep` result is surfaced too. Today `detect_error_map_diagnostics` reads top-level cells only.
- The per-builtin diagnose hook is not used, for the reasons in D5.

**Solve status.** snapshot calls `solve_loop_closure` directly, which returns `LoopSolution { values: Vec<JointValue>, status: SolveStatus }`. `SolveStatus` is `Converged`, `NotConverged { residual }` or `Singular { … }`.

**Deleted:**
- `solve_loop_closure_with_diagnostics`, `LoopClosureReport`, `dof_count_for_balance`, `SINGLE_LOOP_RESIDUAL_COUNT` and the override in snapshot;
- the reify-constraints re-export and `loop_closure_diagnostics_tests.rs`;
- `DiagnosticCode::KinematicOverconstrained` and `KinematicUnderconstrained`, whose only emitter was the wrapper, and whose output snapshot discarded.

**One producer, one emitter.**
- The snapshot Map carries `is_singular: Bool` and `converged: Bool` **always**. `converged` is the conjunction over loops after C5's re-measurement.
- One stdlib function, `snapshot::diagnostics_of(&Value) -> Vec<Diagnostic>`, owns codes and messages. The `engine_eval` value scan calls it, and the duplicated message literal is deleted.
- `task/7185`'s `SnapshotVerdict` / `KinematicClosureNotConverged` design is read before `SolveStatus` is named, and its names are adopted where they fit. Port, don't discard.

**The `joint_jacobian` probe.** The discarded `let _ = eval_builtin("joint_jacobian", …)` probe becomes a direct `tracing::debug!`, and `loop_closure_machinery_solves_planar_in_loop_e2e` is re-pointed at it.

### C7: `Snapshot.free_values` and the warm start (R6)

**The field.** `free_values : Map<Joint, JointPosition>` holds one entry per solved free joint, in units. It is informational: `bind(j, s.free_values[j])` reproduces the configuration.

**The warm start.** `snapshot(m, bindings, prev: Snapshot)` seeds each free joint by identity from `prev.free_values`, via `Joint::decode_position`.
- A joint absent from `prev` seeds at its midpoint.
- `sweep` threads the previous step's Snapshot.
- Newton storage is private.
- The old row-list third argument is removed; no tracked `.ri` passes it.

**The `kinematic.ri` declarations:**
- `Snapshot { free_values : Map<Joint, JointPosition>, is_singular : Bool, converged : Bool }`. ξ adds `converged` together with the field it populates; π corrects `free_values`.
- `LoopClosure { body_id : Int, closing_joint : Joint }`, with the four phantom params dropped.
- The `JointParent` / `joint_parents` mismatch is a recorded follow-up candidate (§11).
- `crates/reify-compiler/tests/harness_mechanics/kinematic_stdlib_compile.rs`, which pins `free_values : List(Real)`, moves with the declaration.

### C8: `MechanismTree` and the dynamics↔snapshot edge

`dynamics/tree.rs` holds `MechanismTree::from_mechanism(&Value) -> Result<MechanismTree, TreeError>`. It is the one place that:
- validates `kind` / `error` / `bodies` / `joint_parents`;
- selects tree bodies as **every body whose id is not a `closing_body_id`**;
- builds `TreeLink { body_index, id, joint: (Value, Joint), parent, subspace, mass, com, inertia, compliance: Option<ComplianceSpec> }` in topological order.

`rnea_links(snapshot, q̇, q̈, positions)` is the single place compliance is decided, for open and closed chains alike. `joint_forces(τ)` reshapes the result. `topo_order`, `slice_generalized` and `frame3_from_transform_value` move into `tree.rs`. The two RNEA entry points become thin consumers. `dynamics/closed_chain.rs` (the KKT kernel) is unchanged.

**The module edges run one way (σ₀).** `scripts/module-scc-baseline.json` records one reify-stdlib SCC, `{dynamics, joints, mechanism, snapshot}`, and σ₀ dissolves it before σ adds its typed call into snapshot:
1. `dynamics/spatial.rs` (pure math, no crate imports) moves to a top-level `crate::spatial` module. joints, dynamics and the trajectory files import it from there, which removes the joints→dynamics edge.
2. snapshot's center-of-mass group (the `center_of_mass` arm, `explicit_mass_weighted_com`, `MassRung`, `body_mass_resolved`, `world_apply_point` and its `diagnose` part) moves to `dynamics/com.rs`, registered from dynamics' dispatcher. This removes snapshot→dynamics.

After σ₀, σ's typed `snapshot_for_sample` call (dynamics→snapshot) and `com.rs`'s use of snapshot helpers run one way. The baseline row is lowered in σ₀'s diff and is not regrown by σ.

### C9: `Shaper` and TOTS outcomes

**The typed shaper.** `trajectory/shaper.rs` holds:
- `Shaper::from_value(&Value) -> Result<Shaper, ShaperError>`, the only structure-name match;
- `Shaper = Impulse(ImpulseShaper) | Tots(TotsSpec)`, with `ImpulseShaper = Zv | Zvd | Ei | Cascaded(Vec<Shaper>)`.

`TotsSpec`'s defaults come from the `.ri` constructor (S15 D1). Absent required limits are a `ShaperError`, never a Rust literal.

**One composer**, the trampoline's, moved into `input_shape.rs`. Both the registered ComputeNode and the value-eval builtin call it; the value-eval arm stops echoing. `force_limit` / `actuator_limits` stay #7083's.

**Outcome diagnostics.** `TotsOutcome::diagnostic()` replaces `code_str`, using two new codes: `DiagnosticCode::TrajectoryConstraintInfeasible` (Error) and `TrajectorySolverNonConvergence` (Warning).
- **Infeasible:** an infeasible outcome, including a non-converged solve whose best iterate is infeasible, returns `ComputeOutcome::Failed` carrying the coded Error, never a profile.
- **Not converged but feasible:** a non-converged feasible result is `Completed` with the Warning.
- **Check/LSP path:** this waits on S8's typed diagnose-hook column (held leaf χ, R5).

### C10: the adoption ratchet (INV candidate 5)

`tests/infra/test_kinematics_typed_dispatch_ratchet.py` uses stdlib `unittest`, with a `.sh` wrapper and a `run-all-classification.manifest` row in α's own diff. It has one independently failing case per row, and each row carries its baseline and target. A seeded fixture per row proves each case can go red. No row asserts comment text.

**Scan rule.**
- Roots: `crates/*/src` and `gui/src-tauri/src`.
- Items inside brace-matched `#[cfg(test)]` / `#[cfg(all(test, …))]` modules are excluded, and so are files under `tests/`.
- A file-level `#[cfg(test)] use` does **not** exclude the rest of the file.
- `//` comment lines are excluded.
- Multi-line calls are joined before matching.

| row | pattern counted | baseline (α measures at its HEAD) | target | lowered by |
|---|---|---|---|---|
| `joint-kind-string-dispatch` | fns/consts holding ≥2 distinct joint-kind literals (lowercase or nominal) outside `reify-ir/src/joint_kind.rs` and the `Joint` home | ≈20 | 0 | β, δ, ι, λ₀, μ, ο, σ, ψ₁ |
| `joint-kinds-const` | definitions **and uses, test code included**, of `JOINT_KINDS` / `DRIVING_JOINT_KINDS` / `MULTI_DOF_KINDS` | measured | 0 | ι (dispatch loops rewritten over `JointKind::ALL`), ψ₁ |
| `internal-eval-builtin` | `eval_builtin(` naming a builtin, in the joints, loop_closure, loop_closure_solver, snapshot and sweep modules and in `dynamics/eval.rs` | ≈26 | 0 | ζ, ι, λ (incl. sweep's `bind`/`snapshot` calls), μ, ξ, σ |
| `relation-name-match` | string-literal arms or `contains` tests on relation names in reify-constraints `relate_solve.rs`, `solvespace.rs`, `classifier.rs` and reify-eval `relate_solve.rs` | measured | 0 | γ |
| `record-key-literal` | `"path_a"`, `"path_b"`, `"chain_a"`, `"chain_b"`, `"closing_joint"` and `"loop_closures"` outside `loop_closure_record.rs` and `mechanism.rs` | ≈16 | 0 | κ (stdlib + modal_ops), ρ (dynamics) |
| `strip-world-sentinel` | `fn strip_world_sentinel` definitions | 2 | 1 | κ |

## 5. Boundary-test sketch (H)

| # | Scenario | Preconditions | Asserts |
|---|---|---|---|
| T1 | Shared-ancestor tie closes | `s6_shared_carriage_tie.ri` | both arms at 0.8 m within 1e-9 m; no singularity warning |
| T2 | Brace solves | `s6_brace_through_snapshot.ri` | j2 = 0 (brace locks it); B at 0.5 m = C's frame; no singularity warning; j1 has one value. Today: `free_values [[0.5, 0.5005]]`, B at 1.0005 m, flagged singular |
| T3 | Ring solves; self-loop rejected | `s6_ring_through_snapshot.ri` with j_a mounted; a ring whose `at` is never mounted; `body(m, s, j, j)` on mounted and unmounted `j` | ring residual within tolerance and the ring body at its closing frame (= T(at)); the never-mounted ring is defined (world-rooted); self-loop → `reify eval` exits 1 with `MechanismSelfLoopClosure` in both cases |
| T4 | CoreXY (R1, R8) | `s6_corexy_tie_orientation.ri`: u = 200 mm, v = 100 mm | **both** tie orientations put the carriage at x = (u+v)/√2 = 0.21213 m, y = (u−v)/√2 = 0.07071 m within 1e-9 m. Today orientation A is right and orientation B leaves it at (0.0005, 0) m silently. A sweep over u stays defined and continuous |
| T5 | Kinematic e2e corpus is the floor | every tracked kinematic example and e2e test | unchanged results, except tests named under D10 (κ's list below) |
| T6 | Open-chain multi-DOF midpoint | `s6_open_planar_midpoint.ri` | `s_planar_unbound` pose equals `s_planar_bound` |
| T7 | Per-joint free values | four-bar, rocker free | `s.free_values[j_rocker]` is a `RevolutePosition` that prints its payload; `bind(j_rocker, s.free_values[j_rocker])` reproduces the snapshot |
| T8 | Warm start by Snapshot | `s6_closed_planar_sweep_warmstart.ri` | every sweep step defined; `snapshot(m, b, s_prev)` lands on the sweep's branch |
| T9 | bind is JointPosition-only | `bind(j_rev, 45deg)`; `bind(j_rev, PrismaticPosition { distance: 1mm })`; `bind(j_rev, p)` with `p` a param | coded `BindPositionMismatch`: statically for the inline cases, at run time for `p`. `bind(couple(…), RevolutePosition {…})` still yields exactly one `E_MECHANISM_NONDRIVING_JOINT` |
| T10 | Uniform joint rejection | a malformed joint Map fed to `transform_at`, `joint_jacobian`, `motion_subspace` | the same `JointParseError` |
| T11 | Mechanism errors coded | `s6_mechanism_errors.ri` | `world_parented_closure` → exit 1 with its code; `duplicate_solid` unchanged |
| T12 | Input failure has a reason | Rust boundary test below the compiler: a record missing `chain_b`, and a chain joint that does not parse | the named `LoopClosureInputError` variant, as a snapshot error Map → coded diagnostic through the engine scan |
| T13 | Status fields are Bools | non-singular, converged snapshot | `s.is_singular` reads `false` and `s.converged` reads `true`; `four_bar_singular.ri` emits exactly one singularity warning |
| T14 | Tree bodies by id | `s6_idyn_open_after_closing.ri` | forces for ids 0, 1, 3 with 45.24 N on id 3 |
| T15 | Closed-chain compliance | `s6_idyn_closed_flexure.ri` | **[Amended 2026-10-30, #9566]** sample 1 τ = [τ_a, τ_b] = [−20.72, 8.85875] N, whose sum equals `s6_idyn_open_flexure_control.ri`'s (derivation, invariant and today's value: "T15 re-derivation after #7312" below). ~~sample 1 τ_a = −14.31375 N, equal to `s6_idyn_open_flexure_control.ri` (this loop's constraint is projected out by `reduce_constraint_rank`, so the closed answer equals the open one)~~ (superseded: #7312 retired that projection) |
| T16 | Coupling dynamics refused loudly | `s6_idyn_coupling.ri` | coded refusal naming the coupling joint |
| T17 | Relation kinds | per-kind relate fixtures; a `fn … -> Relation` wrapper | identical poses and conflict phrases (fasten explicit); the wrapper gets `unconsumable_relation_diagnostic`; `antiparallel` dispatches as itself in `solvespace` |
| T18 | Kind exhaustiveness | review compile probe adding a variant | every consumer named in §1 item 4 fails to compile |
| T19 | One TOTS meaning | `s6_tots_infeasible.ri`, `s6_tots_nonconvergence.ri` | `reify eval` exits 1 with `E_TrajectoryConstraintInfeasible`; `reify check` no longer reports the constraint OK; `W_TrajectorySolverNonConvergence` shown |
| T20 | Shaper family by type | compiled stdlib structures whose `trait_bounds` contain `Shaper` | each recognised by `Shaper::from_value` |
| T21 | Two loops share a joint | two loops sharing an unbound joint, and the #7207 shape (four collinear prismatics; loop 1 closes the shared joint at 2.3 m, loop 2 at 2.1 m) | κ: one value per joint in FK, and every loop's residual re-measured on the final values (the #7207 shape's loop 2 residual is non-zero), shown by a Rust-level test on the solve result. ξ: the #7207 shape reports `converged = false`. Today both loops report converged and loop 2 is open |
| T22 | GUI slider on the enum form | `gui/src-tauri` engine test over a mechanism using `bind(j, RevolutePosition { angle: a })` with `a` a param | the slider resolves `a` as the driving param and writes back. A joint of each kind maps to its `JointKind` descriptor (planar is not `FixedNoMotion`) |

## 6. Resolved design decisions

- **D1: kinds in reify-ir, math in reify-stdlib.** This is the ruling, refined by the orphan rule. The C4 variant names are a `JointKind` method. Heuristics: *SPOT*; *deep modules, narrow interfaces*.
- **D2: `Joint` is a lossless parsed view.** (C3.)
- **D3: normalize closures at construction (R1).**
  - Tie, brace and ring become one form, read by one parser. FK places each closing body at its `chain_b` terminal frame. That keeps #7186's tie rule and the ring's own frame with no class (C5).
  - Implicit roots are world-rooted in FK as in the walk. Self-loops are rejected before either branch.
  - Heuristics: *clear invariants informatively enforced*; *well-defined purpose for each entity*.
- **D4: `JointPosition` wherever a joint value crosses the `.ri` boundary (R6, R7).** The Rust-internal binding tier carries `JointValue` (C4). The migration covers:
  - every `bind(` in tracked `.ri` (34 lines in 11 files, including `prj/printer_v01/printer.ri` ×21; the `s6_*` fixtures are migrated too);
  - the 81 Rust `eval_builtin("bind", …)` sites;
  - the 13 `.ri` sources embedded in 6 Rust test files;
  - the GUI `engine_tests.rs` ×23 and `commands_tests.rs` ×1;
  - `docs/reify-stdlib-reference.md` §13.
- **D5: the solver owns the solve status.** One stdlib function owns the diagnostics text, and emission stays at the engine value scan. The per-builtin hook is not used: `sweep` calls snapshot internally, so the hook sees `name = "sweep"`, and cached-cell replay of runtime diagnostics is unverified. Loop-input errors use the same channel, as error Maps (C6).
- **D6: `MechanismTree` is its own module from the start**, so S15 ο only moves the remaining groups. (C8.)
- **D7: one TOTS composer.** The value-eval arm delegates to it, because Undef would make every `input_shape` constraint Indeterminate on check. Infeasible is `Failed`. (C9.)
- **D8: h14 partitions follow the measured groups.**

  | File | Where its groups go |
  |---|---|
  | `joints.rs` | ι converts it to `joints/{mod, joint}.rs`; ψ₁ adds `joints/ctor.rs` and splits the tests |
  | `loop_closure_solver.rs` | the record group → `loop_closure_record.rs` (κ); numerics → `newton.rs` (ψ₂) |
  | `loop_closure.rs` | keeps chain algebra; the codec goes to `Joint` (ι/μ); the record group → `loop_closure_record.rs` (κ) |
  | `snapshot.rs` | bind → `bindings.rs` (λ); center-of-mass → `dynamics/com.rs` (σ₀); queries → `snapshot_queries.rs` (ψ₂) |
  | `trajectory/trampoline.rs` | marshalling → `marshal.rs` first (τ); its TOTS group → `input_shape.rs` (υ); the rest → `simulate_value.rs` / `track_access.rs` (ψ₃) |

  Further rules:
  - Tests move with their production group as a child `tests.rs`.
  - No `pub(super)` widening for tests.
  - Each move runs `reify-audit --pattern PDCHECK` and repoints broken `delivered_checks` (three writes).
  - A leaf whose test `include_str!`s an `s6_*` fixture registers it in `_RUST_COUPLED_RI_FIXTURES` in the same diff.
- **D9: module-SCC ratchet.**
  - `scripts/module-scc-baseline.json` records one reify-stdlib SCC, `{dynamics, joints, mechanism, snapshot}`.
  - λ removes dynamics→snapshot (`make_binding`). σ₀ removes joints→dynamics (`spatial` moves to the crate root) and snapshot→dynamics (center-of-mass), and lowers the row. σ then adds the typed dynamics→snapshot call one way.
  - Every leaf whose diff removes an edge runs `scripts/assert-module-scc.py` and lowers the row in the same diff. No leaf may grow it.
  - The residual SCC is measured, not promised.
- **D10: corpus corrections are named (R1).** A leaf that changes a kinematic e2e expectation must show the old value pinned incorrect behaviour, and name the test and reason in its commit message. Known in advance:
  - **κ, brace test:** `snapshot_ancestor_parent_conflict_body_keeps_its_own_frame` moves from 0.5 to 1.5. It is a fully bound, violated brace, now placed by the tie rule every other tie already follows; #7185 flags the violation.
  - **κ, ring test:** `snapshot_cycle_body_keeps_its_own_frame_after_later_tree_registration` moves from 0.3 to 1.0. With j_a = 0.3 and j_c = 0.4 bound, the ring is violated, and its body is placed by its declared ring edge, `T(chain_b end) ∘ pose`.
  - **κ, path-shape tests:** the two `mechanism.rs` tests pinning today's world-rooted shapes (`cycle_records_loop_closure_constraint`, `self_loop_records_loop_closure_constraint`) are rewritten to the normalized shape or the self-loop error. `loop_closure_solver.rs::ancestor_parent_conflict_shape_classifies_as_cycle` is deleted with `mechanism_loop_closure_chains`.
  - **κ:** any test where a tree-side unbound joint was frozen at its midpoint (R8).
  - **η:** printed payload-enum goldens gain `{ … }`. Unit-enum output is unchanged.
- **D11: symmetric unknowns (R8).** Every unbound joint of a normalized loop is an unknown, which makes results independent of declaration order. (C5.)
- **D12: one value per joint across loops, re-measured.** (C5; absorbs #7207.)

## 7. Disposition table

| Finding | Disposition | Leaf |
|---|---|---|
| fk-4543a3a88865 joint-kind string tables, drift loops (high) | `JointKind` + `Joint`; tables derive; loops deleted or rewritten over `ALL`; `joint-kind-string-dispatch`, `joint-kinds-const` rows at 0 | ψ₁ |
| fk-6a6bbd16da97 record classes erased, positional tree slice, `duplicate_solid` hardcode (high) | normalized record, one reader, `MechanismError` routing; positional slices → ρ | κ (θ, ρ companions) |
| fk-337e4f0bcfa9 warm-start width (high) | **filed:8545** (in progress). `x_finding_key` is stamped on #8545; π redefines the warm-start contract on top of it | #8545 |
| fk-f584626a8627 snapshot resolver copy, premise-false pinned test (high) | one resolver; `value_for` + `wrap_*` deleted; test asserts the midpoint | μ |
| fk-14c3de3a675c codec has no home (high) | `Joint::encode_position / decode_position` (ι); one ladder; loop_closure + snapshot copies deleted | μ |
| fk-02810f753a67 positional 5-tuple, 77 None origins | named inputs + `LoopClosureInputError` → coded error Map | ν |
| fk-90837877eee7 duplicate `strip_world_sentinel` (**#8547**) | one copy on `is_world`; subsumes #8547 | κ |
| fk-9b8484939308 three record parsers | one reader | κ |
| fk-241b66aa0262 wrapper overridden, magic key, duplicate message | direct solve, `SolveStatus`, `diagnostics_of`, Bool fields | ξ |
| fk-3cfd0be186c0 internal `eval_builtin` calls | typed fns; `internal-eval-builtin` row at 0 | ψ₁ (ζ ι λ μ ο ξ σ lower it) |
| fk-49105d89fd7b snapshot↔dynamics, `make_binding` misplaced | `bindings.rs` | λ |
| fk-691b37115ddf two `input_shape` implementations | `Shaper` + one composer | υ |
| fk-98f62f3f0562 TOTS codes unknown to `DiagnosticCode` | typed codes; registered path now; check/LSP held on S8 | φ (χ held) |
| fk-7bfc9a5014e2 sweep's kind matchers | `Joint`; multi-DOF refusal coded; range-dimension faults coded (subsumes **#7975**'s sweep leg) | ο |
| fk-ca97659733e9 joint and relation name lists | joint half derives from `JointKind`; relation half via γ | δ |
| fk-45c54d189935 relation vocabulary as strings | `RelationKind` on instance and rank; qualified-name parsers route through it | γ |
| fk-6467cfff64bc `per_joint_jacobian_local` | deleted; 2 oracle tests rewritten | ζ |
| fk-a9b0785fd293 `joint_force_value`, kind set defined twice | `Joint::force_value`; coupling refusal coded | σ |
| fk-726aff24a191 conformance Path B (weakened) | Path B + DRIFT NOTE + its test deleted | δ |
| fk-38811e6f7300 duplicated idyn marshalling, compliance dropped (high) | `MechanismTree` | ρ |
| h14 joints.rs, loop_closure_solver.rs, snapshot.rs, trajectory/trampoline.rs, loop_closure.rs | D8 | ι, κ, λ, μ, σ₀, τ, υ, ψ₁, ψ₂, ψ₃ |

Existing tasks dispositioned at decompose:
- **#8547:** cancelled 2026-10-13, subsumed by κ #8837.
- **#7354:** cancelled 2026-10-13, subsumed by θ #8835.
- **#7207:** cancelled 2026-10-13, subsumed by κ #8837 + ξ #8842 (T21).
- **#7975:** cancelled 2026-10-13, subsumed by λ #8839 (list-element faults become compiler-typed payloads) and ο #8843 (sweep-range faults).
- **#7974:** cancelled 2026-10-13, superseded by μ #8840.
- **#7188:** depends on ι #8836 and re-plans on `Joint::Coupling` (R2; re-plan note appended). Its branch is read and ported where it fits.
- **#7185:** depends on ξ #8842 and re-plans on `SolveStatus` (R3; re-plan note appended). Its branch's verdict design is read by ξ. #7204 and #7193, which depend on #7185's types, get a re-plan note.
- **#6669:** depends on γ #8830 (note appended).

## 8. Pre-conditions (G3, verified on main 2026-10-13)

**Substrate probes.** All of these were run with `target/debug/reify` built from main on 2026-10-12; no S6 path has changed since.
- **Enum construction and lookup.** Named-field payload enums construct with bare variant heads from **literal** payloads, are stored in a `Map` keyed by a joint, and are destructured by `match` (`s6_joint_position_enum_substrate.ri`).
  - The qualified head does not parse.
  - `Map` lookup by a joint works: `map{j_a => 300mm}[j_a]` returns `0.3 m`.
  - `Map<Joint, …>` with the trait key type-checks.
  - `Orientation` resolves as a payload type.
- **Non-literal payloads fail** today ("non-constant payload value … not yet supported"). Upstream **#7456**.
- **Every `s6_*` fixture reproduces its §1/§2 claim.** Outputs are in the manifest.
- **reify-eval depends on reify-stdlib**, so the record module's `pub` surface is reachable from `modal_ops` / `engine_eval`.

**Upstream tasks.** Each is a real dependency edge (§10):

| Task | What it does | Leaves that depend on it |
|---|---|---|
| **#8545** (in progress) | `flat_width` (the width defect) | β, κ |
| **#7497** | coupled joints in `chain_b` re-derived inside Newton | κ, which rebases onto whichever route #7497 lands |
| **#6922** (pending; branch `task/6922` active) | rewrites the `dynamics/eval.rs` readers | κ, ρ |
| **#8560** | drops reify-constraints' re-export shim | κ, ξ |
| **#6941** (pending, high) | four workstreams; δ rewrites `jointvalue_from_bound_value` and `joints` `length_input` | ι, μ, τ |
| **#7456** (pending, high) | non-literal enum payloads | λ₀, λ |
| **#8745** (S9 ο) | partitions `conformance/mod.rs` into a "joint bound" module | δ, λ |

**Downstream tasks re-planned on S6** (§7): #7188 → ι; #7185 → ξ; #6669, #8741 → γ; #8718 → ρ, σ, λ.

## 9. Cross-PRD relationship (G4)

| Artefact / PRD | Direction | Mechanism | Owner |
|---|---|---|---|
| Home of `JointKind` / `RelationKind` (program G4 row) | this PRD **owns** `JointKind`; **[Amended 2026-10-20, retro-critic R6]** `RelationKind`'s home is S8-ψ #9418 (reify-builtins), which γ binds | C1, C2; compiler, constraints, eval and GUI tables derive (β, γ, δ, λ₀) | **S6** (`JointKind`); **S8** (`RelationKind`) |
| JointValue flat-storage width contract with `sweep::build_snapshot_list` and the `.ri` warm-start rows (program G4 row) | this PRD **owns**, and **replaces** it | C7: per-joint `free_values` and a Snapshot warm start; flat storage becomes private | **S6** |
| #7188 SingleDofKind / #7974 consolidation (program G4 row) | this PRD **owns**, ruled R2 | #7188 depends on ι; #7974 is cancelled as superseded by μ | **S6** |
| `JointPosition` + `bind` value contract (new row) | this PRD **owns** | C4 | **S6**. S8's reify-builtins `bind` row reads it; #6007 (registry τ5: typing `snapshot`/`sweep` and `Snapshot` fields) adopts C4/C7 |
| `loop_closure_record` pub surface (new row) | this PRD **owns** | C5 | **S6**. reify-eval `modal_ops` / `engine_eval` / `dynamics_ops` and S15 ο consume |
| S9 λ #8741 `RelationSig` | S9 **consumes** S8-ψ #9418 and γ | **[Amended 2026-10-20, retro-critic R6]** λ's `relation_sig(RelationKind) -> { operands, metric_slot }` is a total fn over `reify_builtins::RelationKind`, reading arity and result type from the S8-ψ rows; ~~keyed by `RelationKind`, using the same `from_name`~~ (superseded by retro-critic R6 — there is no `from_name`). Parsing happens at `relation_instance`, not in the compiler (R4), via the registry's `lookup`. `classifier.rs`'s set is γ's | S9 |
| S9 ο #8745 (`conformance/mod.rs` partition) | file contention, **ordered** | δ (Path B deletion) and λ (static bind check) edit the "joint bound" partition after #8745 | S9 |
| S7 ζ #8623 (`conformance/` imports) | file contention | upstream of #8745, so δ and λ are ordered transitively | S7 |
| #6669 LoweringTable | **consumes** γ | keyed by `RelationKind`; its `Row.codim` and λ's `RelationSig.codim` both claim `relation_delta_dof`: one table, owners' call (cross-reference) | that PRD |
| S15 ο #8718 | S15 **consumes** ρ, σ, λ | ο partitions after `MechanismTree`, `Joint` adoption, the bindings move and the center-of-mass move | S15 |
| S15 η #8697 (`MassProperties` in `dynamics/mass_props.rs`) | file contention | σ's `dynamics/com.rs` consumes whichever `MassProperties` reader is current | S15 |
| S8 builtin registry | **produces** χ's substrate and S8-ψ #9418's `RelationKind`; **consumes** C1, C4 | typed diagnose-hook column; **[Amended 2026-10-20, retro-critic R6]** the relation rows mint `RelationKind` and γ binds it (γ #8830 ← S8-ψ #9418) — ~~relation builtin rows derive from `RelationKind::ALL`~~ (superseded by retro-critic R6: direction reversed, rows mint, S6 binds); joint builtin rows derive from `JointKind::from_ctor_name` — **open point (flagged by retro-critic R6, not ruled):** this joint twin claims the same direction a leaf reify-builtins cannot take (#6945 ← #8829) | S8 |
| S10 kernel interface | S10 **consumes** C1, C2 | typed kinds in reify-ir | S10 |
| goal-oriented-error-estimation α #7456 | **builds on** | `VariantCtor` | that PRD |
| S3 INV-ARCH-1 | complies | D9 | S3 (tool), S6 (its rows) |
| S23 prose | S23 **skips** | prose S6 rewrites or deletes (stale #2670 citations, false parity and "appended at the end" comments, `per_joint_jacobian_local` justifications, `reify_constraints::loop_closure` pointers) | S6 |
| #7185 / #7204 / #7193 closure verdicts | **consume** ξ (R3) | `SolveStatus` + `diagnostics_of` | #7185 |
| #6941 / #7083 trajectory readers, `force_limit` | **builds on** #6941; file contention with #7083 (`trampoline.rs`) in υ and ψ₃ | — | those tasks |

## 10. Decomposition plan

Every leaf adds tests to **existing** test binaries. New e2e modules join `harness_fea_solver_e2e`'s module list; GUI tests go into `gui/src-tauri`'s existing test modules. The one new gate-resident member is α's infra test, whose `.sh` wrapper and manifest row land in α's own diff (esc-4914-162). A leaf that changes a kinematic expectation follows D10. A leaf that adds a `DiagnosticCode` edits `crates/reify-core/src/diagnostics.rs` and checks `docs/notes/diagnostic-severity-policy.md`.

| Label | Task | Modules | Observable signal | Prereqs | Keys · INV |
|---|---|---|---|---|---|
| α #8828 | Kinematics typed-dispatch ratchet member (C10): Python unittest + `.sh` wrapper + manifest row; one case per row with baselines measured at its HEAD under C10's scan rule; seeded red fixtures | tests/infra | member green on the real tree; each seeded fixture reds its own case | — | INV cand 5 |
| β #8829 | `JointKind` in reify-ir (C1) incl. `position_variant_name`; stdlib re-export; `flat_len` → `storage_width`; `JointKind::from_str`'s stdlib string table gone | reify-ir, reify-stdlib | adding a variant fails to compile every reify-ir method; kinematic e2e unchanged | α, **#8545** | — |
| γ #8830 | **[Amended 2026-10-20, retro-critic R6]** binds `reify_builtins::RelationKind` (S8-ψ; C2), no reify-ir enum and no `from_name`; `RelationInstance.kind` / `RelationRank.kind`; exhaustive `residual_dispatch` / `describe_demand`; `metric()`; `solvespace.rs` and `classifier.rs` through the registry's `lookup`; uncurated-name test → eval-level skipped-relation test | reify-constraints, reify-eval (each takes reify-builtins as a normal `[dependencies]` entry) | T17; `relation-name-match` at 0 | α, S8-ψ #9418 | fk-45c54d189935 |
| δ #8831 | Compiler and eval joint tables derive from `JointKind`: `joint_ctor_result_type` + the joint part of `JOINT_TYPED_FN_NAMES` (its `joint_jacobian` arm unchanged, see #6261); conformance Path B + DRIFT NOTE + `l2_joint_constructor_name_mapping_exhaustive` deleted; `is_motion_joint_cell_type` via `from_nominal_type_name` + `accepts_relate_mount`, tautological test deleted; new compiler test pins `kinematic.ri` `DrivingJoint` conformance == `is_driving` | reify-compiler, reify-eval | `mechanism_nondriving_joint_diag_e2e` and `mounted_joint_cell_matches_each_driving_joint_kind` unchanged; the cross-check reds on a seeded disagreement | β, γ, **#8745** | fk-ca97659733e9, fk-726aff24a191 |
| ε #8832 | Typed transform primitives `transform_inverse`, `transform_log`, `transform_identity` (`pub(crate)` in `geometry.rs`, called by the builtin arms); the two private identity copies deduplicated | reify-stdlib | transform builtin outputs byte-identical; intermediate for ι, μ | α | — |
| ζ #8833 | Delete `per_joint_jacobian_local`, its 10 tests and every comment site shaped around it; rewrite the two `chain_jacobian_fd_*_matches_analytic` oracles against `joint_jacobian_value` | reify-stdlib, reify-eval test comment | the fn is absent; both oracles green | α | fk-6467cfff64bc |
| η #8834 | `JointPosition` enum in `kinematic.ri` (C4); `Value::Enum` display prints named payload fields in all three formatters; unit-enum output unchanged | reify-compiler stdlib, reify-ir, reify-lsp | `reify eval` on a fixture constructing `RevolutePosition { angle: 45deg }` from the stdlib enum prints `JointPosition::RevolutePosition { angle: 0.785… rad }`; unit-enum goldens unchanged | — | — |
| θ #8835 | `MechanismError { DuplicateSolid, WorldParentedClosure }` with `discriminator()` / `code()`; `engine_eval::detect_mechanism_errors` routes through it; new code for world-parented closure | reify-stdlib, reify-core, reify-eval | T11 | ε | subsumes **#7354** |
| ι #8836 | Typed `Joint` + `Joint::from_value` + codec (C3) in a new `joints/` directory module (`mod.rs`, `joint.rs`); the h14 groups b–e become methods; constructor arms through `JointKind`; joints.rs `eval_builtin` calls → typed; `transform_at_simple_joint` and stale docs deleted; joints.rs's dispatch-coverage loops rewritten over `JointKind::ALL` (loop_closure.rs / loop_closure_value.rs loops are ψ₁'s) | reify-stdlib | T10; joints e2e unchanged; joints rows lowered | β, ε, ζ, η, **#6941** | — (#7188 depends on ι) |
| κ #8837 | Normalized loop-closure record (C5): prefix strip; self-loop guard first; implicit roots world-rooted in FK; `chain_b`-terminal FK rule; symmetric unknowns (R8); record-order multi-loop rule with final re-measurement; `loop_closure_record.rs` the one reader; three parsers + `LoopClosureChain` + `closing_side_contains_closing_joint` + both sentinel copies deleted; `closed_chain_inverse_dynamics` and `modal_ops` on the reader; `MechanismError::SelfLoopClosure`; CoreXY e2e + `examples/kinematic/corexy.ri`; carriage, brace, ring, two-loop tests; D10 list | reify-stdlib (mechanism, loop_closure, loop_closure_solver, snapshot, dynamics/eval, new record module), reify-eval (modal_ops, tests), reify-constraints lib.rs, reify-core, examples | T1, T2, T3, T4, T5, T21 (κ half) | α, β, ε, ζ, θ, **#7497**, **#8545**, **#8560**, **#6922** | fk-6a6bbd16da97, fk-9b8484939308, fk-90837877eee7 · subsumes **#8547**, **#7207** |
| λ₀ #8838 | GUI on the typed forms, both accepted for now: `collect_snapshot_bind_pairs` / `resolve_driving_params_from_ast` learn `XPosition { field: Ident \| Literal }` as well as today's raw form; `extract_joint_descriptor` via `JointKind` (no `_ => FixedNoMotion`) | gui/src-tauri | T22 (enum half); GUI engine tests green with both forms | β, η, **#7456** | — |
| λ #8839 | `bindings.rs` (`make_binding`, `bind_internal`, the `bind` arm, `validate_bindings`, `bound_value`); typed `snapshot::snapshot_of` extracted and sweep's `eval_builtin("bind")` / `eval_builtin("snapshot")` calls converted; `bind` takes only a matching `JointPosition` (C4, R7) with the inline static check; internal tier on `JointValue` incl. `sweep.rs` and `dynamics::snapshot_for_sample`; full D4 migration incl. GUI tests and `docs/reify-stdlib-reference.md` §13; GUI walker's raw form removed; SCC row re-measured (D9) | reify-stdlib, reify-compiler (conformance joint bound), reify-core, examples, prj, gui/src-tauri, tests, docs | T9; T22 (raw form gone); examples and `prj/printer_v01/printer.ri` evaluate to unchanged values | ι, η, κ, λ₀, δ, ρ, **#7456**, **#8745** | fk-49105d89fd7b · subsumes **#7975** (with ο) |
| μ #8840 | Resolver SPOT: one ladder (bound value → coupling follows parent → fixed sentinel → midpoint) over `Joint`, used by snapshot FK and the solver inputs; `snapshot::value_for` / `wrap_midpoint_for_joint` / `wrap_jointvalue_for_joint` and `loop_closure::value_for_joint` / `jointvalue_from_bound_value` deleted; pinned test rewritten to the midpoint pose; loop_closure + snapshot `eval_builtin` calls → typed; stale #2670 text gone | reify-stdlib | T6; rows lowered | ι, λ, κ, ε, σ₀, **#6941** | fk-f584626a8627, fk-14c3de3a675c · supersedes **#7974** |
| ν #8841 | Named `LoopClosureSolverInputs` + `LoopClosureInputError` (C6); length checks deleted; failures become coded snapshot error Maps through θ's engine scan, which now recurses into List/Map values | reify-stdlib, reify-core, reify-eval | T12 | μ, κ, θ | fk-02810f753a67 · INV-SF-1 |
| ξ #8842 | Direct solve + `SolveStatus` + `snapshot::diagnostics_of` + Bool `is_singular` / `converged` (C6), `converged : Bool` declared in `kinematic.ri`; wrapper, report, `dof_count_for_balance`, override, the two unemitted codes and the reify-constraints diagnostics tests deleted; engine message literal deleted; probe → `tracing::debug!` | reify-stdlib, reify-eval, reify-core, reify-constraints, reify-compiler stdlib | T13; T21 (ξ half: the #7207 shape reports `converged = false`); `four_bar_singular.ri` one warning | ν, μ, κ, ι, **#8560** | fk-241b66aa0262 · INV-SF-6 (#7185 depends on ξ) |
| ο #8843 | sweep through `Joint`: `driving_joint_kind` / `wrap_value_for_driving_joint` deleted; multi-DOF sweep and a range whose dimension mismatches the joint are coded diagnostics; internal bindings via `bind_internal` (λ already converted sweep's `bind`/`snapshot` calls) | reify-stdlib, reify-core, reify-expr (diagnose arm) | `dim(planar_joint, …)` and `sweep(m, j_rev, 0mm .. 1mm, 2)` give coded diagnostics instead of undef; sweep e2e unchanged | ι, λ, μ | fk-7bfc9a5014e2 · subsumes **#7975** (with λ) |
| π #8844 | Per-joint `free_values : Map<Joint, JointPosition>`; Snapshot warm start; sweep threads Snapshots; `kinematic.ri` `Snapshot` (incl. `converged`) / `LoopClosure` declarations (C7); `kinematic_stdlib_compile.rs` updated | reify-stdlib, reify-compiler stdlib + tests, reify-eval tests | T7, T8; `m.loop_closures[0].closing_joint` compiles (`s6_loopclosure_closing_joint_member.ri` check exits 0) | ξ, μ, λ, η, ο | INV-PD-2 |
| ρ #8845 | `MechanismTree` in `dynamics/tree.rs` (C8); tree bodies by `closing_body_ids()`; compliance decided once; false comments and positional slices (incl. `diagnose`) deleted; dynamics record-key reads through the reader | reify-stdlib dynamics, examples header prose | T14, T15; `closed_2prismatic_idyn.ri` unchanged | κ, ι, **#6922** | fk-38811e6f7300 |
| σ₀ #8846 | Module edges one way (C8, D9): `dynamics/spatial.rs` → top-level `crate::spatial` (joints, dynamics and trajectory importers repointed); snapshot's center-of-mass group → `dynamics/com.rs`, registered from dynamics' dispatcher; snapshot imports nothing from dynamics and joints nothing from dynamics; `scripts/assert-module-scc.py` and the reify-stdlib baseline row lowered in the same diff | reify-stdlib (spatial, dynamics, snapshot, joints, trajectory imports) | `center_of_mass` e2e unchanged; no `crate::dynamics` path in snapshot.rs or joints; the SCC row shrinks | λ | h14 (snapshot group 4) |
| σ #8847 | Dynamics on `Joint`: `Joint::force_value` replaces `joint_force_value` / `joint_kind`; subspaces via `Joint`; coupling → coded refusal; `snapshot_for_sample` calls `snapshot::snapshot_of` + `bind_internal` (no `eval_builtin("snapshot")`); `assert-module-scc.py` shows no growth (C8) | reify-stdlib dynamics, reify-core | T16 | ρ, ι, λ, σ₀ | fk-a9b0785fd293 |
| τ #8848 | `trajectory/marshal.rs` ← trampoline's marshalling group (pure move, tests move with it); PDCHECK repoint | reify-stdlib trajectory | trajectory e2e unchanged; PDCHECK clean | **#6941** | h14 |
| υ #8849 | `trajectory/shaper.rs` typed `Shaper` (C9); one composer in `input_shape.rs`; value-eval arm delegates; `run_tots`, `canonical_tots_model`, echo, duplicate readers/defaults, `is_tots_shaper_type_name`, `shaper_family_guard`, `SHIPPED_SHAPER_STRUCTURES` deleted; reify-eval `trait_bounds` test | reify-stdlib trajectory, reify-eval tests | T19 (check half), T20; check wall time on `tots_optimal_ptp.ri` no longer about 3.6× eval | τ | fk-691b37115ddf |
| φ #8850 | TOTS codes on the registered path (C9): two `DiagnosticCode`s; `TotsOutcome::diagnostic()`; `code_str` deleted; infeasible → `ComputeOutcome::Failed` | reify-core, reify-stdlib trajectory, reify-eval trajectory_ops | T19 (eval half) | υ | fk-98f62f3f0562 · INV-SF-6 |
| φ₂ #9281 | *(added 2026-10-16, gap-fill §K6-D5)* `gcode_import` outcomes coded: a parse error is a coded `Rejection` carrying its line (`E_GcodeParseError`, specified by done #3874), an unsupported dialect a coded rejection, `ShaperConflict` / `ArcFlattened` / `DialectUnsupported` advisories; new `DiagnosticCode`s in reify-core; `#![allow(dead_code)]` removed; coordinate with #9192 (M82/M83, same file) | reify-core, reify-stdlib trajectory | a malformed Marlin line: `reify eval` exits 1 with `E_GcodeParseError` naming the line (today silent undef); a Klipper `INPUT_SHAPER` file: profiles returned plus the shaper-conflict warning | S8-α1 #8888, φ | fk-5d64b909dfdf · INV-SF-6 |
| χ #8851 | **HELD** (`x_seam_wait = {stream: S8, contract: "typed diagnose-hook column: a builtin returns diagnostics with its value on the value-eval path"}`): TOTS diagnostics on `reify check` / LSP | reify-stdlib trajectory | `reify check tests/prd-gate/fixtures/s6_tots_infeasible.ri` shows `E_TrajectoryConstraintInfeasible` | φ, S8 column leaf | — |
| ψ₁ #8852 | Kind-set and internal dispatch retired: `JOINT_KINDS` / `DRIVING_JOINT_KINDS` / `MULTI_DOF_KINDS` deleted, `is_joint_value` / `is_driving_joint` derived, remaining drift loops deleted; `joints/ctor.rs` split out and tests split by partition | reify-stdlib | ratchet rows `joint-kind-string-dispatch`, `joint-kinds-const`, `internal-eval-builtin` at 0; T18 (review compile probe); joints e2e unchanged | ι, δ, λ₀, μ, ο, σ, ξ | fk-4543a3a88865, fk-3cfd0be186c0 · h14 |
| ψ₂ #8853 | h14 partitions: `loop_closure_solver.rs` → `newton.rs`; `snapshot.rs` → `snapshot_queries.rs`; `loop_closure.rs` chain algebra only; tests as child files; SCC row re-measured; PDCHECK repoint | reify-stdlib | e2e unchanged; each file makes sense in isolation (h13); no `pub(super)` test widening | π, ν, ξ, σ, ψ₁ | h14 |
| ψ₃ #8854 | h14 remainder of `trajectory/trampoline.rs`: `simulate_value.rs`, `track_access.rs`; trampoline.rs keeps the cache keys; PDCHECK repoint | reify-stdlib trajectory | trajectory e2e unchanged | υ, φ | h14 |
| ω₁ #8855 | Docs-truth. A new chunk `crates/reify-mcp/src/tools/chunks/kinematics.md` (registered with the other chunks) documents `bind(j, JointPosition)` and its variants, per-joint `free_values` and the Snapshot warm start, `converged` / `is_singular`, closure semantics (tie, brace, ring; order-independent unknowns; self-loop error), the mechanism, loop-input, bind, sweep and coupling-dynamics diagnostics, and the TOTS diagnostics; `docs/reify-stdlib-reference.md` §13 rewritten; every documented signature compiles in a smoke `.ri`; exemplar `examples/best_practices/kinematic_closed_chain.ri` (a CoreXY reading `free_values`) + `INDEX.md` line; one-line index entry in `.claude/skills/reify-design/SKILL.md`; intent-level findability ("make a linkage whose joints close", "read the solved joint angle", "drive a gantry with two belts") | reify-mcp chunks, docs, examples, skill index | the four docs-truth items | λ, π, κ, θ, ν, ο, σ, φ | — |
| ω₂ #8856 | PRD close: terminal stamp | this PRD + manifest | committed `SHIPPED` header with landed leaf ids, AS-AUTHORED freeze, LIVE/AS-AUTHORED map | every other leaf, χ included | — |

**Docs-truth gate.** This PRD changes language surface: `bind`'s accepted values, `JointPosition`, `Snapshot.free_values` / `converged`, `snapshot`'s third argument, the `LoopClosure` fields, the meaning of closures, and new diagnostics. All four items live in ω₁, with real edges from every leaf that adds surface.

**G7 walk (advisory at author time).**
- INV-SF-1: closed-chain Undefs gain coded causes (θ, κ, ν, σ, ο), and implicit roots no longer Undef the snapshot (κ).
- INV-SF-6: every new diagnostic is coded. ξ deletes two codes no path emits.
- INV-SF-2: the new Errors exit nonzero (θ, κ, λ, φ).
- INV-PD-2: the `Snapshot` / `LoopClosure` declarations are populated (π).
- `nothing-vacuous-and-unowned`:
  - deletions: `per_joint_jacobian_local`, `code_str`, the wrapper and its codes, `mechanism_loop_closure_chains`;
  - `JointParent` / `joint_parents`: recorded follow-up candidate (§11);
  - χ: owned by S8's seam.

No waiver is needed.

## 11. Out of scope

- **Joint identity by structure (#7243).** Keyed `free_values` inherits it.
- **Cross-kind couplings and dimensioned coupling input.** #7188, rebuilt on ι (R2).
- **Closure verdicts.** NotConverged surfacing, fully-bound loop residual checks and range-violation warnings are #7185, on ξ (R3).
- **A rank-based over/underconstraint policy.** A known gap; a future rank test uses the LDLᵀ pivots `solve_normal_equations` already computes.
- **Stacked simultaneous multi-loop solves.** #8145 for dynamics. Snapshot uses C5's record-order rule with final re-measurement.
- **Coupling inverse dynamics.** σ makes it a coded refusal.
- **`reduce_constraint_rank`'s cut-joint treatment of the closing joint.** **[Resolved 2026-10-30 by #7312]** The function now drops only zero or linearly dependent rows, so the KKT constraint has the zero set of snapshot's rigid-tie residual, and T15 is re-derived on that basis. ~~It projects the closing joint's subspace out, which differs from snapshot's rigid-tie rule (§12.7). Follow-up candidate, not filed here.~~ The other half of the mismatch, that a closing body's mass never loads a joint in inverse dynamics, is #8883.
- **TOTS `force_limit` / `actuator_limits` (#7083) and dimension gating of trajectory readers (#6941).**
- **The trajectory `JointValue = Real` alias** (profile scalars).
- **The `JointParent` / `Mechanism.joint_parents` declaration mismatch.** Follow-up candidate.
- **Relation semantics tables.** S9 λ's `RelationSig`.

## 12. Open questions (tactical)

1. **CoreXY example form (κ).** Belt displacements as prismatic drives along (1,1,0)/(1,−1,0), as in the fixture. The motor-angle form needs #7188's cross-kind rack coupling and is upgraded there.
2. **New `DiagnosticCode` names.** *Suggested:* `MechanismWorldParentedClosure`, `MechanismSelfLoopClosure`, `KinematicLoopInputInvalid`, `DynamicsCouplingUnsupported`, `SweepMultiDofUnsupported`, `SweepRangeDimensionMismatch`, `BindPositionMismatch`. Decide in each leaf against `docs/notes/diagnostic-severity-policy.md`.
3. **Under-determined loops after R8 (κ).** A loop with more unknowns than effective residual rows takes Newton's minimum-norm step from the seeds, so non-unique results stay near the midpoints. Pin one such case with a test.
4. **A sweep whose free set changes mid-sweep (π).** A newly free joint seeds at its midpoint. Pin it with a test.
5. **`Value::Enum` display (η).** Measure the payload-enum goldens first (`cli_eval_generic_enum.rs`, `getar_*` fixtures) and list them per D10.
6. **`classifier.rs` and `std::fasten` (γ).** Read `is_geometry_qualified_name`'s consumer, decide whether fasten belongs, and pin it.
7. **The cut-joint vs rigid-tie note for `reduce_constraint_rank`.** **[Resolved 2026-10-30]** T15's equality did depend on it. #7312 retired the projection, and #9566 re-derived T15 ("T15 re-derivation after #7312" below). ~~File a follow-up when ρ lands, if T15's equality turns out to depend on it.~~

## 13. Rulings (Leo, 2026-10-13, this session)

1. **R1, closure shapes:** normalize at construction. The kinematic e2e corpus is not changed lightly; corrections are expressly allowed where a test is found pinning incorrect behaviour (D10). CoreXY kinematics becomes a test case (T4).
2. **R2, #7188:** rebuilds on `Joint` (depends on ι). #7974 is cancelled as superseded by μ.
3. **R3, #7185:** builds on S6. It depends on ξ and re-plans its verdicts on `SolveStatus` + `diagnostics_of`.
4. **R4, relation parse point:** at `relation_instance`. The lead amends S9 λ's held-contract wording.
5. **R5, TOTS diagnostics on check/LSP:** a held leaf on S8's typed diagnose-hook column (χ).
6. **R6, `kinematic.ri` declarations:** correct to runtime. `Snapshot.free_values` is per-joint `Map<Joint, JointPosition>` with a new data-carrying enum, and the warm start takes the previous Snapshot.
7. **R7, `bind` forms:** `JointPosition` only, with the corpus migrated. Consequence recorded after the critic pass: non-literal payloads need #7456, so λ₀ and λ depend on it.
8. **R8, loop unknowns:** every unbound joint of a normalized loop is an unknown, whichever side it is on. Across loops, earlier loops' joint values are bound for later loops, and every loop's residual is re-measured on the final values (absorbs #7207).

## Gap-fill amendments (2026-10-16)

Rulings from `plans/gap-fill-deliberation-reify-20261016.md` (Leo, 2026-10-16). Finding keys are from `/review` run review-reify-20261015.

- **§K6-D5** — new leaf φ₂ #9281 (§10 row above): `gcode_import`'s outcomes coded, after S8-α1 #8888 and φ #8850; ω₂ #8856 depends on it; file contention with #9192 (fk-5d64b909dfdf). S8 §9 records that this arm leaves S8's legacy list.
- **§K6-D6** — S6 follow-on (standalone, like #8881): task #9283 makes `simulate_trajectory`'s `EndEffectorTrack` honest after ψ₃ #8854 moves the accessors — `modal_result` passed through, no fake `nominal_pose`/`combined_pose`, accessors return Undef with a cause instead of 0.0, an unresolvable FaceSelector location is a coded rejection, deviations carry Length, a C4-style parity test (fk-c2cca28ae7c5). #8881 is amended to own per-sample FK from q(t) and the Pose3 / Vec3<Length> population, and depends on #9283 (fk-6166c9f2a331). Notes on #7101 (owners of the two C2′ fakes) and #8119 (`peak_deviation_at`'s runtime returns Real until #9283 lands).

## Retro-critic sitting amendments (2026-10-20)

- **S9 B1 / retro-critic R6 (C1)** — `RelationKind` is `reify_builtins::RelationKind`, the builtin registry's generated `Relation` sub-enum minted by the S8 leaf S8-ψ #9418; supersedes C2's hand-written reify-ir enum (C2 amended in place: `ALL` → `iter()`, `as_str` → the row's name, `from_name` deleted under I-REG-1; R4's parse point stands, the parser is `lookup`). §1 goal, §3 step 1, §9 rows (home of the kinds, S9 λ, S8 — direction reversed: rows mint, S6 binds) and the §10 γ row amended. γ #8830: modules reify-constraints and reify-eval (both take reify-builtins as a normal dependency); reify-ir `relation_kind.rs` is not created; new edge #8830 ← #9418; delivered checks `no-hand-written-relation-kind-parser` (replacing the `pub enum RelationKind` check) and `instance-carries-kind`. **Flagged, not ruled:** the joint twin — §9's S8 row still has joint builtin rows derive from `JointKind::from_ctor_name` (#6945 ← #8829), the same direction a leaf reify-builtins cannot take. Pointer: `plans/review-all-program-reify-20261012.md §Resolved decisions — retro-critic sitting R6`.

## T15 re-derivation after #7312 (2026-10-30)

#7312 (landed `10eb8ca95b`) retired the closing-joint motion-subspace projection in `dynamics/closed_chain.rs::reduce_constraint_rank`, which now drops only zero or linearly dependent rows. T15's old target held only because that projection deleted this loop's one constraint row. #9566 re-derived it by hand, and that derivation is the authority for T15's target. Its inputs are the fixtures' masses and topology plus the open control's τ_open, which anyone can re-measure on main; the scratch build under **Corroboration** only supports it:
- **The loop.** j_a (the flexure, prismatic z, 0.5 kg) and j_b (prismatic z, 0.25 kg) are both world-rooted. The 0.125 kg closing body re-parents j_b onto j_a, so the closure is q_b = q_a: one live row, A = [1, −1] (m_eff = 1, the same shape as `closed_2prismatic_idyn.ri`).
- **Inputs.** M = diag(0.5, 0.25) kg. ρ decides compliance once (C8), so the closed chain's τ_open is the open control's sample 1, measured with `target/debug/reify eval` built from main `ec9e47f729`: [m_a·g + f_s, m_b·g] = [−14.31375, 2.4525] N. The flexure spring term at q_a = 0.3 mm is f_s = τ_a − m_a·g = −19.21875 N.
- **KKT.** λ = (A·M⁻¹·τ_open)/(A·M⁻¹·Aᵀ) = (2·(−14.31375) − 4·2.4525)/(2 + 4) = −6.40625 N, and τ = τ_open + Aᵀλ = [−20.72, 8.85875] N. In closed form λ = f_s·m_b/(m_a + m_b). Gravity alone gives A·M⁻¹·τ_open = 0, which is why today's answer, with no spring term, has λ = 0.
- **Invariant.** Aᵀλ is orthogonal to ker(A) = span(1, 1), so τ_a + τ_b = −11.86125 N equals the open control's sum whatever the split. The split assumes today's massless closing body. #8883 owns that model and re-derives T15 if it changes.
- **Why the runtime reproduces it.** The loop residual is linear in both prismatic coordinates, so the FD Jacobian is ±1 to rounding and the solver's A equals the hand A. ρ asserts the derived value without retuning.
- **Corroboration.** Built the same way, the closed fixture gives [4.905, 2.4525] N today: the derivation's λ = 0 case, since no spring term reaches the closed chain yet. A scratch build, reverted and not recorded anywhere, wired compliance into `closed_chain_inverse_dynamics` the way `snapshot_inverse_dynamics` does and gave sample 1 = [−20.72, 8.85875] N. It cannot be replayed, so it is supporting evidence only; ρ's own run is the first reproducible measurement of the target.
- **T14 is unaffected.** Every joint in its loop carries the same q̈ + g, so λ = 0 and "45.24 N on id 3" stands. Its before-image did move: the closing body is still in today's positional slice, and the now-live row gives ids 0, 1, 2 = 16.45, 40.10, 17.48 N (was 22.62, 33.93, 11.31 N).
- **Records.** #8845's signal carries these values. #8883 is narrowed to the closing-body-mass half.
