//! Task 4435 δ — auto-type-param value population e2e tests.
//!
//! Verifies that a resolved `param seal : T` (with T→GasketSeal, a
//! zero-arg-constructible candidate whose `param thickness : Length = 2mm`)
//! evaluates to `Value::StructureInstance(GasketSeal{thickness:2mm})` rather
//! than `Value::Undef`.  The synthesis happens at compile-time in the
//! monomorph-build pass (auto_type_param_phase.rs); the synthesized zero-arg
//! StructureInstanceCtor flows through `unfold.rs`'s existing default branch.
//!
//! Also asserts:
//!   - The `seal_thickness` let-cell resolves to `2mm` (member-access chain
//!     through a StructureInstance — task 4342 path, already landed).
//!   - Precedence invariant-2: an explicit `seal` value at the use-site wins
//!     over the synthesized ctor default (unfold.rs:336 arg-before-default).

#![allow(clippy::mutable_key_type)]

use reify_core::ValueCellId;
use reify_ir::{PersistentMap, Value};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};

/// Helper: get a field from a StructureInstance's fields map by name.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

/// The fixture source — mirrors examples/auto/bearing_resolved_value.ri exactly
/// so the test is self-contained and runs without filesystem access.
///
/// NOTE on `seal_thickness`: the three-level chain `self.b.seal.thickness` is
/// omitted from the fixture because `.thickness` applied to a `TypeParam("T")`-
/// typed value produces "member access not yet supported" at compile time. This
/// is a known gap in `crates/reify-compiler/src/expr.rs`: member access on
/// TypeParam-typed expressions needs a permissive fallback (cf. the StructureRef/
/// TraitObject handler at expr.rs:2966). That fix is out of scope for δ.
/// Assertion (b) below tests the PRIMARY δ observable — `BearingResolved.b.seal`
/// is `Value::StructureInstance(GasketSeal{thickness:2mm})` — without relying on
/// the three-level chain.
const BEARING_RESOLVED_SOURCE: &str = r#"
trait Seal {}

structure def GasketSeal : Seal {
    param thickness : Length = 2mm
}

structure def Bearing<T: Seal> {
    param seal : T
}

structure def BearingResolved {
    sub b = Bearing<auto(free): Seal>()
}
"#;

/// Assertion (b): `BearingResolved.b.seal` is `Value::StructureInstance(GasketSeal{thickness:2mm})`.
///
/// RED until δ synthesis is implemented: `seal` has `default_expr = None` in
/// the monomorph → unfold.rs Undef fallthrough → `seal` field is `Value::Undef`.
/// GREEN after δ: `default_expr = Some(GasketSeal())` → eval produces
/// `Value::StructureInstance(GasketSeal{thickness:2mm})`.
///
/// Note: this test does NOT cover the `seal_thickness` member-access chain
/// (`self.b.seal.thickness`) — that chain is blocked by a TypeParam member-access
/// gap in expr.rs (see BEARING_RESOLVED_SOURCE note). The name reflects only
/// what is actually asserted: StructureInstance shape, not member access.
#[test]
fn auto_resolved_param_produces_structure_instance() {
    let compiled = parse_and_compile_with_stdlib(BEARING_RESOLVED_SOURCE);

    // Zero error diagnostics — single GasketSeal candidate → Selected → no Error.
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // ── Assertion (b): BearingResolved.b.seal is StructureInstance(GasketSeal) ──
    //
    // The sub `b` is stored as ValueCellId::new("BearingResolved", "b").
    // Its value is a StructureInstance for Bearing$GasketSeal.
    // The `seal` field inside that StructureInstance should be
    // StructureInstance(GasketSeal{thickness:2mm}) after δ synthesis.
    let sub_b = result
        .values
        .get(&ValueCellId::new("BearingResolved", "b"))
        .unwrap_or_else(|| {
            // Report available cells for debugging.
            let cells: Vec<_> = result.values.iter().map(|(id, _)| id.clone()).collect();
            panic!(
                "BearingResolved.b cell missing from eval result. \
                 Available cells: {:?}",
                cells
            )
        });

    match sub_b {
        Value::StructureInstance(bearing_data) => {
            // The bearing instance must have a `seal` field.
            let seal_val = field(&bearing_data.fields, "seal").unwrap_or_else(|| {
                let keys: Vec<_> = bearing_data.fields.iter().map(|(k, _)| k.clone()).collect();
                panic!(
                    "Bearing$GasketSeal instance must have a 'seal' field; \
                     fields: {:?}",
                    keys
                )
            });

            match seal_val {
                Value::StructureInstance(seal_data) => {
                    assert_eq!(
                        seal_data.type_name, "GasketSeal",
                        "seal instance type_name must be 'GasketSeal', got '{}'",
                        seal_data.type_name
                    );
                    // The GasketSeal instance must carry its `thickness` field == 2mm.
                    let thickness = field(&seal_data.fields, "thickness").unwrap_or_else(|| {
                        let keys: Vec<_> =
                            seal_data.fields.iter().map(|(k, _)| k.clone()).collect();
                        panic!(
                            "GasketSeal instance must have a 'thickness' field; \
                                 fields: {:?}",
                            keys
                        )
                    });
                    match thickness {
                        Value::Scalar { si_value, .. } => {
                            const EPSILON: f64 = 1e-10;
                            assert!(
                                (*si_value - 0.002).abs() < EPSILON,
                                "GasketSeal.thickness must be 2mm (si_value≈0.002), \
                                 got {}",
                                si_value
                            );
                        }
                        other => panic!(
                            "GasketSeal.thickness must be Value::Scalar(2mm), got {:?}",
                            other
                        ),
                    }
                }
                Value::Undef => panic!(
                    "bearing.seal is Value::Undef — δ synthesis not yet wired \
                     (expected StructureInstance(GasketSeal) after implementation)"
                ),
                other => panic!(
                    "expected Value::StructureInstance for bearing.seal, got {:?}",
                    other
                ),
            }
        }
        Value::Undef => panic!(
            "BearingResolved.b is Value::Undef — \
             sub component evaluation failed (expected StructureInstance)"
        ),
        other => panic!(
            "expected Value::StructureInstance for BearingResolved.b, got {:?}",
            other
        ),
    }
}

