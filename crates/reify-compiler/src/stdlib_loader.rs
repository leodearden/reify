//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_types::{ModulePath, Severity};

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
                "std.solver.elastic",
                include_str!("../stdlib/solver_elastic.ri"),
            ),
            (
                "std.solver.buckling",
                include_str!("../stdlib/solver_buckling.ri"),
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
                "std.trajectory",
                include_str!("../stdlib/trajectory.ri"),
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
            let parsed = reify_syntax::parse(
                source,
                ModulePath::from_dotted(module_name)
                    .expect("stdlib module name must be a valid dotted path"),
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
