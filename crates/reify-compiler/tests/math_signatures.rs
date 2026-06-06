//! Two-way (compile ⟺ eval) boundary suite for the math-linalg **§3 operation**
//! family (task 4182 δ — the **H** component / acceptance gate).
//!
//! Every §3 operation builtin now carries its documented DIMENSIONAL return
//! type at COMPILE time (the `math_fn_result_type` arms wired through
//! `expr.rs::resolve_function_overload`'s `is_math_typed_fn` arm). This file
//! pins, for one representative dimensioned input per row group, that the
//! FORWARD compile-time cell type and the BACKWARD eval-time `Value` AGREE on
//! both dimension AND kind — the high-stakes invariant, because a cell type
//! that disagrees with the eval `Value` kind raises a runtime
//! `TypeKindMismatch` (the D6/D7 hazard).
//!
//!   FORWARD  = `compile_with_stdlib_helper(src)` → `template.value_cells[..].cell_type`
//!   BACKWARD = full `reify_eval::Engine` (+ `MockConstraintChecker`) `.eval(&module)`
//!              → `result.values.get(&ValueCellId::new(T, cell))`
//!
//! The backward direction uses the SAME compiled module the forward direction
//! reads, so the two views are guaranteed consistent. The N-general
//! determinant / inverse / eigenvalues paths are the nalgebra-backed
//! `reify-stdlib` builtins delivered by the prerequisite tasks β (4180) and γ
//! (4181); the full `Engine` is the real pipeline that dispatches them (the
//! proven pattern in `reify-eval`'s `m8_stdlib_integration.rs`).
//!
//! Enforcement is PERMISSIVE (PRD §5): δ adds NO new bespoke hard error. The
//! two enforcement tests at the bottom pin that the only new diagnostic is the
//! EXISTING typed-let dimensional check firing (because `determinant` now
//! propagates `Scalar<Q^N>`), and that an un-annotated `determinant` produces
//! no error at all.

mod common;

use common::compile_with_stdlib_helper;
use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_eval::EvalResult;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// Tolerance for SI-value comparisons (exact-arithmetic inputs + f64 slack).
const EPS: f64 = 1e-9;

/// The structure whose `let` members are the boundary rows.
const STRUCT: &str = "MathBoundary";

/// One representative call per §3 row group, each on a DIMENSIONED input so the
/// dimensional return type is observable in both directions. `m4` is a clean
/// non-singular diagonal 4×4 of `Length` (det = 2·3·4·5 = 120, every cell
/// dimensioned so the uniform-dimension eval guard passes); the spectral rows
/// use a diagonal `Area` matrix (symmetric → real spectrum = the diagonal) and
/// a dimensionless 90° rotation (genuinely-complex spectrum ±i).
const BOUNDARY_SOURCE: &str = r#"
structure def MathBoundary {
    let m4 = matrix([[2.0m, 0.0m, 0.0m, 0.0m], [0.0m, 3.0m, 0.0m, 0.0m], [0.0m, 0.0m, 4.0m, 0.0m], [0.0m, 0.0m, 0.0m, 5.0m]])

    let sq      = sqrt(4.0m^2)
    let sq_real = sqrt(4.0)
    let dotp    = dot(vec([1.0m, 2.0m]), vec([3.0m, 4.0m]))
    let mag     = magnitude(vec([3.0m, 4.0m]))
    let nrm     = normalize(vec([3.0m, 4.0m]))
    let crs     = cross(vec([1.0m, 0.0m, 0.0m]), vec([0.0m, 1.0m, 0.0m]))
    let det4    = determinant(m4)
    let inv4    = inverse(m4)
    let outp    = outer(vec([1.0m, 2.0m]), vec([3.0m, 4.0m, 5.0m]))
    let trp     = transpose(m4)
    let trc     = trace(m4)
    let eigs    = eigenvalues(diag([3.0m^2, 5.0m^2, 7.0m^2, 9.0m^2]))
    let ceigs   = complex_eigenvalues(matrix([[0.0, -1.0], [1.0, 0.0]]))
    let z       = complex(3.0m, 4.0m)
    let zr      = real(complex(3.0m, 4.0m))
    let zi      = imag(complex(3.0m, 4.0m))
    let zc      = conjugate(complex(3.0m, 4.0m))
    let zmag    = complex_magnitude(complex(3.0m, 4.0m))
    let zph     = phase(complex(1.0m, 1.0m))
}
"#;

