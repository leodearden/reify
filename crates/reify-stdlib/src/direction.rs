//! The `dir(x, y, z)` / `dir(v)` builtin (docs/prds/v0_6/direction-typing.md C4/C11).

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector};
use reify_ir::{DirectionFault, UnitVec3, Value};

use crate::helpers::tensor_components_f64;

/// Why a `dir` call yields no direction. Judged in order: shape, then
/// dimension, then degeneracy.
enum DirFault {
    /// Wrong arity or argument kind; left undiagnosed so an upstream `undef`
    /// is never misattributed to units or degeneracy.
    Shape,
    NotDimensionless(DimensionVector),
    Unnormalisable(DirectionFault),
}

/// The one judgement both [`eval_direction`] and [`diagnose`] read, so the
/// value gate and the diagnostic cannot drift.
fn classify_dir_args(args: &[Value]) -> Result<UnitVec3, DirFault> {
    match args {
        [v] => classify_one(v),
        [x, y, z] => classify_components(&[x, y, z]),
        _ => Err(DirFault::Shape),
    }
}

fn classify_one(v: &Value) -> Result<UnitVec3, DirFault> {
    match v {
        Value::Direction(u) => Ok(*u),
        Value::Vector(_) | Value::Tensor(_) => {
            let (comps, dim) = tensor_components_f64(v).ok_or(DirFault::Shape)?;
            let [x, y, z] = <[f64; 3]>::try_from(comps).map_err(|_| DirFault::Shape)?;
            if dim != DimensionVector::DIMENSIONLESS {
                return Err(DirFault::NotDimensionless(dim));
            }
            unit(x, y, z)
        }
        _ => Err(DirFault::Shape),
    }
}

/// Each component's dimension is judged on its own: a shared-dimension
/// reading would turn `dir(1mm, 0, 0)` into a silent shape fault.
fn classify_components(args: &[&Value; 3]) -> Result<UnitVec3, DirFault> {
    let [x, y, z] = args.map(Value::as_f64);
    let (Some(x), Some(y), Some(z)) = (x, y, z) else {
        return Err(DirFault::Shape);
    };
    if let Some(dim) = args
        .iter()
        .map(|a| a.dimension())
        .find(|d| *d != DimensionVector::DIMENSIONLESS)
    {
        return Err(DirFault::NotDimensionless(dim));
    }
    unit(x, y, z)
}

fn unit(x: f64, y: f64, z: f64) -> Result<UnitVec3, DirFault> {
    UnitVec3::from_vector([x, y, z]).map_err(DirFault::Unnormalisable)
}

pub(crate) fn eval_direction(name: &str, args: &[Value]) -> Option<Value> {
    if name != "dir" {
        return None;
    }
    Some(classify_dir_args(args).map_or(Value::Undef, Value::Direction))
}

