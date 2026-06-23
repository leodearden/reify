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
use reify_core::Severity;
use reify_eval::relate_solve::{
    RealizedDatums, RelateScope, RelateSolution, auto_pose_cell, collect_relate_scope,
    realize_operand_datums, solve_relate_scope,
};
use reify_ir::{CompiledExpr, CompiledExprKind, ExportFormat, Value};
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

// ─── step-13 (OCCT-gated) — redundant-remainder verification (B2) ─────────────
//
// After ζ partitions a per-scope relation set into a driving set + a redundant
// remainder and solves ONLY the driving set, the remainder relations are NOT
// solver constraints — they are verified post-solve as geometry-backed assertions
// against the SOLVED placement (PRD §7.1 steps 2/3/5; the unified-DAG predicate
// path). This is what makes over-constraint order-independent: a redundant relation
// that is *consistent* with the driven placement passes SILENTLY (B2), whereas a
// remainder relation *violated by construction* raises a loud assertion diagnostic.
//
// `reify_eval::relate_solve::solve_relate_scope(scope, realized)` runs the whole
// per-scope pipeline over already-realized LOCAL datums — partition → solve driving
// set → verify remainder — and returns a `RelateSolution` carrying the solved Frame
// per `at auto` sub, the DOF accounting (spent/free + driving/redundant counts), and
// the verification diagnostics. It is pure (kernel-free) given `realized`; the
// realization upstream is OCCT-gated, so these e2e cases are too.
//
// **B2 e2e geometry.** Both variants are the §1 bolt-plate scope (a `bolt` `at auto`
// + a grounded `plate`) with the two §1 driving relations
// (`concentric(shank,hole)` removes 4, `flush(seat,top)` removes a net 1 → spent 5,
// residual 1) PLUS a third relation that is rank-redundant with the driving set (it
// adds no new independent Jacobian direction → it lands in the remainder, not the
// driving set):
//
//   * **consistent** — `parallel(shank_axis, hole_axis)`: its two rotational rows
//     duplicate concentric's tilt rows (redundant), and at the driven coaxial pose
//     the axes ARE parallel → residual ≈ 0 → SILENT.
//   * **violated** — `perpendicular(shank_axis, hole_axis)`: its single row is the
//     orientation gradient `shank·hole`, which is ZERO at the parallel witness
//     (redundant — it pins no new DOF), yet `concentric` drives the axes parallel,
//     so `perpendicular` (which wants them orthogonal) is VIOLATED at the solved
//     placement → a loud assertion diagnostic naming the relation.
//
// (The plan's shorthand "rank 2" describes the abstract synthetic B2 partition unit
// — `crates/reify-constraints/tests/relate_solve_tests.rs::partition_b2_*`; the e2e
// bolt-plate B2 here is rank 5 + 1 redundant, the only shape consistent with "the
// bolt is placed coaxial+flush".)
//
// RED until step-14 adds `solve_relate_scope` + `RelateSolution` to
// `crates/reify-eval/src/relate_solve.rs` — RED-by-missing-symbol (the file fails to
// compile against the absent function/type).

/// The §1 `Bolt`/`Plate` structures + a `BoltPlate` scope whose `relate{}` block
/// holds the two §1 driving relations (concentric + flush) plus one extra `third`
/// relation. Pure test data — the B2 redundant-remainder + B3 conflict variants.
/// Built from the SAME self-contained primitives as
/// `examples/geometric_relations/bolt_plate.ri`.
fn bolt_plate_with_third(third: &str) -> String {
    format!(
        r#"
structure Bolt {{
    let shank = cylinder(3mm, 20mm)
    let shank_axis : Axis = shank.axis
    let seat = rectangle(12mm, 12mm)
    let seat_plane : Plane = seat.plane
}}

structure Plate {{
    let body = box(40mm, 40mm, 5mm)
    let hole = cylinder(3.2mm, 5mm)
    let hole_axis : Axis = hole.axis
    let top = rectangle(40mm, 40mm)
    let top_plane : Plane = top.plane
}}

structure BoltPlate {{
    sub bolt : Bolt at auto
    sub plate : Plate
    relate {{
        concentric(bolt.shank_axis, plate.hole_axis)
        flush(bolt.seat_plane, plate.top_plane)
        {third}
    }}
}}
"#
    )
}