// ── Harness ──────────────────────────────────────────────────────────────────

/// Compile `BOUNDARY_SOURCE` (asserting no Error-severity diagnostics) and eval
/// it with the full `Engine`. Returns the compiled module (for the FORWARD cell
/// type) paired with the eval result (for the BACKWARD value).
fn compile_and_eval() -> (CompiledModule, EvalResult) {
    let module = compile_with_stdlib_helper(BOUNDARY_SOURCE);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "boundary source must produce no Error-severity diagnostics; got: {errs:?}"
    );

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    let result = engine.eval(&module);
    let eval_errs: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errs.is_empty(),
        "boundary eval must produce no Error-severity diagnostics; got: {eval_errs:?}"
    );
    (module, result)
}

/// The two-way `(forward cell type, backward eval value)` for member `cell`.
fn two_way(cell: &str) -> (Type, Value) {
    let (module, result) = compile_and_eval();
    let template = module
        .templates
        .iter()
        .find(|t| t.name == STRUCT)
        .unwrap_or_else(|| panic!("template `{STRUCT}` not found"));
    let cell_type = template
        .value_cells
        .iter()
        .find(|c| c.id.member == cell)
        .unwrap_or_else(|| panic!("cell `{cell}` not found on `{STRUCT}`"))
        .cell_type
        .clone();
    let value = result
        .values
        .get(&ValueCellId::new(STRUCT, cell))
        .unwrap_or_else(|| panic!("eval value for `{cell}` not found"))
        .clone();
    (cell_type, value)
}

/// The dimension carried by a numeric `Value` (`Scalar`/`Complex` → its
/// dimension; `Real`/`Int` → `DIMENSIONLESS`).
fn value_dim(v: &Value) -> DimensionVector {
    match v {
        Value::Scalar { dimension, .. } => *dimension,
        Value::Complex { dimension, .. } => *dimension,
        Value::Real(_) | Value::Int(_) => DimensionVector::DIMENSIONLESS,
        other => panic!("expected a numeric Value, got {other:?}"),
    }
}

/// `Type::Scalar { dimension }` shorthand.
fn sca(dim: DimensionVector) -> Type {
    Type::Scalar { dimension: dim }
}

// ── Scalar / element fns ──────────────────────────────────────────────────────

/// `sqrt(4.0 m²)` ⟺ `Scalar<Length>` / eval `Value::Scalar { 2.0, LENGTH }`.
#[test]
fn sqrt_area_two_way_is_length() {
    let (ty, val) = two_way("sq");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "sqrt(4m²) forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "sqrt eval dimension");
            assert!((si_value - 2.0).abs() < EPS, "sqrt eval value, got {si_value}");
        }
        other => panic!("sqrt(4m²) eval should be Value::Scalar, got {other:?}"),
    }
}

/// `sqrt(4.0)` ⟺ `Real` / eval `Value::Real(2.0)` — the dimensionless boundary
/// stays `Real` (NOT `Scalar<DIMENSIONLESS>`) in BOTH directions.
#[test]
fn sqrt_real_two_way_is_real() {
    let (ty, val) = two_way("sq_real");
    assert_eq!(ty, Type::Real, "sqrt(4.0) forward type must be Real");
    match val {
        Value::Real(x) => assert!((x - 2.0).abs() < EPS, "sqrt(4.0) eval value, got {x}"),
        other => panic!("sqrt(4.0) eval should be Value::Real, got {other:?}"),
    }
}

// ── Vector ops ────────────────────────────────────────────────────────────────

