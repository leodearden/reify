//! End-to-end tests for cross-sub geometry composition (`self.<sub>.<member>`).
//!
//! Exercises the full source-to-build pipeline (parse → compile → eval → build)
//! with a `MockGeometryKernel`, verifying that the parent template's realisations
//! see the child template's named realisation handles through compound-key
//! `named_steps` entries (`"<sub>.<member>"` → handle).
//!
//! See task 3441 — eval-side `GeomRef::Sub` plumbing for cross-template handles.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{FailingMockGeometryKernel, MockGeometryKernel, compile_source};

/// Inner has `body = box(...)`; Outer has `sub inner = Inner()` and
/// `placed = translate(self.inner.body, 10mm, 0mm, 0mm)`.
///
/// Asserts (a) build produces `geometry_output.is_some()`, (b) recorded ops
/// contain a Box (Inner.body) and a Translate whose target == Box's result handle,
/// (c) no Error-severity diagnostics, (d) no "unresolvable GeomRef::Sub" error.
///
/// RED until step-4 (eval-side compound-key named_steps threading) lands.
#[test]
fn cross_sub_translate_resolves_child_body_handle() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (c) No Error-severity diagnostics at compile time.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Build with MockGeometryKernel to capture recorded ops.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error-severity diagnostics from build either.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (d) Specifically no "unresolvable GeomRef::Sub" — the parent's
    // named_steps must have been seeded with the compound key `inner.body`.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) The kernel recorded a Box (Inner.body) and a Translate whose
    // target == the Box's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    assert!(
        recorded.len() >= 2,
        "expected at least 2 recorded kernel ops (Box for Inner.body + Translate \
         for Outer.placed), got {}: {:?}",
        recorded.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Inner.body");
    let box_handle = box_rec.result_handle;

    let translate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .expect("expected a Translate op recorded for Outer.placed");

    match translate_rec.op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Translate target should be Inner.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (a) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Inner has `param body : Solid = box(...)`; Outer has `sub inner = Inner()` and
/// `placed = translate(self.inner.body, 10mm, 0mm, 0mm)`.
///
/// This is the `param body : Solid` variant of the happy-path (compare
/// `cross_sub_translate_resolves_child_body_handle` which uses `let body`).
/// Both forms lower to the same `RealizationDecl` at compile time and must
/// produce the same seeded `named_steps["inner.body"]` entry at eval time.
///
/// Asserts (a) no Error-severity diagnostics at compile or build time,
/// (b) no "unresolvable GeomRef::Sub" error, (c) the kernel records a Box
/// (Inner.body) and a Translate whose target == the Box's result handle,
/// (d) `geometry_output.is_some()`.
///
/// Regression guard for task 3441 step-1 (flipped from diagnostic to
/// working-path): if eval-side `named_steps` seeding breaks for the
/// `param body : Solid` form while compile-side lowering continues to
/// succeed, this test fails while the compile-only diagnostic tests do not.
#[test]
fn cross_sub_translate_param_body_solid_resolves_child_handle() {
    let source = r#"pub structure Inner {
    param body : Solid = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No build-time Error diagnostics.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (b) No "unresolvable GeomRef::Sub" — named_steps seeded for param-body form.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Kernel recorded a Box (Inner.body) and a Translate targeting it.
    let recorded = ops_ref.lock().unwrap().clone();
    assert!(
        recorded.len() >= 2,
        "expected at least 2 recorded kernel ops (Box for Inner.body + Translate \
         for Outer.placed), got {}: {:?}",
        recorded.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Inner.body (param body : Solid)");
    let box_handle = box_rec.result_handle;

    let translate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .expect("expected a Translate op recorded for Outer.placed");

    match translate_rec.op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Translate target should be Inner.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (d) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// A has `body = box(...)`, B has `body = cylinder(...)`, C has
/// `sub a = A()`, `sub b = B()`, `combined = union(self.a.body, self.b.body)`.
///
/// Asserts (a) recorded ops contain a Union whose left == A.body's Box handle
/// and right == B.body's Cylinder handle, (b) build succeeds with
/// `geometry_output.is_some()`, (c) no Error-severity diagnostics.
///
/// RED until step-6 (boolean-op arg-resolution wiring) lands.
#[test]
fn cross_sub_union_two_sub_bodies_composes_in_parent() {
    let source = r#"pub structure A {
    let body = box(10mm, 10mm, 10mm)
}
pub structure B {
    let body = cylinder(5mm, 10mm)
}
pub structure C {
    sub a = A()
    sub b = B()
    let combined = union(self.a.body, self.b.body)
}"#;
    let compiled = compile_source(source);

    // (c) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error diagnostics from build.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (a) Recorded ops contain a Box (A.body), Cylinder (B.body), and a
    // Union whose left == Box's handle and right == Cylinder's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for A.body");
    let cyl_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Cylinder { .. }))
        .expect("expected a Cylinder op recorded for B.body");
    let union_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Union { .. }))
        .expect("expected a Union op recorded for C.combined");

    match union_rec.op {
        GeometryOp::Union { left, right } => {
            assert_eq!(
                left, box_rec.result_handle,
                "Union.left should be A.body's Box handle ({:?}); got {:?}",
                box_rec.result_handle, left
            );
            assert_eq!(
                right, cyl_rec.result_handle,
                "Union.right should be B.body's Cylinder handle ({:?}); got {:?}",
                cyl_rec.result_handle, right
            );
        }
        ref other => panic!("expected Union op, got {:?}", other),
    }

    // (b) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Stage has `body = box(...)`; Assy has `sub stage_left = Stage()` and
