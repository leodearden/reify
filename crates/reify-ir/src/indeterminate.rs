//! Why a constraint's verdict is `Satisfaction::Indeterminate`.
//!
//! The producer that decides an Indeterminate verdict records one of these;
//! every report renders it through [`Display`] and nowhere else. Normative
//! source: `docs/prds/v0_6/declared-intent-consumption-accounting.md` §4.3.
//!
//! - [`TransientReason`]: the verdict may resolve on another run or another
//!   evaluation surface (an input becomes defined, a kernel measures).
//! - [`StructuralReason`]: the verdict is provably run-invariant (R2:
//!   never-false-Inert). Anything unproven is `Transient`.

use std::fmt::{self, Display};

use reify_core::identity::ValueCellId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IndeterminateReason {
    Transient(TransientReason),
    Structural(StructuralReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransientReason {
    /// These cells held no value when the constraint was evaluated.
    UndefInputs { cells: Vec<ValueCellId> },
    /// Every input was defined, but the operator has no meaning for the
    /// operand kinds it met. `kinds` holds display labels and is empty when
    /// no operand was a cell reference.
    OperatorUndefinedForKinds { kinds: Vec<String> },
    /// A geometric measurement the verdict depends on was not obtained.
    MeasurementUnavailable { detail: String },
}

/// Uninhabited until a run-invariant cause can be proven: the first variant
/// arrives with DIC ε #5419, together with its proof, so no Structural reason
/// can be recorded without one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StructuralReason {}

impl Display for IndeterminateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndeterminateReason::Transient(reason) => reason.fmt(f),
            IndeterminateReason::Structural(reason) => reason.fmt(f),
        }
    }
}

impl Display for TransientReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransientReason::UndefInputs { cells } => {
                f.write_str("undefined inputs: ")?;
                write_joined(f, cells)
            }
            TransientReason::OperatorUndefinedForKinds { kinds } => {
                f.write_str("operator undefined for these operand kinds")?;
                if kinds.is_empty() {
                    return Ok(());
                }
                f.write_str(": ")?;
                write_joined(f, kinds)
            }
            TransientReason::MeasurementUnavailable { detail } => f.write_str(detail),
        }
    }
}

impl Display for StructuralReason {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

fn write_joined(f: &mut fmt::Formatter<'_>, items: &[impl Display]) -> fmt::Result {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        item.fmt(f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
