//! What the GUI viewport says about the measurements it never takes (task
//! #6667). [`Engine::tessellate_snapshot`] is the producer of
//! `GuiState.tessellation_diagnostics` and runs neither the GD&T conformance
//! nor the DFM measurement pass, so each such callout is recorded Indeterminate
//! with ζ's `MeasurementUnavailable` cause and stated through ζ's
//! `ConstraintIndeterminate` Info. On-demand GUI measurement (#6898) is what
//! will replace this attribution with a verdict.

use std::collections::HashSet;

use reify_compiler::{CompiledConstraint, CompiledModule};
use reify_core::{Diagnostic, DiagnosticLabel};
use reify_ir::{IndeterminateReason, Satisfaction, Value, ValueMap};

use crate::engine_constraints::{
    UnmeasuredMeasurement, dfm_rule_spec, dfm_thickness_spec, is_geometric_conforms,
    template_instance_data, unmeasured_attribution, unmeasured_dfm_attribution,
};
use crate::{ConstraintCheckEntry, Engine};

impl Engine {
    /// Attribute every measurement the viewport surface leaves untaken.
    pub(crate) fn attribute_viewport_unmeasured(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        constraint_results: &mut Vec<ConstraintCheckEntry>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.attribute_unmeasured_gdt_conformance(module, values, constraint_results, diagnostics);
        self.attribute_unmeasured_dfm_rules(module, values, diagnostics);
    }

    /// One ζ Info per DFM rule a measurement path would measure, found as
    /// `measure_dfm_rules` finds them (module-level declarations, then
    /// instantiated values) and deduped by name across both.
    fn attribute_unmeasured_dfm_rules(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let declared: Vec<Value> = module
            .templates
            .iter()
            .map(|t| template_instance_data(t, values))
            .filter(|data| !data.fields.is_empty())
            .map(|data| Value::StructureInstance(Box::new(data)))
            .collect();
        let instantiated = values.iter().map(|(_, v)| v);
        let mut attributed: HashSet<String> = HashSet::new();
        let mut reason: Option<IndeterminateReason> = None;
        for rule in declared.iter().chain(instantiated) {
            let Some(name) = measurable_dfm_rule_name(rule) else {
                continue;
            };
            if !attributed.insert(name.clone()) {
                continue;
            }
            let reason = reason.get_or_insert_with(|| {
                self.unmeasured_indeterminate_reason(UnmeasuredMeasurement::DfmRule)
            });
            diagnostics.push(unmeasured_dfm_attribution(&name, reason));
        }
    }

    /// Each active geometric Conforms callout becomes Indeterminate with ζ's
    /// recorded cause, plus ζ's Info rendered from it.
    fn attribute_unmeasured_gdt_conformance(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        constraint_results: &mut Vec<ConstraintCheckEntry>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut reason: Option<IndeterminateReason> = None;
        for template in &module.templates {
            for c in Self::collect_active_constraints(template, values) {
                if !is_gdt_callout(c) || has_undefined_operand(c, values) {
                    continue;
                }
                let reason = reason.get_or_insert_with(|| {
                    self.unmeasured_indeterminate_reason(UnmeasuredMeasurement::GdtConformance)
                });
                match constraint_results.iter_mut().find(|e| e.id == c.id) {
                    Some(entry) => {
                        entry.set_verdict(Satisfaction::Indeterminate, Some(reason.clone()))
                    }
                    None => constraint_results.push(ConstraintCheckEntry::new(
                        c.id.clone(),
                        Some("Conforms".to_string()),
                        Satisfaction::Indeterminate,
                        Some(reason.clone()),
                    )),
                }
                let mut attribution =
                    unmeasured_attribution(&c.id, reason).with_label(DiagnosticLabel::new(
                        c.span,
                        "geometric conformance is not measured on this surface",
                    ));
                Self::labeled_diagnostics(
                    std::slice::from_mut(&mut attribution),
                    &c.id,
                    c.label.as_deref(),
                );
                diagnostics.push(attribution);
            }
        }
    }
}

/// The callout `measure_gdt_conformance` measures: an explicit `actual` and a
/// `tolerance`.
fn is_gdt_callout(c: &CompiledConstraint) -> bool {
    is_geometric_conforms(c) && c.arg_bindings.iter().any(|(n, _)| n == "tolerance")
}

/// ζ's decline: an undefined operand is the model's defect, not the surface's,
/// so the checker's own verdict and cause stand.
fn has_undefined_operand(c: &CompiledConstraint, values: &ValueMap) -> bool {
    std::iter::once(&c.expr)
        .chain(c.arg_bindings.iter().map(|(_, e)| e))
        .any(|e| reify_constraints::has_undefined_leaf(e, values))
}

/// The name of a DFM rule that a measurement path recognizes and that has a
/// geometry subject to measure: a non-empty `rule_name`, else its type name.
/// A rule without a geometry subject is measured on no surface.
fn measurable_dfm_rule_name(v: &Value) -> Option<String> {
    let Value::StructureInstance(data) = v else {
        return None;
    };
    if !matches!(
        data.fields.get("subject"),
        Some(Value::GeometryHandle { .. })
    ) {
        return None;
    }
    if dfm_rule_spec(v).is_none() && dfm_thickness_spec(v).is_none() {
        return None;
    }
    match data.fields.get("rule_name") {
        Some(Value::String(name)) if !name.is_empty() => Some(name.clone()),
        _ => Some(data.type_name.clone()),
    }
}