/// `mirrored = mirror(self.stage_left.body, 0, 0, 0, 1, 0, 0)` — origin at
/// the world origin, normal along +X.
///
/// Asserts (a) recorded ops contain a Mirror op whose target == Stage's box
/// handle, (b) build succeeds with `geometry_output.is_some()`, (c) no
/// Error-severity diagnostics.
///
/// Locks down the `mirror(self.<sub>.body, ...)` pattern called out in the
/// task description.  Should pass without code change because `mirror` is in
/// `geometry_arg_indices` returning `[0]` (geometry.rs:163), so the cross-sub
/// pre-check in `compile_geometry_call`'s generic resolution loop already
/// fires for the geometry arg at index 0.
#[test]
fn cross_sub_mirror_uses_child_body_handle() {
    let source = r#"pub structure Stage {
    let body = box(50mm, 30mm, 20mm)
}
pub structure Assy {
    sub stage_left = Stage()
    let mirrored = mirror(self.stage_left.body, 0, 0, 0, 1, 0, 0)
}"#;
    let compiled = compile_source(source);

    // (c) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error diagnostics from build.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (a) Recorded ops contain a Box (Stage.body) and a Mirror whose
    // target == the Box's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Stage.body");
    let box_handle = box_rec.result_handle;

    let mirror_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Mirror { .. }))
        .expect("expected a Mirror op recorded for Assy.mirrored");

    match mirror_rec.op {
        GeometryOp::Mirror { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Mirror.target should be Stage.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Mirror op, got {:?}", other),
    }

    // (b) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Nested transform chain over a cross-sub body: `translate(rotate(self.inner.body,
/// 0, 0, 1, 90deg), 10mm, 0mm, 0mm)`.
///
/// Pins that nesting works end-to-end: a `Sub`-resolved geometry-arg can sit at any
/// depth of a nested transform call, and the resulting ops form a clean
/// Box → Rotate → Translate chain where each step's target is the previous step's
/// result handle.
///
/// Asserts (a) no Error diagnostics, (b) recorded ops include exactly one Rotate
/// whose target is Inner.body's Box handle, followed by exactly one Translate
/// whose target is the Rotate's result handle.
///
/// Anti-cascade guard: confirms `current_offset` accounting in geometry.rs's
/// arg-resolution loop doesn't perturb the step offset for sibling args when a
/// `Sub`-resolved arg short-circuits sub-op accumulation.
#[test]
fn cross_sub_geometry_anti_cascade_no_spurious_errors_in_translate_chain() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let composed = translate(rotate(self.inner.body, 0, 0, 1, 90deg), 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No Error diagnostics from build.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    let recorded = ops_ref.lock().unwrap().clone();

    // (b) Exactly one Box (Inner.body), one Rotate, one Translate.
    let box_count = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .count();
    let rotate_count = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Rotate { .. }))
        .count();
    let translate_count = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .count();
    assert_eq!(
        box_count,
        1,
        "expected exactly 1 Box op (Inner.body); got {} in {:?}",
        box_count,
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        rotate_count,
        1,
        "expected exactly 1 Rotate op (inner of Outer.composed); got {} in {:?}",
        rotate_count,
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        translate_count,
        1,
        "expected exactly 1 Translate op (outer of Outer.composed); got {} in {:?}",
        translate_count,
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Inner.body");
    let rotate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Rotate { .. }))
        .expect("expected a Rotate op recorded inside Outer.composed");
    let translate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .expect("expected a Translate op recorded for Outer.composed");

    // (b) Rotate's target == Inner.body's Box handle.
    match rotate_rec.op {
        GeometryOp::Rotate { target, .. } => {
            assert_eq!(
                target, box_rec.result_handle,
                "Rotate.target should be Inner.body's Box handle ({:?}); got {:?}",
                box_rec.result_handle, target
            );
        }
        ref other => panic!("expected Rotate op, got {:?}", other),
    }

    // (b) Translate's target == Rotate's result handle.
    match translate_rec.op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, rotate_rec.result_handle,
                "Translate.target should be the inner Rotate's result handle ({:?}); got {:?}",
                rotate_rec.result_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // Build should also succeed (geometry_output some).
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

