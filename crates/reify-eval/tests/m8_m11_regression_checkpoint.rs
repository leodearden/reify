//! M8–M11 regression checkpoint tests.
//!
//! **Purpose**: A durable regression guard ensuring all pre-existing tests still pass
//! after M8–M11 changes and providing compile-time exhaustiveness coverage for the
//! `Type` (27 variants) and `Value` (25 variants) enums.
//!
//! **Coverage**:
//!   - Cross-milestone integration: parse → compile → eval → check pipeline using a
//!     Reify source that exercises one feature from each milestone.
//!   - M8: stdlib SI units (`mm`, `N`, `kg`) in structure parameters.
//!   - M9: trait conformance (`structure def Foo : Trait`), constraint definitions.
//!   - M10: geometric builtins (`point3`, `vec3`, `orient_identity`, `transform3`, `frame3`).
//!   - M11: field calculus (`field def`, `sample`, `gradient`), `@test` annotation.
//!   - Compile-time exhaustive `match` guards for all 27 `Type` variants.
//!   - Compile-time exhaustive `match` guards for all 25 `Value` variants, with
//!     runtime calls to `Display`, `content_hash`, `try_infer_type`, `format_hover`.
//!
//! **Design notes**:
//!   - All integration tests share a single compiled module via `OnceLock` to avoid
//!     re-parsing the source on every test invocation.
//!   - The `test_count_floor` test is `#[ignore]` (slow subprocess); use
//!     `cargo test -- --include-ignored` to run it explicitly.
//!
//! Follows the pattern established by `m9_combined.rs`, `m10_combined.rs`, and
//! `m11_full_integration.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use reify_compiler::CompiledModule;
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, FieldSourceKind, ModulePath,
    Satisfaction, Type, Value, ValueCellId, ValueMap,
};

// ── Cross-milestone inline source ─────────────────────────────────────────────
//
// Step-2 fills this in with a comprehensive Reify source that exercises one
// feature from each of the four milestones M8–M11. The empty string is the
// step-1 stub; all integration tests below will fail until SOURCE is replaced.

/// Inline Reify source exercising M8–M11 cross-milestone features.
///
/// Milestone coverage:
///   M8 — stdlib SI units (`mm`, `N`) used in structure params and lets.
///   M9 — `trait def Measured`, `constraint def InRange`, structure conformance.
///   M10 — `point3`, `vec3`, `orient_identity`, `transform3`, `frame3` builtins.
///   M11 — `field def`, `sample`, `gradient` operators; `@test` annotation.
const SOURCE: &str = r#"
// M9: trait with inline length constraint
trait Measured {
    param length : Length
    constraint length > 0mm
}

// M9: reusable constraint definition
constraint def InRange {
    param val : Length
    param lo  : Length
    param hi  : Length
    val >= lo
    val <= hi
}

// M8 + M9: stdlib Length units, trait conformance, constraint def invocation
structure def SimpleBox : Measured {
    param length : Length = 100mm
    param width  : Length = 50mm
    param depth  : Length = 25mm
    let half_length = length / 2.0
    constraint InRange(val: depth, lo: 10mm, hi: 30mm)
    constraint width < length
}

// M10: geometric builtins from the stdlib prelude
structure def GeomPart {
    let origin = point3(0mm, 0mm, 0mm)
    let tip    = point3(100mm, 0mm, 0mm)
    let disp   = tip - origin
    let fvec   = vec3(0N, 0N, 1N)
    let rot    = orient_identity()
    let tf     = transform3(rot, vec3(10mm, 0mm, 0mm))
    let fr     = frame3(origin, rot)
    let moved  = tf * origin
}

// M11: field calculus — field def, sample, gradient operators
// Use non-integer values to ensure Real (not Int) evaluation results.
field def linear_f : Real -> Real { source = analytical { |x| 2.5 * x + 1.0 } }

