//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_core::Severity;

use crate::CompiledModule;
use crate::PreludeContext;
use crate::prelude_context::StdlibScope;
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
/// Registration order is semantically inert: each module compiles against
/// only the modules it declares as imports, and the build order is derived
/// from those imports (`stdlib_topo`), not from this list. Pinned by
/// `harness_modules_ports/stdlib_strict_build_tests`.
///
/// Public for `reify_doc_build::build_stdlib_doc_model`, which pairs each
/// compiled stdlib module with its source by module path.
pub fn stdlib_sources() -> Vec<(&'static str, String)> {
    let si_units_source = si_units::build_si_units_source();
    vec![
        ("std.units", include_str!("../stdlib/units.ri").to_owned()),
        ("std.si_units", si_units_source),
        (
            "std.materials.appearance",
            include_str!("../stdlib/materials_appearance.ri").to_owned(),
        ),
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
        (
            "std.fea.types",
            include_str!("../stdlib/fea_types.ri").to_owned(),
        ),
        (
            "std.solver.elastic",
            include_str!("../stdlib/solver_elastic.ri").to_owned(),
        ),
        (
            "std.fea.multi_case",
            include_str!("../stdlib/fea_multi_case.ri").to_owned(),
        ),
        (
            "std.solver.buckling",
            include_str!("../stdlib/solver_buckling.ri").to_owned(),
        ),
        (
            "std.analysis",
            include_str!("../stdlib/analysis.ri").to_owned(),
        ),
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
        (
            "std.trajectory",
            include_str!("../stdlib/trajectory.ri").to_owned(),
        ),
        ("std.fdm", include_str!("../stdlib/fdm.ri").to_owned()),
        (
            "std.fdm.correlations",
            include_str!("../stdlib/fdm_correlations.ri").to_owned(),
        ),
        (
            "std.fdm.as_printed",
            include_str!("../stdlib/fdm_as_printed.ri").to_owned(),
        ),
        (
            "std.fdm.slice",
            include_str!("../stdlib/fdm_slice.ri").to_owned(),
        ),
        (
            "std.tensegrity",
            include_str!("../stdlib/tensegrity.ri").to_owned(),
        ),
        (
            "std.process",
            include_str!("../stdlib/process.ri").to_owned(),
        ),
        (
            "std.kinematic",
            include_str!("../stdlib/kinematic.ri").to_owned(),
        ),
        (
            "std.flexures",
            include_str!("../stdlib/flexures.ri").to_owned(),
        ),
        ("std.joints", include_str!("../stdlib/joints.ri").to_owned()),
        ("std.ports", include_str!("../stdlib/ports.ri").to_owned()),
        (
            "std.dynamics",
            include_str!("../stdlib/dynamics.ri").to_owned(),
        ),
        (
            "std.modal.mechanism.fns",
            include_str!("../stdlib/modal_mechanism_fns.ri").to_owned(),
        ),
        (
            "std.stackup",
            include_str!("../stdlib/stackup.ri").to_owned(),
        ),
        (
            "std.ports.mechanical",
            include_str!("../stdlib/ports_mechanical.ri").to_owned(),
        ),
        (
            "std.ports.electrical",
            include_str!("../stdlib/ports_electrical.ri").to_owned(),
        ),
        (
            "std.ports.thermal",
            include_str!("../stdlib/ports_thermal.ri").to_owned(),
        ),
        (
            "std.ports.fluid",
            include_str!("../stdlib/ports_fluid.ri").to_owned(),
        ),
        ("std.fields", include_str!("../stdlib/fields.ri").to_owned()),
        (
            "std.option_recovery",
            include_str!("../stdlib/option_recovery.ri").to_owned(),
        ),
        ("std.result", include_str!("../stdlib/result.ri").to_owned()),
        (
            "std.surface_finish",
            include_str!("../stdlib/surface_finish.ri").to_owned(),
        ),
        (
            "std.determinacy.purposes",
            include_str!("../stdlib/determinacy_purposes.ri").to_owned(),
        ),
        (
            "std.prelude",
            include_str!("../stdlib/prelude.ri").to_owned(),
        ),
    ]
}

