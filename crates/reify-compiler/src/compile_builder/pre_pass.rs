//! Pre-pass helpers for [`crate::compile_with_prelude_refs`]: parse-error
//! forwarding, module-pragma validation, and `#no_prelude` shadowing.
//!
//! Each helper operates on `&mut CompilationCtx`, pushing diagnostics but
//! otherwise performing no compilation work — that happens in the later
//! phase modules.

use reify_ast::{Declaration, FieldDef, FnDef, ParsedModule, TraitDecl, TypeAliasDecl, UnitDecl};
use reify_core::{Diagnostic, DiagnosticLabel};

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
