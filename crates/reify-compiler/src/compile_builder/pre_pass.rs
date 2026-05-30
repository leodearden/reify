//! Pre-pass helpers for [`crate::compile_with_prelude_refs`]: parse-error
//! forwarding, module-pragma validation, and `#no_prelude` shadowing.
//!
//! Each helper operates on `&mut CompilationCtx`, pushing diagnostics but
//! otherwise performing no compilation work — that happens in the later
//! phase modules.

use reify_ast::{Declaration, FieldDef, FnDef, ParsedModule, TraitDecl, TypeAliasDecl, UnitDecl};
use reify_core::{Diagnostic, DiagnosticLabel, ModulePath};

use crate::CompiledModule;
use crate::annotations::is_known_module_pragma;
use crate::compile_builder::ctx::CompilationCtx;

/// Forward every entry in `parsed.errors` as a warning diagnostic on `ctx`.
///
/// Parse errors are reported as warnings (not errors) because the parser has
/// already produced a (partial) tree — compilation continues best-effort and
/// any hard failures surface as errors from the phase that can't resolve the
/// malformed declaration.
pub(crate) fn forward_parse_errors(ctx: &mut CompilationCtx, parsed: &ParsedModule) {
    for err in &parsed.errors {
        ctx.diagnostics.push(
            Diagnostic::warning(format!("parse error: {}", err.message))
                .with_label(DiagnosticLabel::new(err.span, "parse error")),
        );
    }
}

/// Validate module-level pragmas, emitting a warning diagnostic for each unrecognized name.
///
/// Recognized names are the union of [`crate::annotations::KNOWN_BLOCK_PRAGMAS`] and
/// [`crate::annotations::MODULE_ONLY_PRAGMAS`], tested via [`is_known_module_pragma`].
pub(crate) fn validate_module_pragmas(ctx: &mut CompilationCtx, parsed: &ParsedModule) {
    for pragma in &parsed.pragmas {
        if !is_known_module_pragma(&pragma.name) {
            ctx.diagnostics.push(
                Diagnostic::warning(format!("unknown pragma #{}", pragma.name))
                    .with_label(DiagnosticLabel::new(pragma.span, "unknown pragma")),
            );
        }
    }
}

/// Apply the `#no_prelude` module pragma: if present, return an empty prelude
/// slice (shadowing the caller's prelude); otherwise return the caller's
/// prelude unchanged.
///
/// This suppresses ALL prelude-dependent behavior — unit seeding, trait/enum/
/// function resolution, and constraint def imports — by funneling every phase
/// through the returned slice.
pub(crate) fn effective_prelude<'a>(
    parsed: &ParsedModule,
    prelude: &'a [&'a CompiledModule],
) -> &'a [&'a CompiledModule] {
    let has_no_prelude = parsed.pragmas.iter().any(|p| p.name == "no_prelude");
    if has_no_prelude { &[] } else { prelude }
}

/// Check the top-of-file `module` declaration against the resolver-derived path.
///
/// Returns `None` if the declared path matches the expected path (spec §7.1: correct).
/// Returns `Some(Diagnostic::warning(...))` if `declared` is `None` (W_MODULE_DECL_MISSING).
/// Returns `Some(Diagnostic::error(...))` if `declared` is `Some` but doesn't match (E_MODULE_PATH_MISMATCH).
///
/// This is a pure helper so it can be unit-tested without a full compilation context.
/// Re-exported from the crate root so `reify-cli` can call it via `reify_compiler::check_module_path_decl`.
pub fn check_module_path_decl(declared: Option<&ModulePath>, expected: &ModulePath) -> Option<Diagnostic> {
    match declared {
        None => Some(Diagnostic::warning(format!(
            "W_MODULE_DECL_MISSING: file has no top-of-file `module` declaration; \
             expected `module {}` (spec \u{00a7}7.1)",
            expected.0.join(".")
        ))),
        Some(d) if d != expected => Some(Diagnostic::error(format!(
            "E_MODULE_PATH_MISMATCH: declared module path '{}' does not match \
             expected path '{}' (derived from file location)",
            d.0.join("."),
            expected.0.join(".")
        ))),
        Some(_) => None,
    }
}

/// References into `parsed.declarations` collected by [`collect_decl_refs`],
/// partitioned by the phase that consumes them.
///
/// Borrows from `parsed`, so cannot be stored on `CompilationCtx` without
/// infecting ctx with a lifetime parameter (see task 2035 design decision #3).
/// Flows as an explicit value from the orchestrator into the downstream phase
/// functions that consume each field.
pub(crate) struct DeclRefs<'a> {
    pub(crate) fn_refs: Vec<&'a FnDef>,
    pub(crate) trait_refs: Vec<&'a TraitDecl>,
    pub(crate) field_refs: Vec<&'a FieldDef>,
    pub(crate) unit_refs: Vec<&'a UnitDecl>,
    pub(crate) alias_refs: Vec<&'a TypeAliasDecl>,
}

