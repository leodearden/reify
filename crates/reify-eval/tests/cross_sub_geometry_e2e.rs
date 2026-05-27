//! End-to-end tests for cross-sub geometry composition (`self.<sub>.<member>`).
//!
//! Exercises the full source-to-build pipeline (parse → compile → eval → build)
//! with a `MockGeometryKernel`, verifying that the parent template's realisations
//! see the child template's named realisation handles through compound-key
//! `named_steps` entries (`"<sub>.<member>"` → handle).
//!
//! See task 3441 — eval-side `GeomRef::Sub` plumbing for cross-template handles.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, compile_source};
use reify_types::{DimensionVector, ExportFormat, GeometryOp, Severity, Value};

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
/// This is a v0.1 documented limitation: the named_steps registry would need
/// to key on the sub *instance* (not template) for two same-template subs to
/// receive distinct handles.  Lifting this requires per-instance realisation
/// of the child template, which is out of scope for the cross-sub composition
/// MVP.  See `engine_build.rs::seed_cross_sub_named_steps` for the
/// same-template aliasing note.
///
/// Regression guard: pins the current behaviour so a future change that
/// inadvertently breaks the same-template aliasing (e.g. an attempt to fix
/// it that mis-keys the registry) fails this test loudly.
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

/// Bare cross-sub geometry access (no enclosing geometry call):
/// `let copy = self.inner.body` produces a value-cell binding rather than a
/// realisation op.  The compile-side `try_resolve_cross_sub_geometry_value_ref`
/// helper emits a `CompiledExpr::value_ref(ValueCellId::new("Outer.inner",
/// "body"), Type::Geometry)`, but the eval side only seeds the *geometry*
/// `named_steps["inner.body"]` entry — it does NOT seed a value cell at
/// `("Outer.inner", "body")`.
///
/// The let binding has no `RealizationDecl` (a bare `MemberAccess` doesn't
/// pass `is_geometry_let`), so the kernel records no ops for `copy`.  The
/// value-cell lookup at eval time finds no binding and resolves to `Undef`.
///
/// This documents the v0.1 boundary: the working path is intended for use
/// *inside* a geometry call (translate / union / mirror / etc.) where the
/// parallel `try_resolve_cross_sub_geom_ref` in geometry.rs lowers the
/// access to a `GeomRef::Sub` and the kernel resolves the handle.  Bare
/// uses produce no kernel ops and no error.
///
/// Regression guard: pins (a) the no-compile-error behaviour, and (b) the
/// no-kernel-op behaviour (the kernel sees no Box for Inner.body and no
/// downstream op for `copy`).  If the bare-use path becomes broken in some
/// other way (e.g. starts emitting a spurious error or wedging the build),
/// this test fails loudly.
#[test]
fn bare_cross_sub_geometry_access_is_documented_v01_value_cell_only() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let copy = self.inner.body
}"#;
    let compiled = compile_source(source);

    // (a) No compile-time Error diagnostics — the working path lowers the
    // value-cell expression to a Type::Geometry ValueRef.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics for bare cross-sub access; \
         got: {:?}",
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

    // (b) The kernel records Inner.body's Box (because Inner's realisation
    // runs), but `copy` itself is not a `RealizationDecl` and produces no
    // additional op.
    let recorded = ops_ref.lock().unwrap().clone();
    let translate_count = recorded
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .count();
    assert_eq!(
        translate_count,
        0,
        "bare cross-sub access must NOT synthesize a Translate or any op for `copy`; \
         got translate_count={} in {:?}",
        translate_count,
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    // No build-time "unresolvable" diagnostic either — the bare path simply
    // doesn't emit kernel ops.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "bare cross-sub access must NOT trigger 'unresolvable GeomRef::Sub'; \
         got: {:?}",
        unresolvable.iter().map(|d| &d.message).collect::<Vec<_>>()
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
        GeometryOp::Box { width, height, depth } => {
            width == &override_size && height == &override_size && depth == &override_size
        }
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
