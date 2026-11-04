//! Phase-9 fields: compile every `field def` declaration.
//!
//! Each field resolves against the merged `resolution_enums` and
//! `resolution_functions` tables populated by earlier phases, against
//! `ctx.alias_registry`, and against the module's structure-name and trait-name
//! sets. The trait set arrives as a parameter because `phase_traits` moves
//! `ctx.resolution_trait_names` out and returns it, mirroring
//! `phase_constraint_defs` and `phase_entities`. No cross-field dependency
//! ordering is needed — the subsequent field composition check runs as a
//! post-pass.

use std::collections::HashSet;

use reify_ast::FieldDef;

use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::compile_field;

/// Run phase-9 (fields). Compiles each `field_def` into a `CompiledField`
/// pushed onto `ctx.fields`.
pub(crate) fn phase_fields(
    ctx: &mut CompilationCtx,
    trait_names: &HashSet<String>,
    field_refs: &[&FieldDef],
) {
    for field_def in field_refs {
        let compiled = compile_field(
            field_def,
            &ctx.resolution_enums,
            &ctx.resolution_functions,
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            trait_names,
            &mut ctx.diagnostics,
        );
        ctx.fields.push(compiled);
    }
}
