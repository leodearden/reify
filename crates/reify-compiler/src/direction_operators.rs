//! The static `Direction` operator policy (`docs/prds/v0_6/direction-typing.md`
//! §5 C5/C6): which `+`/`-` and math-operation forms reject a `Direction`
//! operand, the "write vector(d)" hint every binop `ArithOperandKind`
//! diagnostic carries when an operand is a `Direction`, and the argument check
//! of the `vector(d)` conversion.

use reify_core::direction::DIRECTION_MIGRATION_HINT;
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

/// Besides `vector(D)`, the only math operations that take a Direction:
/// `normalize(D)`, `dot(D, D)` and `cross(D, D)`.
fn is_accepted_direction_form(name: &str, arg_types: &[&Type]) -> bool {
    let all_directions = arg_types.iter().all(|t| matches!(t, Type::Direction));
    let arity_fits = match name {
        "normalize" => arg_types.len() == 1,
        "dot" | "cross" => arg_types.len() == 2,
        _ => false,
    };
    arity_fits && all_directions
}

/// Diagnostic-only: pushes one `ArgTypeMismatch` Error for a math operation
/// that misuses a Direction, and never changes the call's result type.
///
/// `vector` is judged by [`check_vector_arguments`] alone. Any other math
/// operation with a Direction argument outside [`is_accepted_direction_form`]
/// names its argument types and carries [`VECTOR_CONVERSION_HINT`].
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
    if name == "vector" {
        check_vector_arguments(&arg_types, call_span, diagnostics);
        return;
    }
    if !arg_types.iter().any(|t| matches!(t, Type::Direction))
        || is_accepted_direction_form(name, &arg_types)
    {
        return;
    }
    push_once(
        diagnostics,
        Diagnostic::error(format!(
            "{name}: undefined for argument types ({}); {VECTOR_CONVERSION_HINT}",
            listed(&arg_types)
        ))
        .with_code(DiagnosticCode::ArgTypeMismatch)
        .with_label(DiagnosticLabel::new(
            call_span,
            "a Direction is accepted only by normalize, vector, and dot or cross of two Directions",
        )),
    );
}

/// `vector(d: Direction)` (C5) takes exactly one Direction.
///
/// A single wrong argument reads as σ's `ExpectedArg::Direction` slot
/// rejection in `builtin_signatures` reads, C7 hint included, because that
/// slot table is where this check belongs; it stays here only while
/// `vector` has no row there. The one deliberate difference is gradualism: a
/// `Projection` may still resolve to a Direction, so it is skipped
/// ([`is_gradual`]) where σ's arm rejects it. A wrong arity draws no hint:
/// the conversion hint would name the very call being made.
fn check_vector_arguments(
    arg_types: &[&Type],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (message, label) = match arg_types {
        [Type::Direction] => return,
        [actual] => {
            let hint = matches!(actual, Type::Vector { n: 3, .. })
                .then(|| format!("; {DIRECTION_MIGRATION_HINT}"))
                .unwrap_or_default();
            (
                format!("vector: d argument expects Direction, got {actual}{hint}"),
                format!("expected Direction, got {actual}"),
            )
        }
        _ => (
            format!(
                "vector: expects one Direction argument, got ({})",
                listed(arg_types)
            ),
            "expected one Direction argument".to_string(),
        ),
    };
    push_once(
        diagnostics,
        Diagnostic::error(message)
            .with_code(DiagnosticCode::ArgTypeMismatch)
            .with_label(DiagnosticLabel::new(call_span, label)),
    );
}

fn listed(arg_types: &[&Type]) -> String {
    arg_types
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A call nested in a geometry argument is compiled more than once, so an
/// exact repeat (code, span and message, the key σ's
/// `builtin_signatures::emit_mismatch` drops on) is not pushed again.
fn push_once(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    let span = |d: &Diagnostic| d.labels.first().map(|label| label.span);
    let repeated = diagnostics.iter().any(|existing| {
        existing.code == diagnostic.code
            && existing.message == diagnostic.message
            && span(existing) == span(&diagnostic)
    });
    if !repeated {
        diagnostics.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::Value;

    fn dimensionless_vec3() -> Type {
        Type::vec3(Type::dimensionless_scalar())
    }

    fn projection() -> Type {
        Type::Projection {
            base: Box::new(Type::TypeParam("T".into())),
            member: "Dir".into(),
        }
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
            (projection(), Type::Direction),
            (Type::Direction, projection()),
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
        for partner in [Type::Error, Type::TypeParam("T".into()), projection()] {
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
        for (actual, hint) in [
            (dimensionless_vec3(), Some(DIRECTION_MIGRATION_HINT)),
            (Type::Int, None),
        ] {
            let diagnostics = math_diagnostics("vector", vec![actual.clone()]);
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            let message = &diagnostics[0].message;
            assert_eq!(diagnostics[0].code, Some(DiagnosticCode::ArgTypeMismatch));
            let base = format!("vector: d argument expects Direction, got {actual}");
            match hint {
                Some(hint) => assert_eq!(message, &format!("{base}; {hint}")),
                None => assert_eq!(message, &base),
            }
        }
    }

    #[test]
    fn a_repeated_walk_of_one_call_reports_once() {
        let mut diagnostics = Vec::new();
        for (name, ty) in [
            ("magnitude", Type::Direction),
            ("magnitude", Type::Direction),
            ("abs", Type::Direction),
            ("vector", Type::Int),
            ("vector", Type::Int),
        ] {
            let span = SourceSpan::new(0, 1);
            check_math_direction_operands(name, &[arg(ty)], span, &mut diagnostics);
        }
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages.len(),
            3,
            "only exact repeats are dropped: {messages:?}"
        );
    }

    #[test]
    fn vector_at_the_wrong_arity_is_one_unhinted_mismatch() {
        for arg_types in [
            vec![],
            vec![Type::Direction, Type::Direction],
            vec![dimensionless_vec3(), Type::Int],
        ] {
            let diagnostics = math_diagnostics("vector", arg_types.clone());
            assert_eq!(diagnostics.len(), 1, "{arg_types:?}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code, Some(DiagnosticCode::ArgTypeMismatch));
            let message = &diagnostics[0].message;
            assert!(
                message.starts_with("vector: expects one Direction argument, got ("),
                "{message}"
            );
            assert!(!message.contains(VECTOR_CONVERSION_HINT), "{message}");
        }
    }
}
