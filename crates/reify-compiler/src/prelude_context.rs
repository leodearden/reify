//! [`PreludeContext`] — an immutable, pre-built cache of prelude module data.
//!
//! Callers that compile many user modules against the same prelude (e.g.
//! `compile_with_stdlib`) pay the cost of flattening prelude enum definitions
//! only once — at `PreludeContext` construction time — rather than once per
//! `compile_with_prelude` call.
//!
//! # Design
//!
//! `PreludeContext` is a *caller-supplied input*, not internal per-compile
//! mutable state. It lives here (top-level module) rather than inside
//! `compile_builder/` to reflect that distinction: `compile_builder/` owns
//! the durable mutable per-compile state (`CompilationCtx`), while
//! `PreludeContext` is immutable and owned by the caller.
//!
//! The struct has private fields with public accessors, so additional caches
//! (prelude functions, trait names, …) can be added in future tasks without
//! breaking the `compile_with_prelude_context` signature.

use reify_types::EnumDef;

use crate::CompiledModule;
use crate::compile_builder::enums_phase::flatten_prelude_enum_defs;
use crate::types::CompiledTypeAlias;

/// An immutable, pre-built cache of prelude module data.
///
/// Caches the flattened `resolution_enums` and `pub_aliases` from a slice of
/// prelude modules so that the flat_map-then-clone in
/// `enums_phase::build_resolution_enums` (and the analogous alias pass) is
/// executed only once (at construction) rather than on every compile call.
///
/// # Typical usage
///
/// ```ignore
/// // Build once (e.g. at process start or the first time a prelude is needed):
/// let ctx = PreludeContext::from_slice(stdlib_loader::load_stdlib());
///
/// // Reuse for every subsequent user-module compilation:
/// for parsed in user_modules {
///     let compiled = compile_with_prelude_context(parsed, &ctx);
/// }
/// ```
pub struct PreludeContext<'a> {
    /// Borrowed references to the prelude modules.
    modules: Vec<&'a CompiledModule>,
    /// Pre-flattened enum definitions from every prelude module, in source order.
    resolution_enums: Vec<EnumDef>,
    /// Pre-flattened pub type aliases from every prelude module, in source order.
    /// Only `is_pub == true` entries are included (non-pub aliases are filtered out
    /// at construction time so callers never need to filter themselves).
    pub_aliases: Vec<CompiledTypeAlias>,
}

impl<'a> PreludeContext<'a> {
    /// Construct a `PreludeContext` from a slice of prelude module references.
    ///
    /// Computes `resolution_enums` by flattening `enum_defs` from every module
    /// in `prelude` (in source order) using the same `flatten_prelude_enum_defs`
    /// helper that the context-less path calls per compile. The result is cached
    /// on the `PreludeContext` so subsequent `compile_with_prelude_context` calls
    /// skip the flat_map entirely.
    ///
    /// Use [`from_slice`](Self::from_slice) when you own the `CompiledModule`
    /// values (e.g. `&'static [CompiledModule]` from `load_stdlib()`).
    pub fn new(prelude: &[&'a CompiledModule]) -> Self {
        let resolution_enums = flatten_prelude_enum_defs(prelude);
        let pub_aliases = prelude
            .iter()
            .flat_map(|m| m.type_aliases.iter().filter(|a| a.is_pub).cloned())
            .collect();
        Self {
            modules: prelude.to_vec(),
            resolution_enums,
            pub_aliases,
        }
    }

    /// Ergonomic constructor from an owned slice of `CompiledModule` values.
    ///
    /// Equivalent to `PreludeContext::new(&prelude.iter().collect::<Vec<_>>())`.
    /// Mirrors the `compile_with_prelude` → `compile_with_prelude_refs` wrapper
    /// pattern in `lib.rs` that builds a `Vec<&CompiledModule>` from a
    /// `&[CompiledModule]` slice.
    pub fn from_slice(prelude: &'a [CompiledModule]) -> Self {
        let refs: Vec<&'a CompiledModule> = prelude.iter().collect();
        Self::new(&refs)
    }

    /// The prelude module references this context was built from.
    pub fn modules(&self) -> &[&'a CompiledModule] {
        &self.modules
    }

    /// The pre-flattened enum definitions from every prelude module, in source
    /// order (m[0] enums first, then m[1], …).
    ///
    /// This is the cached result of
    /// `prelude.iter().flat_map(|m| m.enum_defs.iter().cloned()).collect()`.
    pub fn resolution_enums(&self) -> &[EnumDef] {
        &self.resolution_enums
    }

    /// The pre-flattened pub type aliases from every prelude module, in source
    /// order (m[0] pub aliases first, then m[1], …).
    ///
    /// Only `is_pub == true` entries are included — non-pub aliases from prelude
    /// modules are not visible to user modules by design.
    ///
    /// This is the cached result of:
    /// `prelude.iter().flat_map(|m| m.type_aliases.iter().filter(|a| a.is_pub).cloned()).collect()`.
    pub fn pub_aliases(&self) -> &[CompiledTypeAlias] {
        &self.pub_aliases
    }

    /// The names of every enum in the prelude, in [`resolution_enums`](Self::resolution_enums)
    /// order.
    ///
    /// Returned as an iterator (zero-allocation) — callers that need a slice
    /// can collect: `enum_names().collect::<Vec<_>>()`.  Used by
    /// `reify_compiler::parse_with_stdlib` to feed
    /// `reify_syntax::parse_with_prelude_enums` so prelude enums participate
    /// in the parser's `EnumAccess` disambiguation pass.
    pub fn enum_names(&self) -> impl Iterator<Item = &str> + '_ {
        self.resolution_enums.iter().map(|e| e.name.as_str())
    }
}