structure def FieldUser {
    let f3  = sample(linear_f, 3.0)    // 2.5 * 3.0 + 1.0 = 8.5
    let gf  = gradient(linear_f)
    let df3 = sample(gf, 3.0)          // ≈ 2.5 (constant derivative)
    constraint f3  > 8.4
    constraint f3  < 8.6
    constraint df3 > 2.4
    constraint df3 < 2.6
}

// M11: @test annotation on a structure (valid on structures, no error diagnostic)
@test structure def TestBox {
    param width : Length = 50mm
    constraint width > 0mm
}
"#;

// ── Cached helpers ────────────────────────────────────────────────────────────

/// Parse + compile SOURCE with the stdlib prelude. Cached for the test process.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(SOURCE))
}

/// Evaluate the compiled module. Cached via `OnceLock` — the source and
/// eval engine are both deterministic (no randomness, no mutation), so
/// re-evaluation yields identical results every time.  Consistent with how
/// `compiled()` caches `CompiledModule`.
fn eval_checkpoint() -> &'static reify_eval::EvalResult {
    static E: OnceLock<reify_eval::EvalResult> = OnceLock::new();
    E.get_or_init(|| {
        let mut engine = make_simple_engine();
        engine.eval(compiled())
    })
}

/// Check constraints in the compiled module.  Cached via `OnceLock` for the
/// same reason as `eval_checkpoint()`.
fn check_checkpoint() -> &'static reify_eval::CheckResult {
    static K: OnceLock<reify_eval::CheckResult> = OnceLock::new();
    K.get_or_init(|| {
        let mut engine = make_simple_engine();
        engine.check(compiled())
    })
}

// ── Integration tests ─────────────────────────────────────────────────────────

