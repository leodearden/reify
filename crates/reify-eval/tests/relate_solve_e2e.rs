//! Per-scope relate-solve end-to-end tests — geometric-relations ζ (task 4386).
//!
//! This file is layered (design "Test layering"):
//!
//!   * **kernel-free unit slices** (this step, step-3) drive the pure
//!     scope-collection logic (`reify_eval::relate_solve::collect_relate_scope`)
//!     over a compiled `TopologyTemplate`. No geometry kernel is needed: ζ step-2
//!     already threaded the flat relation set + the per-sub auto-pose spec onto the
//!     compiled template, so classification reads structurally off the template.
//!   * **OCCT-gated e2e slices** (later steps 5/13/15/17) realize datums + drive
//!     the full build against the real kernel.
//!
//! ## step-3 (this slice) — RED
//!
//! `collect_relate_scope(template)` must, for the §1 `BoltPlate` scope, return a
//! `RelateScope` that partitions the scope into the relate-solve's three inputs:
//!
//!   (i)   the **auto Frame unknowns** — one per `at auto` sub, each carrying the
//!         sub id + the `free` flag + the ordered seed params (from step-2's
//!         threaded `auto_pose` spec);
//!   (ii)  the **flat ordered relation list** — the threaded per-scope relation set
//!         (each a `FunctionCall` retaining its name + operand exprs), in source
//!         order; and
//!   (iii) the **ground set** — the non-auto subs that serve as the fixed anchor.
//!
//! RED until step-4 creates `crates/reify-eval/src/relate_solve.rs` (declared in
//! `lib.rs`) with `collect_relate_scope` + the `RelateScope`/`AutoUnknown` types.
//! The file fails to compile against the missing module — the established
//! RED-by-missing-symbol convention (mirrors `relate_threading_tests.rs`).

use std::collections::HashMap;

use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_eval::relate_solve::{
    RealizedDatums, RelateScope, collect_relate_scope, realize_operand_datums,
};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::{compile_source_with_stdlib, frame_val, orientation_val, point3};

/// Read the committed §1 worked example so the kernel-free unit slice and the
/// step-17/18 e2e build exercise the SAME source — no drift between the collection
/// test and the example. `CARGO_MANIFEST_DIR` is `crates/reify-eval`;
/// `../../examples/...` is the workspace-root example dir.
fn bolt_plate_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/geometric_relations/bolt_plate.ri"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read §1 example {path}: {e}"))
}

/// Find the named template, panicking with the full diagnostics on miss.
fn template<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module.templates.iter().find(|t| t.name == name).unwrap_or_else(|| {
        panic!(
            "no template {name:?} in compiled module; diagnostics: {:#?}",
            module.diagnostics
        )
    })
}

/// The relation's function name + operand count. Each relation compiles to a
/// `FunctionCall` over its datum operands — γ types it `Relation` but keeps the
/// node a `FunctionCall` (no `Value::Relation`), so the name + arity are
/// recoverable here. Mirrors the helper in `relate_threading_tests.rs`.
fn relation_name_arity(expr: &CompiledExpr) -> (String, usize) {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => (function.name.clone(), args.len()),
        other => panic!("a collected relation must be a FunctionCall, got {other:?}"),
    }
}

/// step-3 — the §1 `BoltPlate` scope collects into the relate-solve's three
/// inputs: one auto unknown (the bolt, strict `at auto` → free=false, no seed
/// params), the plate in the ground set, and the two relations in source order.
#[test]
fn collect_relate_scope_classifies_auto_ground_and_relations() {
    let module = compile_source_with_stdlib(&bolt_plate_source());
    let bp = template(&module, "BoltPlate");

    let scope: RelateScope = collect_relate_scope(bp);

    // (i) the auto Frame unknowns — exactly the bolt, carrying its id + free flag
    //     + (empty) seed params. Bare `at auto` is strict ⇒ free=false.
    assert_eq!(
        scope.auto_unknowns.len(),
        1,
        "§1 has exactly one `at auto` sub (the bolt), got {:?}",
        scope
            .auto_unknowns
            .iter()
            .map(|u| u.sub.as_str())
            .collect::<Vec<_>>()
    );
    let bolt = &scope.auto_unknowns[0];
    assert_eq!(bolt.sub, "bolt", "the lone auto unknown is the bolt sub");
    assert!(!bolt.free, "bare `at auto` is strict — free=false");
    assert!(
        bolt.seed_params.is_empty(),
        "bare `at auto` carries no seed/component-fix params, got {:?}",
        bolt.seed_params
    );

    // (iii) the ground set — the non-auto plate sub is the fixed anchor; the auto
    //       bolt is NOT in the ground set.
    assert_eq!(
        scope.ground,
        vec!["plate".to_string()],
        "the grounded `plate` sub (no `at auto`) is the sole anchor; the auto bolt \
         is an unknown, not ground"
    );

    // (ii) the flat relation list — both §1 relations, in source order, each
    //      retaining its name + two operand exprs.
    let rels: Vec<(String, usize)> = scope.relations.iter().map(relation_name_arity).collect();
    assert_eq!(
        rels,
        vec![("concentric".to_string(), 2), ("flush".to_string(), 2)],
        "the §1 scope collects both relations in source order (concentric then \
         flush), each retaining its name + 2 operand exprs"
    );
}

