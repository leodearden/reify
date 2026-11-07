//! What the GUI viewport says about the measurements it never takes (task
//! #6667). [`Engine::tessellate_snapshot`] is the producer of
//! `GuiState.tessellation_diagnostics` and runs neither the GD&T conformance
//! nor the DFM measurement pass, so each such callout or rule is stated through
//! ζ's `ConstraintIndeterminate` Info, rendered from ζ's `MeasurementUnavailable`
//! cause. The panel's verdict is `Engine::check`'s; on-demand GUI measurement
//! (#6898) is what will replace this attribution with a verdict.

use std::collections::HashSet;

use reify_compiler::{CompiledConstraint, CompiledModule};
use reify_core::{Diagnostic, DiagnosticLabel, RealizationNodeId};
use reify_ir::{Satisfaction, Value, ValueMap};

use crate::engine_constraints::{
    declares_geometric_conforms, dfm_rule_candidates, dfm_rule_spec, dfm_thickness_spec,
    gdt_callout_operands,
};
use crate::unmeasured_attribution::{
    UnmeasuredMeasurement, unmeasured_constraint_attribution, unmeasured_dfm_attribution,
};
use crate::{ConstraintCheckEntry, Engine};

impl Engine {
    /// Attribute every measurement the viewport surface leaves untaken.
    pub(crate) fn attribute_viewport_unmeasured(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        constraint_results: &mut [ConstraintCheckEntry],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.attribute_unmeasured_gdt_conformance(module, values, constraint_results, diagnostics);
        self.attribute_unmeasured_dfm_rules(module, values, diagnostics);
    }

    /// One ζ Info per DFM rule a measurement path would measure, discovered as
    /// `measure_dfm_rules` discovers them.
    fn attribute_unmeasured_dfm_rules(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut attributed: HashSet<MeasurableDfmRule> = HashSet::new();
        for candidate in dfm_rule_candidates(module, values, None) {
            let Some(rule) = measurable_dfm_rule(&candidate) else {
                continue;
            };
            if attributed.contains(&rule) {
                continue;
            }
            let reason = self.unmeasured_indeterminate_reason(UnmeasuredMeasurement::DfmRule);
            diagnostics.push(unmeasured_dfm_attribution(&rule.name, &reason));
            attributed.insert(rule);
        }
    }

    /// Each active geometric Conforms callout is recorded Indeterminate with
    /// ζ's cause, so this result never carries the scalar predicate's verdict
    /// for a callout it did not measure (C1), plus ζ's Info rendered from it.
    fn attribute_unmeasured_gdt_conformance(
        &self,
        module: &CompiledModule,
        values: &ValueMap,
        constraint_results: &mut [ConstraintCheckEntry],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if !declares_geometric_conforms(module) {
            return;
        }
        for template in &module.templates {
            for c in Self::collect_active_constraints(template, values) {
                if gdt_callout_operands(c).is_none() || has_undefined_operand(c, values) {
                    continue;
                }
                let reason =
                    self.unmeasured_indeterminate_reason(UnmeasuredMeasurement::GdtConformance);
                let entry = constraint_results.iter_mut().find(|e| e.id == c.id);
                debug_assert!(
                    entry.is_some(),
                    "check_constraints_against_templates emits one entry per active constraint"
                );
                if let Some(entry) = entry {
                    entry.set_verdict(Satisfaction::Indeterminate, Some(reason.clone()));
                }
                let mut attribution = unmeasured_constraint_attribution(&c.id, &reason).with_label(
                    DiagnosticLabel::new(
                        c.span,
                        "geometric conformance is not measured on this surface",
                    ),
                );
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

/// ζ's decline: an undefined operand is the model's defect, not the surface's,
/// so the checker's own verdict and cause stand.
fn has_undefined_operand(c: &CompiledConstraint, values: &ValueMap) -> bool {
    std::iter::once(&c.expr)
        .chain(c.arg_bindings.iter().map(|(_, e)| e))
        .any(|e| reify_constraints::has_undefined_leaf(e, values))
}

/// A DFM rule as attributed: its name (a non-empty `rule_name`, else its type
/// name) and its geometry subject. `measure_dfm_rules` measures per subject, so
/// a rule is attributed once per subject however many discovery sources reach
/// it, and same-named rules on two subjects are two.
#[derive(PartialEq, Eq, Hash)]
struct MeasurableDfmRule {
    name: String,
    subject: RealizationNodeId,
}

/// The rule `v` declares, if a measurement path recognizes it and it has a
/// geometry subject to measure: a rule without one is measured on no surface.
fn measurable_dfm_rule(v: &Value) -> Option<MeasurableDfmRule> {
    let Value::StructureInstance(data) = v else {
        return None;
    };
    let Some(Value::GeometryHandle {
        realization_ref, ..
    }) = data.fields.get("subject")
    else {
        return None;
    };
    if dfm_rule_spec(v).is_none() && dfm_thickness_spec(v).is_none() {
        return None;
    }
    let name = match data.fields.get("rule_name") {
        Some(Value::String(name)) if !name.is_empty() => name.clone(),
        _ => data.type_name.clone(),
    };
    Some(MeasurableDfmRule {
        name,
        subject: realization_ref.clone(),
    })
}
