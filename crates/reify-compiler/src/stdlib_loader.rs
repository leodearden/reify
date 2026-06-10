//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_core::{ModulePath, Severity};

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

/// Returns a reference to the compiled stdlib modules.
///
/// On the first call, parses and compiles all embedded `.ri` stdlib files
/// **sequentially**, threading a growing prelude so each module sees all
/// previously compiled modules. This makes cross-module dependencies
/// (e.g. `Physical : MaterialSpec`) explicit during compilation rather than
/// relying on lazy string resolution.
///
/// Any Error-severity diagnostic in any stdlib module panics immediately
/// rather than caching a broken result: a broken `OnceLock` entry would
/// entrench the broken state for the entire process lifetime, producing
/// confusing downstream errors that are far harder to diagnose than a
/// direct panic at the point of failure.
///
/// Subsequent calls return the cached result with zero overhead.
pub fn load_stdlib() -> &'static [CompiledModule] {
    STDLIB_CACHE.get_or_init(|| {
        // Generate the SI prefix + derived-units source programmatically.
        // The synthetic `std.si_units` module sits between `std.units` (the
        // hand-written SI base + imperial + temperature units) and the
        // downstream stdlib modules, so materials/structural/tolerancing
        // see all SI prefixed and derived units via the prelude-seeding path.
        //
        // Order matters: `std.units` must come first so `std_units_is_first_module`
        // and dependent tests hold. `std.si_units` has no compile-time dependency
        // on `std.units` — its declarations use only dimension names + numeric
        // literals — so it compiles cleanly in second position.
        let si_units_source = si_units::build_si_units_source();

        let sources: Vec<(&str, &str)> = vec![
            ("std.units", include_str!("../stdlib/units.ri")),
            ("std.si_units", si_units_source.as_str()),
            (
                "std.materials.mechanical",
                include_str!("../stdlib/materials_mechanical.ri"),
            ),
            (
                "std.materials.thermal",
                include_str!("../stdlib/materials_thermal.ri"),
            ),
            (
                "std.materials.electrical",
                include_str!("../stdlib/materials_electrical.ri"),
            ),
            (
                "std.materials.optical",
                include_str!("../stdlib/materials_optical.ri"),
            ),
            (
                "std.materials.chemical",
                include_str!("../stdlib/materials_chemical.ri"),
            ),
            (
                "std.structural.physical",
                include_str!("../stdlib/structural_physical.ri"),
            ),
            (
                "std.materials.fea",
                include_str!("../stdlib/materials_fea.ri"),
            ),
            (
                "std.constitutive",
                include_str!("../stdlib/constitutive.ri"),
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
                include_str!("../stdlib/fea_types.ri"),
            ),
            (
                "std.solver.elastic",
                include_str!("../stdlib/solver_elastic.ri"),
            ),
            (
                "std.solver.buckling",
                include_str!("../stdlib/solver_buckling.ri"),
            ),
            // `std.solver.buckling.fns` MUST follow `std.solver.buckling` —
            // function bodies access struct fields (`result.modes[0].eigenvalue`)
            // that require `BucklingResult`/`Mode` to be in the prelude registry.
            // Same split as `std.flexures.types` / `std.flexures`. esc-3851-32.
            (
                "std.solver.buckling.fns",
                include_str!("../stdlib/solver_buckling_fns.ri"),
            ),
            (
                "std.fea.multi_case",
                include_str!("../stdlib/fea_multi_case.ri"),
            ),
            ("std.analysis", include_str!("../stdlib/analysis.ri")),
            ("std.tolerancing", include_str!("../stdlib/tolerancing.ri")),
            (
                "std.geometry.traits",
                include_str!("../stdlib/geometry_traits.ri"),
            ),
            ("std.io", include_str!("../stdlib/io.ri")),
            ("std.stock", include_str!("../stdlib/standard_stock.ri")),
            (
                "std.modal.analysis",
                include_str!("../stdlib/modal_analysis.ri"),
            ),
            // `std.modal.analysis.fns` MUST follow `std.modal.analysis` —
            // the `fn` bodies field-access `ModalResult.modes[n].frequency`,
            // which requires `ModalResult`/`Mode` to already be in the prelude
            // template registry when `phase_functions` runs. Same split +
            // rationale as `std.solver.buckling` / `std.solver.buckling.fns`
            // (esc-3851-32).
            (
                "std.modal.analysis.fns",
                include_str!("../stdlib/modal_analysis_fns.ri"),
            ),
            (
                "std.trajectory",
                include_str!("../stdlib/trajectory.ri"),
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
                include_str!("../stdlib/trajectory_fns.ri"),
            ),
            ("std.fdm", include_str!("../stdlib/fdm.ri")),
            // `std.fdm.correlations` (task β) MUST follow `std.fdm` — its
            // structures reference `InfillPattern` (from std.fdm) and
            // `MaterialPropertyProvenance` (from the earlier std.materials.fea),
            // both resolved via the growing sequential prelude.
            (
                "std.fdm.correlations",
                include_str!("../stdlib/fdm_correlations.ri"),
            ),
            // `std.flexures` — single module containing the RotationalStiffness
            // alias, FlexureCompliance structure_def, and flexure_compliance()
            // accessor. The same-module skeleton pre-pass (task 3895) makes
            // the structure_def visible to the accessor fn body in the same
            // module, so no split is needed.
            (
                "std.flexures",
                include_str!("../stdlib/flexures.ri"),
            ),
            // `std.tensegrity` depends on `std.units` (Length, Area, Force),
            // `std.si_units` (0N literal), and `std.materials.fea`
            // (ElasticMaterial trait) — all earlier in the prelude sequence.
            // End-insertion minimises merge friction with future sibling additions.
            (
                "std.tensegrity",
                include_str!("../stdlib/tensegrity.ri"),
            ),
            // `std.process` depends only on `std.units` (Time, Money — the first
            // module in the sequence). End-append is order-safe and conflict-free.
            // Reconstruction of lost work from task #333 per PRD §Slice B.
            (
                "std.process",
                include_str!("../stdlib/process.ri"),
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
                include_str!("../stdlib/kinematic.ri"),
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
                include_str!("../stdlib/dynamics.ri"),
            ),
            // `std.stackup` declares the tolerance stack-up authoring surface
            // (Distribution, StackupMethod, Contributor, StackupResult).
            // Depends only on built-in Length/Int types and the mm literal
            // (available via std.si_units, earlier in this sequence).
            // Tail placement after std.dynamics is order-safe and conflict-free.
            // PRD v0_6 T6 — task 4004.
            (
                "std.stackup",
                include_str!("../stdlib/stackup.ri"),
            ),
            // `std.ports` declares the Directionality enum and Port base trait.
            // No inter-module dependencies beyond built-in types.
            // Reconstruction of lost work per PRD task α.
            (
                "std.ports",
                include_str!("../stdlib/ports.ri"),
            ),
            // `std.ports.mechanical` refines Port from std.ports and adds
            // mechanical port traits (MechanicalPort, Bore, Shaft, RotaryPort,
            // ThreadedPort, StructurePort) plus the Torque type alias.
            // Must follow std.ports in the growing prelude sequence so Port is
            // resolved when MechanicalPort : Port is compiled.
            // Reconstruction of lost work per PRD task α.
            (
                "std.ports.mechanical",
                include_str!("../stdlib/ports_mechanical.ri"),
            ),
            // `std.ports.electrical` refines Port from std.ports and adds
            // electrical port traits (ElectricalPort, PowerPort, SignalPort)
            // plus the SignalKind enum.
            // Must follow std.ports in the growing prelude sequence so Port is
            // resolved when ElectricalPort : Port is compiled.
            // Reconstruction of lost work per PRD task β.
            (
                "std.ports.electrical",
                include_str!("../stdlib/ports_electrical.ri"),
            ),
            // `std.ports.thermal` refines Port from std.ports and adds the
            // lumped-thermal-port trait ThermalPort (Modelica HeatPort convention:
            // temperature potential + heat_flow through variable).
            // Must follow std.ports in the growing prelude sequence so Port is
            // resolved when ThermalPort : Port is compiled.
            // Reconstruction of lost work per PRD task β.
            (
                "std.ports.thermal",
                include_str!("../stdlib/ports_thermal.ri"),
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
                include_str!("../stdlib/ports_fluid.ri"),
            ),
            // `std.fields` is a documentation-only packaging surface for the
            // existing built-in field differential operators (gradient, divergence,
            // curl, laplacian, sample). It declares no pub fn or pub type and
            // references no other stdlib module → zero ordering constraints;
            // tail-append is safe. Reconstruction per PRD §Slice C.
            ("std.fields", include_str!("../stdlib/fields.ri")),
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
                include_str!("../stdlib/determinacy_purposes.ri"),
            ),
        ];

        // SEQUENTIAL COMPILATION WITH GROWING PRELUDE: each module is compiled
        // against all previously-compiled stdlib modules (`&modules` grows by
        // one each iteration). This implements the cross-module dependency
        // requirement from task #326 suggestion #2 — a stdlib module added
        // later (e.g. std.structural.physical) can freely reference traits and
        // types declared in earlier modules (e.g. std.materials.mechanical).
        let mut modules = Vec::with_capacity(sources.len());
        for (module_name, source) in &sources {
            // Collect enum names from every module compiled so far (the growing
            // prelude) to seed the parser's EnumAccess-disambiguation set.  This
            // mirrors `flatten_prelude_enum_defs` (enums_phase.rs:19), which builds
            // the type-resolver's `resolution_enums`, so value-lowering sees the
            // same prelude enum set as the type resolver.
            //
            // SCOPE: This seeding applies to EVERY stdlib module in `sources`, not
            // only to `std.modal.analysis`.  The change is safe because any stdlib
            // module that already references a prelude enum in value position WITHOUT
            // a local copy would lower to MemberAccess→unresolved-name and trip the
            // Error-severity `assert!` below — meaning `load_stdlib` already panics
            // for such cases.  The seeding only changes lowering for cases that
            // currently error; it cannot silently alter a module that compiles today.
            // `all_stdlib_modules_have_no_errors` (stdlib_loader_tests.rs) is the
            // regression guard for any accidental EnumAccess collision introduced by
            // future stdlib modules whose X.Y value-positions collide with a prelude
            // enum name.
            //
            // We seed from the LOCAL growing `modules` Vec — NOT from
            // `load_stdlib_context()` — to avoid re-entering the `STDLIB_CACHE`
            // OnceLock (which would deadlock: `load_stdlib_context` calls
            // `load_stdlib`, and we are inside `STDLIB_CACHE.get_or_init`).
            // The `prelude_enum_names` borrow ends at the parse call (NLL), so
            // the subsequent `modules.push(compiled)` is unaffected.
            let prelude_enum_names: Vec<&str> = modules
                .iter()
                .flat_map(|m: &CompiledModule| m.enum_defs.iter().map(|e| e.name.as_str()))
                .collect();
            let parsed = reify_syntax::parse_with_prelude_enums(
                source,
                ModulePath::from_dotted(module_name)
                    .expect("stdlib module name must be a valid dotted path"),
                &prelude_enum_names,
            );

            // Fail fast: parse errors in embedded stdlib are always programming errors.
            assert!(
                parsed.errors.is_empty(),
                "stdlib module '{}' has parse errors: {:?}",
                module_name,
                parsed.errors
            );

            // Compile with the growing prelude so each stdlib module sees all
            // previously compiled modules. This ensures cross-module trait
            // refinements (Physical→MaterialSpec) are available during compilation.
            let compiled = crate::compile_with_prelude(&parsed, &modules);

            // Fail fast: Error-severity diagnostics in embedded stdlib are always
            // programming errors. Without this check, a broken module gets permanently
            // cached in OnceLock, producing confusing downstream errors.
            // `assert!` (not `debug_assert!`) is intentional: a broken stdlib module
            // cached in OnceLock is at least as dangerous in release builds as in debug
            // builds, and `debug_assert!` would compile out in exactly the builds where
            // the bug is hardest to diagnose.
            let has_errors = compiled
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error);
            assert!(
                !has_errors,
                "stdlib module '{}' has Error-severity diagnostics: {:?}",
                module_name,
                compiled
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .collect::<Vec<_>>()
            );

            modules.push(compiled);
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
