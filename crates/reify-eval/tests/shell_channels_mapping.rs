//! Rust-up boundary test for `shell_channels_to_value` (task #4067 step-3/4).
//!
//! Verifies the mapping helper with REAL shell kernel output — closes PRD §8
//! "synthetic input" G2 gap. Uses `shell_element_stress` / `shell_element_frame`
//! directly (`pub`, callable from external crates; `UNIT_TRI` / `steel_like()`
//! are `#[cfg(test)]`-only in reify-solver-elastic and cannot be imported).
//! Frame matrix obtained via `build_shell_frame(&nodes).r`.
//!
//! RED until step-4 implements
//! `reify_eval::compute_targets::elastic_static::shell_channels_to_value`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_core::Type;
use reify_eval::persistent_cache::ShellChannels;
use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value};
use reify_solver_elastic::constitutive::IsotropicElastic;
use reify_solver_elastic::shell_assembly::build_shell_frame;
use reify_solver_elastic::shell_result::shell_element_stress;

// 3-node right triangle in the XY plane; side length 1m.
const TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

// ── helpers ───────────────────────────────────────────────────────────────────

/// Flatten a 3×3 row-major tensor to a Vec<f64> (9 floats).
fn flatten_3x3(t: [[f64; 3]; 3]) -> Vec<f64> {
    t.iter().flat_map(|row| row.iter().copied()).collect()
}

