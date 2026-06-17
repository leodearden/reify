//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_core::Severity;

use crate::CompiledModule;
use crate::PreludeContext;
use crate::si_units;

/// Global cache for compiled stdlib modules.
static STDLIB_CACHE: OnceLock<Vec<CompiledModule>> = OnceLock::new();

/// Global cache for the stdlib PreludeContext (pre-built enum + module refs).
///
/// Layered on top of [`STDLIB_CACHE`]: the first call to [`load_stdlib_context`]
/// triggers [`load_stdlib`] (which fills `STDLIB_CACHE` if empty), then builds
/// a [`PreludeContext`] from the cached slice and stores it here permanently.
/// Subsequent calls are a single pointer load.
static STDLIB_CONTEXT_CACHE: OnceLock<PreludeContext<'static>> = OnceLock::new();

/// Returns the raw `(module_name, source)` pairs for all embedded stdlib modules.
///
/// The `std.si_units` source is generated at runtime
/// (`si_units::build_si_units_source()`) and returned as an owned `String`
/// so the caller controls its lifetime. All other sources are `include_str!`
/// literals converted to owned `String` for a uniform element type.
///
/// Order is significant: the topo-sort in `stdlib_topo::compile_modules_topo`
/// is stable (identity when no module declares `import`), so the hand-chosen
/// positions are preserved unchanged. `std.units` stays first,
/// `std.determinacy.purposes` stays last.
///
/// **Ordering footgun:** the hand-chosen positions encode implicit load-order
/// constraints (e.g. `std.solver.buckling.fns` after `std.solver.buckling`,
/// `std.determinacy.purposes` last) that are NOT expressed as `import` edges
/// and are therefore NOT machine-checked by the topo-sort.  Adding any `import`
/// declaration to a stdlib module causes `compile_modules_topo` to reorder it
/// (pulling dependencies earlier), which may silently violate one of these
/// implicit constraints.  Before adding an `import`, re-validate ALL ordering
/// requirements against the new topo output.  The stability regression test
/// `signal_2_real_stdlib_compiles_clean_and_order_is_stable` (in
/// `stdlib_topo::tests`) acts as a permanent guard that the sort remains the
/// identity when no imports are present; it will need updating if imports are
/// intentionally introduced.
pub(crate) fn stdlib_sources() -> Vec<(&'static str, String)> {
    let si_units_source = si_units::build_si_units_source();
    vec![
        ("std.units", include_str!("../stdlib/units.ri").to_owned()),
        ("std.si_units", si_units_source),
        (
            "std.materials.mechanical",
            include_str!("../stdlib/materials_mechanical.ri").to_owned(),
        ),
        (
            "std.materials.thermal",
            include_str!("../stdlib/materials_thermal.ri").to_owned(),
        ),
        (
            "std.materials.electrical",
            include_str!("../stdlib/materials_electrical.ri").to_owned(),
        ),
        (
            "std.materials.optical",
            include_str!("../stdlib/materials_optical.ri").to_owned(),
        ),
        (
            "std.materials.chemical",
            include_str!("../stdlib/materials_chemical.ri").to_owned(),
        ),
        (
            "std.structural.physical",
            include_str!("../stdlib/structural_physical.ri").to_owned(),
        ),
        (
            "std.materials.fea",
            include_str!("../stdlib/materials_fea.ri").to_owned(),
        ),
        (
            "std.constitutive",
            include_str!("../stdlib/constitutive.ri").to_owned(),
        ),
        // `std.fea.types` declares the empty marker traits `Load` and
        // `Support`.  It MUST precede `std.solver.elastic` so that
        // `solve_elastic_static`'s `loads : List<Load>` / `supports :
        // List<Support>` params can resolve in the growing prelude.
        // Zero dependencies (empty marker traits) — placement is free.
        // Mirrors the `std.constitutive` → `std.solver.elastic` pattern
        // for `ConstitutiveLaw`.
        (
            "std.fea.types",
            include_str!("../stdlib/fea_types.ri").to_owned(),
        ),
        (
            "std.solver.elastic",
            include_str!("../stdlib/solver_elastic.ri").to_owned(),
        ),
        (
            "std.solver.buckling",
            include_str!("../stdlib/solver_buckling.ri").to_owned(),
        ),
        (
            "std.fea.multi_case",
            include_str!("../stdlib/fea_multi_case.ri").to_owned(),
        ),
        // `std.solver.buckling.fns` MUST follow BOTH `std.solver.buckling` AND
        // `std.fea.multi_case`:
        //   - `std.solver.buckling` provides `BucklingResult`/`Mode`/`BucklingOptions`/
        //     `MultiCaseBucklingResult` (function bodies field-access these; requires
        //     them in the prelude registry when `phase_functions` runs — esc-3851-32).
        //   - `std.fea.multi_case` provides `LoadCase` (task η adds
        //     `solve_buckling_load_cases(... cases: List<LoadCase> ...)`, which the
        //     type-checker must resolve).
        // Same split + rationale as `std.flexures.types` / `std.flexures`.
        (
            "std.solver.buckling.fns",
            include_str!("../stdlib/solver_buckling_fns.ri").to_owned(),
        ),
        (
            "std.analysis",
            include_str!("../stdlib/analysis.ri").to_owned(),
        ),
        // `std.fea` declares `StressInvariants` — the named output struct
        // for the `stress_invariants` builtin (FEA-5, task 2884). Placed
        // immediately after `std.analysis` (which defines `Stress`/
        // `AnalysisResult`) and before `std.determinacy.purposes` (which
        // MUST remain last). Zero ordering constraints on neighbouring
        // modules — it only uses built-in `Real`.
        ("std.fea", include_str!("../stdlib/fea.ri").to_owned()),
        (
            "std.tolerancing",
            include_str!("../stdlib/tolerancing.ri").to_owned(),
        ),
        (
            "std.geometry.traits",
            include_str!("../stdlib/geometry_traits.ri").to_owned(),
        ),
        ("std.io", include_str!("../stdlib/io.ri").to_owned()),
        (
            "std.stock",
            include_str!("../stdlib/standard_stock.ri").to_owned(),
        ),
        (
            "std.modal.analysis",
            include_str!("../stdlib/modal_analysis.ri").to_owned(),
        ),
        // `std.modal.analysis.fns` MUST follow `std.modal.analysis` —
        // the `fn` bodies field-access `ModalResult.modes[n].frequency`,
        // which requires `ModalResult`/`Mode` to already be in the prelude
        // template registry when `phase_functions` runs. Same split +
        // rationale as `std.solver.buckling` / `std.solver.buckling.fns`
        // (esc-3851-32).
        (
            "std.modal.analysis.fns",
            include_str!("../stdlib/modal_analysis_fns.ri").to_owned(),
        ),
        (
            "std.trajectory",
            include_str!("../stdlib/trajectory.ri").to_owned(),
        ),
        // `std.trajectory.fns` MUST follow `std.trajectory` — the
        // `@optimized simulate_trajectory` body constructs `EndEffectorTrack()`
        // (a no-arg ctor — like `ModalResult()`, which also has no per-field
        // defaults), which requires `EndEffectorTrack` to already be in the
        // prelude template registry when
        // `phase_functions` runs. Same split + rationale as `std.modal.analysis`
        // / `std.modal.analysis.fns` (modal_analysis_fns.ri:6-20). Placed before
        // the later `std.dynamics` / `std.kinematic`, which themselves depend on
        // `std.trajectory`, so order is safe (task π, prereq-2).
        (
            "std.trajectory.fns",
            include_str!("../stdlib/trajectory_fns.ri").to_owned(),
        ),
        ("std.fdm", include_str!("../stdlib/fdm.ri").to_owned()),
        // `std.fdm.correlations` (task β) MUST follow `std.fdm` — its
        // structures reference `InfillPattern` (from std.fdm) and
        // `MaterialPropertyProvenance` (from the earlier std.materials.fea),
        // both resolved via the growing sequential prelude.
        (
            "std.fdm.correlations",
            include_str!("../stdlib/fdm_correlations.ri").to_owned(),
        ),
        // `std.flexures` — single module containing the RotationalStiffness
        // alias, FlexureCompliance structure_def, and flexure_compliance()
        // accessor. The same-module skeleton pre-pass (task 3895) makes
        // the structure_def visible to the accessor fn body in the same
        // module, so no split is needed.
        (
            "std.flexures",
            include_str!("../stdlib/flexures.ri").to_owned(),
        ),
        // `std.tensegrity` depends on `std.units` (Length, Area, Force),
        // `std.si_units` (0N literal), and `std.materials.fea`
        // (ElasticMaterial trait) — all earlier in the prelude sequence.
        // End-insertion minimises merge friction with future sibling additions.
        (
            "std.tensegrity",
            include_str!("../stdlib/tensegrity.ri").to_owned(),
        ),
        // `std.process` depends only on `std.units` (Time, Money — the first
        // module in the sequence). End-append is order-safe and conflict-free.
        // Reconstruction of lost work from task #333 per PRD §Slice B.
        (
            "std.process",
            include_str!("../stdlib/process.ri").to_owned(),
        ),
        // `std.kinematic` declares the DrivingJoint marker trait, per-kind
        // joint structures (Prismatic/Revolute/Cylindrical/Planar/Spherical),
        // non-conforming joints (Coupling/Fixed), and top-level container types
        // (BodyId/Mechanism/Snapshot/SweepDim). Depends on std.trajectory
        // (Vec3 and JointValue aliases) and std.units (Bool/Int/Real).
        // Moved before std.dynamics (mechanism-β, task 4311) so that Mechanism
        // and Snapshot are in scope when dynamics.ri's inverse_dynamics /
        // inverse_dynamics_at_snapshot parameter types are compiled. No
        // circular dependency: std.kinematic only requires std.trajectory +
        // std.units, both of which are earlier in this sequence.
        // KCC-ζ task 3845.
        (
            "std.kinematic",
            include_str!("../stdlib/kinematic.ri").to_owned(),
        ),
        // `std.joints` defines the standard kinematic joint set (revolute /
        // prismatic / cylindrical / planar / spherical / ball) as `joint … with`
        // declarations over the relation vocabulary (geometric-joints γ, task
        // 4397). References only built-in relations (concentric / on / coincident
        // / flush / perpendicular) and built-in datum types (Axis / Plane /
        // Point3 / Orientation / Angle / Length) — declares NO `import`, so
        // the topo-sort remains the identity permutation. Placement after
        // std.kinematic keeps joint definitions organisationally co-located
        // with the kinematic module; there is no formal dependency.
        // `load_stdlib`'s panic-on-Error permanently enforces "all self-checks
        // pass" at prelude build: if any joint body mismatches its declared DOF
        // (E_JOINT_DOF_MISMATCH), the stdlib load panics. γ task 4397.
        (
            "std.joints",
            include_str!("../stdlib/joints.ri").to_owned(),
        ),
        // `std.dynamics` depends on `std.units` (Mass / Length / Time),
        // `std.trajectory` (for the `JointValue` alias used in TrajectorySample),
        // and `std.kinematic` (Mechanism / Snapshot nominal types used in
        // inverse_dynamics / inverse_dynamics_at_snapshot parameter types —
        // updated from Real placeholders by mechanism-β, task 4311).
        // Placement after std.kinematic satisfies all three dependencies.
        // RBD-α task 3822.
        (
            "std.dynamics",
            include_str!("../stdlib/dynamics.ri").to_owned(),
        ),
        // `std.stackup` declares the tolerance stack-up authoring surface
        // (Distribution, StackupMethod, Contributor, StackupResult).
        // Depends only on built-in Length/Int types and the mm literal
        // (available via std.si_units, earlier in this sequence).
        // Tail placement after std.dynamics is order-safe and conflict-free.
        // PRD v0_6 T6 — task 4004.
        (
            "std.stackup",
            include_str!("../stdlib/stackup.ri").to_owned(),
        ),
        // `std.ports` declares the Directionality enum and Port base trait.
        // No inter-module dependencies beyond built-in types.
        // Reconstruction of lost work per PRD task α.
        (
            "std.ports",
            include_str!("../stdlib/ports.ri").to_owned(),
        ),
        // `std.ports.mechanical` refines Port from std.ports and adds
        // mechanical port traits (MechanicalPort, Bore, Shaft, RotaryPort,
        // ThreadedPort, StructurePort) plus the Torque type alias.
        // Must follow std.ports in the growing prelude sequence so Port is
        // resolved when MechanicalPort : Port is compiled.
        // Reconstruction of lost work per PRD task α.
        (
            "std.ports.mechanical",
            include_str!("../stdlib/ports_mechanical.ri").to_owned(),
        ),
        // `std.ports.electrical` refines Port from std.ports and adds
        // electrical port traits (ElectricalPort, PowerPort, SignalPort)
        // plus the SignalKind enum.
        // Must follow std.ports in the growing prelude sequence so Port is
        // resolved when ElectricalPort : Port is compiled.
        // Reconstruction of lost work per PRD task β.
        (
            "std.ports.electrical",
            include_str!("../stdlib/ports_electrical.ri").to_owned(),
        ),
        // `std.ports.thermal` refines Port from std.ports and adds the
        // lumped-thermal-port trait ThermalPort (Modelica HeatPort convention:
        // temperature potential + heat_flow through variable).
        // Must follow std.ports in the growing prelude sequence so Port is
        // resolved when ThermalPort : Port is compiled.
        // Reconstruction of lost work per PRD task β.
        (
            "std.ports.thermal",
            include_str!("../stdlib/ports_thermal.ri").to_owned(),
        ),
        // `std.ports.fluid` refines Port from std.ports and adds the fluid
        // port trait FluidPort (pressure + VolumetricFlowRate + medium).
        // VolumetricFlowRate = Volume / Time type alias is pub (mirrors
        // units.ri Velocity); binary dim-op requires alias indirection.
        // Must follow std.ports in the growing prelude sequence so Port is
        // resolved when FluidPort : Port is compiled.
        // Reconstruction of lost work per PRD task β.
        (
            "std.ports.fluid",
            include_str!("../stdlib/ports_fluid.ri").to_owned(),
        ),
        // `std.fields` packages the built-in field differential operators
        // (gradient, divergence, curl, laplacian, sample) and hosts the
        // single generic exemplar `pub fn through<T>(x: T) -> T` (task
        // 4233 δ — Tier-1 generics gate; fields-api tasks ε/ζ will extend
        // this module further). It references no other stdlib module →
        // zero ordering constraints; tail-append is safe.
        // Reconstruction per PRD §Slice C.
        (
            "std.fields",
            include_str!("../stdlib/fields.ri").to_owned(),
        ),
        // `std.option_recovery` declares the 7 generic Option/Map recovery
        // combinators (unwrap_or / or_else / or_default / fallback /
        // is_some / is_none / get_or) as `pub fn` with typecheck-only
        // placeholder bodies (task α — PRD docs/prds/v0_6/result-and-fallback.md
        // §8 Phase 1).  Resolution and return-type substitution are delivered
        // free by the existing generic-fn resolver (resolve_function_overload →
        // type_compat::unify → substitute_type_params).  Real tag-driven
        // recovery eval is task β (intercept in reify-expr per §11 Q1).
        //
        // No import edges → compile_modules_topo keeps the identity order.
        // MUST be inserted BEFORE std.determinacy.purposes (which MUST remain
        // LAST; see its comment below).  Placement after std.fields satisfies
        // both constraints and keeps the load order stable.
        //
        // map_or is intentionally omitted — it needs an arrow-type grammar
        // production that does not exist; owned by task 4595.
        (
            "std.option_recovery",
            include_str!("../stdlib/option_recovery.ri").to_owned(),
        ),
        // `std.determinacy.purposes` ships the two standard determinacy-check
        // purposes (simulation_ready + design_review, PRD §5) that are merged
        // into every user module via merge_prelude_purposes (task-4016 ζ).
        //
        // MUST be LAST in the source list: merge_prelude_purposes runs for
        // every compile including each intra-stdlib module compile, but
        // no-ops here because std.determinacy.purposes is the only stdlib
        // module with pub purposes and it is registered last — no later
        // stdlib module sees it as a prelude during load_stdlib(), so none
        // inadvertently inherit simulation_ready/design_review during
        // prelude construction. Stdlib-internal count/hash goldens stay
        // byte-stable.
        (
            "std.determinacy.purposes",
            include_str!("../stdlib/determinacy_purposes.ri").to_owned(),
        ),
    ]
}