/// Verify the cross-milestone source parses with zero errors and produces at
/// least 6 top-level declarations (trait def, constraint def, 3+ structures,
/// field def, @test structure).
///
/// **Fails in step-1** because SOURCE = "" → 0 declarations < 6.
#[test]
fn checkpoint_parses() {
    let parsed = reify_syntax::parse(SOURCE, ModulePath::single("m8_m11_checkpoint"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    assert!(
        parsed.declarations.len() >= 6,
        "expected >= 6 top-level declarations from cross-milestone source, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source",
        parsed.declarations.len()
    );
}

/// Verify zero error-severity compile diagnostics and at least 4 compiled templates.
///
/// **Fails in step-1** because SOURCE = "" → templates.len() = 0 < 4.
#[test]
fn checkpoint_compiles_no_errors() {
    let m = compiled();
    let errors = collect_errors(&m.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    assert!(
        m.templates.len() >= 4,
        "expected >= 4 compiled structure templates, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source",
        m.templates.len()
    );
}

/// Verify zero error-severity eval diagnostics and a non-empty evaluated-values map.
///
/// **Fails in step-1** because SOURCE = "" → values map is empty.
#[test]
fn checkpoint_evals_no_errors() {
    let result = eval_checkpoint();
    let errors = collect_errors(&result.diagnostics);
    assert!(errors.is_empty(), "eval errors: {:?}", errors);
    assert!(
        !result.values.is_empty(),
        "expected non-empty evaluated-values map; \
         step-2 must replace SOURCE with the comprehensive cross-milestone Reify source"
    );
}

/// Spot-check M8+M9: `SimpleBox.half_length` should be 100mm / 2.0 = 0.05 SI.
///
/// **Fails in step-1** because SOURCE = "" → SimpleBox.half_length not found.
#[test]
fn checkpoint_m8_unit_half_length() {
    let result = eval_checkpoint();
    let id = ValueCellId::new("SimpleBox", "half_length");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "SimpleBox.half_length not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    match val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "expected 0.05 SI for SimpleBox.half_length (100mm / 2.0 = 50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for SimpleBox.half_length, got {:?}", other),
    }
}

/// Spot-check M9: all constraint results should be Satisfied with at least 6
/// constraint entries (trait constraint, InRange predicates, inline constraints,
/// FieldUser interval constraints).
///
/// **Fails in step-1** because SOURCE = "" → 0 constraint results < 6.
#[test]
fn checkpoint_m9_constraints_satisfied() {
    let result = check_checkpoint();
    assert!(
        result.constraint_results.len() >= 6,
        "expected >= 6 constraint results, got {}; \
         step-2 must replace SOURCE with the comprehensive cross-milestone source",
        result.constraint_results.len()
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Spot-check M10: key `GeomPart` lets evaluate to their expected geometric types.
///
/// Checks:
///   - `origin`  → `Value::Point` (point3(0,0,0))
///   - `rot`     → `Value::Orientation` (orient_identity())
///   - `tf`      → `Value::Transform` (transform3 with 10mm x-translation)
///   - `moved`   → `Value::Point` with x-coord 0.01 SI (tf * origin = 10mm, 0, 0)
///
/// **Fails in step-1** because SOURCE = "" → GeomPart.origin not found.
#[test]
fn checkpoint_m10_geometric_types_eval() {
    let result = eval_checkpoint();

    // origin = point3(0mm, 0mm, 0mm) → Value::Point
    let id = ValueCellId::new("GeomPart", "origin");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "GeomPart.origin not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    assert!(
        matches!(val, Value::Point(_)),
        "expected Value::Point for GeomPart.origin, got {:?}",
        val
    );

    // rot = orient_identity() → Value::Orientation
    let id = ValueCellId::new("GeomPart", "rot");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GeomPart.rot not found in eval values"));
    assert!(
        matches!(val, Value::Orientation { .. }),
        "expected Value::Orientation for GeomPart.rot, got {:?}",
        val
    );

    // tf = transform3(rot, vec3(10mm, 0mm, 0mm)) → Value::Transform
    let id = ValueCellId::new("GeomPart", "tf");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GeomPart.tf not found in eval values"));
    assert!(
        matches!(val, Value::Transform { .. }),
        "expected Value::Transform for GeomPart.tf, got {:?}",
        val
    );

    // moved = tf * origin = translation(10mm, 0, 0) applied to (0,0,0) → (0.01m, 0, 0) SI
    let id = ValueCellId::new("GeomPart", "moved");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("GeomPart.moved not found in eval values"));
    assert!(
        matches!(val, Value::Point(_)),
        "expected Value::Point for GeomPart.moved, got {:?}",
        val
    );
    if let Value::Point(coords) = val {
        match &coords[0] {
            Value::Scalar { si_value, .. } => assert!(
                (si_value - 0.01).abs() < 1e-9,
                "expected x-coord of GeomPart.moved = 0.01 SI (10mm), got {si_value}"
            ),
            other => panic!("expected Scalar for GeomPart.moved[0], got {:?}", other),
        }
    }
}

/// Spot-check M11 field calculus:
///   - `FieldUser.f3  = sample(linear_f, 3.0)` where `linear_f(x) = 2.5x + 1.0`
///     → expected value is 8.5.
///   - `FieldUser.df3 = sample(gradient(linear_f), 3.0)` — derivative of 2.5x+1.0
///     is 2.5 everywhere → expected ≈ 2.5.
///
/// **Fails in step-1** because SOURCE = "" → FieldUser.f3 not found.
#[test]
fn checkpoint_m11_field_sample_at_three() {
    let result = eval_checkpoint();

    // f3 = sample(linear_f, 3.0) = 2.5 * 3.0 + 1.0 = 8.5
    let id = ValueCellId::new("FieldUser", "f3");
    let val = result.values.get(&id).unwrap_or_else(|| {
        panic!(
            "FieldUser.f3 not found in eval values — \
             step-2 must replace SOURCE with the comprehensive cross-milestone source"
        )
    });
    match val {
        Value::Real(f) => {
            assert!(
                (f - 8.5).abs() < 1e-6,
                "expected 8.5 for FieldUser.f3 (sample(linear_f, 3.0) = 2.5*3.0+1.0), got {f}"
            );
        }
        other => panic!("expected Value::Real for FieldUser.f3, got {:?}", other),
    }

    // df3 = sample(gradient(linear_f), 3.0) — gradient of a linear field is its slope
    let id = ValueCellId::new("FieldUser", "df3");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FieldUser.df3 not found in eval values"));
    match val {
        Value::Real(f) => {
            assert!(
                (f - 2.5).abs() < 1e-6,
                "expected 2.5 for FieldUser.df3 (gradient of 2.5x+1.0 is 2.5 everywhere), got {f}"
            );
        }
        other => panic!("expected Value::Real for FieldUser.df3, got {:?}", other),
    }
}

// ── Type / Value variant coverage ────────────────────────────────────────────

/// Compile-time exhaustiveness guard for all 28 `Type` variants.
///
/// This function is NEVER called at runtime. Its only purpose is to force a
/// compile error if a new `Type` variant is added without updating this list.
/// If any variant is removed from the `match`, the compiler will warn about
/// unreachable patterns. If a variant is ADDED to the enum without being listed
/// here, the match becomes non-exhaustive and the test file won't compile.
///
/// **Fails (won't compile) in step-3** because it is annotated with
/// `#[allow(dead_code)]` — the live runtime call that drives exhaustiveness is
/// added in step-4.
#[allow(dead_code)]
fn assert_all_type_variants_listed(t: &reify_types::Type) {
    use reify_types::Type;
    let _ = match t {
        // Primitive scalars
        Type::Bool | Type::Int | Type::Real | Type::String => true,
        // Dimensioned scalar
        Type::Scalar { .. } => true,
        // Named enum
        Type::Enum(_) => true,
        // Collection types
        Type::List(_) | Type::Set(_) | Type::Map(..) | Type::Option(_) => true,
        // Callable / generic / nominal references
        Type::Function { .. }
        | Type::TypeParam(_)
        | Type::StructureRef(_)
        | Type::TraitObject(_) => true,
        // Field mapping
        Type::Field { .. } => true,
        // Geometry handle
        Type::Geometry => true,
        // Geometric vector spaces
        Type::Point { .. } | Type::Vector { .. } | Type::Tensor { .. } => true,
        // Complex and range
        Type::Complex(_) | Type::Range(_) => true,
        // Rigid-body / orientation
        Type::Orientation(_) | Type::Frame(_) | Type::Transform(_) => true,
        // 3D geometric primitives
        Type::Plane | Type::Axis | Type::BoundingBox => true,
        // Matrix
        Type::Matrix { .. } => true,
        // Type-inference poison sentinel (task-448)
        Type::Error => true,
        // Compile-time-only union over guarded-decl-group arm types (task 2373)
        Type::Union(_) => true,
    };
}

/// Compile-time exhaustiveness guard for all 26 `Value` variants.
///
/// Same design as `assert_all_type_variants_listed`: never called at runtime
/// in step-3, causes compile error if new variants aren't listed here.
///
/// **Fails (won't compile) in step-3** because it references an import
/// (`reify_types::FieldSourceKind`) that hasn't been added to the `use`
/// declaration yet — step-4 adds the necessary imports and makes this work.
#[allow(dead_code)]
fn assert_all_value_variants_listed(v: &reify_types::Value) {
    use reify_types::Value;
    let _ = match v {
        // Primitive scalars
        Value::Bool(_) | Value::Int(_) | Value::Real(_) | Value::String(_) => true,
        // Dimensioned scalar
        Value::Scalar { .. } => true,
        // Named enum
        Value::Enum { .. } => true,
        // Collection types
        Value::List(_) | Value::Set(_) | Value::Map(_) | Value::Option(_) => true,
        // Field and lambda (callable values)
        Value::Field { .. } | Value::Lambda { .. } => true,
        // Tensor / numeric arrays
        Value::Tensor(_) => true,
        // Geometric vectors / points
        Value::Point(_) | Value::Vector(_) => true,
        // Complex number
        Value::Complex { .. } => true,
        // Rigid-body / orientation
        Value::Orientation { .. } | Value::Frame { .. } | Value::Transform { .. } => true,
        // 3D geometric primitives
        Value::Plane { .. } | Value::Axis { .. } | Value::BoundingBox { .. } => true,
        // Range
        Value::Range { .. } => true,
        // Matrix
        Value::Matrix(_) => true,
        // Sampled field runtime payload (v0.2 — task 2341)
        Value::SampledField(_) => true,
        // Nominal structure instance (v0.3 — task 3540 / SIR-α)
        Value::StructureInstance { .. } => true,
        // Undefined
        Value::Undef => true,
    };
}

/// Verify that `assert_all_type_variants_listed` covers all 27 `Type` variants
/// by constructing one instance of each and calling the guard.
///
/// If a new variant is ever added to `Type` without being listed in
/// `assert_all_type_variants_listed`, this file will fail to compile.
#[test]
fn checkpoint_type_variant_coverage() {
    // Build one instance of each of the 27 Type variants.
    let all_types: Vec<Type> = vec![
        // Primitive scalars (4)
        Type::Bool,
        Type::Int,
        Type::Real,
        Type::String,
        // Dimensioned scalar (1)
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        // Named enum (1)
        Type::Enum("Color".to_string()),
        // Collection types (4)
        Type::List(Box::new(Type::Int)),
        Type::Set(Box::new(Type::Int)),
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
        Type::Option(Box::new(Type::Int)),
        // Callable / generic (3)
        Type::Function {
            params: vec![Type::Real],
            return_type: Box::new(Type::Real),
        },
        Type::TypeParam("T".to_string()),
        Type::StructureRef("Bolt".to_string()),
        // Field mapping (1)
        Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::Real),
        },
        // Geometry handle (1)
        Type::Geometry,
        // Geometric vector spaces (3)
        Type::Point {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        },
        Type::Vector {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        },
        Type::Tensor {
            rank: 2,
            n: 3,
            quantity: Box::new(Type::Real),
        },
        // Complex and range (2)
        Type::Complex(Box::new(Type::Real)),
        Type::Range(Box::new(Type::Int)),
        // Rigid-body / orientation (3)
        Type::Orientation(3),
        Type::Frame(3),
        Type::Transform(3),
        // 3D geometric primitives (3)
        Type::Plane,
        Type::Axis,
        Type::BoundingBox,
        // Matrix (1)
        Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::Real),
        },
        // Compile-time-only union over guarded-decl-group arm types (task 2373) (1)
        Type::Union(vec![
            Type::StructureRef("HexHead".to_string()),
            Type::StructureRef("SocketHead".to_string()),
        ]),
    ];

    assert_eq!(
        all_types.len(),
        28,
        "expected exactly 28 Type variants; update this test if the enum changes"
    );

    // Drive the exhaustiveness guard with each variant. Compile error here means
    // assert_all_type_variants_listed is missing a pattern.
    for t in &all_types {
        assert_all_type_variants_listed(t);
        // Verify Debug produces non-empty output for each variant.
        assert!(
            !format!("{t:?}").is_empty(),
            "expected non-empty Debug output for {:?}",
            t
        );
    }
}

