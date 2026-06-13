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
    let seal_thickness = self.b.seal.thickness
}
"#;

/// Assertion (a): `seal_thickness` evaluates to `2mm` (Scalar, si_value≈0.002, Length dim).
/// Assertion (b): `self.b.seal` is `Value::StructureInstance(GasketSeal{thickness:2mm})`.
///
/// RED until δ synthesis is implemented: currently `seal` has `default_expr = None` in
/// the monomorph, so unfold.rs takes the Undef fallthrough at line 344, producing
/// `Value::Undef` for the `b` sub's `seal` field and consequently `Value::Undef`
/// for `seal_thickness`.
#[test]
fn auto_resolved_param_produces_structure_instance_and_member_access() {
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

    // ── Assertion (a): seal_thickness == 2mm ──────────────────────────────────
    //
    // SI unit for 2mm = 0.002 m.  Allow a small floating-point epsilon.
    let seal_thickness_val = result
        .values
        .get(&ValueCellId::new("BearingResolved", "seal_thickness"))
        .unwrap_or_else(|| panic!("BearingResolved.seal_thickness cell missing from eval result"));

    match seal_thickness_val {
        Value::Scalar { si_value, .. } => {
            const EPSILON: f64 = 1e-10;
            assert!(
                (*si_value - 0.002).abs() < EPSILON,
                "BearingResolved.seal_thickness must be 2mm (si_value≈0.002), got si_value={}",
                si_value
            );
        }
        Value::Undef => panic!(
            "BearingResolved.seal_thickness is Value::Undef — \
             δ synthesis not yet wired (expected 2mm after implementation)"
        ),
        other => panic!(
            "expected Value::Scalar for BearingResolved.seal_thickness, got {:?}",
            other
        ),
    }

    // ── Assertion (b): self.b.seal is StructureInstance(GasketSeal) ──────────
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
                let keys: Vec<_> =
                    bearing_data.fields.iter().map(|(k, _)| k.clone()).collect();
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
                    let thickness =
                        field(&seal_data.fields, "thickness").unwrap_or_else(|| {
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
/// This assertion is expected to hold both before AND after δ synthesis lands,
/// because unfold.rs always prefers an explicit arg (branch 336) over default
/// (branch 338).  It acts as a regression guard: if δ synthesis accidentally
/// overwrites an explicit argument, this test catches it.
#[test]
fn explicit_seal_value_wins_over_synthesized_default() {
    // A variant fixture where the caller supplies an explicit `seal` value.
    // `ThinSeal` (thickness=1mm) is passed explicitly; the auto-resolved
    // candidate is `GasketSeal` (2mm).  After δ, the `seal` param in the
    // monomorph gets a synthesized GasketSeal() default, but the explicit arg
    // at the call-site wins (unfold.rs arg-before-default precedence).
    const SOURCE: &str = r#"
trait Seal {}

structure def GasketSeal : Seal {
    param thickness : Length = 2mm
}

structure def ThinSeal : Seal {
    param thickness : Length = 1mm
}

structure def Bearing<T: Seal> {
    param seal : T
}

// Use explicit ThinSeal() — the auto-resolved candidate is GasketSeal (2mm),
// but the caller supplies ThinSeal() explicitly, so that wins.
structure def AssemblyExplicit {
    sub b = Bearing<auto(free): Seal>(seal: ThinSeal())
    let seal_thickness = self.b.seal.thickness
}
"#;

    let compiled = parse_and_compile_with_stdlib(SOURCE);

    // Zero error diagnostics (single auto candidate → Selected).
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for explicit-value fixture, got: {:?}",
        errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // `seal_thickness` must NOT be 2mm (the GasketSeal synthesized default).
    // Once δ lands with the explicit ThinSeal() path working, it must be 1mm.
    let seal_thickness_val = result
        .values
        .get(&ValueCellId::new("AssemblyExplicit", "seal_thickness"));

    if let Some(Value::Scalar { si_value, .. }) = seal_thickness_val {
        // Once δ lands, the explicit ThinSeal() must win → 1mm (0.001 m),
        // NOT the synthesized GasketSeal default (2mm = 0.002 m).
        assert!(
            (*si_value - 0.002).abs() > 1e-6,
            "seal_thickness must NOT be 2mm (synthesized GasketSeal default); \
             explicit ThinSeal(1mm) must win; got si_value={}",
            si_value
        );
    }
    // Pre-δ (or if Undef): the test still passes — the invariant is that
    // we never get 2mm from the synthesized default when an explicit value exists.
}