/// An identity placeholder seed Frame for the bolt's `at auto` unknown (the local
/// datums are pose-independent, so the realization ignores it — see step-5).
fn identity_bolt_seeds() -> HashMap<String, Value> {
    [("bolt".to_string(), seed_frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]))]
        .into_iter()
        .collect()
}

/// Compile `source`, collect the `BoltPlate` scope, realize its operand datums
/// against a real OCCT kernel, and run the full per-scope relate-solve.
fn solve_bolt_plate(source: &str) -> RelateSolution {
    let module = compile_source_with_stdlib(source);
    let bp = template(&module, "BoltPlate");
    let scope: RelateScope = collect_relate_scope(bp);
    let mut engine = occt_engine();
    let realized = realize_operand_datums(&scope, &module, &mut engine, &identity_bolt_seeds());
    solve_relate_scope(&scope, &realized)
}

/// step-13 — a redundant-remainder relation CONSISTENT with the driven placement
/// passes silently (B2). The driving set (concentric + flush) seats the bolt
/// coaxial+flush; the rank-redundant `parallel` relation is verified post-solve and,
/// being satisfied at that placement, raises NO diagnostic.
#[test]
fn remainder_consistent_relation_is_silent() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping remainder_consistent_relation_is_silent (B2): OCCT not available");
        return;
    }

    let solution = solve_bolt_plate(&bolt_plate_with_third(
        "parallel(bolt.shank_axis, plate.hole_axis)",
    ));

    // The driving-set solve seats the bolt: a solved Frame exists for the auto sub.
    let bolt_pose = solution
        .poses
        .get("bolt")
        .expect("the `at auto` bolt sub must receive a solved Frame");
    assert!(
        matches!(bolt_pose, Value::Frame { .. }),
        "the solved bolt pose is a Value::Frame, got {bolt_pose:?}"
    );

    // DOF accounting: concentric(4) + flush's independent normal offset(1) = spent 5,
    // residual 1 (spin about the shank axis). The third `parallel` relation is
    // rank-redundant → driving 2, redundant 1.
    assert_eq!(solution.spent, 5, "concentric(4) + flush net(1) spends 5 DOF");
    assert_eq!(solution.free, 1, "the lone residual DOF is spin about the shank axis");
    assert_eq!(solution.driving, 2, "concentric + flush are the driving set");
    assert_eq!(solution.redundant, 1, "the parallel relation is the redundant remainder");

    // The consistent remainder is SILENT — no assertion-conflict error.
    let errors: Vec<&str> = solution
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "a redundant relation consistent with the placement must be silent, got: {errors:?}"
    );
}

/// step-13 — a redundant-remainder relation VIOLATED by construction raises a loud
/// assertion diagnostic naming the relation (B2 negative). `perpendicular` is
/// rank-redundant (zero orientation gradient at the parallel witness) so it never
/// enters the driving set, yet `concentric` drives the axes parallel — so at the
/// solved placement `perpendicular` is violated and the post-solve verification
/// emits an error. The bolt is still placed (the driving solve succeeds).
#[test]
fn remainder_violated_relation_emits_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping remainder_violated_relation_emits_diagnostic (B2): OCCT not available");
        return;
    }

    let solution = solve_bolt_plate(&bolt_plate_with_third(
        "perpendicular(bolt.shank_axis, plate.hole_axis)",
    ));

    // The driving-set solve still succeeds — the violated relation is in the
    // remainder, not the driving set — so the bolt is placed.
    assert!(
        matches!(solution.poses.get("bolt"), Some(Value::Frame { .. })),
        "the driving set (concentric + flush) still solves; the bolt is placed"
    );
    assert_eq!(solution.driving, 2, "concentric + flush are the driving set");
    assert_eq!(
        solution.redundant, 1,
        "perpendicular is rank-redundant (zero gradient at the parallel config)"
    );

    // The violated remainder is LOUD — an Error diagnostic that names the offending
    // relation (`perpendicular`), the geometry-backed assertion failure.
    let errors: Vec<&str> = solution
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        !errors.is_empty(),
        "a redundant relation violated at the placement must raise an assertion diagnostic"
    );
    assert!(
        errors.iter().any(|m| m.contains("perpendicular")),
        "the assertion diagnostic must name the violated relation `perpendicular`, got: {errors:?}"
    );
}

