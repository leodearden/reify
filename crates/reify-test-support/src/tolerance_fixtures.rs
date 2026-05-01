//! Shared fixture builders for tolerance integration tests.
//!
//! Houses the template/purpose builders used by `tolerance_combine`,
//! `tolerance_import_promise`, and future tolerance integration tests. The
//! recognition shapes these fixtures produce must stay byte-identical across
//! all test files — centralising them here ensures a single source of truth
//! and lets co-located unit tests pin each shape explicitly.

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::ValueCellKind;
    use reify_types::{
        CompiledExprKind, ConstraintNodeId, DimensionVector, Type, Value, ValueCellId,
    };

    // ── step_output_template ────────────────────────────────────────────────

    /// Pins the recognition shape that `extract_output_tolerance_bound` matches
    /// against: template name `"STEPOutput"`, a single `subject :
    /// StructureRef("Structure")` param, and a single constraint at index 0
    /// whose expression is a `RepresentationWithin(value_ref subject.self,
    /// Scalar{si=1e-6, dim=LENGTH})` user-function call.
    #[test]
    fn step_output_template_pins_step_output_recognition_shape() {
        let template = step_output_template(1e-6);

        assert_eq!(template.name, "STEPOutput");
        assert_eq!(template.value_cells.len(), 1, "exactly one value cell");

        let cell = &template.value_cells[0];
        assert_eq!(cell.id, ValueCellId::new("STEPOutput", "subject"));
        assert_eq!(
            cell.cell_type,
            Type::StructureRef("Structure".to_string()),
            "subject param type must be StructureRef(\"Structure\")"
        );
        assert!(cell.default_expr.is_none(), "subject param has no default");
        assert!(
            matches!(cell.kind, ValueCellKind::Param),
            "subject must be a Param cell"
        );

        assert_eq!(template.constraints.len(), 1, "exactly one constraint");
        let constraint = &template.constraints[0];
        assert_eq!(
            constraint.id,
            ConstraintNodeId::new("STEPOutput", 0),
            "constraint id must be (STEPOutput, 0)"
        );

        // Inspect the RepresentationWithin expression.
        let CompiledExprKind::UserFunctionCall { function_name, args } = &constraint.expr.kind else {
            panic!(
                "constraint expr must be a UserFunctionCall, got {:?}",
                constraint.expr.kind
            );
        };
        assert_eq!(function_name, "RepresentationWithin");
        assert_eq!(args.len(), 2, "RepresentationWithin must have 2 args");
        assert_eq!(
            constraint.expr.result_type,
            Type::Bool,
            "RepresentationWithin must have result_type Bool"
        );

        // arg[0]: value_ref subject.self : StructureRef("Structure")
        let arg0 = &args[0];
        assert!(
            matches!(&arg0.kind, CompiledExprKind::ValueRef(id) if *id == ValueCellId::new("subject", "self")),
            "arg[0] must be ValueRef(subject.self), got {:?}",
            arg0.kind
        );
        assert_eq!(
            arg0.result_type,
            Type::StructureRef("Structure".to_string()),
            "arg[0] result_type must be StructureRef(\"Structure\")"
        );

        // arg[1]: Scalar literal si=1e-6 dim=LENGTH
        let arg1 = &args[1];
        let CompiledExprKind::Literal(Value::Scalar { si_value, dimension }) = &arg1.kind else {
            panic!("arg[1] must be a Scalar literal, got {:?}", arg1.kind);
        };
        assert!(
            (si_value - 1e-6).abs() < f64::EPSILON,
            "arg[1] si_value must be 1e-6, got {si_value}"
        );
        assert_eq!(*dimension, DimensionVector::LENGTH, "arg[1] dimension must be LENGTH");
        assert_eq!(
            arg1.result_type,
            Type::Scalar { dimension: DimensionVector::LENGTH },
        );
    }

    /// Pins that `step_output_template_without_rep_within` builds an `STEPOutput`
    /// template whose index-0 constraint body is a `Bool(true)` literal
    /// (not a `RepresentationWithin`), so `extract_output_tolerance_bound`
    /// returns `None` for it.
    #[test]
    fn step_output_template_without_rep_within_pins_bool_body_shape() {
        let template = step_output_template_without_rep_within();

        assert_eq!(template.name, "STEPOutput");
        assert_eq!(template.value_cells.len(), 1);
        assert_eq!(template.value_cells[0].id, ValueCellId::new("STEPOutput", "subject"));
        assert_eq!(template.constraints.len(), 1);
        assert_eq!(template.constraints[0].id, ConstraintNodeId::new("STEPOutput", 0));

        // Body must be Bool(true) literal.
        assert!(
            matches!(
                &template.constraints[0].expr.kind,
                CompiledExprKind::Literal(Value::Bool(true))
            ),
            "constraint expr must be Bool(true) literal, got {:?}",
            template.constraints[0].expr.kind
        );
        assert_eq!(template.constraints[0].expr.result_type, Type::Bool);
    }

    /// Proves that `step_output_template_with_body` just inserts the caller-
    /// supplied expression into the standard STEPOutput skeleton (name,
    /// subject param, index-0 constraint). A sentinel `Int(7)` literal
    /// round-trips verbatim.
    #[test]
    fn step_output_template_with_body_passes_through_body() {
        use reify_types::CompiledExpr;
        let sentinel = CompiledExpr::literal(Value::Int(7), Type::Int);
        let template = step_output_template_with_body(sentinel.clone());

        assert_eq!(template.name, "STEPOutput");
        assert_eq!(template.value_cells.len(), 1);
        assert_eq!(template.constraints.len(), 1);
        assert_eq!(template.constraints[0].id, ConstraintNodeId::new("STEPOutput", 0));
        assert!(
            matches!(
                &template.constraints[0].expr.kind,
                CompiledExprKind::Literal(Value::Int(7))
            ),
            "sentinel Int(7) must round-trip verbatim through step_output_template_with_body"
        );
        assert_eq!(template.constraints[0].expr.content_hash, sentinel.content_hash);
    }

    // ── step_input_template ─────────────────────────────────────────────────

    /// Pins the recognition shape that `extract_input_tolerance_promise`
    /// matches against: template name `"STEPInput"`, a single `tolerance :
    /// Length` param whose default expression is `Scalar{si=50e-6, dim=LENGTH}`.
    #[test]
    fn step_input_template_pins_step_input_recognition_shape() {
        let template = step_input_template(50e-6);

        assert_eq!(template.name, "STEPInput");
        assert_eq!(template.value_cells.len(), 1, "exactly one value cell");

        let cell = &template.value_cells[0];
        assert_eq!(cell.id, ValueCellId::new("STEPInput", "tolerance"));
        assert_eq!(
            cell.cell_type,
            Type::Scalar { dimension: DimensionVector::LENGTH },
            "tolerance param type must be Length scalar"
        );
        assert!(
            matches!(cell.kind, ValueCellKind::Param),
            "tolerance must be a Param cell"
        );

        let default_expr = cell
            .default_expr
            .as_ref()
            .expect("tolerance param must have a default expression");
        let CompiledExprKind::Literal(Value::Scalar { si_value, dimension }) =
            &default_expr.kind
        else {
            panic!(
                "default expr must be a Scalar literal, got {:?}",
                default_expr.kind
            );
        };
        assert!(
            (si_value - 50e-6).abs() < f64::EPSILON,
            "default si_value must be 50e-6, got {si_value}"
        );
        assert_eq!(*dimension, DimensionVector::LENGTH);
        assert_eq!(
            default_expr.result_type,
            Type::Scalar { dimension: DimensionVector::LENGTH }
        );

        // No constraints on STEPInput.
        assert_eq!(template.constraints.len(), 0, "STEPInput has no constraints");
    }

    // ── manufacturing_purpose ───────────────────────────────────────────────

    /// Pins the recognition shape that `extract_output_tolerance_bound` (via
    /// the purpose side) matches against: purpose name as supplied, single
    /// `subject : Structure` param, single constraint at `(subject, 0)` whose
    /// expression is `RepresentationWithin(value_ref subject.self :
    /// StructureRef("Structure"), Scalar{si=1e-6, dim=LENGTH})`.
    #[test]
    fn manufacturing_purpose_pins_purpose_recognition_shape() {
        let purpose = manufacturing_purpose("manufacturing", 1e-6);

        assert_eq!(purpose.name, "manufacturing");
        assert_eq!(purpose.params.len(), 1, "single param");
        assert_eq!(purpose.params[0].name, "subject");
        assert_eq!(purpose.params[0].entity_kind, "Structure");

        assert_eq!(purpose.constraints.len(), 1, "single constraint");
        let constraint = &purpose.constraints[0];
        assert_eq!(
            constraint.id,
            ConstraintNodeId::new("subject", 0),
            "constraint id must be (subject, 0)"
        );

        let CompiledExprKind::UserFunctionCall { function_name, args } = &constraint.expr.kind else {
            panic!(
                "constraint expr must be a UserFunctionCall, got {:?}",
                constraint.expr.kind
            );
        };
        assert_eq!(function_name, "RepresentationWithin");
        assert_eq!(args.len(), 2);
        assert_eq!(constraint.expr.result_type, Type::Bool);

        // arg[0]: value_ref subject.self : StructureRef("Structure")
        assert!(
            matches!(&args[0].kind, CompiledExprKind::ValueRef(id) if *id == ValueCellId::new("subject", "self")),
            "arg[0] must be ValueRef(subject.self)"
        );
        assert_eq!(args[0].result_type, Type::StructureRef("Structure".to_string()));

        // arg[1]: Scalar literal si=1e-6 dim=LENGTH
        let CompiledExprKind::Literal(Value::Scalar { si_value, dimension }) = &args[1].kind else {
            panic!("arg[1] must be a Scalar literal, got {:?}", args[1].kind);
        };
        assert!(
            (si_value - 1e-6).abs() < f64::EPSILON,
            "arg[1] si_value must be 1e-6, got {si_value}"
        );
        assert_eq!(*dimension, DimensionVector::LENGTH);
        assert_eq!(args[1].result_type, Type::Scalar { dimension: DimensionVector::LENGTH });
    }

    // ── my_design_template ──────────────────────────────────────────────────

    /// Pins that `my_design_template` builds a `"MyDesign"` template with
    /// exactly one `thickness : Real` param and no constraints.
    #[test]
    fn my_design_template_pins_thickness_param_shape() {
        let template = my_design_template();

        assert_eq!(template.name, "MyDesign");
        assert_eq!(template.value_cells.len(), 1, "exactly one value cell");

        let cell = &template.value_cells[0];
        assert_eq!(cell.id, ValueCellId::new("MyDesign", "thickness"));
        assert_eq!(cell.cell_type, Type::Real, "thickness param type must be Real");
        assert!(cell.default_expr.is_none(), "thickness has no default");
        assert!(matches!(cell.kind, ValueCellKind::Param));

        assert_eq!(template.constraints.len(), 0, "MyDesign has no constraints");
    }
}
