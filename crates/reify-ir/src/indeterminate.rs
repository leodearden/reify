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

use crate::expr::{BinOp, UnOp};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IndeterminateReason {
    Transient(TransientReason),
    Structural(StructuralReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransientReason {
    /// These cells held no value when the constraint was evaluated.
    UndefInputs { cells: Vec<ValueCellId> },
    /// Every leaf was defined, but the undefined result was not traced to a
    /// pure refusal: it arose in geometry, a user or `@optimized` call, a
    /// selector, or over a non-data operand. `kinds` holds the cell
    /// operands' display labels and is empty when no operand was a cell
    /// reference. The rendered wording is legacy; #10034 owns retiring it.
    OperatorUndefinedForKinds { kinds: Vec<String> },
    /// A geometric measurement the verdict depends on was not obtained.
    MeasurementUnavailable { detail: String },
    /// Every operand was a defined plain-data value and this pure operation
    /// itself returned no value, so the same inputs refuse on every run;
    /// `operands` are positional display labels.
    OperationRefused {
        operation: RefusedOperation,
        operands: Vec<String>,
    },
}

/// The pure operation behind a [`TransientReason::OperationRefused`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefusedOperation {
    Binary(BinOp),
    Unary(UnOp),
    Builtin(String),
}

/// Uninhabited until a run-invariant cause can be proven: the first variant
/// arrives with DIC ε #5419, together with its proof, so no Structural reason
/// can be recorded without one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StructuralReason {}

impl IndeterminateReason {
    /// See [`TransientReason::is_unexpected`].
    pub fn is_unexpected(&self) -> bool {
        match self {
            IndeterminateReason::Transient(reason) => reason.is_unexpected(),
            IndeterminateReason::Structural(reason) => match *reason {},
        }
    }
}

impl TransientReason {
    /// Whether this cause is *unexpected* under INV-SF-4's doctrine
    /// (`docs/legibility/design-invariants.md`): plain `reify check` fails on
    /// an unexpected cause, and anything not proven unexpected is tolerated.
    pub fn is_unexpected(&self) -> bool {
        match self {
            TransientReason::OperationRefused { .. } => true,
            TransientReason::UndefInputs { .. }
            | TransientReason::OperatorUndefinedForKinds { .. }
            | TransientReason::MeasurementUnavailable { .. } => false,
        }
    }
}

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
            TransientReason::OperationRefused {
                operation,
                operands,
            } => {
                write!(f, "{operation} is undefined")?;
                if operands.is_empty() {
                    return Ok(());
                }
                f.write_str(" for these operands: ")?;
                write_joined(f, operands)
            }
        }
    }
}

impl Display for RefusedOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            RefusedOperation::Binary(op) => binary_symbol(*op),
            RefusedOperation::Unary(op) => unary_symbol(*op),
            RefusedOperation::Builtin(name) => name.as_str(),
        };
        write!(f, "`{name}`")
    }
}

fn binary_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "^",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Implies => "implies",
    }
}

fn unary_symbol(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "not",
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

    fn refused(operation: RefusedOperation, operands: &[&str]) -> TransientReason {
        TransientReason::OperationRefused {
            operation,
            operands: operands.iter().map(|label| label.to_string()).collect(),
        }
    }

    fn division_refused() -> TransientReason {
        refused(RefusedOperation::Binary(BinOp::Div), &["Real", "Real"])
    }

    fn sqrt_refused() -> TransientReason {
        refused(RefusedOperation::Builtin("sqrt".into()), &["Real"])
    }

    fn negation_refused() -> TransientReason {
        refused(RefusedOperation::Unary(UnOp::Neg), &["String"])
    }

    fn builtin_refused_without_operands() -> TransientReason {
        refused(RefusedOperation::Builtin("f".into()), &[])
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
            division_refused(),
            sqrt_refused(),
            negation_refused(),
            builtin_refused_without_operands(),
        ] {
            assert_eq!(
                IndeterminateReason::Transient(transient.clone()).to_string(),
                transient.to_string()
            );
        }
    }

    #[test]
    fn operation_refused_names_the_operation_and_its_operands_in_order() {
        assert_eq!(
            division_refused().to_string(),
            "`/` is undefined for these operands: Real, Real"
        );
        assert_eq!(
            sqrt_refused().to_string(),
            "`sqrt` is undefined for these operands: Real"
        );
        assert_eq!(
            negation_refused().to_string(),
            "`-` is undefined for these operands: String"
        );
    }

    #[test]
    fn operation_refused_without_operands_has_no_trailing_colon() {
        assert_eq!(
            builtin_refused_without_operands().to_string(),
            "`f` is undefined"
        );
    }

    #[test]
    fn every_operator_renders_its_source_symbol() {
        let expected = [
            (RefusedOperation::Binary(BinOp::Add), "+"),
            (RefusedOperation::Binary(BinOp::Sub), "-"),
            (RefusedOperation::Binary(BinOp::Mul), "*"),
            (RefusedOperation::Binary(BinOp::Div), "/"),
            (RefusedOperation::Binary(BinOp::Mod), "%"),
            (RefusedOperation::Binary(BinOp::Pow), "^"),
            (RefusedOperation::Binary(BinOp::Eq), "=="),
            (RefusedOperation::Binary(BinOp::Ne), "!="),
            (RefusedOperation::Binary(BinOp::Lt), "<"),
            (RefusedOperation::Binary(BinOp::Le), "<="),
            (RefusedOperation::Binary(BinOp::Gt), ">"),
            (RefusedOperation::Binary(BinOp::Ge), ">="),
            (RefusedOperation::Binary(BinOp::And), "and"),
            (RefusedOperation::Binary(BinOp::Or), "or"),
            (RefusedOperation::Binary(BinOp::Implies), "implies"),
            (RefusedOperation::Unary(UnOp::Neg), "-"),
            (RefusedOperation::Unary(UnOp::Not), "not"),
        ];
        for (operation, symbol) in expected {
            assert_eq!(
                refused(operation, &["Bool"]).to_string(),
                format!("`{symbol}` is undefined for these operands: Bool")
            );
        }
    }

    #[test]
    fn only_an_operation_refusal_is_unexpected() {
        let expected = [
            (undef_inputs(), false),
            (kinds_absent(), false),
            (kinds_named(), false),
            (measurement_unavailable(), false),
            (division_refused(), true),
            (sqrt_refused(), true),
            (negation_refused(), true),
            (builtin_refused_without_operands(), true),
        ];
        for (transient, unexpected) in expected {
            assert_eq!(transient.is_unexpected(), unexpected, "{transient:?}");
            assert_eq!(
                IndeterminateReason::Transient(transient.clone()).is_unexpected(),
                transient.is_unexpected(),
                "{transient:?}"
            );
        }
    }
}
