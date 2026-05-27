//! Phase-4 units: seed prelude units into the registry, then compile each
//! module-local unit declaration.
//!
//! Prelude seeding runs first so module-local code can reference prelude
//! units by name. Cross-prelude collisions produce a last-wins warning;
//! the warning fires only for *genuine* cross-module pairs — intra-module
//! duplicates are suppressed here because `compile()` rejects them earlier.
//! Module-local duplicates produce an error keyed on the original
//! registration's provenance (stdlib / user-module / module-local).
//!
//! **Cross-phase note:** see `prelude_context` § "Cross-prelude collision
//! policy" for a comparison of all three sibling-phase policies (units =
//! last-wins/warns; aliases = first-wins/warns; functions =
//! first-wins/silent).

use reify_ast::UnitDecl;
use reify_core::{Diagnostic, DiagnosticLabel, SourceSpan};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::type_resolution::compile_unit;
use crate::types::CompiledUnit;
use crate::units::UnitEntry;

/// Run phase-4 (units).
///
/// Seeds every `pub` unit from `prelude` into `ctx.unit_registry`, emitting a
/// `last-wins` warning when two prelude modules declare the same unit name.
/// Then compiles each `unit_decl` via [`compile_unit`] and registers it in
/// `ctx.unit_registry` / `ctx.compiled_units`, emitting a duplicate-unit
/// error (specialized by provenance) if a name is already taken.
pub(crate) fn phase_units(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    unit_refs: &[&UnitDecl],
) {
    // Seed prelude units into the registry so module-local code can reference them.
    // Only pub units are seeded (private units are module-internal).
    for prelude_module in prelude {
        let module_display = prelude_module.path.to_string();
        for cu in &prelude_module.units {
            if cu.is_pub {
                // Detect cross-prelude collision before overwriting: if another
                // prelude module already seeded this unit name, emit a warning.
                // Guard: only warn for genuine cross-module pairs. Intra-module
                // duplicates (same source_module as current) are rejected earlier
                // by compile() and must not produce "declared in both 'X' and 'X'".
                if let Some(existing) = ctx.unit_registry.lookup(&cu.name)
                    && existing.source_module.as_deref() != Some(&module_display[..])
                {
                    let first_module: &str =
                        existing.source_module.as_deref().unwrap_or("<unknown>");
                    ctx.diagnostics.push(
                        Diagnostic::warning(format!(
                            "prelude unit '{}' declared in both '{}' and '{}'; last-wins",
                            cu.name, first_module, module_display
                        ))
                        .with_label(DiagnosticLabel::new(
                            SourceSpan::prelude(),
                            "cross-prelude collision",
                        )),
                    );
                }
                ctx.unit_registry
                    .seed_prelude_unit(UnitEntry::from_compiled_for_prelude(
                        cu,
                        module_display.clone(),
                    ));
            }
        }
    }

    for unit_decl in unit_refs {
        if let Some(entry) = compile_unit(unit_decl, &ctx.unit_registry, &mut ctx.diagnostics) {
            match ctx.unit_registry.register(entry) {
                Ok(()) => {
                    // Entry was registered; retrieve it to build CompiledUnit
                    let entry = ctx.unit_registry.lookup(&unit_decl.name).unwrap();
                    ctx.compiled_units.push(CompiledUnit {
                        name: entry.name.clone(),
                        is_pub: entry.is_pub,
                        dimension: entry.dimension,
                        factor: entry.factor,
                        offset: entry.offset,
                        content_hash: entry.content_hash,
                    });
                }
                Err(dup_entry) => {
                    // Duplicate unit name — find the original entry to determine provenance.
                    let original = ctx.unit_registry.lookup(&dup_entry.name).unwrap();
                    match &original.source_module {
                        Some(m) if m.starts_with("std/") => {
                            // Original is a stdlib prelude unit.
                            // Emit a two-label diagnostic: primary is the user's
                            // duplicate decl; secondary is the prelude sentinel
                            // carrying provenance text.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}' — already defined in stdlib prelude",
                                    dup_entry.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    "duplicate of stdlib unit",
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    "defined in stdlib prelude",
                                )),
                            );
                        }
                        Some(m) => {
                            // Original was seeded from a user module — name that module.
                            // Emit a two-label diagnostic: primary is the user's
                            // duplicate decl; secondary is the prelude sentinel
                            // carrying provenance text.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}' — already defined in module '{}'",
                                    dup_entry.name, m
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    format!("duplicate of unit from '{}'", m),
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    format!("defined in module '{}' prelude", m),
                                )),
                            );
                        }
                        None => {
                            // Module-local duplicate — show both source locations.
                            ctx.diagnostics.push(
                                Diagnostic::error(format!(
                                    "duplicate unit declaration '{}'",
                                    dup_entry.name
                                ))
                                .with_label(DiagnosticLabel::new(
                                    dup_entry.span,
                                    "duplicate declared here",
                                ))
                                .with_label(DiagnosticLabel::new(
                                    original.span,
                                    "first declared here",
                                )),
                            );
                        }
                    }
                }
            }
        }
    }
}