// ─── step-15 (OCCT-gated) — conflicting relations fail loud (B3) ──────────────
//
// B3 is the §1 bolt-plate scope (concentric + flush drive the bolt coaxial+flush)
// PLUS a `distance(bolt.shank_axis, plate.hole_axis, 5mm)` relation that directly
// CONTRADICTS `concentric`: concentric forces the two axes coincident (0 mm apart)
// while distance requires them 5 mm apart — no placement can satisfy both.
//
// The build must FAIL loud with a diagnostic that
//
//   (a) identifies the MINIMAL conflict set — the two mutually-inconsistent
//       relations `concentric` + `distance`, NOT the whole block (the independent,
//       consistent `flush` relation is excluded);
//   (b) gives a GEOMETRIC explanation referencing the conflicting magnitudes
//       (concentric → coincident / 0 mm, distance → 5 mm) with NO libslvs internals
//       in the text; and
//   (c) flags the NEWEST-declared member (`distance`, declared last) as the primary
//       conflict.
//
// **Why this routes through the remainder, not an infeasible driving set.** At the
// coaxial witness the two axes are coincident, so `distance`'s perpendicular-distance
// gradient lies entirely in `concentric`'s perpendicular-translation span → it is
// rank-redundant and lands in the remainder (driving = {concentric, flush}). The
// post-solve verification finds it violated by 5 mm against the coincident placement
// `concentric` drives; because it shares BOTH datum operands with a driving relation
// that pins the same quantity to a different magnitude, ζ raises a CONFLICT diagnostic
// (minimal set + magnitudes + newest-primary), not a bare assertion. A genuinely
// infeasible *driving* set is the same mapping via a different entry point (step-16).
//
// RED until step-16 maps the conflict into the minimal-set + geometric-magnitude +
// newest-primary diagnostic; the current remainder check names only `distance` with a
// raw residual number (no `concentric`, no `mm` magnitudes, no primary framing).

/// step-15 — conflicting relations (concentric vs distance) fail the build with a
/// minimal, geometric, newest-primary conflict diagnostic (B3).
#[test]
fn conflicting_relations_fail_with_minimal_geometric_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping conflicting_relations_fail_with_minimal_geometric_diagnostic (B3): \
             OCCT not available"
        );
        return;
    }

    let solution = solve_bolt_plate(&bolt_plate_with_third(
        "distance(bolt.shank_axis, plate.hole_axis, 5mm)",
    ));

    // The build FAILS: at least one Error diagnostic.
    let errors: Vec<&str> = solution
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        !errors.is_empty(),
        "conflicting relations (concentric vs distance) must fail the build, got no errors"
    );
    let combined = errors.join("\n");
    let lower = combined.to_lowercase();

    // (a) minimal conflict set: BOTH mutually-inconsistent relations are named …
    assert!(
        combined.contains("concentric") && combined.contains("distance"),
        "the conflict diagnostic must name the minimal conflict set \
         (`concentric` + `distance`), got: {combined:?}"
    );
    // … and the independent, consistent `flush` relation is EXCLUDED (the conflict
    //     set is minimal — not the whole `relate{}` block).
    assert!(
        !combined.contains("flush"),
        "the consistent, independent `flush` relation must NOT be in the conflict set \
         (minimal conflict set, not the whole block), got: {combined:?}"
    );

    // (b) geometric explanation referencing the conflicting magnitudes …
    assert!(
        combined.contains("5") && lower.contains("mm"),
        "the explanation must reference distance's 5 mm magnitude, got: {combined:?}"
    );
    assert!(
        combined.contains("0 mm") || lower.contains("coincident"),
        "the explanation must reference concentric's coincident / 0 mm demand, got: {combined:?}"
    );
    // … in geometry, NEVER libslvs internals.
    assert!(
        !lower.contains("slvs"),
        "the explanation must speak geometry, never libslvs internals, got: {combined:?}"
    );

    // (c) the newest-declared member (`distance`) is flagged as the primary conflict.
    assert!(
        lower.contains("newest") || lower.contains("primary"),
        "the diagnostic must flag the newest-declared relation as the primary conflict, \
         got: {combined:?}"
    );
}

