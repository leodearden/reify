//! Reserved builtin type name lint (task 4591 — W_RESERVED_TYPE_NAME).
//!
//! Walks the parsed AST once and emits a Warning diagnostic
//! ([`DiagnosticCode::ReservedTypeName`]) whenever a user `enum`,
//! `structure`, `occurrence`, `trait`, or `type` alias declaration uses a
//! name that is also resolvable by the builtin type resolver
//! ([`resolve_type_name`]).
//!
//! # Why warn
//!
//! `resolve_type_with_aliases` checks builtin types BEFORE user-declared
//! type aliases, structure/trait names; enum names chain after builtin
//! resolution via `resolve_enum_type`. A user `enum Direction`,
//! `structure Frame`, `trait Axis`, or `type Direction = Bool` is
//! therefore silently shadowed by the builtin with no diagnostic. This
//! lint surfaces the collision so authors can rename their declarations or
//! are at minimum aware of the precedence.
//!
//! # Severity
//!
//! Warning (not Error): programs must keep compiling with the builtin
//! winning. This is purely advisory — no behavioral change to the type
//! resolver.
//!
//! # Policy
//!
//! The collision predicate is exactly `resolve_type_name(name).is_some()`,
//! using the function as the single source of truth. This automatically
//! covers future builtin additions without requiring a hardcoded name list.
//!
//! # Diagnostic label span
//!
//! The label is attached to the whole declaration span (e.g., the entire
//! `enum Direction { In, Out }` range) because the AST declaration types
//! (`EnumDecl`, `StructureDef`, `OccurrenceDef`, `TraitDecl`,
//! `TypeAliasDecl`) do not expose a dedicated `name_span` field. Using a
//! name-only span would reduce noise for multi-line declarations, but
//! adding `name_span` to those structs is out of scope for task 4591.

use reify_ast::ParsedModule;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel};

use crate::type_resolution::resolve_type_name;

/// Walk every top-level declaration in `parsed` and emit a
/// [`DiagnosticCode::ReservedTypeName`] Warning for each user
/// `enum`/`structure`/`occurrence`/`trait` whose name is resolvable by
/// [`resolve_type_name`].
///
/// The lint is registered in `compile_builder/mod.rs` and invoked in
/// `lib.rs` immediately after `shadow_lint::lint_module`.
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    use reify_ast::Declaration;

    for decl in &parsed.declarations {
        let (kind_label, name, span) = match decl {
            Declaration::Enum(e) => ("enum", e.name.as_str(), e.span),
            Declaration::Structure(s) => ("structure", s.name.as_str(), s.span),
            Declaration::Occurrence(o) => ("occurrence", o.name.as_str(), o.span),
            Declaration::Trait(t) => ("trait", t.name.as_str(), t.span),
            Declaration::TypeAlias(a) => ("type alias", a.name.as_str(), a.span),
            _ => continue,
        };

        if resolve_type_name(name).is_some() {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "{kind_label} '{name}' shadows a builtin type name; \
                     the builtin takes precedence in type position"
                ))
                .with_code(DiagnosticCode::ReservedTypeName)
                .with_label(DiagnosticLabel::new(span, "shadows a builtin type name")),
            );
        }
    }
}