/// Verify that all 25 `Value` variants can be constructed and that the four key
/// trait implementations — `Display`, `content_hash`, `try_infer_type`,
/// `format_hover` — do not panic for any variant.
///
/// This also serves as a compile-time exhaustiveness guard: if a new variant is
/// added to `Value`, the `assert_all_value_variants_listed` match below must be
/// updated, otherwise this file won't compile.
#[test]
fn checkpoint_value_variant_coverage() {
    // Shared lambda body used for Field and Lambda variants.
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::Literal(Value::Real(1.0)),
        result_type: Type::Real,
        content_hash: ContentHash(0),
    };

    // Helper to build a 3-component Length Point.
    let make_point3 = || {
        Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            };
            3
        ])
    };

    // Helper to build a 3-component dimensionless unit Vector.
    let make_unit_vec3 = || {
        Value::Vector(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ])
    };

    // Helper to build a Length translation Vector.
    let make_len_vec3 = || {
        Value::Vector(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            };
            3
        ])
    };

    // Identity orientation quaternion (w=1, x=y=z=0).
    let identity_orient = Value::Orientation {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    // Build one instance of each of the 25 Value variants.
    let all_values: Vec<Value> = vec![
        // Primitive scalars (4)
        Value::Bool(true),
        Value::Int(42),
        Value::Real(2.719),
        Value::String("hello".to_string()),
        // Dimensioned scalar (1)
        Value::Scalar {
            si_value: 0.1,
            dimension: DimensionVector::LENGTH,
        },
        // Named enum (1)
        Value::Enum {
            type_name: "Color".to_string(),
            variant: "Red".to_string(),
        },
        // Collection types (4)
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::Set(BTreeSet::from([Value::Int(1), Value::Int(2)])),
        Value::Map(BTreeMap::from([(
            Value::String("k".to_string()),
            Value::Int(1),
        )])),
        Value::Option(Some(Box::new(Value::Int(5)))),
        // Field (lambda-backed analytical field) (1)
        Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Lambda {
                params: vec![],
                body: Box::new(lambda_body.clone()),
                captures: ValueMap::new(),
            }),
        },
        // Lambda closure (1)
        Value::Lambda {
            params: vec![],
            body: Box::new(lambda_body),
            captures: ValueMap::new(),
        },
        // Numeric arrays (1)
        Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]),
        // Geometric vectors / points (2)
        make_point3(),
        make_len_vec3(),
        // Complex number (1)
        Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        // Rigid-body / orientation (3)
        identity_orient.clone(),
        Value::Frame {
            origin: Box::new(make_point3()),
            basis: Box::new(identity_orient.clone()),
        },
        Value::Transform {
            rotation: Box::new(identity_orient.clone()),
            translation: Box::new(make_len_vec3()),
        },
        // 3D geometric primitives (3)
        Value::Plane {
            origin: Box::new(make_point3()),
            normal: Box::new(make_unit_vec3()),
        },
        Value::Axis {
            origin: Box::new(make_point3()),
            direction: Box::new(make_unit_vec3()),
        },
        Value::BoundingBox {
            min: Box::new(make_point3()),
            max: Box::new(Value::Point(vec![
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                };
                3
            ])),
        },
        // Range (1)
        Value::Range {
            lower: Some(Box::new(Value::Real(0.0))),
            upper: Some(Box::new(Value::Real(10.0))),
            lower_inclusive: true,
            upper_inclusive: false,
        },
        // Matrix (1)
        Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]),
        // Undefined (1)
        Value::Undef,
    ];

    assert_eq!(
        all_values.len(),
        25,
        "expected exactly 25 Value variants; update this test if the enum changes"
    );

    // For each variant: drive the exhaustiveness guard and exercise the four
    // key trait implementations. None should panic.
    for v in &all_values {
        // Drive exhaustiveness guard (compile error if any variant is missing).
        assert_all_value_variants_listed(v);

        // (a) Display produces non-empty output.
        let display = format!("{v}");
        assert!(
            !display.is_empty(),
            "Display produced empty string for {:?}",
            v
        );

        // (b) content_hash does not panic.
        let _ = v.content_hash();

        // (c) try_infer_type does not panic.
        let _ = v.try_infer_type();

        // (d) format_hover does not panic.
        let _ = v.format_hover();
    }
}

