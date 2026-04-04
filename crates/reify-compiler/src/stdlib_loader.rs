//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_types::{ModulePath, Severity};

use crate::CompiledModule;

/// Embedded source for stdlib/units.ri.
const UNITS_SRC: &str = include_str!("../stdlib/units.ri");

/// Embedded source for stdlib/materials_mechanical.ri.
const MATERIALS_MECHANICAL_SRC: &str = include_str!("../stdlib/materials_mechanical.ri");

/// Embedded source for stdlib/structural_physical.ri.
const STRUCTURAL_PHYSICAL_SRC: &str =
    include_str!("../stdlib/structural_physical.ri");

/// Embedded source for stdlib/tolerancing.ri.
const TOLERANCING_SRC: &str = include_str!("../stdlib/tolerancing.ri");

/// Global cache for compiled stdlib modules.
static STDLIB_CACHE: OnceLock<Vec<CompiledModule>> = OnceLock::new();

/// Returns a reference to the compiled stdlib modules.
///
/// On the first call, parses and compiles all embedded `.ri` stdlib files
/// **sequentially**, threading a growing prelude so each module sees all
/// previously compiled modules. This makes cross-module dependencies
/// (e.g. `Physical : Material`, `ElasticallyDeformable : Elastic`) explicit
/// during compilation rather than relying on lazy string resolution.
///
/// Subsequent calls return the cached result with zero overhead.
pub fn load_stdlib() -> &'static [CompiledModule] {
    STDLIB_CACHE.get_or_init(|| {
        let sources: &[(&str, &str)] = &[
            ("std.units", UNITS_SRC),
            ("std.materials.mechanical", MATERIALS_MECHANICAL_SRC),
            ("std.structural.physical", STRUCTURAL_PHYSICAL_SRC),
            ("std.tolerancing", TOLERANCING_SRC),
        ];

        let mut modules = Vec::with_capacity(sources.len());
        for (module_name, source) in sources {
            let segments: Vec<String> = module_name.split('.').map(String::from).collect();
            let parsed = reify_syntax::parse(source, ModulePath::new(segments));

            // Fail fast: parse errors in embedded stdlib are always programming errors.
            assert!(
                parsed.errors.is_empty(),
                "stdlib module '{}' has parse errors: {:?}",
                module_name, parsed.errors
            );

            // Compile with the growing prelude so each stdlib module sees all
            // previously compiled modules. This ensures cross-module trait
            // refinements (Physical→Material, ElasticallyDeformable→Elastic)
            // are available during compilation.
            let compiled = crate::compile_with_prelude(&parsed, &modules);

            // Fail fast: Error-severity diagnostics in embedded stdlib are always
            // programming errors. Without this check, a broken module gets permanently
            // cached in OnceLock, producing confusing downstream errors.
            let error_diagnostics: Vec<_> = compiled
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .collect();
            assert!(
                error_diagnostics.is_empty(),
                "stdlib module '{}' has Error-severity diagnostics: {:?}",
                module_name, error_diagnostics
            );

            modules.push(compiled);
        }
        modules
    })
}