/// `dot(vec([1m,2m]), vec([3m,4m]))` ⟺ `Scalar<Area>` / eval dim `AREA`
/// (1·3 + 2·4 = 11 m²).
#[test]
fn dot_two_way_is_area() {
    let (ty, val) = two_way("dotp");
    assert_eq!(ty, sca(DimensionVector::AREA), "dot forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::AREA, "dot eval dimension");
            assert!((si_value - 11.0).abs() < EPS, "dot eval value, got {si_value}");
        }
        other => panic!("dot eval should be Value::Scalar, got {other:?}"),
    }
}

/// `magnitude(vec([3m,4m]))` ⟺ `Scalar<Length>` / eval `Scalar { 5.0, LENGTH }`.
#[test]
fn magnitude_two_way_is_length() {
    let (ty, val) = two_way("mag");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "magnitude forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "magnitude eval dimension");
            assert!((si_value - 5.0).abs() < EPS, "magnitude eval value, got {si_value}");
        }
        other => panic!("magnitude eval should be Value::Scalar, got {other:?}"),
    }
}

/// `normalize(vec([3m,4m]))` ⟺ `Vector<2, Real>` / eval `Value::Vector` of
/// dimensionless `Real` components (N preserved, dimension stripped).
#[test]
fn normalize_two_way_is_dimensionless_vector() {
    let (ty, val) = two_way("nrm");
    assert_eq!(
        ty,
        Type::Vector {
            n: 2,
            quantity: Box::new(Type::Real)
        },
        "normalize forward type"
    );
    match val {
        Value::Vector(comps) => {
            assert_eq!(comps.len(), 2, "normalize eval component count");
            for (i, c) in comps.iter().enumerate() {
                assert!(
                    matches!(c, Value::Real(_)),
                    "normalize eval component {i} should be Value::Real, got {c:?}"
                );
            }
        }
        other => panic!("normalize eval should be Value::Vector, got {other:?}"),
    }
}

/// `cross(vec([1m,0m,0m]), vec([0m,1m,0m]))` ⟺ `Vector<3, Scalar<Area>>` /
/// eval `Value::Vector` of 3 `Area` scalars (= [0,0,1] m²).
#[test]
fn cross_two_way_is_vector3_area() {
    let (ty, val) = two_way("crs");
    assert_eq!(
        ty,
        Type::Vector {
            n: 3,
            quantity: Box::new(sca(DimensionVector::AREA))
        },
        "cross forward type"
    );
    match val {
        Value::Vector(comps) => {
            assert_eq!(comps.len(), 3, "cross eval component count");
            for (i, c) in comps.iter().enumerate() {
                assert_eq!(
                    value_dim(c),
                    DimensionVector::AREA,
                    "cross eval component {i} dimension"
                );
            }
        }
        other => panic!("cross eval should be Value::Vector, got {other:?}"),
    }
}

// ── Matrix ops ────────────────────────────────────────────────────────────────

/// `determinant(m4)` of a dimensioned 4×4 `Length` matrix ⟺ `Scalar<Length⁴>` /
/// eval `Scalar { 120.0, LENGTH⁴ }` (det = 2·3·4·5).
#[test]
fn determinant_two_way_is_length_pow4() {
    let (ty, val) = two_way("det4");
    let l4 = DimensionVector::LENGTH.pow(4);
    assert_eq!(ty, sca(l4), "determinant forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, l4, "determinant eval dimension (Length⁴)");
            assert!((si_value - 120.0).abs() < EPS, "determinant eval value, got {si_value}");
        }
        other => panic!("determinant eval should be Value::Scalar, got {other:?}"),
    }
}

/// `inverse(m4)` ⟺ `Tensor<rank 2, n 4, Scalar<Length⁻¹>>` / eval nested
/// `Value::Tensor` whose cells carry `Length⁻¹` (shape preserved, dim negated).
#[test]
fn inverse_two_way_is_inverse_length_tensor() {
    let (ty, val) = two_way("inv4");
    let inv_dim = DimensionVector::DIMENSIONLESS.div(&DimensionVector::LENGTH);
    assert_eq!(
        ty,
        Type::Tensor {
            rank: 2,
            n: 4,
            quantity: Box::new(sca(inv_dim))
        },
        "inverse forward type"
    );
    match val {
        Value::Tensor(rows) => {
            assert_eq!(rows.len(), 4, "inverse eval row count");
            match &rows[0] {
                Value::Tensor(cells) => {
                    assert_eq!(cells.len(), 4, "inverse eval column count");
                    assert_eq!(
                        value_dim(&cells[0]),
                        inv_dim,
                        "inverse eval cell dimension (Length⁻¹)"
                    );
                }
                other => panic!("inverse eval row should be nested Value::Tensor, got {other:?}"),
            }
        }
        other => panic!("inverse eval should be Value::Tensor, got {other:?}"),
    }
}