/// Returns a reference to the compiled stdlib modules.
///
/// On the first call, builds every embedded `.ri` stdlib module through
/// [`build_stdlib_modules`]: each compiles against only its declared imports,
/// in a canonical order derived from them
/// ([`crate::stdlib_topo::compile_modules_topo`]), so the slice is
/// topologically ordered and does not depend on the order of [`stdlib_sources`].
///
/// An import cycle, an import of a module outside the stdlib, or any
/// Error-severity diagnostic in any stdlib module panics rather than caching a
/// broken result.
///
/// Subsequent calls return the cached result with zero overhead.
pub fn load_stdlib() -> &'static [CompiledModule] {
    STDLIB_CACHE.get_or_init(|| {
        let owned = stdlib_sources();
        let sources: Vec<(&str, &str)> = owned.iter().map(|(n, s)| (*n, s.as_str())).collect();
        build_stdlib_modules(&sources)
    })
}

/// Compile a stdlib source set: topo-sort, compile, then enforce the two
/// stdlib-build invariants (no Error diagnostics, no cross-module pub-name
/// collisions).  Panics rather than returning `Err` on any violation — a broken
/// stdlib is always a programming error, never a recoverable condition.
///
/// This is exactly the body [`load_stdlib`] runs; it is `pub` so tests can drive
/// a synthetic source set through the SAME path production uses.  That matters
/// for the NS-P2 gate below: the real stdlib is `include_str!`-embedded, so
/// there is no way to inject a duplicate stdlib module from a `.ri` fixture, and
/// the observable the PRD asks for is "the stdlib BUILD fails" — not "a helper
/// function returns findings".
pub fn build_stdlib_modules(sources: &[(&str, &str)]) -> Vec<CompiledModule> {
    let modules = crate::stdlib_topo::compile_modules_topo(sources)
        .unwrap_or_else(|e| panic!("stdlib import graph: {e}"));

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
            module.path.to_dotted(),
            module
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .collect::<Vec<_>>()
        );
    }

    assert_no_cross_module_name_collisions(&modules);

    modules
}

/// One cross-module name collision: the same key declared by two stdlib modules.
/// The module fields are spelled DOTTED, as keyed in [`stdlib_sources`],
/// because that list is where a collision gets fixed.
struct NameCollision {
    kind: &'static str,
    name: String,
    first_module: String,
    second_module: String,
}

