//! What the GUI viewport says about the measurements it never takes (task
//! #6667). [`Engine::tessellate_snapshot`] is the producer of
//! `GuiState.tessellation_diagnostics` and runs neither the GD&T conformance
//! nor the DFM measurement pass, so each such callout is recorded Indeterminate
//! with ζ's `MeasurementUnavailable` cause and stated through ζ's
//! `ConstraintIndeterminate` Info. On-demand GUI measurement (#6898) is what
//! will replace this attribution with a verdict.

use reify_compiler::{CompiledConstraint, CompiledModule};
use reify_core::{Diagnostic, DiagnosticLabel};
use reify_ir::{IndeterminateReason, Satisfaction, ValueMap};

use crate::engine_constraints::{
    UnmeasuredMeasurement, is_geometric_conforms, unmeasured_attribution,
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
                if !is_gdt_callout(c) {
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