// ─── amendment regression guards (reviewer suggestions #4 + #5) ───────────────

/// Two singular subs of the same child template share one kernel handle for
/// the child's body — `sub a = Inner(); sub b = Inner();` lower to
/// `GeomRef::Sub("a.body")` and `GeomRef::Sub("b.body")` respectively, and
/// the eval-side `module_named_steps` registry keys by the *structure name*
/// (`sub.structure_name`) so both sub names get seeded from the same
/// per-template snapshot.
///
/// **Scope note (task 3814):** this shared-handle behaviour applies only when
/// both subs carry **no explicit constructor args** (`sub.args.is_empty()`).
/// When subs carry distinct args — e.g. `sub a = Inner(size: 50mm)` vs
/// `sub b = Inner(size: 80mm)` — `seed_cross_sub_named_steps` re-executes
/// the child template's ops independently for each sub, producing distinct
/// kernel handles.  See `cross_sub_two_subs_with_distinct_overrides_get_distinct_handles`
/// for the override-aware counterpart and `engine_build.rs::seed_cross_sub_named_steps`
/// for the two-mode rustdoc.
///
/// Regression guard: pins the no-args shared-handle behaviour so a future
/// change that inadvertently breaks it (e.g. an attempt to generalise the
/// override path to arg-free subs) fails this test loudly.
#[test]
fn cross_sub_same_template_subs_share_kernel_handle() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub a = Inner()
    sub b = Inner()
    let placed_a = translate(self.a.body, 10mm, 0mm, 0mm)
    let placed_b = translate(self.b.body, 0mm, 10mm, 0mm)
}"#;
    let compiled = compile_source(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // Inner's `body` is realised exactly once; both translate ops target the
    // same Box handle.  This is the same-template aliasing limitation
    // documented above.
    let recorded = ops_ref.lock().unwrap().clone();
    let box_handles: Vec<_> = recorded
        .iter()
        .filter_map(|rec| match rec.op {
            GeometryOp::Box { .. } => Some(rec.result_handle),
            _ => None,
        })
        .collect();
    assert_eq!(
        box_handles.len(),
        1,
        "expected exactly 1 Box op (Inner.body realised once); got {} handles in {:?}",
        box_handles.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    let inner_body_handle = box_handles[0];

    let translate_targets: Vec<_> = recorded
        .iter()
        .filter_map(|rec| match rec.op {
            GeometryOp::Translate { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(
        translate_targets.len(),
        2,
        "expected exactly 2 Translate ops (placed_a + placed_b); got {} in {:?}",
        translate_targets.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        translate_targets[0], inner_body_handle,
        "translate(self.a.body, ...) should target Inner's single Box handle; got {:?}",
        translate_targets[0]
    );
    assert_eq!(
        translate_targets[1], inner_body_handle,
        "translate(self.b.body, ...) should target the SAME Inner Box handle \
         (v0.1 same-template aliasing limitation); got {:?}",
        translate_targets[1]
    );
}

/// Forward-declared sub: parent template `Outer` is declared *before* the
/// child template `Inner` in source order.  Because the eval seed loop
/// processes templates in declaration order and only seeds compound-key
/// entries from already-processed templates, the parent's `named_steps`
/// will NOT contain `"inner.body"` at the time Outer's realisations run,
/// and the `GeomRef::Sub("inner.body")` reference falls through to the
/// `geometry_ops.rs::resolve_geom_ref` "unresolvable GeomRef::Sub" error
/// path.
///
/// This is the v0.1 fallback for forward-declared / recursive subs.  Users
/// will see a runtime error rather than a compile-time diagnostic; lifting
/// this requires either a topological pre-pass over the template graph or
/// a compile-time forward-reference check.  See
/// `engine_build.rs::seed_cross_sub_named_steps` rustdoc.
///
/// Regression guard: pins the current error message so a future cleaner
/// diagnostic path can flip this test to assert the new message rather
/// than relying on the runtime fallback.
#[test]
fn cross_sub_forward_declared_sub_yields_unresolvable_geom_ref_error() {
    // Outer is declared BEFORE Inner — at the time Outer's realisations run,
    // Inner has not been realised yet, so `module_named_steps` does not
    // contain Inner's "body" entry to seed `inner.body` into Outer's
    // `named_steps`.
    let source = r#"pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}
pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}"#;
    let compiled = compile_source(source);
    // Compile-side passes — the working-path lowering does not check
    // declaration order.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics (forward-ref is a runtime fallback); got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Runtime: the parent's `named_steps` is missing the compound-key
    // `"inner.body"`, so `GeomRef::Sub("inner.body")` cannot resolve.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("unresolvable GeomRef::Sub") || d.message.contains("inner.body")
        })
        .collect();
    assert!(
        !unresolvable.is_empty(),
        "expected a runtime diagnostic naming 'unresolvable GeomRef::Sub' or 'inner.body' \
         for forward-declared sub; got diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

// ─── task 3814: per-instance constructor param overrides reach geometry ────────

/// Constructor named-param override `sub inner = Inner(size: 70mm)` must reach
/// the kernel box dimensions.
///
/// The override-aware per-instance path (task 3814, step-2) re-executes
/// `Inner`'s realization with the scoped override value, producing a box whose
/// `width`/`height`/`depth` match the 70 mm override rather than `Inner`'s
/// default 10 mm.
///
/// **RED** on main: the structure-keyed snapshot path copies `Inner`'s
/// default-args handle (`box(10mm, 10mm, 10mm)`) into `named_steps["inner.body"]`,
/// so the recorded `Box` op has `si_value = 0.01` and assertion (c) fails.
#[test]
fn cross_sub_constructor_named_param_override_reaches_geometry() {
    let source = r#"pub structure Inner {
    param size : Length = 10mm
    let body = box(size, size, size)
}
pub structure Outer {
    sub inner = Inner(size: 70mm)
    let body = translate(self.inner.body, 200mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No build-time Error diagnostics.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (b) No "unresolvable GeomRef::Sub" — inner.body must be seeded.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let recorded = ops_ref.lock().unwrap().clone();

    // (c) Find the per-instance Box with width/height/depth == 70 mm.
    // 70 mm = 0.07 m in SI units, stored as Value::Scalar with LENGTH dimension.
    let override_size = Value::Scalar {
        si_value: 0.07,
        dimension: DimensionVector::LENGTH,
    };
    let per_instance_box = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Box {
            width,
            height,
            depth,
        } => width == &override_size && height == &override_size && depth == &override_size,
        _ => false,
    });
    let per_instance_box = per_instance_box.expect(
        "expected a Box op with width=height=depth=0.07 m (70 mm) for Inner(size: 70mm) \
         override; on main this fails because the structure-keyed snapshot uses Inner's \
         default 10mm param (si_value=0.01)",
    );
    let override_box_handle = per_instance_box.result_handle;

    // (d) Exactly one Translate whose target == the per-instance Box handle.
    let translate_recs: Vec<_> = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .collect();
    assert_eq!(
        translate_recs.len(),
        1,
        "expected exactly 1 Translate op (Outer.body); got {}: {:?}",
        translate_recs.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    match translate_recs[0].op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, override_box_handle,
                "Translate target should be the per-instance Box handle ({:?}); \
                 on main it targets the default-args Box instead — got {:?}",
                override_box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (e) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Partial named-param override (override one param, default-fallthrough the
/// other) propagates to cross-sub geometry.
///
/// Source:
/// ```text
/// pub structure Inner {
///     param w : Length = 10mm
///     param h : Length = 20mm
///     let body = box(w, h, w)
/// }
/// pub structure Outer {
///     sub inner = Inner(h: 50mm)
///     let body = translate(self.inner.body, 200mm, 0mm, 0mm)
/// }
/// ```
///
/// This is distinct from `cross_sub_constructor_named_param_override_reaches_geometry`
/// (step-1) which overrides the SOLE param — here `Inner` has TWO params and
/// the constructor only overrides `h`, leaving `w` to fall through to its
/// declared default.  This pins the overlay-loop semantics in
/// `seed_cross_sub_named_steps`: per-instance value scope contains the
/// `h=50mm` override AND the un-overridden `w=10mm` resolves correctly to
/// `Inner`'s default during re-execution (i.e. the overlay does not stomp
/// non-overridden params with placeholder/empty values).
///
/// HISTORY: the original plan called for a positional-syntax variant
/// (`Inner(50mm)`), but the Reify grammar's `sub_declaration` rule restricts
/// constructor args to `named_argument_list` only — positional syntax is not
/// a language feature for sub declarations (grammar.js:488; only
/// `function_call` accepts the broader `argument_list`).  The positional
/// variant was therefore replaced with this partial-override variant per the
/// task-3814 esc-3814-35 resolution (Option A) — a strictly more interesting
/// shape than a duplicate of step-1, exercising the default-fallthrough arm
/// of the overlay loop in addition to the override arm.
///
/// **RED** on main: the structure-keyed snapshot path produces a Box with
/// `width = Value::Scalar { si_value: 0.01, … }` AND
/// `height = Value::Scalar { si_value: 0.02, … }` (both `Inner` defaults).
/// **GREEN** after step-2: per-instance re-realization with the overlay
/// produces `width = 0.01 m`, `height = 0.05 m`, `depth = 0.01 m`.
#[test]
fn cross_sub_constructor_partial_param_override_reaches_geometry() {
    let source = r#"pub structure Inner {
    param w : Length = 10mm
    param h : Length = 20mm
    let body = box(w, h, w)
}
pub structure Outer {
    sub inner = Inner(h: 50mm)
    let body = translate(self.inner.body, 200mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No build-time Error diagnostics.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (b) No "unresolvable GeomRef::Sub" — inner.body must be seeded.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let recorded = ops_ref.lock().unwrap().clone();

    // (c) Find the per-instance Box with width=depth=10mm (default for `w`)
    // and height=50mm (override for `h`).  This is the discriminator:
    // on main the structure-keyed snapshot path produces a Box with
    // height=20mm (Inner's default), so this assertion fails.
    let default_w = Value::Scalar {
        si_value: 0.01,
        dimension: DimensionVector::LENGTH,
    };
    let override_h = Value::Scalar {
        si_value: 0.05,
        dimension: DimensionVector::LENGTH,
    };
    let per_instance_box = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Box {
            width,
            height,
            depth,
        } => width == &default_w && height == &override_h && depth == &default_w,
        _ => false,
    });
    let per_instance_box = per_instance_box.expect(
        "expected a Box op with width=depth=0.01 m (10 mm default for `w`) AND height=0.05 m \
         (50 mm override for `h`) for Inner(h: 50mm); on main this fails because the \
         structure-keyed snapshot uses Inner's default 20mm height (si_value=0.02)",
    );
    let override_box_handle = per_instance_box.result_handle;

    // (d) Exactly one Translate whose target == the per-instance Box handle.
    let translate_recs: Vec<_> = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .collect();
    assert_eq!(
        translate_recs.len(),
        1,
        "expected exactly 1 Translate op (Outer.body); got {}: {:?}",
        translate_recs.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    match translate_recs[0].op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, override_box_handle,
                "Translate target should be the per-instance Box handle ({:?}) from \
                 Inner(h: 50mm) partial override; on main it targets the default-args \
                 Box instead — got {:?}",
                override_box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (e) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some (partial override); got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Two subs of the same child template with *distinct* named-param overrides
/// each receive their own per-instance kernel handle.
///
/// Source:
/// ```
/// pub structure Inner { param size : Length = 10mm  let body = box(size, size, size) }
/// pub structure Outer {
///     sub a = Inner(size: 50mm)
///     sub b = Inner(size: 80mm)
///     let placed_a = translate(self.a.body, 0mm, 0mm, 0mm)
///     let placed_b = translate(self.b.body, 0mm, 0mm, 0mm)
/// }
/// ```
///
/// Assertions:
/// * (a) No Error-severity diagnostics at compile or build time.
/// * (b) Among recorded `GeometryOp::Box` ops, exactly one has
///   `width == 50mm` (handle `H_a`) and exactly one has `width == 80mm`
///   (handle `H_b`), and `H_a != H_b`.
/// * (c) The two `GeometryOp::Translate` ops have targets `{H_a, H_b}` (one
///   each, in any order).
///
/// This pins the DISTINCT-handle semantics introduced in task 3814's step-2.
/// Compare with `cross_sub_same_template_subs_share_kernel_handle` which pins
/// the SHARED-handle semantics for the no-args case: when `sub.args` is empty,
/// two subs of the same template still share a single kernel handle from the
/// structure-keyed snapshot.
///
/// **RED** on main because `seed_cross_sub_named_steps` uses the structure-
/// keyed snapshot for both `a` and `b`, so both translates target `Inner`'s
/// default 10mm Box rather than the 50mm and 80mm per-instance boxes.
#[test]
fn cross_sub_two_subs_with_distinct_overrides_get_distinct_handles() {
    let source = r#"pub structure Inner {
    param size : Length = 10mm
    let body = box(size, size, size)
}
pub structure Outer {
    sub a = Inner(size: 50mm)
    sub b = Inner(size: 80mm)
    let placed_a = translate(self.a.body, 0mm, 0mm, 0mm)
    let placed_b = translate(self.b.body, 0mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No build-time Error diagnostics.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    let recorded = ops_ref.lock().unwrap().clone();

    // (b) Find distinct Box handles for 50mm and 80mm.
    let size_50 = Value::Scalar {
        si_value: 0.05,
        dimension: DimensionVector::LENGTH,
    };
    let size_80 = Value::Scalar {
        si_value: 0.08,
        dimension: DimensionVector::LENGTH,
    };

    let box_50 = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Box { width, .. } => width == &size_50,
        _ => false,
    });
    let box_80 = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Box { width, .. } => width == &size_80,
        _ => false,
    });

    let h_a = box_50
        .expect(
            "expected a Box op with width=0.05m (50mm) for sub a = Inner(size: 50mm); \
             on main this fails because the structure-keyed snapshot uses Inner's 10mm default",
        )
        .result_handle;
    let h_b = box_80
        .expect(
            "expected a Box op with width=0.08m (80mm) for sub b = Inner(size: 80mm); \
             on main this fails because the structure-keyed snapshot uses Inner's 10mm default",
        )
        .result_handle;

    assert_ne!(
        h_a, h_b,
        "sub a (50mm) and sub b (80mm) must receive DISTINCT kernel handles; \
         got identical handle {:?} for both",
        h_a
    );

    // (c) Each Translate targets the correct per-instance Box handle.
    let translate_targets: Vec<_> = recorded
        .iter()
        .filter_map(|rec| match rec.op {
            GeometryOp::Translate { target, .. } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(
        translate_targets.len(),
        2,
        "expected exactly 2 Translate ops (placed_a + placed_b); got {} in {:?}",
        translate_targets.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    // Both per-instance handles must appear as translate targets (one each).
    let targets_set: std::collections::HashSet<_> = translate_targets.iter().copied().collect();
    assert!(
        targets_set.contains(&h_a),
        "expected one Translate targeting h_a ({:?}, the 50mm Box); \
         translate targets were {:?}",
        h_a,
        translate_targets
    );
    assert!(
        targets_set.contains(&h_b),
        "expected one Translate targeting h_b ({:?}, the 80mm Box); \
         translate targets were {:?}",
        h_b,
        translate_targets
    );
}

/// Multi-op child realization: `Inner.body = translate(box(size,size,size), 5mm, 0mm, 0mm)`.
///
/// This pins that `per_instance_step_handles` is correctly threaded across ops
/// within one realization so `GeomRef::Step(0)` (the Box handle) resolves to
/// the per-instance Box when the second op (Translate) is compiled during
/// the override-path re-realization.
///
/// Source:
/// ```text
/// pub structure Inner {
///     param size : Length = 10mm
///     let body = translate(box(size, size, size), 5mm, 0mm, 0mm)
/// }
/// pub structure Outer {
///     sub inner = Inner(size: 70mm)
///     let placed = translate(self.inner.body, 200mm, 0mm, 0mm)
/// }
/// ```
///
/// Assertions:
/// * (a) No Error-severity diagnostics.
/// * (b) A Box op with width/height/depth = 70mm (per-instance box) exists.
/// * (c) A Translate op with x = 5mm (Inner's internal translate) whose
///   target is the per-instance Box handle (H_box) exists.
/// * (d) A Translate op with x = 200mm (Outer's placed) whose target equals
///   the Inner-translate handle (H_inner_translate — the LAST handle of
///   Inner.body's realization) exists.
/// * (e) `result.geometry_output.is_some()`.
///
/// **RED** on a broken per_instance_step_handles accumulator: the inner
/// Translate's target would be an invalid step reference (or handle 0), so
/// assertion (c) or (d) would fail.
#[test]
fn cross_sub_multi_op_child_per_instance_step_resolution() {
    let source = r#"pub structure Inner {
    param size : Length = 10mm
    let body = translate(box(size, size, size), 5mm, 0mm, 0mm)
}
pub structure Outer {
    sub inner = Inner(size: 70mm)
    let placed = translate(self.inner.body, 200mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No build-time Error diagnostics.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    let recorded = ops_ref.lock().unwrap().clone();

    // (b) Find the per-instance Box with width/height/depth = 70mm.
    let size_70 = Value::Scalar {
        si_value: 0.07,
        dimension: DimensionVector::LENGTH,
    };
    let box_70 = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Box {
            width,
            height,
            depth,
        } => width == &size_70 && height == &size_70 && depth == &size_70,
        _ => false,
    });
    let h_box = box_70
        .expect(
            "expected a Box op with width=height=depth=0.07m (70mm per-instance override); \
             on a broken per_instance_step_handles accumulator this would be absent or \
             have the default 10mm dimensions",
        )
        .result_handle;

    // (c) Find an inner Translate with dx = 5mm (0.005 m) targeting the per-instance Box.
    let inner_translate = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Translate { target, dx, .. } => {
            *target == h_box && (*dx - 0.005_f64).abs() < 1e-12
        }
        _ => false,
    });
    let h_inner_translate = inner_translate
        .expect(
            "expected a Translate op with dx=0.005m (5mm, Inner's internal shift) targeting \
             the per-instance Box handle; on a broken step accumulator the target would be \
             wrong or the op absent",
        )
        .result_handle;

    // (d) Find the outer Translate with dx = 200mm (0.2 m) targeting the inner-translate handle.
    let outer_translate = recorded.iter().find(|rec| match &rec.op {
        GeometryOp::Translate { target, dx, .. } => {
            *target == h_inner_translate && (*dx - 0.2_f64).abs() < 1e-12
        }
        _ => false,
    });
    assert!(
        outer_translate.is_some(),
        "expected a Translate with x=0.2m (200mm, Outer.placed) targeting the inner-translate \
         handle ({:?}); the outer translate should target the LAST handle of Inner.body's \
         re-realization (the translate result), not the intermediate Box handle; \
         recorded ops: {:?}",
        h_inner_translate,
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    // (e) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Kernel error during per-instance re-realization: emits the
/// "per-instance re-realization kernel error" diagnostic and does not panic.
///
/// Uses `FailingMockGeometryKernel` which always returns
/// `GeometryError::OperationFailed` from `execute()`.  Because the override
/// path calls `kernel.execute_with_history` which delegates to `execute()`,
/// every op in every named realization of every overridden sub will hit the
/// error branch at engine_build.rs, pushing a
/// `"per-instance re-realization kernel error for …"` diagnostic.
///
/// Assertions:
/// * (a) `engine.build(...)` returns without panicking.
/// * (b) At least one diagnostic message contains
///   `"per-instance re-realization kernel error"`.
///
/// This pins the kernel-error branch (suggestion 3b) so a future refactor
/// that accidentally swallows or renames the error message will fail here.
#[test]
fn cross_sub_per_instance_kernel_error_emits_diagnostic() {
    let source = r#"pub structure Inner {
    param size : Length = 10mm
    let body = box(size, size, size)
}
pub structure Outer {
    sub inner = Inner(size: 70mm)
    let placed = translate(self.inner.body, 200mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // Compile errors are expected to be absent (the source is valid).
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    // FailingMockGeometryKernel always fails execute() with
    // GeometryError::OperationFailed("simulated kernel failure").
    let kernel = FailingMockGeometryKernel;

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    // (a) Must not panic.
    let result = engine.build(&compiled, ExportFormat::Step);

    // (b) At least one "per-instance re-realization kernel error" diagnostic.
    let per_instance_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message
                .contains("per-instance re-realization kernel error")
        })
        .collect();
    assert!(
        !per_instance_errors.is_empty(),
        "expected at least one 'per-instance re-realization kernel error' diagnostic; \
         all diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Pinning test: nested sub-of-sub in override path produces a clear
/// compile-error diagnostic rather than panicking or silently succeeding
/// with wrong geometry.
///
/// v0.1 scope boundary (task 3814): the per-instance re-realization path
/// handles ONE level of override depth (Outer → direct child).  When a
/// child template (Mid) itself has subs (sub inner = Inner()) and its
/// realization references `self.inner.body`, that `GeomRef::Sub("inner.body")`
/// cannot be resolved during Outer's per-instance re-realization of Mid —
/// the child's op compiler receives an empty named-steps map (by design, to
/// enforce the scope boundary).
///
/// Source:
/// ```text
/// pub structure Inner { let body = box(10mm, 10mm, 10mm) }
/// pub structure Mid {
///     param size : Length = 5mm
///     sub inner = Inner()
///     let body = translate(self.inner.body, size, 0mm, 0mm)
/// }
/// pub structure Outer {
///     sub mid = Mid(size: 20mm)
///     let placed = translate(self.mid.body, 100mm, 0mm, 0mm)
/// }
/// ```
///
/// Assertions:
/// * (a) `engine.build(...)` returns without panicking.
/// * (b) At least one diagnostic contains
///   `"per-instance re-realization compile error"` for `Outer.mid.body`
///   (the unresolvable `GeomRef::Sub("inner.body")` inside Mid's op).
///
/// This pins the one-level-deep scope boundary so it is observable and
/// checked by CI.  If nested overrides are ever implemented, this test must
/// be updated to assert the nested result rather than the error.
#[test]
fn cross_sub_nested_sub_in_override_path_produces_compile_error() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 10mm, 10mm)
}
pub structure Mid {
    param size : Length = 5mm
    sub inner = Inner()
    let body = translate(self.inner.body, size, 0mm, 0mm)
}
pub structure Outer {
    sub mid = Mid(size: 20mm)
    let placed = translate(self.mid.body, 100mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // Compile-time errors are NOT expected; the source is valid Reify.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    // (a) Must not panic.
    let result = engine.build(&compiled, ExportFormat::Step);

    // (b) At least one "per-instance re-realization compile error" diagnostic
    // whose message refers to Outer.mid.body (the failing re-realization).
    // The exact message will contain "inner.body" as the unresolvable sub ref.
    let per_instance_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message
                .contains("per-instance re-realization compile error")
                && d.message.contains("Outer.mid.body")
        })
        .collect();
    assert!(
        !per_instance_errors.is_empty(),
        "expected a 'per-instance re-realization compile error for Outer.mid.body' diagnostic \
         (v0.1 scope boundary: nested sub-of-sub override not supported); \
         all diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

// ─── task 3891: bare cross-sub geometry let emits a realization ───────────────

/// `let body = self.inner.body` — a bare cross-sub geometry member access with
/// no wrapping geometry op — must emit a realization (mesh) in addition to the
/// GHR-γ value cell.
///
/// Asserts:
///   (a) No compile-time Error diagnostics.
///   (b) No build-time "unresolvable GeomRef::Sub" diagnostic.
///   (c) Kernel recorded exactly one `GeometryOp::Box` (Inner.body) and at
///       least one `GeometryOp::Translate` (Outer.body's synthetic identity-
///       translate) whose `target` resolves to the Box's handle.
///   (d) `result.geometry_output.is_some()`.
///   (e) The `Outer` template still has exactly one `ValueCellDecl` named
///       `body` with `cell_type == Type::Geometry` — proves the realization is
///       ADDED alongside, not instead of, the GHR-γ value cell.
///
/// RED on current main: the bare let produces no realization (translate_count
/// == 0); assertion (c) fails.
#[test]
fn bare_cross_sub_geometry_let_realizes_lifted_handle() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let body = self.inner.body
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics for bare cross-sub let; \
         got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (e) Outer still has exactly one ValueCellDecl named "body" with
    //     cell_type == Type::Geometry — the GHR-γ value cell must be preserved.
    let outer_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template not found");
    let body_value_cells: Vec<_> = outer_template
        .value_cells
        .iter()
        .filter(|c| c.id.member == "body")
        .collect();
    assert_eq!(
        body_value_cells.len(),
        1,
        "expected exactly 1 ValueCellDecl named 'body' on Outer (GHR-γ value \
         cell must be preserved); got: {:#?}",
        body_value_cells
    );
    assert_eq!(
        body_value_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for Outer.body value cell"
    );

    // Build with MockGeometryKernel to capture recorded ops.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (a) No Error-severity diagnostics from build either.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (b) Specifically no "unresolvable GeomRef::Sub" — named_steps must be
    //     seeded with "inner.body" and the synthetic Translate must resolve.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) Kernel recorded a Box (Inner.body) and a Translate (Outer.body's
    //     synthetic identity-translate) whose target == the Box's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    assert!(
        recorded.len() >= 2,
        "expected at least 2 recorded kernel ops (Box for Inner.body + \
         Translate for Outer.body identity lift), got {}: {:?}",
        recorded.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Inner.body");
    let box_handle = box_rec.result_handle;

    let translate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .expect(
            "expected a Translate op recorded for Outer.body (synthetic \
             identity-translate); bare cross-sub let must emit a realization",
        );

    match translate_rec.op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Translate target should be Inner.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (d) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}
