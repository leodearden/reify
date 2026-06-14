//! RED test: structure-ctor named-argument binder — bind by name, not position.
//!
//! Fully self-contained — no stdlib. Reproduces the positional-binder misbind
//! when a structure refines a trait and a beyond-trait param (`c`) is declared
//! AFTER a defaulted trait param (`b`). A call that supplies `a` and `c` by
//! name (skipping `b`) should leave `b` at its default (1mm), but the positional
//! binder maps arg-index 1 (value 9mm) into param-index 1 (= `b`), giving b=9mm.
//!
//! Why self-contained (not using stdlib tolerancing structures):
//!   `reify_test_support::eval_source` compiles via `compile_source` which does
//!   NOT load stdlib. A snippet referencing `effective_tolerance_zone`,
//!   `MaterialCondition`, or `ZoneShape` would fail to compile — a doomed RED
//!   that fails for the wrong reason. This file reproduces the identical binder
//!   mechanism with plain `Length` units.
//!
//! Test setup:
//!   trait T { param a : Length; param b : Length = 1mm; let z = b }
//!   structure def S : T { param a : Length; param b : Length = 1mm; param c : Length = 5mm }
//!
//! `c` is the BEYOND-TRAIT param declared AFTER the defaulted `b`.
//!
//! Call `S(a: 2mm, c: 9mm)` — supplies `a` and `c` by name, SKIPPING `b`.
//! Expected: b = default = 1mm → z = b = 1mm.
//! Actual (bug): positional binder maps arg[1]=9mm → param[1]=b → b=9mm, z=9mm.
//!
//! After the fix (step-4 by-name binder), the RED test turns GREEN.
//!
//! End-to-end tolerancing fidelity for the stdlib case stays guarded by the
//! existing cli_tolerancing_eval.rs par_zone/pos_zone/soa_zone/runout_zone/prof_zone pins.

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;

/// Source: trait T with a beyond-trait param `c` in structure S.
///
/// Params in template declaration order: a (no default), b (default 1mm), c (default 5mm).
/// Call `S(a: 2mm, c: 9mm)` — named, skipping b.
///
/// Probe reads `zr = S(a: 2mm, c: 9mm).z` via a local let binding.
/// PositiveProbe supplies all three named args to guard against over-correction.
const SOURCE: &str = r#"
trait T {
    param a : Length
    param b : Length = 1mm
    let z = b
}

structure def S : T {
    param a : Length
    param b : Length = 1mm
    param c : Length = 5mm
}

structure Probe {
    let s = S(a: 2mm, c: 9mm)
    let zr = s.z
}

structure PositiveProbe {
    let s = S(a: 2mm, b: 3mm, c: 9mm)
    let zr = s.z
}
"#;

/// Named-argument binding for structure constructors: beyond-trait param `c`
/// does NOT displace the defaulted trait param `b` when called by name.
///
/// Call: `S(a: 2mm, c: 9mm)` — supplies `a` and `c` by name, leaving `b` at
/// its declared default of 1mm.
///
/// After eval, `Probe.zr = S(a:2mm, c:9mm).z = b = 1mm`.
///
/// RED today: the positional binder maps arg[1] = 9mm into param[1] = `b`,
/// yielding z = 9mm. The assertion `zr == 1mm` therefore fails.
/// GREEN after step-4: the by-name binder puts 9mm into `c`, b defaults to
/// 1mm, and z = 1mm.
#[test]
fn named_arg_beyond_trait_param_binds_by_name_not_position() {
    let values = reify_test_support::eval_source(SOURCE).values;

    let zr_id = ValueCellId::new("Probe", "zr");
    let zr = values.get(&zr_id).unwrap_or_else(|| {
        let mut present_keys: Vec<String> =
            values.iter().map(|(k, _)| k.to_string()).collect();
        present_keys.sort();
        panic!(
            "Probe.zr must be present in eval result; present keys: {present_keys:?}"
        )
    });

    match zr {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Probe.zr must be Length-dimensioned; got {:?}",
                dimension
            );
            // 1mm = 0.001 m SI (b's default value)
            let expected_si = 0.001_f64;
            assert!(
                (si_value - expected_si).abs() < 1e-12,
                "Probe.zr must be 1mm (b's default, 0.001 m SI); got {si_value} m SI.\n\
                 POSITIONAL BINDER BUG: args[1]=9mm was placed into param[1]=b\n\
                 instead of the named target c, so z=b=9mm instead of z=b=1mm.",
            );
        }
        other => panic!(
            "Probe.zr must be a Length Scalar; got {other:?}.\n\
             (An unexpected Value variant here indicates a compilation or eval issue\n\
              unrelated to the binder defect — check compile errors above.)"
        ),
    }
}

/// Positive control: when `b` is explicitly supplied, z reflects that value.
///
/// Call: `S(a: 2mm, b: 3mm, c: 9mm)` — all three params named.
/// After eval, `PositiveProbe.zr = z = b = 3mm`.
///
/// This guards against over-correction: the fix must not silently ignore
/// named args or always apply defaults, but must correctly bind each named
/// arg to the param of that name.
///
/// Passes both RED (today) and GREEN (after fix) because all params are
/// supplied: positional and by-name give the same result when the call order
/// matches declaration order and all params are provided.
#[test]
fn named_arg_explicit_b_overrides_default() {
    let values = reify_test_support::eval_source(SOURCE).values;

    let zr_id = ValueCellId::new("PositiveProbe", "zr");
    let zr = values
        .get(&zr_id)
        .unwrap_or_else(|| panic!("PositiveProbe.zr must be present in eval result"));

    match zr {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "PositiveProbe.zr must be Length-dimensioned; got {:?}",
                dimension
            );
            // 3mm = 0.003 m SI (explicitly supplied b=3mm)
            let expected_si = 0.003_f64;
            assert!(
                (si_value - expected_si).abs() < 1e-12,
                "PositiveProbe.zr must be 3mm (explicitly supplied b=3mm, 0.003 m SI); \
                 got {si_value} m SI",
            );
        }
        other => panic!(
            "PositiveProbe.zr must be a Length Scalar; got {other:?}"
        ),
    }
}