// ─── step-5 (OCCT-gated) — operand datum realization, single-shot ─────────────
//
// `realize_operand_datums(scope, module, engine, seeds)` realizes each relation
// operand's LOCAL datum — for §1 the four operands
//
//   * `bolt.shank_axis` / `plate.hole_axis` → `Value::Axis`, and
//   * `bolt.seat_plane` / `plate.top_plane`  → `Value::Plane`
//
// — by realizing each referenced sub's structure (`Bolt`, `Plate`) single-shot in
// its OWN local frame and projecting the operand datum via the ε feature→datum
// bridge + β datum projections. This needs the REAL OCCT kernel: the analytic
// `GeomAbs_*` feature classification that turns `shank.axis` into a `Value::Axis`
// cannot be performed by the mock kernel (it only replays staged datums), so the
// realization is OCCT-gated — mirroring `feature_datum_tests.rs`'s B8 e2e setup.
//
// **The single-shot property (the load-bearing assertion):** local datums are
// pose-independent. The `seeds` argument is the relate-solve's *current Frame
// estimate* for each `at auto` unknown (the assembly pose the bolt would be
// placed at). Realizing the operands under two DIFFERENT seed Frames must yield
// bit-identical LOCAL datums — proving the realization happens ONCE (single-shot)
// and is never re-run per solver iteration. `realize_operand_datums` therefore
// computes each operand datum in the sub's own frame, BEFORE placement.
//
// RED until step-6 implements `realize_operand_datums` + the `RealizedDatums`
// type in `crates/reify-eval/src/relate_solve.rs` — RED-by-missing-symbol (the
// file fails to compile against the absent function/type).

/// Spawn an OCCT-backed `Engine` (the real geometry kernel + a simple constraint
/// checker), mirroring `feature_datum_tests.rs`'s B8 setup. Used to realize the
/// subs' local geometry → datum projections.
fn occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)))
}

/// A placeholder seed Frame for one `at auto` unknown, at `origin` with the given
/// quaternion `orientation` — a stand-in assembly pose the local-datum realization
/// must ignore.
fn seed_frame(origin: [f64; 3], q: [f64; 4]) -> Value {
    frame_val(
        point3(origin[0], origin[1], origin[2]),
        orientation_val(q[0], q[1], q[2], q[3]),
    )
}

/// step-5 — realizing the §1 scope's relation operands yields concrete datum
/// `Value`s of the right kind, and those LOCAL datums are pose-independent
/// (identical under two distinct placeholder seed Frames → single-shot).
#[test]
fn realize_operand_datums_yields_concrete_pose_independent_local_datums() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping realize_operand_datums (§1 B1) realization: OCCT not available");
        return;
    }

    let module = compile_source_with_stdlib(&bolt_plate_source());
    let bp = template(&module, "BoltPlate");
    let scope = collect_relate_scope(bp);

    // Two DISTINCT placeholder seed Frames for the bolt's `at auto` unknown: the
    // identity pose, and a translated + 90°-about-Z pose. The plate is grounded
    // (not in `seeds`); its local datums realize at identity either way.
    let seeds_a: HashMap<String, Value> =
        [("bolt".to_string(), seed_frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]))]
            .into_iter()
            .collect();
    let seeds_b: HashMap<String, Value> = [(
        "bolt".to_string(),
        seed_frame(
            [0.1, 0.05, 0.2],
            [std::f64::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2],
        ),
    )]
    .into_iter()
    .collect();

    // Two independent engines (no shared build cache) so the second realization is
    // a genuine re-run — identical results then truly prove seed-independence, not
    // a memoized echo of the first call.
    let mut engine_a = occt_engine();
    let mut engine_b = occt_engine();
    let datums_a: RealizedDatums = realize_operand_datums(&scope, &module, &mut engine_a, &seeds_a);
    let datums_b: RealizedDatums = realize_operand_datums(&scope, &module, &mut engine_b, &seeds_b);

    // (i) Each of the four operands realizes to the right concrete datum KIND.
    let operands = [
        ("bolt", "shank_axis"),
        ("plate", "hole_axis"),
        ("bolt", "seat_plane"),
        ("plate", "top_plane"),
    ];
    for (sub, member) in operands {
        let v = datums_a
            .get(sub, member)
            .unwrap_or_else(|| panic!("{sub}.{member} must realize to a datum Value"));
        let is_axis = matches!(v, Value::Axis { .. });
        let is_plane = matches!(v, Value::Plane { .. });
        if member.ends_with("axis") {
            assert!(is_axis, "{sub}.{member} must realize to a Value::Axis, got {v:?}");
        } else {
            assert!(is_plane, "{sub}.{member} must realize to a Value::Plane, got {v:?}");
        }
    }

    // (ii) single-shot pose-independence: every LOCAL datum is identical under the
    //      two seed Frames. The bolt's seed differs between the two runs, so its
    //      `shank_axis`/`seat_plane` invariance is the load-bearing guard that the
    //      datum is realized BEFORE (independent of) the assembly placement.
    for (sub, member) in operands {
        assert_eq!(
            datums_a.get(sub, member),
            datums_b.get(sub, member),
            "{sub}.{member} LOCAL datum must be pose-independent — identical under two \
             distinct placeholder seed Frames (single-shot realization)"
        );
    }
}
