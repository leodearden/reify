//! Post-compilation passes that run after entities are compiled: recursion
//! detection + hash remix (step 15) and duplicate-signature / field
//! composition / purpose compilation (step 16).
//!
//! Each function takes `&mut CompilationCtx` (and `&ParsedModule` for
//! `phase_purposes`) and mutates the relevant ctx fields in place, with
//! the exception of `phase_purposes` which returns `Vec<CompiledPurpose>`
//! since purposes are not owned by `CompilationCtx`.

use std::collections::HashMap;

use reify_syntax::ParsedModule;
use reify_types::{ContentHash, Diagnostic, Type};

use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::check_field_composition_types;
use crate::scc;
use crate::termination::check_recursive_termination;
use crate::traits::compile_purpose;
use crate::types::{CompiledField, CompiledFieldSource, CompiledPurpose, TopologyTemplate};

/// Phase-12 post-compilation: detect recursive sub-component cycles via
/// DFS on the template reference graph, verify recursive structures have
/// valid termination conditions, and remix `is_recursive` into each
/// recursive template's `content_hash`.
///
/// Without the hash remix, two templates with identical raw content but
/// different recursion status would hash identically, causing incorrect
/// incremental compilation cache hits. Non-recursive templates are
/// untouched so existing cache entries remain valid for them.
pub(crate) fn phase_recursion_detection(ctx: &mut CompilationCtx) {
    // Detect recursive sub-component cycles; tag participating templates
    // with is_recursive=true and emit a warning diagnostic per cycle.
    let cyclic_sccs = scc::detect_recursive_structures(&mut ctx.templates, &mut ctx.diagnostics);

    // Verify recursive structures have valid termination conditions.
    check_recursive_termination(&ctx.templates, &cyclic_sccs, &mut ctx.diagnostics);

    // Remix is_recursive into each recursive template's content_hash.
    for template in &mut ctx.templates {
        if template.is_recursive {
            template.content_hash = template.content_hash.combine(ContentHash::of(&[1u8]));
        }
    }
}

/// Check for duplicate function signatures: same `name` + same param-type
/// sequence. Emits one `duplicate function signature: {name}({types})`
/// error diagnostic per colliding pair (after the first entry seen).
pub(crate) fn phase_dup_sig_check(ctx: &mut CompilationCtx) {
    let mut seen: HashMap<(String, Vec<Type>), usize> = HashMap::new();
    for (idx, f) in ctx.functions.iter().enumerate() {
        let key = (
            f.name.clone(),
            f.params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
        );
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
            e.insert(idx);
        } else {
            ctx.diagnostics.push(Diagnostic::error(format!(
                "duplicate function signature: {}({})",
                f.name,
                f.params
                    .iter()
                    .map(|(_, t)| format!("{}", t))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
}

/// Post-compilation pass: check field composition type compatibility for
/// composed fields. If a composed field's body references other fields,
/// verify that the codomain of the inner field matches the domain of the
/// outer field. Delegates to [`check_field_composition_types`].
pub(crate) fn phase_field_composition(ctx: &mut CompilationCtx) {
    let field_registry: HashMap<&str, &CompiledField> =
        ctx.fields.iter().map(|f| (f.name.as_str(), f)).collect();

    for field in &ctx.fields {
        if let CompiledFieldSource::Composed { expr } = &field.source {
            check_field_composition_types(expr, &field_registry, &mut ctx.diagnostics);
        }
    }
}

/// Purpose compilation pass. Compiles every `Declaration::Purpose` in
/// `parsed.declarations` against a phase-local template registry built
/// from `ctx.templates`, returning the accumulated `Vec<CompiledPurpose>`
/// to the orchestrator (purposes are not owned by `CompilationCtx` —
/// they flow straight into the assembled `CompiledModule`).
///
/// Runs after templates are fully populated so reflective schema queries
/// inside purpose bodies can resolve against `TopologyTemplate`s.
pub(crate) fn phase_purposes(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
) -> Vec<CompiledPurpose> {
    let purpose_template_registry: HashMap<String, &TopologyTemplate> = ctx
        .templates
        .iter()
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .collect();

    let mut purposes = Vec::new();
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Purpose(purpose_def) = decl {
            let compiled = compile_purpose(
                purpose_def,
                &ctx.resolution_enums,
                &ctx.resolution_functions,
                &purpose_template_registry,
                &ctx.unit_registry,
                &mut ctx.diagnostics,
            );
            purposes.push(compiled);
        }
    }
    purposes
}
