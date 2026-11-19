//! Decides which `+`/`-` and math-operation forms reject a `Direction` operand,
//! and the fix hint they carry (`docs/prds/v0_6/direction-typing.md` §5 C6).

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type};
use reify_ir::CompiledExpr;

use crate::math_signatures::MATH_OPERATION_NAMES;

/// The fix every rejected Direction form points at: convert explicitly.
pub(crate) const VECTOR_CONVERSION_HINT: &str = "write vector(d)";

/// An operand whose type is not yet known (or already poisoned) cannot be
/// adjudicated a Direction mismatch.
fn is_gradual(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Error | Type::TypeParam(_) | Type::Projection { .. }
    )
}

/// `+`/`-` with a `Direction` operand is an `ArithOperandKind` error: a sum of
/// unit vectors is not a unit vector. The fix is `vector(d)`. A gradual
/// partner is left alone (no pile-on).
pub(crate) fn add_sub_direction_reject(left: &Type, right: &Type) -> bool {
    (matches!(left, Type::Direction) || matches!(right, Type::Direction))
        && !is_gradual(left)
        && !is_gradual(right)
}

/// The hint an `ArithOperandKind` diagnostic carries for this operand pair.
pub(crate) fn arith_operand_hint(left: &Type, right: &Type) -> Option<&'static str> {
    (matches!(left, Type::Direction) || matches!(right, Type::Direction))
        .then_some(VECTOR_CONVERSION_HINT)
}

/// The only math operations that take a Direction: `normalize(D)`,
/// `vector(D)`, `dot(D, D)` and `cross(D, D)`.
fn is_accepted_direction_form(name: &str, arg_types: &[&Type]) -> bool {
    let all_directions = arg_types.iter().all(|t| matches!(t, Type::Direction));
    let arity_fits = match name {
        "normalize" | "vector" => arg_types.len() == 1,
        "dot" | "cross" => arg_types.len() == 2,
        _ => false,
    };
    arity_fits && all_directions
}

/// Diagnostic-only: pushes one `ArgTypeMismatch` Error for a math operation
/// that misuses a Direction, and never changes the call's result type.
///
/// `vector(x)` with a non-Direction `x` names the Direction it expects; any
/// other math operation with a Direction argument outside
/// [`is_accepted_direction_form`] names its argument types and carries
/// [`VECTOR_CONVERSION_HINT`].
pub(crate) fn check_math_direction_operands(
    name: &str,
    args: &[CompiledExpr],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !MATH_OPERATION_NAMES.contains(&name) {
        return;
    }
    let arg_types: Vec<&Type> = args.iter().map(|a| &a.result_type).collect();
    if arg_types.iter().any(|t| is_gradual(t)) {
        return;
    }
    if let ("vector", [actual]) = (name, arg_types.as_slice())
        && !matches!(actual, Type::Direction)
    {
        diagnostics.push(
            Diagnostic::error(format!("vector: argument expects Direction, got {actual}"))
                .with_code(DiagnosticCode::ArgTypeMismatch)
                .with_label(DiagnosticLabel::new(
                    call_span,
                    format!("expected Direction, got {actual}"),
                )),
        );
        return;
    }
    if !arg_types.iter().any(|t| matches!(t, Type::Direction))
        || is_accepted_direction_form(name, &arg_types)
    {
        return;
    }
    let listed = arg_types
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    diagnostics.push(
        Diagnostic::error(format!(
            "{name}: undefined for argument types ({listed}); {VECTOR_CONVERSION_HINT}"
        ))
        .with_code(DiagnosticCode::ArgTypeMismatch)
        .with_label(DiagnosticLabel::new(
            call_span,
            "a Direction is accepted only by normalize, vector, and dot or cross of two Directions",
        )),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::Value;

    fn dimensionless_vec3() -> Type {
        Type::vec3(Type::dimensionless_scalar())
    }

    fn arg(ty: Type) -> CompiledExpr {
        CompiledExpr::literal(Value::Undef, ty)
    }

    fn math_diagnostics(name: &str, arg_types: Vec<Type>) -> Vec<Diagnostic> {
        let args: Vec<CompiledExpr> = arg_types.into_iter().map(arg).collect();
        let mut diagnostics = Vec::new();
        check_math_direction_operands(name, &args, SourceSpan::new(0, 1), &mut diagnostics);
        diagnostics
    }

    #[test]
    fn add_sub_direction_reject_fires_on_any_direction_operand() {
        let pairs = [
            (Type::Direction, Type::Direction),
            (Type::Direction, dimensionless_vec3()),
            (dimensionless_vec3(), Type::Direction),
            (Type::Direction, Type::Int),
        ];
        for (left, right) in pairs {
            assert!(add_sub_direction_reject(&left, &right), "{left} ± {right}");
        }
    }

    #[test]
    fn add_sub_direction_reject_ignores_non_direction_and_gradual_operands() {
        let pairs = [
            (dimensionless_vec3(), dimensionless_vec3()),
            (Type::Int, Type::Int),
            (Type::Error, Type::Direction),
            (Type::TypeParam("T".into()), Type::Direction),
        ];
        for (left, right) in pairs {
            assert!(!add_sub_direction_reject(&left, &right), "{left} ± {right}");
        }
    }

    #[test]
    fn arith_operand_hint_is_direction_scoped() {
        assert_eq!(
            arith_operand_hint(&Type::Direction, &Type::Int),
            Some(VECTOR_CONVERSION_HINT)
        );
        assert_eq!(
            arith_operand_hint(&Type::Int, &Type::Direction),
            Some(VECTOR_CONVERSION_HINT)
        );
        assert_eq!(
            arith_operand_hint(&dimensionless_vec3(), &dimensionless_vec3()),
            None
        );
    }

    #[test]
    fn accepted_direction_forms_draw_no_diagnostic() {
        for (name, arg_types) in [
            ("normalize", vec![Type::Direction]),
            ("vector", vec![Type::Direction]),
            ("dot", vec![Type::Direction, Type::Direction]),
            ("cross", vec![Type::Direction, Type::Direction]),
        ] {
            assert!(math_diagnostics(name, arg_types).is_empty(), "{name}");
        }
    }

    #[test]
    fn math_check_skips_gradual_partners() {
        for partner in [Type::Error, Type::TypeParam("T".into())] {
            assert!(math_diagnostics("dot", vec![Type::Direction, partner.clone()]).is_empty());
            assert!(math_diagnostics("vector", vec![partner]).is_empty());
        }
    }

    #[test]
    fn math_check_ignores_non_math_names() {
        assert!(
            math_diagnostics("parallel", vec![Type::Direction, dimensionless_vec3()]).is_empty()
        );
        assert!(math_diagnostics("frame_at", vec![Type::Direction]).is_empty());
    }

    #[test]
    fn misused_direction_is_one_arg_type_mismatch_with_the_hint() {
        let diagnostics = math_diagnostics("dot", vec![Type::Direction, dimensionless_vec3()]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(diagnostics[0].message.starts_with("dot: "));
        assert!(diagnostics[0].message.ends_with(VECTOR_CONVERSION_HINT));
    }

    #[test]
    fn vector_of_a_non_direction_names_direction() {
        let diagnostics = math_diagnostics("vector", vec![dimensionless_vec3()]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .starts_with("vector: argument expects Direction"),
            "{diagnostics:?}"
        );
    }
}
