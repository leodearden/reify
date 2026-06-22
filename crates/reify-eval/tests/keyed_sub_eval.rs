//! Keyed<T> sub elaboration eval tests (task 3931 γ).
//!
//! Step-7 (RED until step-8) pins the leaf signal: a string index on a
//! `Keyed<T>` sub resolves, at eval time, to the keyed member's value with its
//! per-key override applied.
//!
//!   `sub vents : Keyed<Vent> { "intake" => { area = 5mm } }`
//!   `let a = vents["intake"].area`   // evaluates to 5mm
//!
//! User-observable signal:
//!   cargo test -p reify-eval --test keyed_sub_eval

use reify_core::{Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{
    assert_has_diagnostic, compile_source, make_simple_engine, parse_and_compile_with_stdlib,
};

/// The canonical keyed-vents module: two keyed members with per-key `area`
/// overrides, plus two lets that resolve each member's `area` by key.
const KEYED_VENTS_SRC: &str = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
        "exhaust" => { area = 8mm }
    }
    let a = vents["intake"].area
    let b = vents["exhaust"].area
}
"#;

/// Leaf signal (task 3931 γ): `vents["intake"].area == 5mm` and
/// `vents["exhaust"].area == 8mm`, with the per-key scoped child cell
/// `Manifold.vents["intake"].area` carrying the override applied via `args`.
///
/// RED today: no keyed elaboration exists, so the per-key scoped child cells
/// are never created and `a`/`b` resolve to `Undef`. Flips GREEN after step-8.
#[test]
fn keyed_member_access_evaluates_to_override_value() {
    let module = parse_and_compile_with_stdlib(KEYED_VENTS_SRC);
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);

    let a = result.values.get(&ValueCellId::new("Manifold", "a"));
    assert_eq!(
        a,
        Some(&Value::length(0.005)),
        "vents[\"intake\"].area must evaluate to 5mm (0.005m), got {a:?}",
    );

    let b = result.values.get(&ValueCellId::new("Manifold", "b"));
    assert_eq!(
        b,
        Some(&Value::length(0.008)),
        "vents[\"exhaust\"].area must evaluate to 8mm (0.008m), got {b:?}",
    );

    // The per-key scoped child cell must carry the override (area = 5mm) applied
    // to the "intake" child entity at scope `Manifold.vents["intake"]`.
    let intake_area = result
        .values
        .get(&ValueCellId::new("Manifold.vents[\"intake\"]", "area"));
    assert_eq!(
        intake_area,
        Some(&Value::length(0.005)),
        "scoped cell Manifold.vents[\"intake\"].area must be 5mm, got {intake_area:?}",
    );
}

/// A module that accesses a missing keyed key (`ghost` ∉ {intake}). The keyed
/// member set is statically known, so the miss is a compile diagnostic — but
/// the access still lowers to `Undef`, so eval must proceed WITHOUT a panic and
/// leave the `b` cell `Undef` (spec §3.4: a clean eval-graph failure).
const MISSING_KEY_SRC: &str = r#"
structure def Vent {
    param area : Length = 1mm
}
structure def Manifold {
    sub vents : Keyed<Vent> {
        "intake" => { area = 5mm }
    }
    let a = vents["intake"].area
    let b = vents["ghost"].area
}
"#;

/// Missing-key eval (task 3931 γ, spec §3.4): a `vents["ghost"]` access must NOT
/// panic the engine; the named missing-key diagnostic is emitted at compile
/// time and the `b` cell resolves to `Undef`, while the valid `a` still
/// evaluates to 5mm. Uses the raw `compile_source` + `make_simple_engine().eval`
/// pipeline (NOT `parse_and_compile_with_stdlib`, which panics on the error).
#[test]
fn missing_keyed_key_eval_is_clean_failure_not_panic() {
    // compile_source does NOT assert no-errors, so the missing-key Error is
    // retained rather than panicking the test harness.
    let module = compile_source(MISSING_KEY_SRC);
    assert_has_diagnostic(
        &module.diagnostics,
        Severity::Error,
        "no keyed member 'ghost' in keyed sub 'vents'",
    );

    // Raw eval — must not panic on the Undef-resolving ghost access.
    let mut engine = make_simple_engine();
    let result = engine.eval(&module);

    // The valid key still evaluates.
    assert_eq!(
        result.values.get(&ValueCellId::new("Manifold", "a")),
        Some(&Value::length(0.005)),
        "valid key vents[\"intake\"].area must still evaluate to 5mm",
    );

    // The missing key resolves to Undef (present-as-Undef or absent — never a
    // wrong concrete value, never a panic).
    let b = result.values.get(&ValueCellId::new("Manifold", "b"));
    assert!(
        b.is_none() || b == Some(&Value::Undef),
        "missing-key cell `b` must be Undef or absent, got {b:?}",
    );
}