// ─── step-17 (OCCT-gated) — the §1 leaf consumer signal (B1) ──────────────────
//
// The committed §1 worked example `examples/geometric_relations/bolt_plate.ri`
// must build END-TO-END through the full `Engine::build` pipeline. ζ step-18 wires
// the per-scope relate-solve into the Resolution-node build pass: it solves the
// `at auto` bolt sub's 6-DOF assembly Frame from the two §1 relations (`concentric`
// removes 4, `flush` removes a net 1 → spent 5, residual 1) and writes the solved
// Frame back as the bolt sub's pose value. The surfacing walk's `eval_sub_pose`
// auto arm then reads that Frame and places the bolt via a `GeometryOp::ApplyTransform`
// (task 3901) — coaxial with the plate hole axis and flush to the plate top plane —
// while the grounded `plate` sits at identity.
//
// This is the integration leaf: it proves the committed example is a valid `.ri`
// that builds, the relate-solve is wired into `build`, and the solved placement is
// written back where the surfacing walk consumes it.
//
// **What is asserted.**
//   1. The full `Engine::build` of the committed example raises NO Error
//      diagnostics and produces non-empty geometry output (valid, builds e2e).
//   2. `result.values` carries the bolt sub's solved auto-pose Frame under
//      `auto_pose_cell("BoltPlate", "bolt")` — the writeback ζ step-18 performs and
//      the surfacing walk reads back under the SAME key — a `Value::Frame`.
//   3. The grounded `plate` sub has NO auto-pose cell (it is the fixed anchor,
//      placed at identity, never solver-determined).
//   4. The build's written-back Frame agrees with the independently-solved Frame's
//      origin (the build pass and a direct `solve_relate_scope` run the SAME solve
//      over the SAME realized datums, so they must agree).
//   5. DOF accounting: the §1 scope spends exactly 5 DOF (concentric 4 + flush net
//      1) with 1 residual (spin about the shared axis) — exact integer codimensions
//      (design "single-knob tolerance": DOF counts are exact, not tuned epsilons).
//      The coaxial+flush placement holds "within the solver's convergence tolerance"
//      — the method guarantee of `solve_frame` returning `Solved`, surfaced here as
//      the bolt receiving a concrete `Value::Frame` (never Undef / Infeasible).
//
// RED until step-18 (a) adds `auto_pose_cell` to `reify_eval::relate_solve` and
// (b) wires the relate-solve into `build_with_geometry_output` + the surfacing
// walk's `eval_sub_pose` auto arm — RED-by-missing-symbol (`auto_pose_cell`).

