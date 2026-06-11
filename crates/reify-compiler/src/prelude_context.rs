//! [`PreludeContext`] — an immutable, pre-built cache of prelude module data.
//!
//! Callers that compile many user modules against the same prelude (e.g.
//! `compile_with_stdlib`) pay the cost of flattening prelude enum definitions
//! and pub type alias lists only once — at `PreludeContext` construction time —
//! rather than once per `compile_with_prelude` call.
//!
//! # Cached fields
//!
//! - **`resolution_enums`** — all `EnumDef`s from every prelude module, in
//!   source order.  Used by `enums_phase::build_resolution_enums_from_cache`
//!   so the flat_map-then-clone is done once at construction.
//!
//! - **`pub_aliases`** — all `CompiledTypeAlias` entries marked `is_pub == true`
//!   from every prelude module, in source order.  Filtered at construction so
//!   callers never need to re-apply the `is_pub` predicate.  Used by
//!   `aliases_phase::phase_aliases` to seed `ctx.alias_registry` before
//!   resolving user-module aliases (task 2750).
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
//!
//! # Cross-prelude collision policy
//!
//! Three sibling phases all consume `PreludeContext` data but apply **different**
//! collision policies when two prelude modules declare the same name:
//!
//! | Phase | Call site | Policy | Diagnostic |
//! |-------|-----------|--------|------------|
//! | **units** | `compile_builder::units_phase::phase_units` | last-wins | `Severity::Warning` "declared in both X and Y; last-wins" |
//! | **aliases** | `compile_builder::aliases_phase::phase_aliases` (emission in `compile_with_prelude_context`) | first-wins | `Severity::Warning` "declared in both X and Y; first-wins" |
//! | **functions** | `merge_prelude_functions` | first-wins | none (silent) |
//!
//! **Why the divergence exists (architectural root):**
//!
//! - *Aliases* are pre-computed and cached on `PreludeContext` at construction
//!   time (here, in `PreludeContext::new`). Eager dedup is the natural
//!   cache-build flow; first-wins falls out of "skip if already in HashMap."
//!   Cross-prelude alias collisions are therefore resolved **before** the seed
//!   loop in `phase_aliases` ever runs — the seed loop sees a pre-deduplicated
//!   slice and is idempotent.
//!
//! - *Units* are **not** cached on `PreludeContext` (there is no `pub_units`
//!   field). The seed loop in `phase_units` iterates raw `prelude_module.units`
//!   on every compile call; overwrite-on-collision is the natural
//!   register-as-you-go flow, and emitting the warning at the overwrite site is
//!   the natural emission point.
//!
//! - *Functions* go through the separate `merge_prelude_functions` helper
//!   (used by `compile_with_prelude_refs` to build the overload-resolution
//!   table). Silent first-wins is documented as deliberate: rely on
//!   stdlib-level review for accidental prelude-vs-prelude function collisions
//!   rather than a diagnostic.
//!
//! **This divergence is deliberate** (documented in task 2776). Runtime
//! contracts are pinned by existing regression tests:
//!
//! - *units last-wins*: `prelude_module_unit_collision_emits_warning`,
//!   `three_prelude_collision_emits_two_chained_warnings`,
//!   `intra_module_duplicate_prelude_units_suppresses_nonsense_collision_warning`
//!   (`tests/unit_registry_tests.rs`).
//! - *aliases first-wins*: `cross_prelude_alias_collision_emits_warning`
//!   (`tests/cross_module_alias_propagation_tests.rs`).
//! - *functions first-wins-silent*: no diagnostic to pin; stdlib-level review
//!   is the guard.

use reify_ir::EnumDef;
use std::collections::HashMap;

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
    /// Cross-prelude pub alias name collisions detected at construction time.
    ///
    /// Each element is `(alias_name, first_module_path, second_module_path)`.
    /// Stored with first-wins deduplication: only the first-seen alias per name
    /// survives in `pub_aliases`; subsequent modules with the same pub alias name
    /// are recorded here for diagnostic emission by `compile_with_prelude_context`.
    collision_warnings: Vec<(String, String, String)>,
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
        // Build pub_aliases with first-wins deduplication across prelude modules.
        // If two modules declare a pub alias with the same name, only the first
        // survives in pub_aliases; the collision is recorded in collision_warnings
        // for diagnostic emission at compile time.
        let mut alias_first_module: HashMap<String, String> = HashMap::new();
        let mut collision_warnings: Vec<(String, String, String)> = Vec::new();
        let mut pub_aliases: Vec<CompiledTypeAlias> = Vec::new();
        for m in prelude {
            let module_path = m.path.to_string();
            for a in &m.type_aliases {
                if a.is_pub {
                    if let Some(first_module) = alias_first_module.get(&a.name) {
                        collision_warnings.push((
                            a.name.clone(),
                            first_module.clone(),
                            module_path.clone(),
                        ));
                        // first-wins: skip this duplicate
                    } else {
                        alias_first_module.insert(a.name.clone(), module_path.clone());
                        pub_aliases.push(a.clone());
                    }
                }
            }
        }
        Self {
            modules: prelude.to_vec(),
            resolution_enums,
            pub_aliases,
            collision_warnings,
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

    /// Cross-prelude pub alias name collisions detected at construction time.
    ///
    /// Each element is `(alias_name, first_module_path, second_module_path)`.
    /// Used by `compile_with_prelude_context` to emit `Severity::Warning`
    /// diagnostics so stdlib reviewers are notified of accidental pub alias
    /// name collisions across prelude modules.
    pub(crate) fn pub_alias_collision_warnings(&self) -> &[(String, String, String)] {
        &self.collision_warnings
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