/// `outer(vec([1m,2m]), vec([3m,4m,5m]))` ⟺ `Tensor<rank 2, n 3, Scalar<Area>>` /
/// eval `Value::Tensor` (2 rows × 3 cols) whose cells carry `Area` (= the L·L
/// outer product [[3,4,5],[6,8,10]] m²). The forward `n` is the column count =
/// second-arg N (3). `outer`/`transpose`/`trace` were forward-only before this
/// amendment — they produce a Tensor/Scalar, exactly the D6/D7 kind+dimension
/// runtime hazard the two-way suite exists to lock down.
#[test]
fn outer_two_way_is_tensor_area() {
    let (ty, val) = two_way("outp");
    assert_eq!(
        ty,
        Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(sca(DimensionVector::AREA))
        },
        "outer forward type"
    );
    match val {
        Value::Tensor(rows) => {
            assert_eq!(rows.len(), 2, "outer eval row count");
            match &rows[0] {
                Value::Tensor(cells) => {
                    assert_eq!(cells.len(), 3, "outer eval column count");
                    for (i, c) in cells.iter().enumerate() {
                        assert_eq!(
                            value_dim(c),
                            DimensionVector::AREA,
                            "outer eval cell {i} dimension"
                        );
                    }
                }
                other => panic!("outer eval row should be a nested Value::Tensor, got {other:?}"),
            }
        }
        other => panic!("outer eval should be Value::Tensor, got {other:?}"),
    }
}

/// `transpose(m4)` of the dimensioned 4×4 `Length` matrix ⟺ `Tensor<rank 2,
/// n 4, Scalar<Length>>` (identity kind+shape+dimension) / eval `Value::Tensor`
/// (4×4) whose cells carry `Length` (transpose of a diagonal = the diagonal).
#[test]
fn transpose_two_way_is_length_tensor() {
    let (ty, val) = two_way("trp");
    assert_eq!(
        ty,
        Type::Tensor {
            rank: 2,
            n: 4,
            quantity: Box::new(sca(DimensionVector::LENGTH))
        },
        "transpose forward type"
    );
    match val {
        Value::Tensor(rows) => {
            assert_eq!(rows.len(), 4, "transpose eval row count");
            match &rows[0] {
                Value::Tensor(cells) => {
                    assert_eq!(cells.len(), 4, "transpose eval column count");
                    assert_eq!(
                        value_dim(&cells[0]),
                        DimensionVector::LENGTH,
                        "transpose eval cell dimension"
                    );
                }
                other => {
                    panic!("transpose eval row should be a nested Value::Tensor, got {other:?}")
                }
            }
        }
        other => panic!("transpose eval should be Value::Tensor, got {other:?}"),
    }
}

/// `trace(m4)` of the dimensioned 4×4 `Length` matrix ⟺ `Scalar<Length>` / eval
/// `Scalar { 14.0, LENGTH }` (sum of the diagonal 2+3+4+5).
#[test]
fn trace_two_way_is_length_scalar() {
    let (ty, val) = two_way("trc");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "trace forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "trace eval dimension");
            assert!(
                (si_value - 14.0).abs() < EPS,
                "trace eval value (2+3+4+5), got {si_value}"
            );
        }
        other => panic!("trace eval should be Value::Scalar, got {other:?}"),
    }
}

// ── Spectral ops ──────────────────────────────────────────────────────────────