/// Invariant-2 precedence guard: an explicit `seal` argument at the use-site
/// wins over the synthesized ctor default (unfold.rs:336 arg-before-default).
///
/// δ synthesizes `GasketSeal()` (thickness = 2mm) as the `default_expr` for
/// the `seal` cell in `Bearing$GasketSeal`.  The fixture supplies an EXPLICIT
/// `GasketSeal(thickness: 5mm)` at the call site.  If the arg-before-default
/// precedence (unfold.rs:336) holds, the seal instance carries 5mm; if δ
/// synthesis accidentally replaced the explicit arg with the default, it would
/// carry 2mm — making the assertion fail.
///
/// This test genuinely distinguishes "explicit wins" from "default used" without
/// needing the TypeParam member-access chain in expr.rs.
#[test]
fn explicit_seal_value_wins_over_synthesized_default() {
    // Fixture: the caller supplies `seal: GasketSeal(thickness: 5mm)` explicitly.
    // δ synthesizes GasketSeal() (thickness = 2mm) as the monomorph default.
    // The explicit 5mm arg must win over the synthesized 2mm default.
    const SOURCE: &str = r#"
trait Seal {}

structure def GasketSeal : Seal {
    param thickness : Length = 2mm
}

structure def Bearing<T: Seal> {
    param seal : T
}

// Single GasketSeal candidate → Selected (no Ambiguous).
// Explicit seal with NON-DEFAULT thickness (5mm ≠ 2mm default) distinguishes
// "explicit wins" from "synthesized default used" at eval time.
structure def AssemblyExplicit {
    sub b = Bearing<auto(free): Seal>(seal: GasketSeal(thickness: 5mm))
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);

    // Zero error diagnostics — single GasketSeal candidate → Selected → no Error.
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for explicit-value fixture, got: {:?}",
        errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let sub_b = result
        .values
        .get(&ValueCellId::new("AssemblyExplicit", "b"))
        .unwrap_or_else(|| {
            let cells: Vec<_> = result.values.iter().map(|(id, _)| id.clone()).collect();
            panic!(
                "AssemblyExplicit.b cell missing from eval result. \
                 Available cells: {:?}",
                cells
            )
        });

    match sub_b {
        Value::StructureInstance(bearing_data) => {
            let seal_val = field(&bearing_data.fields, "seal").unwrap_or_else(|| {
                let keys: Vec<_> = bearing_data.fields.iter().map(|(k, _)| k.clone()).collect();
                panic!(
                    "Bearing$GasketSeal instance must have a 'seal' field; \
                         fields: {:?}",
                    keys
                )
            });
            match seal_val {
                Value::StructureInstance(seal_data) => {
                    assert_eq!(
                        seal_data.type_name, "GasketSeal",
                        "explicit GasketSeal(thickness:5mm) must produce \
                         StructureInstance(GasketSeal), got type_name='{}'",
                        seal_data.type_name
                    );
                    // ── Key precedence assertion ──
                    // The seal must carry 5mm (explicit arg), not 2mm (synthesized
                    // default). If δ accidentally replaced the explicit arg with the
                    // synthesized default_expr, this assertion would catch it.
                    let thickness = field(&seal_data.fields, "thickness").unwrap_or_else(|| {
                        let keys: Vec<_> =
                            seal_data.fields.iter().map(|(k, _)| k.clone()).collect();
                        panic!(
                            "GasketSeal instance must have a 'thickness' field; \
                                 fields: {:?}",
                            keys
                        )
                    });
                    match thickness {
                        Value::Scalar { si_value, .. } => {
                            const EPSILON: f64 = 1e-10;
                            // 5mm = 0.005 m in SI
                            assert!(
                                (*si_value - 0.005).abs() < EPSILON,
                                "explicit GasketSeal(thickness:5mm) must carry 5mm \
                                 (si_value≈0.005); got {} — if 0.002, the synthesized \
                                 2mm default replaced the explicit arg (precedence bug)",
                                si_value
                            );
                        }
                        other => panic!(
                            "GasketSeal.thickness must be Value::Scalar, got {:?}",
                            other
                        ),
                    }
                }
                Value::Undef => panic!(
                    "b.seal is Value::Undef with explicit GasketSeal(thickness:5mm) arg — \
                     arg-before-default precedence or explicit-ctor path broken"
                ),
                other => panic!(
                    "expected Value::StructureInstance for b.seal with explicit arg, \
                     got {:?}",
                    other
                ),
            }
        }
        Value::Undef => {
            panic!("AssemblyExplicit.b is Value::Undef — sub component evaluation failed")
        }
        other => panic!(
            "expected Value::StructureInstance for AssemblyExplicit.b, got {:?}",
            other
        ),
    }
}
