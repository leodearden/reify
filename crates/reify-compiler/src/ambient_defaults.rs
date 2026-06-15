//! Per-scope ambient-default table and innermost-wins resolver.
//!
//! Wires semantics onto the `default <TypeName> = <expr>` grammar produced by
//! task A (4496). An ambient default declares a value that fills any unprovided
//! structure member whose type matches the named type (DD1: keyed by type
//! *name*, machinery type-generic; the v1 surface only exercises `Material`).
//!
//! Two scopes carry ambient defaults:
//!   * **file level** — top-level `default` declarations, applied file-wide to
//!     top-level structures (the only structures that exist; DD6); and
//!   * **purpose level** — `default` declarations nested directly in a `purpose`
//!     body. Purpose-scoped defaults receive duplicate + declaration-site type
//!     checks but are NEVER injected into a structure (DD6 — structures cannot
//!     nest in a purpose, so a purpose default can never reach a definition-site
//!     injection). The nested map is the forward-compatible seam that lets a
//!     later task layer structure-in-purpose overrides on non-breakingly.
//!
//! Resolution is innermost-wins: a lookup for a given purpose consults that
//! purpose's own map first and falls back to the file level on a miss.

use std::collections::HashMap;

use reify_core::{SourceSpan, Type};

/// A resolved ambient-default entry.
///
/// Carries the default value expression (compiled at injection time exactly
/// like a trait-declared param default — DD2), the type the default was
/// declared against (resolved via `resolve_type_expr_with_aliases`), and the
/// declaration span (used to anchor the declaration-site type-mismatch and
/// duplicate diagnostics — DD4/DD5).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedAmbientDefault {
    /// The default value expression (e.g. the `Material(...)` constructor call).
    pub value: reify_ast::Expr,
    /// The type the `default` was declared against (e.g. `StructureRef("Material")`).
    pub declared_type: Type,
    /// Span of the `default ...` declaration, for diagnostics.
    pub span: SourceSpan,
}

/// Per-scope ambient-default table keyed by type name (DD1).
///
/// Holds a file-level map plus a per-purpose map. [`AmbientDefaults::resolve`]
/// walks innermost (purpose) → outermost (file).
#[derive(Debug, Clone, Default)]
pub(crate) struct AmbientDefaults {
    /// File-level (top-level) defaults: type name → entry.
    file_level: HashMap<String, ResolvedAmbientDefault>,
    /// Purpose-level defaults: purpose name → (type name → entry).
    purpose_level: HashMap<String, HashMap<String, ResolvedAmbientDefault>>,
}

impl AmbientDefaults {
    /// Resolve the ambient default for `type_name` as seen from the given
    /// scope, innermost-wins: a `Some(purpose)` lookup consults that purpose's
    /// own map first and falls back to the file level on a miss; a `None`
    /// (file-scope) lookup consults only the file level.
    ///
    /// STUB (pre-1 scaffolding): always returns `None`. The real innermost-wins
    /// walk lands in step-2.
    pub(crate) fn resolve(
        &self,
        type_name: &str,
        purpose: Option<&str>,
    ) -> Option<&ResolvedAmbientDefault> {
        let _ = (type_name, purpose);
        None
    }

    /// Insert a file-level (top-level) default for `type_name`.
    pub(crate) fn insert_file_level(&mut self, type_name: String, entry: ResolvedAmbientDefault) {
        self.file_level.insert(type_name, entry);
    }

    /// Insert a purpose-level default for `type_name` under `purpose`.
    pub(crate) fn insert_purpose_level(
        &mut self,
        purpose: String,
        type_name: String,
        entry: ResolvedAmbientDefault,
    ) {
        self.purpose_level
            .entry(purpose)
            .or_default()
            .insert(type_name, entry);
    }
}
