use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::{binary, complex_abs, sanitize_value, tensor_components_f64, unary};

pub(crate) fn eval_linalg(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Linear algebra: dot, cross, magnitude, normalize ---
        "normalize" => unary(args, |v| {
            // Determine the output wrapper based on input variant.
            let wrap: fn(Vec<Value>) -> Value = match v {
                Value::Vector(_) => Value::Vector,
                Value::Point(_) => Value::Point,
                _ => Value::Tensor,
            };
            let (vals, _dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            // Reject non-finite inputs early — a partially-Undef Tensor is not
            // a meaningful unit vector, so we return a single Undef for the
            // whole result rather than per-component sanitization.
            if vals.iter().any(|x| !x.is_finite()) {
                return Value::Undef;
            }
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            // mag is finite here, but squaring can still overflow to Inf.
            if !mag.is_finite() || mag == 0.0 {
                return Value::Undef;
            }
            wrap(vals.iter().map(|x| Value::Real(x / mag)).collect())
        }),

        "magnitude" => unary(args, |v| {
            // Handle Complex before the Tensor fallback.
            if let Value::Complex { re, im, dimension } = v {
                return complex_abs(*re, *im, *dimension);
            }
            let (vals, dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(mag))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: mag,
                    dimension: dim,
                })
            }
        }),

        "cross" => binary(args, |a, b| {
            // Cross product of two vectors → vector; point inputs are
            // semantically invalid (cross is only defined for vectors).
            let wrap: fn(Vec<Value>) -> Value = match (a, b) {
                (Value::Point(_), _) | (_, Value::Point(_)) => return Value::Undef,
                (Value::Vector(_), Value::Vector(_)) => Value::Vector,
                _ => Value::Tensor,
            };
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != 3 || b_vals.len() != 3 {
                return Value::Undef;
            }
            let (a0, a1, a2) = (a_vals[0], a_vals[1], a_vals[2]);
            let (b0, b1, b2) = (b_vals[0], b_vals[1], b_vals[2]);
            let cx = a1 * b2 - a2 * b1;
            let cy = a2 * b0 - a0 * b2;
            let cz = a0 * b1 - a1 * b0;
            let result_dim = a_dim.mul(&b_dim);
            let make_component = |v: f64| -> Value {
                if result_dim == DimensionVector::DIMENSIONLESS {
                    sanitize_value(Value::Real(v))
                } else {
                    sanitize_value(Value::Scalar {
                        si_value: v,
                        dimension: result_dim,
                    })
                }
            };
            wrap(vec![
                make_component(cx),
                make_component(cy),
                make_component(cz),
            ])
        }),

        "dot" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != b_vals.len() {
                return Value::Undef;
            }
            let sum: f64 = a_vals.iter().zip(b_vals.iter()).map(|(x, y)| x * y).sum();
            let result_dim = a_dim.mul(&b_dim);
            if result_dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(sum))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: sum,
                    dimension: result_dim,
                })
            }
        }),

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_macros::make_scalar_vec3;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // --- dot() tests: dimensionless vectors (step-1) ---

    #[test]
    fn dot_orthogonal_dimensionless() {
        // dot([1,0,0], [0,1,0]) == 0.0
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 0.0);
    }

    #[test]
    fn dot_dimensionless_vec3() {
        // dot([1,2,3], [4,5,6]) == 4+10+18 == 32
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn dot_mismatched_lengths_returns_undef() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "mismatched lengths should be Undef"
        );
    }

    #[test]
    fn dot_non_tensor_arg_returns_undef() {
        assert!(
            eval_builtin("dot", &[Value::Real(1.0), Value::Real(2.0)]).is_undef(),
            "dot of non-Tensor args should be Undef"
        );
    }

    // --- normalize() tests (step-9) ---

    #[test]
    fn normalize_3_4_0() {
        // normalize([3,4,0]) ≈ [0.6, 0.8, 0.0]
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Tensor, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn normalize_zero_vector_returns_undef() {
        let v = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of zero vector should be Undef"
        );
    }

    #[test]
    fn normalize_dimensioned_vector_returns_real_components() {
        // normalize([3m,4m,0m]) should return Real components (dimensionless direction)
        let v = Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Tensor, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn normalize_non_tensor_returns_undef() {
        assert!(
            eval_builtin("normalize", &[Value::Real(5.0)]).is_undef(),
            "normalize of non-Tensor should be Undef"
        );
    }

    #[test]
    fn normalize_single_element_tensor() {
        // normalize([5.0]) == [1.0]
        let v = Value::Tensor(vec![Value::Real(5.0)]);
        let result = eval_builtin("normalize", &[v]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 1);
                let val = items[0].as_f64().unwrap();
                assert!((val - 1.0).abs() < 1e-12, "expected 1.0, got {}", val);
            }
            other => panic!("expected Tensor([1.0]), got {:?}", other),
        }
    }

    // --- normalize() sanitization tests (step-13) ---

    #[test]
    fn normalize_nan_component_returns_undef() {
        // A NaN component makes sum_sq NaN → mag NaN → mag==0.0 is false →
        // without an up-front guard we'd produce a Tensor with NaN Real values.
        let v = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(1.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor containing NaN should return Undef"
        );
    }

    #[test]
    fn normalize_inf_component_returns_undef() {
        // An Inf component makes sum_sq Inf → mag Inf → Inf/Inf = NaN for the
        // Inf component, other components become 0.0 (finite/Inf).  Without a
        // guard we'd produce a mixed Tensor instead of Undef.
        let v = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(1.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor containing Inf should return Undef"
        );
    }

    #[test]
    fn normalize_overflow_returns_undef() {
        // Squaring f64::MAX overflows to Inf → sum_sq = Inf → mag = Inf →
        // x / mag produces NaN or 0.0 — the result is not a valid unit vector.
        let v = Value::Tensor(vec![
            Value::Real(f64::MAX),
            Value::Real(f64::MAX),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor whose magnitude overflows to Inf should return Undef"
        );
    }

    // --- magnitude() tests (step-7) ---

    #[test]
    fn magnitude_3_4_0_equals_5() {
        // magnitude([3,4,0]) == 5.0
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_dimensioned_vector() {
        // magnitude([3mm,4mm,0mm]) == 5mm = 0.005m as Scalar{LENGTH}
        let v = Value::Tensor(vec![
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.000,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[v]),
            0.005,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn magnitude_zero_vector_returns_zero() {
        let v = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 0.0);
    }

    #[test]
    fn magnitude_2d_vector() {
        // magnitude([3,4]) == 5.0
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_non_tensor_returns_undef() {
        assert!(
            eval_builtin("magnitude", &[Value::Real(5.0)]).is_undef(),
            "magnitude of non-Tensor should be Undef"
        );
    }

    #[test]
    fn magnitude_empty_tensor_returns_undef() {
        let v = Value::Tensor(vec![]);
        assert!(
            eval_builtin("magnitude", &[v]).is_undef(),
            "magnitude of empty Tensor should be Undef"
        );
    }

    // --- cross() tests: dimensionless vectors (step-4) ---

    #[test]
    fn cross_x_hat_y_hat_equals_z_hat() {
        // cross([1,0,0], [0,1,0]) == [0,0,1]
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_anti_commutativity() {
        // cross(a,b) == -cross(b,a)
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        let ab = eval_builtin("cross", &[a.clone(), b.clone()]);
        let ba = eval_builtin("cross", &[b, a]);
        match (ab, ba) {
            (Value::Tensor(ab_items), Value::Tensor(ba_items)) => {
                for (ai, bi) in ab_items.iter().zip(ba_items.iter()) {
                    let av = ai.as_f64().unwrap();
                    let bv = bi.as_f64().unwrap();
                    assert!(
                        (av + bv).abs() < 1e-12,
                        "anti-commutativity failed: {} + {} != 0",
                        av,
                        bv
                    );
                }
            }
            other => panic!("expected two Tensors, got {:?}", other),
        }
    }

    #[test]
    fn cross_orthogonality() {
        // dot(a, cross(a, b)) == 0
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        let c = eval_builtin("cross", &[a.clone(), b]);
        let dot_result = eval_builtin("dot", &[a, c]);
        assert_real_approx!(dot_result, 0.0);
    }

    #[test]
    fn cross_length_2_tensor_returns_undef() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross on 2-element Tensor should be Undef"
        );
    }

    #[test]
    fn cross_length_4_tensor_returns_undef() {
        let a = Value::Tensor(vec![
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let b = Value::Tensor(vec![
            Value::Real(0.0),
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross on 4-element Tensor should be Undef"
        );
    }

    #[test]
    fn cross_non_tensor_returns_undef() {
        assert!(
            eval_builtin("cross", &[Value::Real(1.0), Value::Real(2.0)]).is_undef(),
            "cross of non-Tensor args should be Undef"
        );
    }

    // --- cross() tests: dimensioned vectors (step-5) ---

    #[test]
    fn cross_length_force_vectors() {
        // cross([1m,0,0], [0,1N,0]) == [0,0,1 m·N] each component has Length*Force dimension
        let length_force = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_core::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: reify_core::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_core::dimension::FORCE,
            },
        ]);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3, "cross product must have 3 components");
                // [1,0,0] x [0,1,0] = [0*0-0*1, 0*0-1*0, 1*1-0*0] = [0, 0, 1]
                for (i, item) in items.iter().enumerate() {
                    match item {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert_eq!(
                                *dimension, length_force,
                                "component {} dimension mismatch",
                                i
                            );
                            let expected = if i == 2 { 1.0 } else { 0.0 };
                            assert!(
                                (si_value - expected).abs() < 1e-12,
                                "component {}: expected {}, got {}",
                                i,
                                expected,
                                si_value
                            );
                        }
                        other => panic!("expected Scalar at component {}, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Tensor, got {:?}", other),
        }
    }

    // --- dot() tests: dimensioned vectors (step-2) ---

    #[test]
    fn dot_length_force_vectors() {
        // dot([1m, 0, 0], [1N, 0, 0]) -> Scalar { si_value: 1.0, dimension: Length*Force }
        let length_force = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: reify_core::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_core::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_core::dimension::FORCE,
            },
        ]);
        assert_scalar_approx!(eval_builtin("dot", &[a, b]), 1.0, length_force);
    }

    // ── dot() with Value::Vector inputs (step-1) ────────────────────────────

    #[test]
    fn dot_vector_orthogonal() {
        // dot(Vector([1,0,0]), Vector([0,1,0])) == 0.0
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 0.0);
    }

    #[test]
    fn dot_vector_dimensioned() {
        // dot(Vector([1m,0,0]), Vector([1N,0,0])) -> Scalar{1.0, Length*Force}
        let length_force = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
        let a = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::LENGTH);
        let b = make_scalar_vec3([1.0, 0.0, 0.0], reify_core::dimension::FORCE);
        assert_scalar_approx!(eval_builtin("dot", &[a, b]), 1.0, length_force);
    }

    // ── cross() with Value::Vector inputs (step-3) ──────────────────────────

    #[test]
    fn cross_vector_returns_vector_wrapper() {
        // cross(Vector([1,0,0]), Vector([0,1,0])) must return Value::Vector([0,0,1])
        // NOT Value::Tensor — verifies wrapper-preservation at line 312
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Vector, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_vector_dimensioned_preserves_dimension() {
        // cross(Vector([1m,0,0]), Vector([0,1N,0])) each component has Length*Force dimension
        let length_force = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
        let a = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::LENGTH);
        let b = make_scalar_vec3([0.0, 1.0, 0.0], reify_core::dimension::FORCE);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                // z component should be 1.0 m·N, others 0.0
                for item in &items {
                    match item {
                        Value::Scalar { dimension, .. } => {
                            assert_eq!(
                                *dimension, length_force,
                                "cross component dimension mismatch"
                            );
                        }
                        other => panic!("expected Scalar component, got {:?}", other),
                    }
                }
                let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!(
                    (vals[2] - 1.0).abs() < 1e-12,
                    "z: expected 1.0, got {}",
                    vals[2]
                );
            }
            other => panic!(
                "expected Value::Vector for dimensioned cross, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn cross_2d_vector_returns_undef() {
        // cross of 2-element Value::Vector returns Undef (cross is only defined for 3-vectors)
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross of 2-element Vector should be Undef"
        );
    }

    // ── normalize() with Value::Vector inputs (step-5) ──────────────────────

    #[test]
    fn normalize_vector_returns_vector_wrapper() {
        // normalize(Vector([3,4,0])) returns Value::Vector([0.6,0.8,0.0]) with Real components
        // NOT Value::Tensor — verifies wrapper-preservation at line 266
        let v = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Vector, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn normalize_zero_vector_input_returns_undef() {
        // normalize(Vector([0,0,0])) -> Undef
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of zero Vector should be Undef"
        );
    }

    #[test]
    fn normalize_dimensioned_vector_input() {
        // normalize(Vector([3m,4m,0m])) -> Value::Vector with dimensionless Real components
        let v = make_scalar_vec3([3.0, 4.0, 0.0], DimensionVector::LENGTH);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Vector, result, [0.6, 0.8, 0.0]);
    }

    // ── magnitude() with Value::Vector inputs (step-7) ──────────────────────

    #[test]
    fn magnitude_vector_3_4_0() {
        // magnitude(Vector([3,4,0])) == Real(5.0)
        let v = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_vector_dimensioned() {
        // magnitude(Vector([3mm,4mm,0])) == Scalar{0.005, LENGTH}
        // 3mm=0.003m, 4mm=0.004m -> magnitude=0.005m
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[v]),
            0.005,
            DimensionVector::LENGTH
        );
    }

    // ── mixed component dimensions (step-3) ─────────────────────────────────

    #[test]
    fn dot_mixed_component_dimensions_returns_undef() {
        // A Tensor with mixed dimensions is not a valid physical vector
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "dot of vector with mixed component dimensions should be Undef"
        );
    }

    // ── tensor_components_f64 with Point/Vector inputs (task 398, step-13) ────

    #[test]
    fn magnitude_point_dimensioned_3m_4m_0m() {
        // magnitude(Point([3m,4m,0m])) ≈ Scalar{0.005, LENGTH}
        // 3mm=0.003m, 4mm=0.004m → |v|=0.005m
        let p = Value::Point(vec![
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[p]),
            0.005,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn normalize_point_returns_point_wrapper() {
        // normalize(Point([3,4,0])) → Point([0.6,0.8,0.0])
        let p = Value::Point(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[p]);
        assert_vector3_approx!(Point, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn dot_point_point_returns_scalar() {
        // dot(Point([1,2,3]), Point([4,5,6])) = 1*4 + 2*5 + 3*6 = 32
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Point(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn cross_point_point_returns_undef() {
        // cross is semantically invalid for points — only defined for vectors
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Point(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross of two Points should return Undef"
        );
    }

    // ── mixed-type contract tests (task 379) ─────────────────────────────────

    #[test]
    fn cross_vector_tensor_returns_tensor_wrapper() {
        // cross(Vector, Tensor) falls through to Tensor wrapper (line 366: _ => Value::Tensor)
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_tensor_vector_returns_tensor_wrapper() {
        // cross(Tensor, Vector) also falls through to Tensor wrapper
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_point_vector_returns_undef() {
        // ANY Point input to cross returns Undef (line 364)
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross(Point, Vector) should return Undef"
        );
    }

    #[test]
    fn cross_vector_point_returns_undef() {
        // Second-arg Point also returns Undef
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Point(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross(Vector, Point) should return Undef"
        );
    }

    #[test]
    fn dot_point_vector_returns_scalar() {
        // dot accepts mixed Point+Vector inputs via tensor_components_f64
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Vector(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn dot_vector_point_returns_scalar() {
        // Argument order symmetry for mixed dot
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Point(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn normalize_point_dimensioned_returns_point() {
        // normalize(Point([3m,4m,0m])) → Point([0.6, 0.8, 0.0]) with Real components
        let p = Value::Point(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let result = eval_builtin("normalize", &[p]);
        assert_vector3_approx!(Point, result, [0.6, 0.8, 0.0]);
    }
}
