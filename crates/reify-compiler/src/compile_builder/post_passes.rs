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
use crate::functions::{
    check_field_composition_types, collect_composed_field_dependencies,
};
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
    let recursion_tag = ContentHash::of_str("is_recursive");
    for template in &mut ctx.templates {
        if template.is_recursive {
            template.content_hash = template.content_hash.combine(recursion_tag);
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

/// Post-compilation pass: for each composed field, inject the
/// `__field.<name>` cell IDs of every other field referenced inside its
/// compiled lambda body into the lambda's `captures` Vec. This surfaces
/// field-to-field dependencies through the existing
/// `Lambda { captures, .. }` arm of `collect_value_refs_inner`, so
/// `extract_dependency_trace` and the reverse-dependency index pick them
/// up without any new traversal mode.
///
/// Self-references are excluded by removing the outer field's name from
/// the registry passed to [`collect_composed_field_dependencies`] for
/// each iteration. Existing entries in `captures` (from lambda-time
/// scope analysis) are preserved; only missing field-cell deps are added.
///
/// Runs after `phase_field_composition` so the field registry shape is
/// identical and any future field-related post-pass can reuse the
/// pattern.
pub(crate) fn phase_augment_composed_captures(ctx: &mut CompilationCtx) {
    // First, collect all field names so we can build a per-iteration
    // registry that excludes the outer field. We snapshot the names into
    // owned Strings to avoid borrowing `ctx.fields` while we mutate it.
    let field_names: Vec<String> = ctx.fields.iter().map(|f| f.name.clone()).collect();

    // Snapshot read-only field references keyed by name, used to rebuild a
    // registry for each composed field with the outer name omitted. Stored
    // as `(name, Box::leak-style)` is unnecessary — we re-collect against
    // `&ctx.fields` after the mutating loop completes for each field, but
    // because we mutate `ctx.fields[i].source.expr.kind.captures` and not
    // structural fields, we can hold a parallel index of (i, deps) computed
    // before the mutation. Simpler: pre-compute `(field_idx, deps_to_add)`
    // pairs in a read-only pass, then merge them in a separate mutating
    // pass. This avoids any aliased borrow.
    let mut deps_to_add: Vec<(usize, Vec<reify_types::ValueCellId>)> = Vec::new();

    for (idx, field) in ctx.fields.iter().enumerate() {
        if let CompiledFieldSource::Composed { expr } = &field.source {
            // Build a registry mapping every *other* field's name → field ref.
            // Excluding the outer field ensures recursive composed fields
            // (e.g. `composed { |p| f3(...) }` inside f3) do not produce a
            // self-capture entry.
            let mut registry: HashMap<&str, &CompiledField> = HashMap::new();
            for other in &ctx.fields {
                if other.name != field.name {
                    registry.insert(other.name.as_str(), other);
                }
            }
            // The body lives inside a Lambda; walking the outer expr is fine
            // because `walk` recurses into Lambda bodies and will surface
            // FunctionCall nodes referencing fields. This matches the
            // traversal `check_field_composition_types` already performs.
            let deps = collect_composed_field_dependencies(expr, &registry);
            deps_to_add.push((idx, deps));
        }
    }

    // Suppress dead_code warning: `field_names` is reserved for a future
    // diagnostic-emission path and is not currently consumed.
    let _ = field_names;

    // Mutating pass: merge deps into each composed field's lambda captures.
    for (idx, new_caps) in deps_to_add {
        let field = &mut ctx.fields[idx];
        if let CompiledFieldSource::Composed { expr } = &mut field.source
            && let reify_types::CompiledExprKind::Lambda { captures, .. } = &mut expr.kind
        {
            for cap in new_caps {
                if !captures.contains(&cap) {
                    captures.push(cap);
                }
            }
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