/// NS-P2 (task 5496, PRD docs/prds/v0_6/stdlib-namespace.md §7): no two stdlib
/// modules may declare the same pub name of the same kind.
///
/// Why this is an Error at stdlib BUILD time rather than a per-compile Warning:
/// before this gate, a cross-module duplicate was resolved silently and — worse
/// — *inconsistently*. Buckling and modal analysis both declared
/// `structure def Mode`; compile-side template resolution took last-wins (modal)
/// while the eval-side instantiation registry took first-wins (buckling), so
/// `Mode(frequency: …).frequency` type-checked clean and then evaluated to
/// Undef. Nothing in the pipeline reported it. The pre-existing per-kind prelude
/// warnings (units in `units_phase.rs`, pub aliases in `lib.rs`) are deliberately
/// left alone: they also fire for user-module compiles, where shadowing the
/// stdlib is legitimate. This gate is additional, and stdlib-only.
///
/// The scan reads the surface as MATERIALIZED in [`CompiledModule`], not the
/// syntax, and is deliberately not filtered on a syntactic `pub` marker:
/// buckling's `Mode` was declared without `pub` yet was prelude-visible, so a
/// pub-filtered syntactic scan would have missed the very collision this gate
/// exists to catch.
///
/// Scope is CROSS-module by construction (a map from key to first-declaring
/// module, walked in load order). Same-module duplicates — e.g. the three
/// `solve_elastic_static` overloads in `solver_elastic.ri` — are out of reach
/// here and remain the business of the per-module duplicate path in `ctx.rs`.
fn assert_no_cross_module_name_collisions(modules: &[CompiledModule]) {
    use std::collections::HashMap;

    let mut collisions: Vec<NameCollision> = Vec::new();

    // One map for every kind. The key is (kind, dedup_key) so each kind gets its
    // OWN namespace — `structure def Planar` (kinematic.ri) and
    // `trait Planar {}` (geometry_traits.ri) coexist on main today, and a
    // kind-agnostic key would force an out-of-scope rename. `dedup_key` is the
    // declared name for every kind except functions (see below).
    let mut seen: HashMap<(&'static str, String), String> = HashMap::new();

    let mut record = |kind: &'static str, dedup_key: String, name: &str, module: &str| {
        match seen.get(&(kind, dedup_key.clone())) {
            Some(first) => collisions.push(NameCollision {
                kind,
                name: name.to_string(),
                first_module: first.clone(),
                second_module: module.to_string(),
            }),
            None => {
                seen.insert((kind, dedup_key), module.to_string());
            }
        }
    };

    for module in modules {
        let m = module.path.to_dotted();

        // Templates carry their own kind label so `structure` and `occurrence`
        // are distinguishable in the message; derived, never hard-coded.
        for t in &module.templates {
            record(t.entity_kind.as_label(), t.name.clone(), &t.name, &m);
        }
        for e in &module.enum_defs {
            record("enum", e.name.clone(), &e.name, &m);
        }
        for t in &module.trait_defs {
            record("trait", t.name.clone(), &t.name, &m);
        }
        for u in &module.units {
            record("unit", u.name.clone(), &u.name, &m);
        }
        // Type aliases: `is_pub`-filtered, mirroring the filter the existing
        // PreludeContext alias-collision path already applies, so the new Error
        // and the pre-existing Warning agree on what counts as a collision.
        //
        // CRITICAL: read this vector, never the syntax. `type_aliases` holds
        // MODULE-LEVEL aliases only. Associated types declared inside a
        // trait/structure body (`type MotionValue` in `trait HasMotion`,
        // kinematic.ri) are members of their template and are correctly
        // absent here. A syntactic scan sees four `MotionValue`s and panics
        // the real stdlib build immediately.
        for a in module.type_aliases.iter().filter(|a| a.is_pub) {
            record("type alias", a.name.clone(), &a.name, &m);
        }
        for c in &module.constraint_defs {
            record("constraint def", c.name.clone(), &c.name, &m);
        }
        // Own purposes only: an importer's merged prelude copies are not declarations.
        for p in crate::name_env::own_purposes(module) {
            record("purpose", p.name.clone(), &p.name, &m);
        }
        for f in &module.fields {
            record("field", f.name.clone(), &f.name, &m);
        }
        // Functions key on (name, SIGNATURE) because cross-module overloads are
        // legitimate and live: `unwrap_or`, `or_else` and `fallback` are each
        // declared in both option_recovery.ri and result.ri with a different
        // first-parameter type. A name-only key panics the real stdlib build on
        // all three pairs.
        //
        // The signature is the param TYPES in order (names excluded — renaming a
        // param must not manufacture a false overload) plus the return type.
        // Deliberately NOT `content_hash`, which is also available on
        // CompiledFunction: it hashes the BODY, so two functions with an
        // identical signature and different bodies would key apart and the
        // collision would be missed — precisely the silent-first-wins hole this
        // gate exists to close.
        for f in &module.functions {
            let sig = format!(
                "{}({:?}) -> {:?}",
                f.name,
                f.params.iter().map(|(_, ty)| ty).collect::<Vec<_>>(),
                f.return_type
            );
            record("function", sig, &f.name, &m);
        }
    }

    if collisions.is_empty() {
        return;
    }

    // Aggregate: report EVERY collision in one panic. A first-offender-only
    // panic makes a multi-collision stdlib change a fix-and-rerun grind, the
    // same reason the `#no_prelude` invariant was refactored to collect-all.
    collisions.sort_by(|a, b| {
        (a.kind, &a.name, &a.first_module, &a.second_module).cmp(&(
            b.kind,
            &b.name,
            &b.first_module,
            &b.second_module,
        ))
    });
    let mut msg = String::from(
        "intra-stdlib pub-name collision (NS-P2) — stdlib registration order must be \
         semantically inert:",
    );
    for c in &collisions {
        msg.push_str(&format!(
            "\n  {} '{}' declared in both '{}' and '{}'",
            c.kind, c.name, c.first_module, c.second_module
        ));
    }
    panic!("{msg}");
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
    STDLIB_CONTEXT_CACHE.get_or_init(|| {
        PreludeContext::from_slice(load_stdlib()).with_stdlib_scope(StdlibScope::Ambient)
    })
}
