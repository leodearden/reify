//! Grounding / self-datum eval tests (geometric-relations η, task 4387).
//!
//! Two surfaces live here:
//!  - step-9 (this file): the intrinsic `self.*` datum VALUES. A structure's
//!    `self` is its own identity frame (design §6), so `self.origin` is the
//!    world origin, `self.frame` the identity frame, `self.x/.y/.z` the unit
//!    axes, and `self.xy_plane/.yz_plane/.zx_plane` the principal planes —
//!    all kernel-free constants. The compiler (η step-8) lowers `self.<datum>`
//!    to a `MethodCall { object: ValueRef(__self : StructureRef), method, [] }`;
//!    eval (η step-10) intercepts it and yields the intrinsic constant.
//!  - step-17 (added later): the trace-to-ground / global-float check.
//!
//! RED (step-9): eval has no self-datum projection handler, so every `self.*`
//! datum cell evaluates to `Value::Undef` (the `__self` receiver resolves to
//! Undef and the `MethodCall` short-circuits). These value assertions fail
//! until step-10 lands the handler.

use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::relate_solve::{
    RealizedDatums, RelateScope, collect_relate_scope, solve_relate_scope, trace_to_ground,
};
use reify_ir::Value;
use reify_test_support::{compile_source, eval_source};

const EPS: f64 = 1e-12;

/// Extract the three numeric components (SI for dimensioned) of a 3-component
/// `Value::Point` or `Value::Vector`.
fn comps3(v: &Value) -> [f64; 3] {
    let comps = match v {
        Value::Point(c) | Value::Vector(c) => c,
        other => panic!("expected a 3-component Point/Vector, got {other:?}"),
    };
    assert_eq!(comps.len(), 3, "expected 3 components, got {comps:?}");
    [
        comps[0].as_f64().expect("component 0 numeric"),
        comps[1].as_f64().expect("component 1 numeric"),
        comps[2].as_f64().expect("component 2 numeric"),
    ]
}

fn approx3(actual: [f64; 3], expected: [f64; 3]) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() < EPS,
            "component {i}: got {}, expected {}",
            actual[i],
            expected[i]
        );
    }
}

/// Compile+eval a one-structure source kernel-free and return the value of cell
/// `member` in structure `S`.
fn eval_cell(source: &str, member: &str) -> Value {
    let result = eval_source(source);
    let id = ValueCellId::new("S", member);
    result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("cell `{member}` missing from eval result"))
        .clone()
}

/// A structure binding every intrinsic self-datum to a let, so each cell can be
/// read back independently.
const SELF_DATUM_SRC: &str = r#"structure S {
    let o = self.origin
    let fr = self.frame
    let dx = self.x
    let dy = self.y
    let dz = self.z
    let pxy = self.xy_plane
    let pyz = self.yz_plane
    let pzx = self.zx_plane
}"#;

#[test]
fn self_origin_is_world_origin() {
    let o = eval_cell(SELF_DATUM_SRC, "o");
    approx3(comps3(&o), [0.0, 0.0, 0.0]);
}

#[test]
fn self_frame_is_identity_frame() {
    let fr = eval_cell(SELF_DATUM_SRC, "fr");
    match fr {
        Value::Frame { origin, basis } => {
            approx3(comps3(&origin), [0.0, 0.0, 0.0]);
            match *basis {
                Value::Orientation { w, x, y, z } => {
                    assert!((w - 1.0).abs() < EPS, "w should be 1, got {w}");
                    assert!(x.abs() < EPS, "x should be 0, got {x}");
                    assert!(y.abs() < EPS, "y should be 0, got {y}");
                    assert!(z.abs() < EPS, "z should be 0, got {z}");
                }
                other => panic!("frame basis should be Orientation, got {other:?}"),
            }
        }
        other => panic!("expected Value::Frame, got {other:?}"),
    }
}

#[test]
fn self_unit_axes_are_directions() {
    // x = (1,0,0), y = (0,1,0), z = (0,0,1)
    for (member, expected) in [
        ("dx", [1.0, 0.0, 0.0]),
        ("dy", [0.0, 1.0, 0.0]),
        ("dz", [0.0, 0.0, 1.0]),
    ] {
        let v = eval_cell(SELF_DATUM_SRC, member);
        match v {
            Value::Direction { x, y, z } => approx3([x, y, z], expected),
            other => panic!("expected Value::Direction for `{member}`, got {other:?}"),
        }
    }
}

#[test]
fn self_principal_planes_have_origin_and_axis_normals() {
    // xy_plane normal (0,0,1); yz_plane normal (1,0,0); zx_plane normal (0,1,0).
    // All three pass through the origin.
    for (member, normal) in [
        ("pxy", [0.0, 0.0, 1.0]),
        ("pyz", [1.0, 0.0, 0.0]),
        ("pzx", [0.0, 1.0, 0.0]),
    ] {
        let v = eval_cell(SELF_DATUM_SRC, member);
        match v {
            Value::Plane { origin, normal: n } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.0]);
                approx3(comps3(&n), normal);
            }
            other => panic!("expected Value::Plane for `{member}`, got {other:?}"),
        }
    }
}