/// `eigenvalues(diag([3m²,5m²,7m²,9m²]))` ⟺ `List<Scalar<Area>>` / eval
/// `Value::List` whose elements are `Area` scalars (the diagonal = the spectrum).
#[test]
fn eigenvalues_two_way_is_list_of_area() {
    let (ty, val) = two_way("eigs");
    assert_eq!(
        ty,
        Type::List(Box::new(sca(DimensionVector::AREA))),
        "eigenvalues forward type"
    );
    match val {
        Value::List(items) => {
            assert_eq!(items.len(), 4, "eigenvalues eval element count");
            for (i, it) in items.iter().enumerate() {
                assert!(
                    matches!(it, Value::Scalar { .. }),
                    "eigenvalues eval element {i} should be Value::Scalar, got {it:?}"
                );
                assert_eq!(
                    value_dim(it),
                    DimensionVector::AREA,
                    "eigenvalues eval element {i} dimension"
                );
            }
        }
        other => panic!("eigenvalues eval should be Value::List, got {other:?}"),
    }
}

/// `complex_eigenvalues([[0,-1],[1,0]])` (90° rotation, spectrum ±i) ⟺
/// `List<Complex<Real>>` / eval `Value::List` of dimensionless `Value::Complex`.
#[test]
fn complex_eigenvalues_two_way_is_list_of_complex() {
    let (ty, val) = two_way("ceigs");
    assert_eq!(
        ty,
        Type::List(Box::new(Type::Complex(Box::new(Type::Real)))),
        "complex_eigenvalues forward type"
    );
    match val {
        Value::List(items) => {
            assert_eq!(items.len(), 2, "complex_eigenvalues eval element count");
            for (i, it) in items.iter().enumerate() {
                assert!(
                    matches!(it, Value::Complex { .. }),
                    "complex_eigenvalues eval element {i} should be Value::Complex, got {it:?}"
                );
                assert_eq!(
                    value_dim(it),
                    DimensionVector::DIMENSIONLESS,
                    "complex_eigenvalues eval element {i} dimension"
                );
            }
        }
        other => panic!("complex_eigenvalues eval should be Value::List, got {other:?}"),
    }
}

// ── Complex fns ───────────────────────────────────────────────────────────────

/// `complex(3m, 4m)` ⟺ `Complex<Scalar<Length>>` / eval
/// `Value::Complex { 3, 4, LENGTH }`.
#[test]
fn complex_two_way_is_complex_length() {
    let (ty, val) = two_way("z");
    assert_eq!(
        ty,
        Type::Complex(Box::new(sca(DimensionVector::LENGTH))),
        "complex forward type"
    );
    match val {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "complex eval dimension");
            assert!((re - 3.0).abs() < EPS && (im - 4.0).abs() < EPS, "complex eval re/im");
        }
        other => panic!("complex eval should be Value::Complex, got {other:?}"),
    }
}

/// `real(complex(3m,4m))` ⟺ `Scalar<Length>` / eval `Scalar { 3.0, LENGTH }`.
#[test]
fn real_two_way_strips_to_length_scalar() {
    let (ty, val) = two_way("zr");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "real forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "real eval dimension");
            assert!((si_value - 3.0).abs() < EPS, "real eval value, got {si_value}");
        }
        other => panic!("real eval should be Value::Scalar, got {other:?}"),
    }
}

/// `imag(complex(3m,4m))` ⟺ `Scalar<Length>` / eval `Scalar { 4.0, LENGTH }`.
#[test]
fn imag_two_way_strips_to_length_scalar() {
    let (ty, val) = two_way("zi");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "imag forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "imag eval dimension");
            assert!((si_value - 4.0).abs() < EPS, "imag eval value, got {si_value}");
        }
        other => panic!("imag eval should be Value::Scalar, got {other:?}"),
    }
}

/// `conjugate(complex(3m,4m))` ⟺ `Complex<Scalar<Length>>` (identity kind) /
/// eval `Value::Complex { 3, -4, LENGTH }`.
#[test]
fn conjugate_two_way_is_complex_length() {
    let (ty, val) = two_way("zc");
    assert_eq!(
        ty,
        Type::Complex(Box::new(sca(DimensionVector::LENGTH))),
        "conjugate forward type"
    );
    match val {
        Value::Complex { re, im, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "conjugate eval dimension");
            assert!((re - 3.0).abs() < EPS && (im + 4.0).abs() < EPS, "conjugate eval re/im");
        }
        other => panic!("conjugate eval should be Value::Complex, got {other:?}"),
    }
}