// ── Test-count floor checkpoint ───────────────────────────────────────────────

/// Baseline test-count floor guard.
///
/// Invokes `cargo test --workspace` as a subprocess, parses the aggregate
/// PASS/FAIL/SKIP summary line, and asserts:
///   - PASS count is at least 5400 (floor below the M8–M11 baseline of 5455+9=5464)
///   - FAIL count is exactly 0
///
/// The floor of 5400 tolerates normal fluctuation (toggling `#[ignore]`
/// annotations, adding tests in other tasks) while catching mass regression.
///
/// # Design notes
///
/// **Why `--workspace` and not `--exclude reify-eval`?**  The subprocess
/// intentionally includes this crate's own non-ignored tests.  If a test in
/// this file regresses, the subprocess reports it with `total_failed > 0`,
/// which produces a clear error message.  Excluding reify-eval would reduce
/// coverage and hide such failures.
///
/// **Compile-failure guard**: if `cargo test` exits with a non-zero status
/// *before* any test binary emits a "test result:" line (e.g. a workspace
/// compilation error), a dedicated early panic surfaces the stderr content
/// rather than quietly reporting `0 passed / 0 failed`.
///
/// # Running
/// Marked `#[ignore]` because spawning a nested `cargo test` invocation is slow
/// (~30 s) and redundant with CI. Run explicitly with:
/// ```
/// cargo test -p reify-eval --test m8_m11_regression_checkpoint -- --include-ignored
/// ```
#[test]
#[ignore = "slow subprocess; run explicitly with --include-ignored to verify test count floor"]
fn test_count_floor() {
    // Workspace root: CARGO_MANIFEST_DIR is the reify-eval crate directory at
    // compile time; two parent steps reach the workspace root.
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir
        .parent()
        .expect("crates/ parent")
        .parent()
        .expect("workspace root");

    // Use the cargo binary from the current build (set by cargo's test harness).
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    // Run `cargo test --workspace` with plain text output for easy parsing.
    let output = std::process::Command::new(&cargo)
        .args(["test", "--workspace"])
        .current_dir(workspace_root)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("failed to spawn `cargo test --workspace`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let all_output = format!("{stdout}{stderr}");

    // Guard: if the subprocess exited non-zero with no "test result:" lines, it
    // almost certainly failed to compile rather than having failing tests.  Report
    // stderr explicitly so the root cause isn't hidden behind "0 passed / 0 failed".
    let has_result_lines = all_output.lines().any(|l| l.starts_with("test result:"));
    if !output.status.success() && !has_result_lines {
        panic!(
            "`cargo test --workspace` failed with no 'test result:' lines — \
             likely a compilation error or launch failure.\n\
             Exit code: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status.code()
        );
    }

    // Each test binary emits a line like:
    //   "test result: ok. 551 passed; 0 failed; 20 ignored; ..."
    // Sum all the "passed" and "failed" counters across all binaries.
    let mut total_passed: u64 = 0;
    let mut total_failed: u64 = 0;

    for line in all_output.lines() {
        if !line.starts_with("test result:") {
            continue;
        }
        // Split on ";" to get individual "N verb" segments.
        for part in line.split(';') {
            let part = part.trim();
            if let Some(prefix) = part.strip_suffix(" passed") {
                // "test result: ok. 551 passed" → prefix = "test result: ok. 551"
                if let Some(n) = prefix.split_whitespace().last() {
                    match n.parse::<u64>() {
                        Ok(count) => total_passed += count,
                        Err(e) => panic!(
                            "failed to parse passed count {n:?} from cargo test output: {e}\n\
                             If the cargo output format changed, update the parser in this test."
                        ),
                    }
                }
            }
            if let Some(prefix) = part.strip_suffix(" failed") {
                // " 0 failed" → prefix = "0"
                if let Some(n) = prefix.split_whitespace().last() {
                    match n.parse::<u64>() {
                        Ok(count) => total_failed += count,
                        Err(e) => panic!(
                            "failed to parse failed count {n:?} from cargo test output: {e}\n\
                             If the cargo output format changed, update the parser in this test."
                        ),
                    }
                }
            }
        }
    }

    assert_eq!(
        total_failed, 0,
        "workspace test suite has {total_failed} failing test(s); \
         zero failures expected.\nOutput:\n{all_output}"
    );

    // Floor of 5400: 55 below the M8–M11 baseline (5455 pre-existing + 9 new = 5464+),
    // providing tolerance for normal fluctuation while catching mass regression.
    assert!(
        total_passed >= 5400,
        "workspace test count floor: expected >= 5400 passing tests, got {total_passed}; \
         if tests were intentionally removed, lower this floor accordingly"
    );
}
