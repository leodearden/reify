//! [`PreludeContext`] — an immutable, pre-built cache of prelude module data.
//!
//! Callers that compile many user modules against the same prelude (e.g.
//! `compile_with_stdlib`) pay the cost of flattening prelude enum definitions
//! and pub type alias lists only once — at `PreludeContext` construction time —
//! rather than once per `compile_with_prelude` call.
//!
//! # Cached fields
//!
//! - **`resolution_enums`** — the enums the prelude binds on its own under
//!   NS-P1 ([`crate::name_env`]): one per name, in precedence order. Feeds the
//!   parser's enum-name seed ([`PreludeContext::enum_names`]).
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

use crate::CompiledModule;
use crate::compile_builder::enums_phase::flatten_prelude_enum_defs;
use crate::name_env;
use crate::types::CompiledTypeAlias;

/// An immutable, pre-built cache of prelude module data.
///
/// Caches the prelude's bound `resolution_enums` and `pub_aliases` so they
/// are computed only once (at construction) rather than on every compile call.
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
    /// The prelude's own NS-P1 enum bindings, in precedence order.
    resolution_enums: Vec<EnumDef>,
    /// The prelude's own NS-P1 binding of pub type aliases, in precedence order.
    pub_aliases: Vec<CompiledTypeAlias>,
    /// Cross-prelude pub alias name collisions detected at construction time:
    /// one `(alias_name, winning_module_path, shadowed_module_path)` per pub
    /// alias that lost its name, for diagnostic emission by
    /// `compile_with_prelude_context`.
    collision_warnings: Vec<(String, String, String)>,
}

impl<'a> PreludeContext<'a> {
    /// Construct a `PreludeContext` from a slice of prelude module references.
    ///
    /// Computes `resolution_enums` once, via `flatten_prelude_enum_defs`.
    ///
    /// Use [`from_slice`](Self::from_slice) when you own the `CompiledModule`
    /// values (e.g. `&'static [CompiledModule]` from `load_stdlib()`).
    pub fn new(prelude: &[&'a CompiledModule]) -> Self {
        let resolution_enums = flatten_prelude_enum_defs(prelude);
        // The prelude's own NS-P1 alias binding: winners are seeded, and every
        // shadowed pub alias is recorded for a collision warning naming the
        // winning module first.
        let alias_bindings = name_env::bind::<CompiledTypeAlias, _>(&[], prelude.iter().copied());
        let pub_aliases: Vec<CompiledTypeAlias> = alias_bindings.winners().cloned().collect();
        let collision_warnings: Vec<(String, String, String)> = alias_bindings
            .shadowings()
            .iter()
            .filter_map(|s| {
                let winner = s.winner.origin.module()?;
                let shadowed = s.shadowed.origin.module()?;
                Some((s.key.to_string(), winner.to_string(), shadowed.to_string()))
            })
            .collect();
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

    /// The enums the prelude binds on its own under NS-P1: one per name, user
    /// imports' before std modules', each tier in slice order.
    pub fn resolution_enums(&self) -> &[EnumDef] {
        &self.resolution_enums
    }

    /// The pub type aliases the prelude binds on its own under NS-P1: one per
    /// name, user imports' before std modules', each tier in slice order.
    /// Non-pub prelude aliases are not visible to user modules by design.
    pub fn pub_aliases(&self) -> &[CompiledTypeAlias] {
        &self.pub_aliases
    }

    /// Cross-prelude pub alias name collisions detected at construction time.
    ///
    /// Each element is `(alias_name, winning_module_path, shadowed_module_path)`.
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