/// `complex_magnitude(complex(3m,4m))` ⟺ `Scalar<Length>` / eval
/// `Scalar { 5.0, LENGTH }`.
#[test]
fn complex_magnitude_two_way_is_length() {
    let (ty, val) = two_way("zmag");
    assert_eq!(ty, sca(DimensionVector::LENGTH), "complex_magnitude forward type");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::LENGTH, "complex_magnitude eval dimension");
            assert!((si_value - 5.0).abs() < EPS, "complex_magnitude eval value, got {si_value}");
        }
        other => panic!("complex_magnitude eval should be Value::Scalar, got {other:?}"),
    }
}

/// `phase(complex(1m,1m))` ⟺ `Angle` (== `Scalar<ANGLE>`) / eval
/// `Scalar { π/4, ANGLE }` (atan2(1,1)).
#[test]
fn phase_two_way_is_angle() {
    let (ty, val) = two_way("zph");
    assert_eq!(ty, Type::angle(), "phase forward type must be Angle (Scalar<ANGLE>)");
    match val {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(dimension, DimensionVector::ANGLE, "phase eval dimension");
            assert!(
                (si_value - std::f64::consts::FRAC_PI_4).abs() < EPS,
                "phase eval value should be ≈π/4, got {si_value}"
            );
        }
        other => panic!("phase eval should be Value::Scalar, got {other:?}"),
    }
}

// ── Permissive enforcement (PRD §5) ───────────────────────────────────────────

/// A dimensioned non-singular diagonal 4×4 `Length` matrix literal, shared by
/// the enforcement snippets. `determinant` of it propagates `Scalar<Length⁴>`.
const M4_LITERAL: &str =
    "matrix([[2.0m, 0.0m, 0.0m, 0.0m], [0.0m, 3.0m, 0.0m, 0.0m], [0.0m, 0.0m, 4.0m, 0.0m], [0.0m, 0.0m, 0.0m, 5.0m]])";

/// A typed `let bad : Length = determinant(m4)` now mismatches its annotation
/// (`Length`) against the propagated `Scalar<Length⁴>`. The diagnostic that
/// fires must be the EXISTING typed-let dimensional check (trait-default
/// injection cross-check, `TypeMismatchForTraitMember`) — δ introduces no new
/// bespoke determinant error. This is the PERMISSIVE-enforcement user signal:
/// δ only PROPAGATES the correct type; the diagnostic is downstream and
/// pre-existing.
#[test]
fn typed_let_determinant_mismatch_fires_existing_check() {
    let source = format!(
        r#"
trait HasDet {{
    let bad : Length = determinant({M4_LITERAL})
}}
structure UsesDet : HasDet {{
}}
"#
    );
    let module = compile_with_stdlib_helper(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errs.is_empty(),
        "`let bad : Length = determinant(m4)` must emit a dimension-mismatch Error now that \
         determinant propagates Scalar<Length⁴>; got none"
    );
    let msg = format!("{errs:?}");
    assert!(
        msg.contains("type mismatch for trait let"),
        "the diagnostic must be the EXISTING typed-let dimensional check (no new bespoke \
         determinant error); got: {msg}"
    );
}

/// The converse permissive guard: `determinant(m4)` WITHOUT a type annotation
/// must produce NO error at all. δ rejects only a statically-known mismatch the
/// user explicitly asked for (the annotation) — never the bare correctly-typed
/// result.
#[test]
fn determinant_without_annotation_emits_no_error() {
    let source = format!(
        r#"
structure def PlainDet {{
    let d = determinant({M4_LITERAL})
}}
"#
    );
    let module = compile_with_stdlib_helper(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "un-annotated determinant(m4) must produce NO new bespoke error (permissive); got: {errs:?}"
    );
}
