//! Phase-4 units: seed prelude units into the registry, then compile each
//! module-local unit declaration.
//!
//! Prelude seeding ([`seed_prelude_units`]) runs first so module-local code
//! can reference prelude units by name. It binds by the `crate::name_env`
//! first-wins rule and warns once per cross-module collision. Module-local
//! duplicates produce an error keyed on the original registration's
//! provenance (stdlib / user-module / module-local).

use reify_ast::UnitDecl;
use reify_core::{Diagnostic, DiagnosticLabel};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::name_env;
use crate::type_resolution::compile_unit;
use crate::types::CompiledUnit;
use crate::units::{UnitEntry, UnitRegistry};

/// Seed the `pub` unit each name binds in `prelude` into `registry`,
/// returning one collision warning per unit shadowed by another module's.
///
/// Policy: the `crate::name_env` first-wins rule — import-tier modules before
/// std modules, each tier in slice order. A duplicate within one module is not
/// reported here, since `compile()` already rejects intra-module duplicates.
pub fn seed_prelude_units(
    registry: &mut UnitRegistry,
    prelude: &[&CompiledModule],
) -> Vec<Diagnostic> {
    let bindings = name_env::bind::<CompiledUnit, _>(&[], prelude.iter().copied());
    for winner in bindings.winning_candidates() {
        let module = winner
            .origin
            .module()
            .expect("a prelude-only bind has no local candidates");
        registry.seed_prelude_unit(UnitEntry::from_compiled_for_prelude(
            winner.definition,
            module.to_string(),
        ));
    }
    bindings
        .shadowings()
        .iter()
        .filter(|shadowing| shadowing.winner.origin != shadowing.shadowed.origin)
        .map(|shadowing| name_env::collision_warning(shadowing, ""))
        .collect()
}

/// Run phase-4 (units).
///
/// Seeds the `prelude` units into `ctx.unit_registry` via
/// [`seed_prelude_units`], keeping its collision warnings. Then compiles each
/// `unit_decl` via [`compile_unit`] and registers it in `ctx.unit_registry` /
/// `ctx.compiled_units`, emitting a duplicate-unit error (specialized by
/// provenance) if a name is already taken.
pub(crate) fn phase_units(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    unit_refs: &[&UnitDecl],
) {
    let seed_warnings = seed_prelude_units(&mut ctx.unit_registry, prelude);
    ctx.diagnostics.extend(seed_warnings);

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