/// Extract a `Value::Frame`'s origin coordinates in SI metres (panicking on a
/// non-Frame / non-Point origin) — used to tie the build placement to the solve.
fn frame_origin_m(v: &Value) -> [f64; 3] {
    let Value::Frame { origin, .. } = v else {
        panic!("expected Value::Frame, got {v:?}");
    };
    let Value::Point(cs) = origin.as_ref() else {
        panic!("frame origin must be a Value::Point, got {origin:?}");
    };
    let mut o = [0.0_f64; 3];
    for (i, c) in cs.iter().take(3).enumerate() {
        o[i] = c.as_f64().unwrap_or(f64::NAN);
    }
    o
}

/// step-17 — the committed §1 bolt-plate example builds end-to-end and places the
/// `at auto` bolt at the solved coaxial+flush Frame (B1).
#[test]
fn bolt_plate_example_builds_and_places_auto_bolt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping bolt_plate_example_builds_and_places_auto_bolt (B1): OCCT not available"
        );
        return;
    }

    let source = bolt_plate_source();
    let module = compile_source_with_stdlib(&source);
    let compile_errors: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        compile_errors.is_empty(),
        "the committed §1 example must compile cleanly, got: {compile_errors:?}"
    );

    // (1) full Engine::build of the committed example — end-to-end, no errors.
    let mut engine = occt_engine();
    let result = engine.build(&module, ExportFormat::Step);
    let build_errors: Vec<&str> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        build_errors.is_empty(),
        "the §1 example must build end-to-end with no errors, got: {build_errors:?}"
    );
    let output = result
        .geometry_output
        .as_ref()
        .expect("the §1 build must produce geometry output");
    assert!(
        !output.is_empty(),
        "the §1 build geometry output must be non-empty"
    );

    // (2) the bolt sub's solved auto-pose Frame is written back where the surfacing
    //     walk reads it.
    let bolt_cell = auto_pose_cell("BoltPlate", "bolt");
    let bolt_pose = result.values.get(&bolt_cell).unwrap_or_else(|| {
        panic!(
            "the `at auto` bolt sub must have a solved auto-pose Frame written back under \
             {bolt_cell:?}; the build pass did not run / write back the relate-solve"
        )
    });
    assert!(
        matches!(bolt_pose, Value::Frame { .. }),
        "the bolt's written-back auto-pose must be a Value::Frame, got {bolt_pose:?}"
    );

    // (3) the grounded plate sub is the fixed anchor — never solver-placed.
    assert!(
        result
            .values
            .get(&auto_pose_cell("BoltPlate", "plate"))
            .is_none(),
        "the grounded `plate` sub must NOT receive an auto-pose Frame (it sits at identity)"
    );

    // (4)+(5) the build's placement agrees with the direct solve, which reports the
    //     exact DOF accounting and the coaxial+flush Solved guarantee.
    let solution = solve_bolt_plate(&source);
    let solved = solution
        .poses
        .get("bolt")
        .expect("the direct solve must place the bolt");
    assert!(
        matches!(solved, Value::Frame { .. }),
        "the solved bolt pose is a Value::Frame, got {solved:?}"
    );
    // The build pass and the direct solve run the SAME relate-solve over the SAME
    // realized datums (identity seed) → the same placement. Tie them together via
    // the Frame origin (loose 1 µm tolerance absorbs any OCCT/solver float noise).
    let built_o = frame_origin_m(bolt_pose);
    let solved_o = frame_origin_m(solved);
    for k in 0..3 {
        assert!(
            (built_o[k] - solved_o[k]).abs() < 1e-6,
            "the build's placement must agree with the direct solve at axis {k}: \
             built {built_o:?} vs solved {solved_o:?}"
        );
    }

    // Exact codimension counts — concentric(4) + flush net(1) = spent 5, residual 1.
    assert_eq!(solution.spent, 5, "§1 spends 5 DOF (concentric 4 + flush net 1)");
    assert_eq!(
        solution.free, 1,
        "§1 leaves 1 residual DOF (spin about the shared axis)"
    );
    assert_eq!(solution.driving, 2, "both §1 relations are driving");
    assert_eq!(solution.redundant, 0, "§1 has no redundant remainder");
}