/// Returns a reference to the compiled stdlib modules.
///
/// On the first call, compiles all embedded `.ri` stdlib files via
/// [`crate::stdlib_topo::compile_modules_topo`], which performs a stable
/// topological sort so each module is compiled against a growing prelude
/// of its dependencies (dependency-first order).  When no module declares
/// an `import`, the sort is the identity permutation and output order is
/// identical to the hand-chosen order in [`stdlib_sources`].
///
/// Any cycle in the stdlib import graph panics immediately (should never
/// occur with the embedded sources).  Any Error-severity diagnostic in any
/// stdlib module also panics rather than caching a broken result.
///
/// Subsequent calls return the cached result with zero overhead.
pub fn load_stdlib() -> &'static [CompiledModule] {
    STDLIB_CACHE.get_or_init(|| {
        let owned = stdlib_sources();
        let sources: Vec<(&str, &str)> = owned.iter().map(|(n, s)| (*n, s.as_str())).collect();

        let modules = crate::stdlib_topo::compile_modules_topo(&sources)
            .unwrap_or_else(|cycle| panic!("stdlib import cycle: {}", cycle.message));

        // Fail fast: Error-severity diagnostics in embedded stdlib are always
        // programming errors. Without this check, a broken module gets permanently
        // cached in OnceLock, producing confusing downstream errors.
        // `assert!` (not `debug_assert!`) is intentional: a broken stdlib module
        // cached in OnceLock is at least as dangerous in release builds as in debug
        // builds, and `debug_assert!` would compile out in exactly the builds where
        // the bug is hardest to diagnose.
        for module in &modules {
            let has_errors = module
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error);
            assert!(
                !has_errors,
                "stdlib module '{}' has Error-severity diagnostics: {:?}",
                module.path,
                module
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .collect::<Vec<_>>()
            );
        }

        modules
    })
}

/// Returns a reference to the cached stdlib [`PreludeContext`].
///
/// On the first call, this triggers [`load_stdlib`] (if not already cached),
/// then constructs a [`PreludeContext`] from the resulting `&'static [CompiledModule]`
/// via [`PreludeContext::from_slice`] and stores it in [`STDLIB_CONTEXT_CACHE`].
///
/// The context pre-computes `resolution_enums` once so that every subsequent
/// [`compile_with_stdlib`][crate::compile_with_stdlib] call avoids re-flattening
/// the enum definitions across all stdlib modules on every compilation.
///
/// Subsequent calls return the same `&'static PreludeContext<'static>` with
/// zero overhead (a single atomic pointer load).
pub fn load_stdlib_context() -> &'static PreludeContext<'static> {
    STDLIB_CONTEXT_CACHE.get_or_init(|| PreludeContext::from_slice(load_stdlib()))
}
