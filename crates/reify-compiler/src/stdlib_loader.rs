//! Stdlib loader — embeds, parses, compiles and caches .ri stdlib files.
//!
//! Uses `include_str!` to embed stdlib source at compile time and `OnceLock`
//! for thread-safe, zero-cost-after-init caching.

use std::sync::OnceLock;

use reify_types::ModulePath;

use crate::CompiledModule;

/// Embedded source for stdlib/materials_mechanical.ri.
const MATERIALS_MECHANICAL_SRC: &str =
    include_str!("../stdlib/materials_mechanical.ri");

/// Global cache for compiled stdlib modules.
static STDLIB_CACHE: OnceLock<Vec<CompiledModule>> = OnceLock::new();

/// Returns a reference to the compiled stdlib modules.
///
/// On the first call, parses and compiles all embedded `.ri` stdlib files.
/// Subsequent calls return the cached result with zero overhead.
pub fn load_stdlib() -> &'static [CompiledModule] {
    STDLIB_CACHE.get_or_init(|| {
        let sources: &[(&str, &str)] = &[
            ("std.materials.mechanical", MATERIALS_MECHANICAL_SRC),
        ];

        sources
            .iter()
            .map(|(module_name, source)| {
                let segments: Vec<String> = module_name.split('.').map(String::from).collect();
                let parsed = reify_syntax::parse(source, ModulePath::new(segments));
                crate::compile(&parsed)
            })
            .collect()
    })
}