/// Post-`Value::Undef` classifier for `dir`, drained by reify-expr's
/// `emit_undef_builtin_diagnostics`.
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    if name != "dir" {
        return None;
    }
    match classify_dir_args(args) {
        Ok(_) | Err(DirFault::Shape) => None,
        Err(DirFault::NotDimensionless(dim)) => Some(
            Diagnostic::error(format!(
                "dir: components must be dimensionless (Int, Real or a dimensionless \
                 Scalar); got dimension {dim}"
            ))
            .with_code(DiagnosticCode::DimensionedArgRejected),
        ),
        Err(DirFault::Unnormalisable(fault)) => Some(
            Diagnostic::error(format!(
                "E_DegenerateDirection: dir: {fault}; give a non-zero, finite vector"
            ))
            .with_code(DiagnosticCode::DegenerateDirection),
        ),
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, Severity};
    use reify_ir::{UnitVec3, Value};

    fn mm(x: f64) -> Value {
        Value::Scalar {
            si_value: x * 1e-3,
            dimension: DimensionVector::LENGTH,
        }
    }

    fn dimensionless(x: f64) -> Value {
        Value::Scalar {
            si_value: x,
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }

    fn real_vec3(c: [f64; 3]) -> Value {
        Value::Vector(c.iter().map(|x| Value::Real(*x)).collect())
    }

    fn reals(c: [f64; 3]) -> Vec<Value> {
        c.iter().map(|x| Value::Real(*x)).collect()
    }

    fn ints(c: [i64; 3]) -> Vec<Value> {
        c.iter().map(|x| Value::Int(*x)).collect()
    }

    fn eval_dir(args: &[Value]) -> Value {
        crate::eval_builtin("dir", args)
    }

    fn components(v: &Value) -> [f64; 3] {
        match v {
            Value::Direction(u) => u.components(),
            other => panic!("expected a Direction, got {other:?}"),
        }
    }

    fn assert_direction_bits(args: &[Value], want: [f64; 3]) {
        let got = components(&eval_dir(args));
        assert_eq!(
            got.map(f64::to_bits),
            want.map(f64::to_bits),
            "dir({args:?}): got {got:?}, want {want:?}"
        );
    }

    fn assert_direction_close(args: &[Value], want: [f64; 3]) {
        let got = components(&eval_dir(args));
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() <= 1e-15,
                "dir({args:?}) component {i}: got {got:?}, want {want:?}"
            );
        }
    }

    fn fault_diagnostic(args: &[Value]) -> Diagnostic {
        assert_eq!(eval_dir(args), Value::Undef, "dir({args:?}) must be undef");
        let diag = crate::direction_diagnose("dir", args)
            .unwrap_or_else(|| panic!("dir({args:?}) must be diagnosed"));
        assert_eq!(diag.severity, Severity::Error, "dir({args:?})");
        diag
    }

    fn assert_degenerate(args: &[Value]) -> Diagnostic {
        let diag = fault_diagnostic(args);
        assert_eq!(diag.code, Some(DiagnosticCode::DegenerateDirection));
        assert!(
            diag.message.starts_with("E_DegenerateDirection: dir:"),
            "dir({args:?}) message: {}",
            diag.message
        );
        diag
    }

    fn assert_dimensioned(args: &[Value]) {
        let diag = fault_diagnostic(args);
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::DimensionedArgRejected),
            "dir({args:?}): {}",
            diag.message
        );
        assert!(diag.message.contains("dir"), "{}", diag.message);
        assert!(
            !diag.message.contains("E_DegenerateDirection"),
            "dir({args:?}) must not be blamed on degeneracy: {}",
            diag.message
        );
    }

    fn assert_silent_undef(args: &[Value]) {
        assert_eq!(eval_dir(args), Value::Undef, "dir({args:?}) must be undef");
        assert!(
            crate::direction_diagnose("dir", args).is_none(),
            "a shape fault must stay undiagnosed: dir({args:?})"
        );
    }

    #[test]
    fn three_components_normalise_to_a_unit_direction() {
        assert_direction_bits(&ints([0, 1, 0]), [0.0, 1.0, 0.0]);
        assert_direction_bits(&reals([0.0, 2.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_direction_close(
            &[dimensionless(3.0), Value::Real(0.0), Value::Real(4.0)],
            [0.6, 0.0, 0.8],
        );
        assert_direction_bits(&reals([2e-12, 0.0, 0.0]), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn one_vector_normalises_and_a_direction_is_identity() {
        assert_direction_bits(
            &[Value::Vector(vec![
                Value::Int(0),
                Value::Int(0),
                Value::Int(3),
            ])],
            [0.0, 0.0, 1.0],
        );
        assert_direction_close(&[real_vec3([3.0, 4.0, 0.0])], [0.6, 0.8, 0.0]);
        let u = UnitVec3::from_vector([0.6, 0.8, 0.0]).unwrap();
        assert_direction_bits(&[Value::Direction(u)], u.components());
    }

    #[test]
    fn zero_and_sub_floor_vectors_are_degenerate() {
        assert_degenerate(&ints([0, 0, 0]));
        assert_degenerate(&reals([1e-13, 0.0, 0.0]));
        assert_degenerate(&[real_vec3([0.0, 0.0, 0.0])]);
    }

    #[test]
    fn non_finite_components_are_degenerate_and_named_non_finite() {
        for bad in [f64::NAN, f64::INFINITY] {
            let diag = assert_degenerate(&reals([bad, 0.0, 0.0]));
            assert!(diag.message.contains("non-finite"), "{}", diag.message);
        }
    }

    #[test]
    fn dimensioned_components_are_rejected_before_degeneracy() {
        let zero = Value::Real(0.0);
        assert_dimensioned(&[mm(1.0), zero.clone(), zero.clone()]);
        assert_dimensioned(&[zero.clone(), zero.clone(), mm(1.0)]);
        assert_dimensioned(&[mm(1.0), mm(0.0), mm(0.0)]);
        assert_dimensioned(&[mm(0.0), mm(0.0), mm(0.0)]);
        assert_dimensioned(&[Value::Vector(vec![mm(1.0), mm(0.0), mm(0.0)])]);
        let angle = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::ANGLE,
        };
        assert_dimensioned(&[angle, zero.clone(), zero]);
    }

    #[test]
    fn shape_faults_stay_undiagnosed_undef() {
        let zero = Value::Real(0.0);
        assert_silent_undef(&[]);
        assert_silent_undef(&[zero.clone(), zero.clone()]);
        assert_silent_undef(&[zero.clone(), zero.clone(), zero.clone(), zero.clone()]);
        assert_silent_undef(&[Value::String("up".into())]);
        assert_silent_undef(&[Value::Bool(true), zero.clone(), zero.clone()]);
        assert_silent_undef(&[Value::String("x".into()), zero.clone(), zero.clone()]);
        assert_silent_undef(&[Value::Point(reals([0.0, 1.0, 0.0]))]);
        assert_silent_undef(&[Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)])]);
        assert_silent_undef(&[Value::Vector(vec![mm(1.0), zero.clone(), zero])]);
        assert_silent_undef(&[Value::List(reals([0.0, 1.0, 0.0]))]);
    }

    #[test]
    fn valid_calls_and_other_names_are_not_diagnosed() {
        assert!(crate::direction_diagnose("dir", &ints([0, 1, 0])).is_none());
        assert!(crate::direction_diagnose("normalize", &[real_vec3([0.0, 0.0, 0.0])]).is_none());
    }
}