/// Single-pass scan over `parsed.declarations` that:
///
/// * pushes each `Enum` into `ctx.enum_defs`,
/// * reserves entity-namespace names for `Field` / `Structure` / `Occurrence`
///   / `Constraint` via `ctx.seen_entity_names` (emitting a duplicate-entity
///   error via `ctx.diagnostics` on collision — spec §4.2.1 unified entity
///   namespace),
/// * and accumulates borrow-slices of `Fn`, `Trait`, `Field`, `Unit`,
///   `TypeAlias` decls for later phases into the returned [`DeclRefs`].
///
/// `Import` and `Purpose` are deliberately untouched — they are handled in
/// the entities phase and the dedicated purpose pass, respectively.
pub(crate) fn collect_decl_refs<'a>(
    ctx: &mut CompilationCtx,
    parsed: &'a ParsedModule,
) -> DeclRefs<'a> {
    let mut refs = DeclRefs {
        fn_refs: Vec::new(),
        trait_refs: Vec::new(),
        field_refs: Vec::new(),
        unit_refs: Vec::new(),
        alias_refs: Vec::new(),
    };

    for decl in &parsed.declarations {
        // Parsed decls have unique spans within one module, so `is_first_entity_def`
        // is equivalent to "name absent" here — the span-equality branch is unreachable.
        match decl {
            Declaration::Enum(e) => {
                ctx.enum_defs.push(reify_ir::EnumDef {
                    name: e.name.clone(),
                    variants: e.variants.clone(),
                    doc: e.doc.clone(),
                });
            }
            Declaration::Function(fn_def) => {
                refs.fn_refs.push(fn_def);
            }
            Declaration::Trait(trait_decl) => {
                refs.trait_refs.push(trait_decl);
            }
            Declaration::Field(field_def) => {
                if ctx.record_or_report_duplicate(&field_def.name, field_def.span, "field") {
                    refs.field_refs.push(field_def);
                }
            }
            Declaration::Structure(structure) => {
                ctx.record_or_report_duplicate(&structure.name, structure.span, "structure");
            }
            Declaration::Occurrence(occurrence) => {
                ctx.record_or_report_duplicate(&occurrence.name, occurrence.span, "occurrence");
            }
            Declaration::Constraint(constraint) => {
                // Constraints reserve names in the entity namespace (spec §4.2.1)
                // even though constraint compilation is not yet implemented.
                ctx.record_or_report_duplicate(&constraint.name, constraint.span, "constraint");
            }
            Declaration::Unit(unit_decl) => {
                refs.unit_refs.push(unit_decl);
            }
            Declaration::TypeAlias(alias_decl) => {
                refs.alias_refs.push(alias_decl);
            }
            // Import, Purpose handled in pass 2 / purpose pass
            _ => {}
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use reify_core::{ModulePath, Severity};

    use super::check_module_path_decl;

    #[test]
    fn absent_decl_returns_warning_with_expected_path() {
        let expected = ModulePath::single("foo");
        let diag = check_module_path_decl(None, &expected)
            .expect("should return Some(diag) for absent declaration");
        assert_eq!(diag.severity, Severity::Warning);
        assert!(
            diag.message.contains("W_MODULE_DECL_MISSING"),
            "message should contain 'W_MODULE_DECL_MISSING', got: {}",
            diag.message
        );
    }

    #[test]
    fn matching_decl_returns_none() {
        let path = ModulePath::single("foo");
        let result = check_module_path_decl(Some(&path), &path);
        assert!(result.is_none(), "should return None when paths match");
    }

    #[test]
    fn mismatched_decl_returns_error_with_both_paths() {
        let declared = ModulePath::from_dotted("a.b.c").unwrap();
        let expected = ModulePath::single("foo");
        let diag = check_module_path_decl(Some(&declared), &expected)
            .expect("should return Some(diag) for mismatch");
        assert_eq!(diag.severity, Severity::Error);
        assert!(
            diag.message.contains("E_MODULE_PATH_MISMATCH"),
            "message should contain 'E_MODULE_PATH_MISMATCH', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("a.b.c"),
            "message should name the declared path 'a.b.c', got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("foo"),
            "message should name the expected path 'foo', got: {}",
            diag.message
        );
    }
}
