#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::identity::ValueCellId;

    fn undef_inputs() -> TransientReason {
        TransientReason::UndefInputs {
            cells: vec![
                ValueCellId::new("Bracket", "thickness"),
                ValueCellId::new("Bracket", "width"),
            ],
        }
    }

    fn kinds_absent() -> TransientReason {
        TransientReason::OperatorUndefinedForKinds { kinds: vec![] }
    }

    fn kinds_named() -> TransientReason {
        TransientReason::OperatorUndefinedForKinds {
            kinds: vec!["Enum<Fit>".into(), "Scalar<m>".into()],
        }
    }

    fn measurement_unavailable() -> TransientReason {
        TransientReason::MeasurementUnavailable {
            detail: "no geometry kernel available".into(),
        }
    }

    #[test]
    fn undef_inputs_names_each_cell_in_order() {
        assert_eq!(
            undef_inputs().to_string(),
            "undefined inputs: Bracket.thickness, Bracket.width"
        );
    }

    #[test]
    fn operator_undefined_without_kinds_has_no_trailing_colon() {
        assert_eq!(
            kinds_absent().to_string(),
            "operator undefined for these operand kinds"
        );
    }

    #[test]
    fn operator_undefined_with_kinds_lists_them() {
        assert_eq!(
            kinds_named().to_string(),
            "operator undefined for these operand kinds: Enum<Fit>, Scalar<m>"
        );
    }

    #[test]
    fn measurement_unavailable_renders_its_detail_verbatim() {
        assert_eq!(
            measurement_unavailable().to_string(),
            "no geometry kernel available"
        );
    }

    #[test]
    fn transient_reason_renders_identically_to_its_payload() {
        for transient in [
            undef_inputs(),
            kinds_absent(),
            kinds_named(),
            measurement_unavailable(),
        ] {
            assert_eq!(
                IndeterminateReason::Transient(transient.clone()).to_string(),
                transient.to_string()
            );
        }
    }
}