// ── step-17 RED: trace-to-ground / global-float (B6) ─────────────────────────
//
// `trace_to_ground(scope)` is a kernel-free structural connectivity check over the
// relation operand graph: nodes are the `at auto` subs ∪ a synthetic ground anchor;
// a relation unions its operand subs, and any relation referencing a GROUND sub (a
// non-auto anchor) or a `self.*` datum operand unions its subs into the anchor. An
// auto sub not connected to the anchor is FLOATING → the B6 `AssemblyGlobalFloat`
// global-float error, emitted pre-solve in `solve_relate_scope`.
//
// Built on REAL compiled scopes (compile_source + collect_relate_scope, kernel-free),
// so the operand shapes are exactly what `decode_operand` / the self-anchor decoder
// see at `reify build`. RED: `trace_to_ground` does not exist and `solve_relate_scope`
// emits no global-float diagnostic yet.

/// Compile `src` kernel-free and collect structure `name`'s relate-solve scope.
fn scope_of(src: &str, name: &str) -> RelateScope {
    let compiled = compile_source(src);
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("structure `{name}` present in compiled module"));
    collect_relate_scope(template)
}

/// Two `at auto` subs related ONLY to each other — no ground sub, no `self.*` operand
/// — the canonical globally-floating assembly.
const GLOBAL_FLOAT_SRC: &str = r#"structure Widget { param w : Length = 1mm }
structure S {
    sub a : Widget at auto
    sub b : Widget at auto
    relate { fasten(a.frame, b.frame) }
}"#;

/// An auto sub fastened to a NON-auto ground sub (`base`) — grounded via the anchor.
const GROUNDED_VIA_ANCHOR_SRC: &str = r#"structure Widget { param w : Length = 1mm }
structure S {
    sub a : Widget at auto
    sub base : Widget
    relate { fasten(a.frame, base.frame) }
}"#;

/// An auto sub grounded via the `ground(a)` sugar (desugars to
/// `fasten(a.frame, self.frame)`) — grounded via `self`.
const GROUNDED_VIA_SELF_SRC: &str = r#"structure Widget { param w : Length = 1mm }
structure S {
    sub a : Widget at auto
    relate { ground(a) }
}"#;

/// Case A — two auto subs relating only to each other float: `trace_to_ground`
/// returns BOTH sub names, and `solve_relate_scope` emits the B6 `AssemblyGlobalFloat`
/// diagnostic ("floats in `self`" / "ground a part").
#[test]
fn global_float_two_autos_relating_only_to_each_other() {
    let scope = scope_of(GLOBAL_FLOAT_SRC, "S");

    let mut floating = trace_to_ground(&scope);
    floating.sort();
    assert_eq!(
        floating,
        vec!["a".to_string(), "b".to_string()],
        "both auto subs float — neither traces to a grounded anchor"
    );

    // Pre-solve, the global float is reported as an AssemblyGlobalFloat Error.
    let solution = solve_relate_scope(&scope, &RealizedDatums::default());
    let float_diag = solution
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::AssemblyGlobalFloat))
        .expect("a global-float scope emits an AssemblyGlobalFloat diagnostic");
    assert!(
        float_diag.message.contains("floats in `self`")
            && float_diag.message.contains("ground a part"),
        "B6 message guides the fix; got: {}",
        float_diag.message
    );
}

/// Case B — an auto sub fastened to a non-auto ground sub traces to the anchor: no
/// floating subs, no global-float diagnostic.
#[test]
fn grounded_via_ground_sub_does_not_float() {
    let scope = scope_of(GROUNDED_VIA_ANCHOR_SRC, "S");
    assert!(
        trace_to_ground(&scope).is_empty(),
        "the auto sub traces to the grounded anchor `base`"
    );
    let solution = solve_relate_scope(&scope, &RealizedDatums::default());
    assert!(
        !solution
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::AssemblyGlobalFloat)),
        "a grounded assembly draws no global-float diagnostic"
    );
}

/// Case C — an auto sub grounded via `self` (the `ground(a)` sugar →
/// `fasten(a.frame, self.frame)`) traces to the anchor: no floating subs.
#[test]
fn grounded_via_self_does_not_float() {
    let scope = scope_of(GROUNDED_VIA_SELF_SRC, "S");
    assert!(
        trace_to_ground(&scope).is_empty(),
        "the auto sub traces to `self` via the ground() sugar"
    );
    let solution = solve_relate_scope(&scope, &RealizedDatums::default());
    assert!(
        !solution
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::AssemblyGlobalFloat)),
        "grounding via self draws no global-float diagnostic"
    );
}