/// Build a minimal 1D Sampled Value::Field whose SampledField carries `data`.
/// Grid: evenly-spaced nodes over [0, n-1]; domain/codomain = Type::Real.
/// The helper clones this structure when building top/bottom, swapping only data.
fn make_sampled_field(name: &str, data: Vec<f64>) -> Value {
    let n = data.len();
    let axis_grid: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![(n.saturating_sub(1)) as f64],
        spacing: vec![1.0],
        axis_grids: vec![axis_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: Type::Real,
        codomain_type: Type::Real,
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Extract the flat data from a `Value::Field { source: Sampled, lambda: SampledField(_) }`.
fn sampled_field_data(v: &Value) -> Vec<f64> {
    match v {
        Value::Field {
            source: FieldSourceKind::Sampled,
            lambda,
            ..
        } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("expected SampledField in lambda, got: {:?}", other),
        },
        other => panic!("expected Sampled Value::Field, got: {:?}", other),
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Rust-up: `shell_channels_to_value(Some(_), mid)` returns a ShellStress
/// StructureInstance where:
///   - `type_name == "ShellStress"`
///   - `fields["mid"] == mid_stress` (PartialEq, I-2 preserved)
///   - `fields["top"].data == channels.top` (data carried through)
///   - `fields["bottom"].data == channels.bottom` (data carried through)
///
/// Uses a bending-dominant displacement (u[2] = 1e-3 at node 0) so that
/// top ≠ mid ≠ bottom in the kernel output.
///
/// RED until step-4 implements `shell_channels_to_value`.
#[test]
fn shell_channels_to_value_some_yields_shell_stress_instance() {
    let mat = IsotropicElastic {
        youngs_modulus: 200.0e9,
        poisson_ratio: 0.3,
    };
    let thickness = 0.01;
    // Non-trivial 18-DOF displacement: vertical displacement at node 0 only,
    // producing a bending-dominant state so top != mid != bottom.
    let mut u = [0.0_f64; 18];
    u[2] = 1e-3;

    let s = shell_element_stress(&TRI, thickness, &mat, &u);
    let f_frame = build_shell_frame(&TRI).r;

    let top_flat = flatten_3x3(s.top);
    let mid_flat = flatten_3x3(s.mid);
    let bot_flat = flatten_3x3(s.bottom);
    let frame_flat = flatten_3x3(f_frame);

    let channels = ShellChannels {
        top: top_flat.clone(),
        bottom: bot_flat.clone(),
        frame: frame_flat,
    };
    let mid_stress = make_sampled_field("mid_stress", mid_flat);

    let result = reify_eval::compute_targets::elastic_static::shell_channels_to_value(
        &Some(channels),
        &mid_stress,
    );

    let data = match &result {
        Value::StructureInstance(d) => d,
        other => panic!(
            "shell_channels_to_value(Some(_), mid) must be StructureInstance, got: {:?}",
            other
        ),
    };
    assert_eq!(
        data.type_name, "ShellStress",
        "type_name must be ShellStress, got: {:?}",
        data.type_name
    );

    let mid_val = data.fields.get(&"mid".to_string()).unwrap_or_else(|| {
        panic!(
            "ShellStress missing 'mid' field; keys: {:?}",
            data.fields.keys().collect::<Vec<_>>()
        )
    });
    assert_eq!(
        mid_val, &mid_stress,
        "ShellStress.mid must equal mid_stress (I-2)"
    );

    let top_val = data
        .fields
        .get(&"top".to_string())
        .unwrap_or_else(|| panic!("ShellStress missing 'top' field"));
    assert_eq!(
        sampled_field_data(top_val),
        top_flat,
        "ShellStress.top data must equal channels.top"
    );

    let bot_val = data
        .fields
        .get(&"bottom".to_string())
        .unwrap_or_else(|| panic!("ShellStress missing 'bottom' field"));
    assert_eq!(
        sampled_field_data(bot_val),
        bot_flat,
        "ShellStress.bottom data must equal channels.bottom"
    );
}

/// Rust-up: `shell_channels_to_value(None, mid)` returns `Value::Undef` (I-3
/// honest absence — tet/solid results carry no shell channels).
///
/// RED until step-4 implements `shell_channels_to_value`.
#[test]
fn shell_channels_to_value_none_yields_undef() {
    let mid_stress = make_sampled_field("mid_stress", vec![0.0; 9]);
    let result =
        reify_eval::compute_targets::elastic_static::shell_channels_to_value(&None, &mid_stress);
    assert_eq!(
        result,
        Value::Undef,
        "shell_channels_to_value(None, ..) must return Undef (I-3)"
    );
}

/// Covers the `build_channel_field` defensive fallback: when `mid_stress` is
/// NOT a `Value::Field { source: Sampled }` (here `Value::Undef`), the helper
/// must still produce a valid `ShellStress` StructureInstance — top/bottom come
/// back as minimal 1D Sampled fields whose `data` equals the raw channel vectors.
///
/// This exercises the previously-untested fallback branch in `build_channel_field`.
#[test]
fn shell_channels_to_value_non_sampled_mid_uses_fallback() {
    let top_data: Vec<f64> = (0..9).map(|i| i as f64).collect();
    let bot_data: Vec<f64> = (9..18).map(|i| i as f64).collect();
    let channels = ShellChannels {
        top: top_data.clone(),
        bottom: bot_data.clone(),
        frame: vec![0.0; 9],
    };
    // Non-Sampled mid → triggers the defensive fallback in build_channel_field.
    let mid_stress = Value::Undef;

    let result = reify_eval::compute_targets::elastic_static::shell_channels_to_value(
        &Some(channels),
        &mid_stress,
    );

    let data = match &result {
        Value::StructureInstance(d) => d,
        other => panic!(
            "expected StructureInstance with non-Sampled mid, got: {:?}",
            other
        ),
    };
    assert_eq!(data.type_name, "ShellStress");

    // top / bottom must be 1D Sampled Real fields carrying the raw channel data.
    let top_val = data
        .fields
        .get(&"top".to_string())
        .expect("ShellStress missing 'top' field");
    assert_eq!(
        sampled_field_data(top_val),
        top_data,
        "fallback top data must equal channels.top"
    );

    let bot_val = data
        .fields
        .get(&"bottom".to_string())
        .expect("ShellStress missing 'bottom' field");
    assert_eq!(
        sampled_field_data(bot_val),
        bot_data,
        "fallback bottom data must equal channels.bottom"
    );

    // mid must equal mid_stress (Value::Undef) unchanged.
    let mid_val = data
        .fields
        .get(&"mid".to_string())
        .expect("ShellStress missing 'mid' field");
    assert_eq!(
        mid_val,
        &Value::Undef,
        "mid must equal mid_stress unchanged"
    );
}
