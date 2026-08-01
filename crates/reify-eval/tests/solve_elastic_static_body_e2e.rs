// `Value` is used as a `BTreeMap` key when reading the multi_case `MultiCaseResult`
// (`Value::Map{"cases" → Map<String, ElasticResult>}`); mirror the sibling
// `multi_case_compute_node.rs`'s allow (task 4088) so the Value-keyed map reads
// don't trip `clippy::mutable_key_type`.
#![allow(clippy::mutable_key_type)]
//! Capstone end-to-end test for the FEA **body-arg** substrate (task 4870,
//! step-9/10): the full `.ri` → VolumeMesh-demand → realize (OCCT→Gmsh tet) →
//! project → consume chain for the arity-5 `body : Solid` overload of
//! `solve_elastic_static`.
//!
//! Drives `fixtures/fea_body_cantilever.ri` (`let body = box(1000mm, 100mm,
//! 100mm)` + a body-arg solve) through a real OCCT+Gmsh engine and asserts the
//! solve ran on the **realized** tet VolumeMesh — the body realization is
//! `(VolumeMesh, Gmsh)` and the ElasticResult §7a resample grid follows the
//! realized AABB (its node count ≠ the 854-node synthetic `nx×1×6` box the
//! scalar-dims path builds), converged. A companion asserts the unchanged
//! scalar-dims fixture still yields the 854-node synthetic grid — the new
//! overload is strictly additive.
//!
//! ## Why register the trampoline manually (NOT `register_compute_fns`)
//!
//! `compute_targets::register_compute_fns` registers `solver::elastic_static` as
//! BOTH VolumeMesh-demanding AND *boundary*-demanding (mod.rs — task 4092). A
//! boundary-demanded body realization routes the tessellated OCCT surface
//! through gmsh's *attributed* producer (`mesh_surface_to_volume_attributed`),
//! which **SIGSEGVs in tetgen boundary recovery on real OCCT-tessellated
//! surfaces** (#4876 — a crash `catch_unwind` cannot trap; see
//! `tests/fea_face_selector_bc_e2e.rs`, whose end-to-end assertion is `#[ignore]`d
//! on exactly this). Task 4870 scope is the plain VolumeMesh realization
//! (task 4743); face-selector BC attribution (task 4092) is explicitly out of
//! scope. So this harness mirrors `volume_mesh_realization_e2e.rs`: it installs
//! ONLY the `solver::elastic_static` trampoline + `register_volume_mesh_demand`
//! (the plain `mesh_surface_to_volume` producer, no boundary attribution), which
//! meshes the box cleanly.
//!
//! ## Gmsh dead-strip discipline (CRITICAL)
//!
//! `reify-kernel-gmsh` is a **dev-dependency** of `reify-eval`. A dev-dep rlib is
//! only linked into a test binary that references one of its symbols; otherwise
//! the linker strips it and the crate's `inventory::submit!` (register.rs) never
//! fires, leaving `"gmsh"` invisible to `Engine::ensure_gmsh_kernel()`. The
//! `extern crate reify_kernel_gmsh as _;` anchor below forces the rlib to link
//! unconditionally, mirroring `volume_mesh_realization_e2e.rs`.
//!
//! **Do NOT reference any `reify_kernel_gmsh` symbol from other (non-gmsh)
//! reify-eval test binaries** — doing so pulls gmsh's `inventory::submit!` into
//! their binaries and breaks their OCCT-only `kernel_count` / registry-size
//! assertions. This binary is a *gmsh* binary, so the anchor is expected.
//!
//! ## `has_gmsh` coverage in the verify pipeline (task 5008 review #1)
//!
//! Both capstones below are `#[cfg(has_gmsh)]`-gated (`build.rs` sets it when
//! `reify_build_utils::find(NativeDep::Gmsh)` locates the prebuilt native lib
//! under `/opt/reify-deps`, per CLAUDE.md "Native deps"). Confirmed present in
//! this workspace's verify environment: `libgmsh.so*` resolves under
//! `/opt/reify-deps/lib`, and `cargo test -p reify-eval --test
//! solve_elastic_static_body_e2e -- --list` enumerates all three tests in this
//! binary (i.e. `#[cfg(has_gmsh)]` compiles TRUE, not out) — so these
//! capstones exercise the full `.ri` → demand → realize → consume chain under
//! real OCCT+Gmsh in the gate, not just opportunistically. If a future verify
//! environment lacks the native gmsh lib, `has_gmsh` compiles both capstones
//! out entirely (the `not(has_gmsh)` skip-stub covers the gap) and the
//! gmsh-free unit tests in `elastic_static.rs`
//! (`compute_demanded_reprs_resolves_stdlib_optimized_consumer_via_self_functions`
//! for GAP A; the `volume_mesh_to_solver_mesh_*orphan*` family for GAP B)
//! remain the load-bearing regression guards for the two mechanisms these
//! capstones compose.

// Gmsh linker anchor — see the module doc above.
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

// OCCT linker anchor. The body `box(...)` realization needs a real BRep kernel
// as the lex-min default so it tessellates into a closed surface the gmsh tet
// path can volume-mesh. `make_occt_engine()` references
// `reify_kernel_occt::OcctKernelHandle` directly (dev-dep), so this is
// belt-and-suspenders for the link.
#[cfg(has_gmsh)]
extern crate reify_kernel_occt as _;

/// Grid node count of the scalar-dims **synthetic** cantilever mesh for the
/// standard 1 m × 100 mm × 100 mm beam: `synthetic_grid_counts(1.0, 0.1)` gives
/// `nx = round(1.0/0.1 × 6) = 60`, `ny = 1`, `nz = 6`, so nodes =
/// `(60+1)(1+1)(6+1) = 61×2×7 = 854`. The realized-mesh path MUST differ from
/// this (its `ny` follows the 100 mm width → `ny = 6`, not the hardcoded 1).
const SYNTHETIC_GRID_NODES: usize = 854;

/// Build a fresh `Engine` backed by a real OCCT kernel as the lex-min BRep
/// default (so `box(...)` realizes into a tessellatable closed surface),
/// mirroring `volume_mesh_realization_e2e.rs::make_occt_engine`.
#[cfg(has_gmsh)]
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// Extract a named field from an ElasticResult `Value` (StructureInstance or the
/// Map fallback), mirroring `solve_elastic_static_e2e.rs::extract_field`.
fn extract_field(result: &reify_ir::Value, field: &str) -> Option<reify_ir::Value> {
    match result {
        reify_ir::Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        reify_ir::Value::Map(m) => m.get(&reify_ir::Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Extract the `SampledField` behind a named `Value::Field { Sampled }` in a
/// result. Panics with a descriptive message if the field is absent or not a
/// Sampled field (the body/scalar solve always emits Sampled displacement).
fn sampled_field(result: &reify_ir::Value, field: &str) -> reify_ir::SampledField {
    let field_val =
        extract_field(result, field).unwrap_or_else(|| panic!("field '{field}' not found in result"));
    match &field_val {
        reify_ir::Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, reify_ir::FieldSourceKind::Sampled),
                "field '{field}' source must be Sampled, got {source:?}"
            );
            match lambda.as_ref() {
                reify_ir::Value::SampledField(sf) => sf.clone(),
                other => panic!("field '{field}' lambda must be Value::SampledField, got {other:?}"),
            }
        }
        other => panic!("field '{field}' must be Value::Field, got {other:?}"),
    }
}

/// `cfg(has_gmsh)`: the body-arg `solve_elastic_static` runs on the realized tet
/// VolumeMesh (task 4870 capstone).
///
/// Registers ONLY the `solver::elastic_static` trampoline + its VolumeMesh
/// demand (NOT `register_compute_fns` — see the module doc's boundary-demand /
/// #4876 rationale), acquires gmsh, and `build()`s the body fixture. The
/// module-static demand pass overrides `body`'s realization to VolumeMesh; the
/// realization edge tessellates the OCCT box and gmsh-tet-meshes it; the
/// post-hydration redispatch projects the realized VolumeMesh into the solver
/// node's `realization_inputs`, where the trampoline's body path consumes it as
/// `provided_mesh`.
///
/// Asserts:
///   (1) provenance — the `body` realization is `(VolumeMesh, Gmsh)` (the
///       demand → realize edge fired and re-terminated the body on gmsh);
///   (2) the ElasticResult §7a grid follows the REALIZED AABB, NOT the synthetic
///       box: the displacement grid's Y axis has `ny+1 = 7` nodes (the realized
///       100 mm width heuristic `ny = 6`, vs the synthetic hardcoded `ny = 1` →
///       2 nodes) and the total node count `61×7×7 = 2989 ≠ 854`;
///   (3) converged.
///
/// The full `.ri` → demand → realize → consume chain composes end-to-end
/// (steps 2/4/6/8 built the substrate; task 5008 GAP A/B closed the two
/// engine-side gaps; this is the integration capstone).
#[cfg(has_gmsh)]
#[test]
fn body_solve_runs_on_realized_volume_mesh() {
    use reify_core::{KernelId, Severity, ValueCellId};
    use reify_ir::{ExportFormat, ReprKind, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping body_solve_runs_on_realized_volume_mesh: OCCT not available \
             (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/fea_body_cantilever.ri"
    ));

    let mut engine = make_occt_engine();
    // Manual registration — see the module doc. Trampoline + VolumeMesh demand
    // ONLY; NO boundary demand (that routes through the #4876-SIGSEGV attributed
    // producer).
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("solver::elastic_static");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    // build() (not eval()) realizes geometry through the kernel and runs the
    // post-hydration redispatch — the only path that projects a VolumeMesh into a
    // geometry-consuming @optimized consumer.
    let build_result = engine.build(&compiled, ExportFormat::Step);

    // No Error diagnostics — a clean realize + solve is required before asserting
    // on the result values.
    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the body-arg build, got: {errors:?}"
    );

    // ── (1) provenance: body realized as a tet VolumeMesh on gmsh ─────────────
    let provenance = engine.realization_kernel_provenance();
    assert!(
        provenance
            .iter()
            .any(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh),
        "the VolumeMesh-demanded `body` realization must be re-terminated at \
         repr == VolumeMesh on kernel == Gmsh (the demand → realize edge). \
         provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );

    // ── (2) the ElasticResult §7a grid follows the REALIZED mesh, not synthetic ─
    let result_cell = ValueCellId::new("FeaBodyCantilever", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyCantilever.result not found in build values"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "body-arg result must be a populated ElasticResult (StructureInstance/Map), \
         got: {result_val:?} — a pre-hydration Failed/Undef here means the \
         redispatch did not deliver the realized mesh to the trampoline"
    );

    let disp = sampled_field(result_val, "displacement");

    // Realized-path §7a grid derivation (elastic_static.rs realized arm):
    //   ext = box AABB = [1.0, 0.1, 0.1] m, nz = 6, dz = ext_z = 0.1
    //   nx = round(ext_x/dz × 6) = round(60) = 60
    //   ny = round(ext_y/dz × 6) = round(6)  = 6   (vs synthetic ny = 1)
    //   nodes = (60+1)(6+1)(6+1) = 61 × 7 × 7 = 2989
    const REALIZED_GRID_NODES: usize = 61 * 7 * 7; // 2989
    let node_count = disp.data.len() / 3;

    // Structural proof the realized-AABB heuristic (not the synthetic hardcode)
    // drove the grid: the synthetic path fixes ny = 1 (2 nodes on Y); the realized
    // 100 mm width gives ny = 6 (7 nodes on Y).
    assert_eq!(
        disp.axis_grids.len(),
        3,
        "displacement must be a 3D Regular grid, got {} axes",
        disp.axis_grids.len()
    );
    assert!(
        disp.axis_grids[1].len() > 2,
        "the Y axis must have > 2 grid nodes (realized ny = 6 → 7 nodes); a 2-node \
         Y axis means the SYNTHETIC ny = 1 box drove the solve, not the realized \
         mesh. axis_grids Y len = {}",
        disp.axis_grids[1].len()
    );
    assert_ne!(
        node_count, SYNTHETIC_GRID_NODES,
        "the body-arg solve must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
         synthetic grid — the realized VolumeMesh must drive the §7a resample grid"
    );
    assert_eq!(
        node_count, REALIZED_GRID_NODES,
        "the body-arg §7a grid node count must equal the realized-AABB heuristic \
         {REALIZED_GRID_NODES} (61×7×7 for the 1 m × 100 mm × 100 mm box), got {node_count}"
    );

    // ── (3) converged ─────────────────────────────────────────────────────────
    assert_eq!(
        extract_field(result_val, "converged"),
        Some(Value::Bool(true)),
        "the body-arg realized-mesh solve must converge"
    );
}

/// Two-case **body-arg** fixture for the multi-case shared-realization capstone
/// (step-11). Same 1 m × 100 mm × 100 mm box `body` as `fea_body_cantilever.ri`,
/// but solved through the arity-4 `body : Solid` overload of `solve_load_cases`
/// with two `LoadCase`s differing only in tip force (1000 N vs 2000 N). Both
/// cases share the ONE realized tet `VolumeMesh` of `body` — the multi_case
/// trampoline forwards `realization_inputs` unchanged to every per-case sub-solve.
#[cfg(has_gmsh)]
const MULTI_CASE_BODY_SOURCE: &str = r#"
structure FeaBodyMultiCase {
    let material = Steel_AISI_1045()
    let body     = box(1000mm, 100mm, 100mm)
    let lc1 = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let lc2 = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "tip", force: 2000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result = solve_load_cases(material, body, [lc1, lc2], ElasticOptions())
}
"#;

/// `cfg(has_gmsh)`: the body-arg `solve_load_cases` runs EVERY case on the ONE
/// shared realized tet VolumeMesh (task 4870 multi-case capstone, step-11).
///
/// Drives `solve_load_cases(material, body, [operating, overload],
/// ElasticOptions())` through the same OCCT+Gmsh `build()` path as the
/// single-case capstone. The outer consumer is `solver::multi_case` (registered
/// VolumeMesh-demanding, NO boundary demand — see the module doc's #4876
/// rationale); each per-case sub-solve routes through `solver::elastic_static`,
/// which the multi_case trampoline dispatches with `realization_inputs` forwarded
/// UNCHANGED (step-6), so all cases share ONE realization.
///
/// Asserts:
///   (b) EXACTLY ONE `(VolumeMesh, Gmsh)` realization exists for the shared
///       `body` across BOTH cases — no re-mesh per case;
///   (a) EACH case's ElasticResult §7a grid follows the REALIZED AABB, NOT the
///       synthetic box: the Y axis has `ny+1 = 7` nodes (realized `ny = 6`, vs
///       synthetic `ny = 1` → 2) and total `61×7×7 = 2989 ≠ 854`; converged.
///
/// The multi_case body path composes the SAME `.ri` → demand → realize →
/// consume chain as the single-case capstone
/// (`body_solve_runs_on_realized_volume_mesh`), closed by task 5008 GAP A/B.
#[cfg(has_gmsh)]
#[test]
fn multi_case_body_solve_shares_one_realization_across_cases() {
    use reify_core::{KernelId, Severity, ValueCellId};
    use reify_ir::{ExportFormat, ReprKind, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping multi_case_body_solve_shares_one_realization_across_cases: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(MULTI_CASE_BODY_SOURCE);

    let mut engine = make_occt_engine();
    // Manual registration — see the module doc. The outer consumer is
    // solver::multi_case (VolumeMesh-demanding, NO boundary demand); each per-case
    // sub-solve routes through solver::elastic_static, so BOTH trampolines are
    // registered. NOT register_compute_fns (that installs the #4876-SIGSEGV
    // boundary-attributed producer).
    engine.register_compute_fn(
        "solver::multi_case",
        reify_eval::compute_targets::multi_case::solve_multi_case_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("solver::multi_case");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    // build() (not eval()) realizes geometry through the kernel and runs the
    // post-hydration redispatch — the only path that projects a VolumeMesh into a
    // geometry-consuming @optimized consumer.
    let build_result = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the multi-case body build, got: {errors:?}"
    );

    // ── (b) EXACTLY ONE (VolumeMesh, Gmsh) realization for the shared body ─────
    //
    // `body` is a single geometry `let`, so the demand → realize edge must produce
    // exactly ONE tet VolumeMesh; the multi_case trampoline forwards it unchanged
    // to every per-case sub-solve (step-6). A count > 1 would mean the body was
    // re-meshed per case (broken forwarding / non-shared realization).
    let provenance = engine.realization_kernel_provenance();
    let vm_realizations: Vec<_> = provenance
        .iter()
        .filter(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh)
        .collect();
    assert_eq!(
        vm_realizations.len(),
        1,
        "the shared `body` must produce EXACTLY ONE (VolumeMesh, Gmsh) realization \
         for BOTH cases — the multi_case trampoline forwards realization_inputs \
         unchanged, so cases share one realization (no re-mesh per case). \
         VolumeMesh/Gmsh realizations: {:?}",
        vm_realizations
    );

    // ── (a) EACH case ran on the REALIZED mesh, not the synthetic box ─────────
    let result_cell = ValueCellId::new("FeaBodyMultiCase", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyMultiCase.result not found in build values"));

    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_load_cases result must be a MultiCaseResult Value::Map, got: {other:?} \
             — a pre-hydration Failed/Undef here means the redispatch did not deliver \
             the realized mesh to the multi_case trampoline"
        ),
    };
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries (operating, overload), got {}",
        cases_map.len()
    );

    // Realized-path §7a grid: box AABB [1.0, 0.1, 0.1] m ⇒ nx=60, ny=6, nz=6 ⇒
    // (60+1)(6+1)(6+1) = 61×7×7 = 2989 (vs the synthetic ny=1 → 854). See the
    // single-case capstone above for the full derivation.
    const REALIZED_GRID_NODES: usize = 61 * 7 * 7; // 2989

    for case_name in ["operating", "overload"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "cases map must contain \"{case_name}\"; got: {:?}",
                    cases_map.keys().collect::<Vec<_>>()
                )
            });

        let disp = sampled_field(case_val, "displacement");
        let node_count = disp.data.len() / 3;

        // Structural proof the realized-AABB heuristic (not the synthetic
        // hardcode) drove each case: synthetic fixes ny=1 (2 Y-nodes); the
        // realized 100 mm width gives ny=6 (7 Y-nodes).
        assert_eq!(
            disp.axis_grids.len(),
            3,
            "case \"{case_name}\": displacement must be a 3D Regular grid, got {} axes",
            disp.axis_grids.len()
        );
        assert!(
            disp.axis_grids[1].len() > 2,
            "case \"{case_name}\": the Y axis must have > 2 grid nodes (realized ny=6 → \
             7 nodes); a 2-node Y axis means the SYNTHETIC ny=1 box drove the solve, \
             not the realized mesh. axis_grids Y len = {}",
            disp.axis_grids[1].len()
        );
        assert_ne!(
            node_count, SYNTHETIC_GRID_NODES,
            "case \"{case_name}\": must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
             synthetic grid — the shared realized VolumeMesh must drive the §7a grid"
        );
        assert_eq!(
            node_count, REALIZED_GRID_NODES,
            "case \"{case_name}\": §7a grid node count must equal the realized-AABB \
             heuristic {REALIZED_GRID_NODES} (61×7×7 for the 1 m × 100 mm × 100 mm box), \
             got {node_count}"
        );
        assert_eq!(
            extract_field(case_val, "converged"),
            Some(Value::Bool(true)),
            "case \"{case_name}\": the shared realized-mesh solve must converge"
        );
    }
}

/// Single-case **NON-PRISMATIC** body fixture (task 4152, step-12).
/// Structurally identical to `MULTI_CASE_BODY_SOURCE` above and to
/// `fixtures/fea_body_cantilever.ri` — the ONLY material change is the `let body`
/// expression, so any behavioural difference is attributable solely to the shape.
///
/// ## Why a primitive `cylinder(...)` and NOT a CSG solid
///
/// The obvious "non-prismatic" body is a boolean — `difference(box, cyl)` — and
/// it is the WRONG choice here. Two independent reasons, both recorded in-tree:
///
///   1. **Mesher hazard.** `fixtures/morph_box.ri`'s own header records that a
///      boolean-derived B-rep leaves coplanar/seam triangulation that crashes
///      tetgen's boundary recovery (`recoveredgebyflips`), which is why that
///      fixture deliberately uses a primitive box. The only `.ri`-level
///      CSG → volume-mesh test (`morph_arm_e2e.rs`'s
///      `e2e_structural_tick_remeshes_and_records_ineligible`) is `#[ignore]`d on
///      exactly that. A primitive with a curved lateral surface gives us the
///      shape-agnosticism signal this test is FOR, with no boolean seams.
///   2. **Syntax.** `difference` is a strictly-BINARY compiler builtin
///      (`geometry_boolean.rs`, arity-enforced in `units.rs`), not an `.ri`
///      stdlib fn, and there is NO `-` operator sugar for solids (`BinOp::Sub` is
///      numeric/length only) — so `box(...) - box(...)` does not even parse.
///
/// So: do NOT "improve" this into a `difference(...)`. If curved-vs-boolean
/// coverage is wanted, that is a separate fixture gated on the tetgen fix.
///
/// ## Why the realized grid cannot collide with the synthetic one
///
/// `cylinder(radius, height)` rises along +Z, so the realized AABB extent is
/// `[2r, 2r, h] = [0.1, 0.1, 0.2] m`. The realized §7a arm
/// (`elastic_static.rs`) derives `nz = 6`, `dz = ext_z`,
/// `nx = round(ext_x/dz · nz) = round(3) = 3`, `ny = round(3) = 3` — so
/// `(3+1)(3+1)(6+1) = 112` nodes, nowhere near the synthetic 854. The x-extent
/// (`2r = 0.1 m`) is also far above `MIN_SOLVE_X_EXTENT` (1e-9), so
/// `has_usable_realized_solver_mesh`'s non-degeneracy gate passes and the
/// coordinate-selected clamp (`x ≈ x_min`) / tip (`x ≈ x_max`) node sets land on
/// opposite sides of the circular cross-section. That is mechanically unusual
/// for a cantilever, but it is valid for what this test asserts — that a
/// realization occurred and produced real Sampled fields — not beam-theory
/// accuracy, which the prismatic capstones above already cover.
///
/// ACHIEVABILITY BASIS (measured, not assumed): `reify-kernel-conformance`'s
/// `occt_fixtures_mesh_to_volume_and_revalidate_through_gmsh` iterates the
/// `["box", "cylinder", "boolean", "fillet"]` fixtures through OCCT-tessellate →
/// `GmshKernel::mesh_surface_to_volume` → `assert_valid_volume_mesh` and PASSES
/// (not `#[ignore]`d) — so a real OCCT cylinder is already proven to mesh
/// through the exact PLAIN producer this test uses.
#[cfg(has_gmsh)]
const NON_PRISMATIC_BODY_SOURCE: &str = r#"
structure FeaBodyNonPrismatic {
    let material = Steel_AISI_1045()
    let body     = cylinder(50mm, 200mm)
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount    = FixedSupport(target: "root")
    let result = solve_elastic_static(material, body, [tip_load], [mount], ElasticOptions())
}
"#;

/// Two-case **NON-PRISMATIC** body fixture (task 4152, step-12 / step-13).
/// The same `cylinder(50mm, 200mm)` body as `NON_PRISMATIC_BODY_SOURCE`, solved
/// through the arity-4 `body : Solid` overload of `solve_load_cases` with two
/// `LoadCase`s differing ONLY in tip force (1000 N vs 2000 N) — so
/// `(body, material, element_order, mesh_size)` is shared by construction and the
/// single realized tet VolumeMesh must serve BOTH cases.
///
/// Mirrors `MULTI_CASE_BODY_SOURCE`'s verified syntax exactly, including the
/// INLINE `ElasticOptions()` argument (not a shared `let opts` binding): sharing
/// is already guaranteed by the single `body`/`material` args, and the inline
/// form is the shape proven to compile here. See `NON_PRISMATIC_BODY_SOURCE`
/// above for the cylinder-vs-CSG rationale.
///
/// ## Why this fixture declares a `RepresentationWithin` bound — PRECONDITION
///
/// **Without the bound, `realization_entries` cannot move for ANY body shape.**
/// This is not incidental; it is a deliberate production semantic, and reading
/// this fixture without understanding it will mislead.
///
/// The terminal realization-cache insert (`engine_build.rs`, the site
/// `insert_terminal` was wired into) is triple-gated on
/// `is_terminal_realization && demanded_tol.is_some() && realization_name.is_some()`.
/// `demanded_tol` comes from `compute_demanded_tols` →
/// `demanded_tolerance_for_output(&t.name, &r.id.entity)`, whose priority chain
/// is `extract_output_tolerance_bound` then `active_tolerance_for`. A `.ri`
/// module with NO tolerance contract yields `None` on both, and
/// `engine_build.rs` states the consequence outright: *"when both return None no
/// cache entry is written (the helper preserves historical 'no tolerance
/// contract → no caching' semantics)"*. Measured on the un-bounded form of this
/// very fixture: `realization_cache().len() == 0` and the counter stayed 0, even
/// though `realization_kernel_provenance()` reported one `(VolumeMesh, Gmsh)`
/// realization — the mesh IS built, it is simply never cached.
///
/// So the bound below is what makes the re-mesh-avoidance signal OBSERVABLE. It
/// does not manufacture the signal: the realization count is whatever it is;
/// the bound only decides whether the cache records it.
///
/// Two mechanical notes on the shape, both load-bearing:
///   * `extract_output_tolerance_bound`'s gate 1 is `id.entity !=
///     output_template_name → continue`, and `output_template_name` is bound to
///     the template that OWNS the realization. So the constraint must live in
///     THIS structure — a sibling checker structure (the
///     `examples/representation_within.ri` / `fea_bracket_member_access.ri`
///     shape) would be filtered out here.
///   * The extractor DISCARDS the subject (`let Some((_vcid, _struct_name,
///     si_value)) = …`) and min-folds only the bound, so the subject need only
///     satisfy the recognition shape: `arg0.result_type` must be
///     `Type::StructureRef(_)` and `arg0.kind` a bare `ValueRef`. `mesh_tol`
///     below is exactly that. It is deliberately NOT the cylinder: pointing the
///     assertion at realized geometry would make it a three-valued
///     Satisfied/Violated *measurement* of chord deviation, which is
///     `representation_within.ri`'s subject and not this test's. Here it stays
///     Indeterminate (no realization under `FeaMeshTolerance`), which is the
///     graceful no-diagnostic path.
#[cfg(has_gmsh)]
const NON_PRISMATIC_MULTI_CASE_BODY_SOURCE: &str = r#"
structure FeaMeshTolerance {
    param nominal : Real = 1.0
}

structure FeaBodyNonPrismaticMultiCase {
    param mesh_tol : FeaMeshTolerance = FeaMeshTolerance()
    constraint RepresentationWithin(mesh_tol, 100um)

    let material = Steel_AISI_1045()
    let body     = cylinder(50mm, 200mm)
    let lc1 = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let lc2 = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "tip", force: 2000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result = solve_load_cases(material, body, [lc1, lc2], ElasticOptions())
}
"#;

/// ONE-case control variant of [`NON_PRISMATIC_MULTI_CASE_BODY_SOURCE`] — byte-
/// identical but for the dropped `lc2` (task 4152, step-13 assertion 2).
///
/// This is the shape-robust half of the B9 signal. Asserting "the two-case build
/// realizes exactly once" alone is weak: a module that realized once for reasons
/// unrelated to case-sharing would satisfy it. Comparing the two-case delta
/// against the ONE-case delta over the SAME body isolates the actual claim —
/// adding a second load case adds ZERO new realization entries — independently
/// of how many realizations the module contains for other reasons.
#[cfg(has_gmsh)]
const NON_PRISMATIC_ONE_CASE_BODY_SOURCE: &str = r#"
structure FeaMeshTolerance {
    param nominal : Real = 1.0
}

structure FeaBodyNonPrismaticMultiCase {
    param mesh_tol : FeaMeshTolerance = FeaMeshTolerance()
    constraint RepresentationWithin(mesh_tol, 100um)

    let material = Steel_AISI_1045()
    let body     = cylinder(50mm, 200mm)
    let lc1 = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result = solve_load_cases(material, body, [lc1], ElasticOptions())
}
"#;

/// `cfg(has_gmsh)`: de-risking capstone — a **NON-PRISMATIC** (curved-surface)
/// body realizes and solves on the realized tet VolumeMesh (task 4152, step-11).
///
/// Every body-arg capstone above drives a `box(...)`, whose realized AABB grid
/// happens to be near-prismatic. This test proves the chain is not box-specific:
/// a body with a genuinely curved lateral surface realizes through the same
/// OCCT-tessellate → gmsh `mesh_surface_to_volume` producer and drives the §7a
/// resample grid off ITS AABB, so the resulting node count cannot coincide with
/// the {`SYNTHETIC_GRID_NODES`}-node synthetic `nx×1×6` box.
///
/// Uses the arity-5 `solve_elastic_static(material, body, loads, supports,
/// options)` overload (`stdlib/solver_elastic.ri`), registering ONLY the
/// `solver::elastic_static` trampoline + `register_volume_mesh_demand` — NOT
/// `register_compute_fns` (see the module doc's #4876 boundary-demand
/// rationale).
///
/// Asserts:
///   (1) `build()` yields no `Severity::Error` diagnostics;
///   (2) EXACTLY ONE `(VolumeMesh, Gmsh)` realization (the single geometry
///       `let body` yields one realization edge);
///   (3) `displacement` is a 3-axis Regular Sampled field whose node count
///       DIFFERS from the {`SYNTHETIC_GRID_NODES`}-node synthetic grid — i.e.
///       the realized curved-body AABB, not the synthetic box, drove the grid;
///   (4) converged.
#[cfg(has_gmsh)]
#[test]
fn non_prismatic_body_solve_runs_on_realized_volume_mesh() {
    use reify_core::{KernelId, Severity, ValueCellId};
    use reify_ir::{ExportFormat, ReprKind, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping non_prismatic_body_solve_runs_on_realized_volume_mesh: OCCT not \
             available (no BRep kernel to build the cylinder body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(NON_PRISMATIC_BODY_SOURCE);

    let mut engine = make_occt_engine();
    // Manual registration — see the module doc. Trampoline + VolumeMesh demand
    // ONLY; NO boundary demand (that routes through the #4876-SIGSEGV attributed
    // producer, which is exactly the path a curved/seamed B-rep is most likely to
    // crash on).
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("solver::elastic_static");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    let build_result = engine.build(&compiled, ExportFormat::Step);

    // ── (1) no Error diagnostics — the curved body tessellated and tet-meshed ──
    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the non-prismatic body build — a mesher \
         failure on the curved lateral surface would surface here. got: {errors:?}"
    );

    // ── (2) exactly one (VolumeMesh, Gmsh) realization ────────────────────────
    let provenance = engine.realization_kernel_provenance();
    let vm_realizations: Vec<_> = provenance
        .iter()
        .filter(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh)
        .collect();
    assert_eq!(
        vm_realizations.len(),
        1,
        "the single geometry `let body` must produce EXACTLY ONE (VolumeMesh, Gmsh) \
         realization. provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );

    // ── (3) the §7a grid follows the REALIZED curved AABB, not the synthetic box ─
    let result_cell = ValueCellId::new("FeaBodyNonPrismatic", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyNonPrismatic.result not found in build values"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "non-prismatic body result must be a populated ElasticResult \
         (StructureInstance/Map), got: {result_val:?} — a pre-hydration Failed/Undef \
         here means the redispatch did not deliver the realized mesh to the trampoline"
    );

    let disp = sampled_field(result_val, "displacement");
    let node_count = disp.data.len() / 3;
    eprintln!(
        "non-prismatic body: realized §7a grid axes = {:?}, nodes = {node_count}",
        disp.axis_grids.iter().map(|a| a.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        disp.axis_grids.len(),
        3,
        "displacement must be a 3D Regular grid, got {} axes",
        disp.axis_grids.len()
    );
    assert_ne!(
        node_count, SYNTHETIC_GRID_NODES,
        "the non-prismatic body solve must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
         synthetic grid — the realized curved-body VolumeMesh must drive the §7a \
         resample grid"
    );

    // ── (4) converged ─────────────────────────────────────────────────────────
    assert_eq!(
        extract_field(result_val, "converged"),
        Some(Value::Bool(true)),
        "the non-prismatic realized-mesh solve must converge"
    );
}

/// Build a fresh OCCT+Gmsh engine with the multi-case FEA trampolines installed,
/// run `build()` on `source`, and return `(realization_entries delta, build)`.
///
/// Manual registration (`solver::multi_case` + `solver::elastic_static` +
/// `register_volume_mesh_demand`) — NOT `register_compute_fns`, see the module
/// doc's #4876 boundary-demand rationale. The delta is taken across the build on
/// a FRESH engine, so it is attributable to this build alone.
#[cfg(has_gmsh)]
fn build_multi_case_and_count_realizations(
    source: &str,
) -> (usize, reify_eval::BuildResult) {
    use reify_ir::ExportFormat;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);

    let mut engine = make_occt_engine();
    engine.register_compute_fn(
        "solver::multi_case",
        reify_eval::compute_targets::multi_case::solve_multi_case_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("solver::multi_case");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    let before = engine.cache_stats().realization_entries;
    assert_eq!(
        before, 0,
        "a freshly constructed engine must report zero terminal realization \
         cache entries before any build()"
    );

    let build_result = engine.build(&compiled, ExportFormat::Step);
    let after = engine.cache_stats().realization_entries;

    // Cross-check the counter against provenance so a divergence between "what
    // was realized" and "what was cached" is diagnosable rather than a bare
    // count mismatch.
    let vm_count = engine
        .realization_kernel_provenance()
        .iter()
        .filter(|p| p.repr == reify_ir::ReprKind::VolumeMesh && p.kernel == reify_core::KernelId::Gmsh)
        .count();
    assert_eq!(
        vm_count, 1,
        "expected EXACTLY ONE (VolumeMesh, Gmsh) realization for the shared body; \
         provenance disagrees with the realization_entries delta of {}. \
         provenance: {:?}",
        after - before,
        engine
            .realization_kernel_provenance()
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
    );

    (after - before, build_result)
}

/// `cfg(has_gmsh)`: **B9** — a two-case `solve_load_cases` over a shared
/// non-prismatic body realizes and caches the volume mesh EXACTLY ONCE
/// (PRD `docs/prds/v0_4/fea-result-model.md` boundary-test B9, re-homed from
/// task 4088 to task 4152).
///
/// This is the first reader of `CacheStats::realization_entries` on a real FEA
/// solve, and the re-mesh-avoidance claim the counter exists to express: two
/// load cases sharing `(body, material, element_order, mesh_size)` — they share
/// the single `body`/`material` args and the one inline `ElasticOptions()`, and
/// differ ONLY in tip force (1000 N vs 2000 N) — must mesh the body ONCE.
///
/// **Read the `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` doc before changing this
/// fixture.** The counter can only move because that fixture declares a
/// `RepresentationWithin` bound; with no tolerance contract the terminal
/// cache insert is gated off and the delta is 0 for every body shape. That is a
/// deliberate production semantic ("no tolerance contract → no caching"), NOT a
/// bug in the counter.
///
/// Asserts:
///   (1) the two-case build's `realization_entries` delta is exactly 1;
///   (2) the shape-robust control — the ONE-case variant of the same body
///       yields the SAME delta, so adding a second case adds ZERO entries;
///   (3) `result["cases"]` is a 2-entry `Value::Map` keyed "operating"/
///       "overload", neither `Undef`;
///   (4) each case carries REAL Sampled fields — `displacement` a 3-axis
///       Regular grid of all-finite data, `stress` present — and converged;
///   (5) per-case independence still holds
///       (`overload.max_von_mises > operating.max_von_mises`).
///
/// # Why this is `#[ignore]`d — assertions (1)/(2) PASS, (3)/(4)/(5) cannot
///
/// The assertion is deliberately preserved VERBATIM rather than weakened. Its
/// two halves are currently satisfiable only by mutually exclusive fixtures, and
/// that is a production defect (#5951), not a defect in this test or in the
/// counter.
///
/// Measured 2×2 on this harness — a fresh engine per build, manual trampolines,
/// `build(&compiled, ExportFormat::Step)`:
///
/// | fixture                       | `realization_entries` delta | `cases`   |
/// |-------------------------------|-----------------------------|-----------|
/// | cylinder, no bound            | 0                           | populated |
/// | cylinder + 100 µm bound       | 1                           | **empty** |
/// | box + 100 µm bound            | 1                           | **empty** |
/// | box, no bound                 | 0                           | populated |
///
/// (the last row is today's green `multi_case_body_solve_shares_one_realization_
/// across_cases`). So it is the tolerance contract, not the body shape —
/// consistent with the single-case
/// `non_prismatic_body_solve_runs_on_realized_volume_mesh` above, which is green
/// and carries no bound.
///
/// Assertions (1) and (2) were OBSERVED PASSING before (3) panicked: the delta is
/// exactly 1 for the two-case build, the one-case control matches it, and the
/// provenance cross-check finds exactly one `(VolumeMesh, Gmsh)` realization. So
/// the re-mesh-avoidance half of B9 is real and measurable. What fails is that
/// the same bound which makes the counter observable also makes the solve return
/// the stdlib's inline `MultiCaseResult()` fallback with an empty `cases` map —
/// SILENTLY: the only diagnostic in the whole build is an unrelated
/// `Severity::Warning` for the Indeterminate `RepresentationWithin` itself.
///
/// Fixing that is out of scope here (it is FEA-solve / demand-pass territory,
/// tasks 4870/4092), so it is filed as its own task with the full measurement
/// set. This test goes green unchanged once #5951 lands — do not adjust the
/// assertions to make it pass sooner.
#[cfg(has_gmsh)]
#[test]
#[ignore = "blocked on #5951 — a demanded-tolerance bound is required for realization_entries to move, but that same bound silently empties the multi-case solve's `cases` map; assertions (1)/(2) pass, (3)+ cannot"]
fn multi_case_non_prismatic_body_caches_one_realization_for_both_cases() {
    use reify_core::{Severity, ValueCellId};
    use reify_ir::Value;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping multi_case_non_prismatic_body_caches_one_realization_for_both_cases: \
             OCCT not available (no BRep kernel to build the cylinder body)"
        );
        return;
    }

    // ── (1) the two-case build realizes + caches the shared mesh exactly once ──
    let (two_case_delta, build_result) =
        build_multi_case_and_count_realizations(NON_PRISMATIC_MULTI_CASE_BODY_SOURCE);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the two-case non-prismatic build, got: {errors:?}"
    );

    assert_eq!(
        two_case_delta, 1,
        "the shared `body` must be realized and cached EXACTLY ONCE across BOTH \
         load cases — the multi_case trampoline forwards realization_inputs \
         unchanged, so no case re-meshes. realization_entries delta = {two_case_delta}"
    );

    // ── (2) shape-robust control: the second case adds ZERO new entries ───────
    let (one_case_delta, one_case_build) =
        build_multi_case_and_count_realizations(NON_PRISMATIC_ONE_CASE_BODY_SOURCE);
    let one_case_errors: Vec<_> = one_case_build
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        one_case_errors.is_empty(),
        "expected no Error diagnostics from the ONE-case control build, got: {one_case_errors:?}"
    );
    assert_eq!(
        two_case_delta, one_case_delta,
        "adding a SECOND load case over the same body must add ZERO new \
         realization entries: one-case delta = {one_case_delta}, two-case delta \
         = {two_case_delta}. This is the shape-robust half of B9 — it holds \
         regardless of how many realizations the module contains for other reasons."
    );

    // ── (3) both cases present and populated ─────────────────────────────────
    let result_cell = ValueCellId::new("FeaBodyNonPrismaticMultiCase", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| {
            panic!("cell FeaBodyNonPrismaticMultiCase.result not found in build values")
        });
    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_load_cases result must be a MultiCaseResult Value::Map, got: {other:?} \
             — a pre-hydration Failed/Undef here means the redispatch did not deliver \
             the realized mesh to the multi_case trampoline"
        ),
    };
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries (operating, overload), got {}",
        cases_map.len()
    );

    // ── (4) each case carries REAL Sampled fields, converged ──────────────────
    let mut von_mises: Vec<(&str, f64)> = Vec::new();
    for case_name in ["operating", "overload"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "cases map must contain \"{case_name}\"; got: {:?}",
                    cases_map.keys().collect::<Vec<_>>()
                )
            });
        assert!(
            !matches!(case_val, Value::Undef),
            "case \"{case_name}\" must not be Undef"
        );

        let disp = sampled_field(case_val, "displacement");
        assert_eq!(
            disp.axis_grids.len(),
            3,
            "case \"{case_name}\": displacement must be a 3D Regular grid, got {} axes",
            disp.axis_grids.len()
        );
        assert!(
            !disp.data.is_empty() && disp.data.iter().all(|v| v.is_finite()),
            "case \"{case_name}\": displacement data must be non-empty and all-finite \
             (a NaN/inf here means the solve diverged on the realized curved mesh)"
        );
        assert_ne!(
            disp.data.len() / 3,
            SYNTHETIC_GRID_NODES,
            "case \"{case_name}\": must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
             synthetic grid — the shared realized VolumeMesh must drive the §7a grid"
        );

        // `stress` must be a real Sampled field too, not merely present.
        let stress = sampled_field(case_val, "stress");
        assert!(
            !stress.data.is_empty() && stress.data.iter().all(|v| v.is_finite()),
            "case \"{case_name}\": stress data must be non-empty and all-finite"
        );

        assert_eq!(
            extract_field(case_val, "converged"),
            Some(Value::Bool(true)),
            "case \"{case_name}\": the shared realized-mesh solve must converge"
        );

        match extract_field(case_val, "max_von_mises") {
            Some(Value::Scalar { si_value, .. }) => von_mises.push((case_name, si_value)),
            other => panic!(
                "case \"{case_name}\": max_von_mises must be a Scalar, got {other:?}"
            ),
        }
    }

    // ── (5) per-case independence: 2× the tip force ⇒ strictly higher stress ──
    let operating = von_mises.iter().find(|(n, _)| *n == "operating").unwrap().1;
    let overload = von_mises.iter().find(|(n, _)| *n == "overload").unwrap().1;
    assert!(
        overload > operating,
        "the 2000 N \"overload\" case must yield strictly higher max_von_mises than \
         the 1000 N \"operating\" case — equal values would mean the cases were not \
         solved independently (one result reused for both). operating = {operating}, \
         overload = {overload}"
    );
}

/// `cfg(has_gmsh)`: companion regression — the unchanged scalar-dims fixture
/// still yields the 854-node synthetic grid.
///
/// The additive `body : Solid` overload must not perturb the classic prismatic
/// `solve_elastic_static(material, length, width, height, loads, supports,
/// options)` path: `examples/fea_cantilever_smoke.ri` still resolves to the
/// arity-7 dims overload and solves on the synthetic `nx×1×6` box (854 nodes).
/// Uses a kernel-less `make_simple_engine` + `eval` (the scalar path realizes no
/// VolumeMesh), matching `solve_elastic_static_e2e.rs`'s grid-count assertion.
#[cfg(has_gmsh)]
#[test]
fn scalar_dims_solve_still_yields_synthetic_854_grid() {
    use reify_core::{Severity, ValueCellId};
    use reify_ir::Value;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "../../../examples/fea_cantilever_smoke.ri"
    ));

    let mut engine = reify_test_support::make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the scalar-dims eval, got: {errors:?}"
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval values"));

    let disp = sampled_field(result_val, "displacement");
    let node_count = disp.data.len() / 3;
    assert_eq!(
        node_count, SYNTHETIC_GRID_NODES,
        "the unchanged scalar-dims fixture must still yield the {SYNTHETIC_GRID_NODES}-node \
         synthetic grid (the additive body overload must not perturb it), got {node_count}"
    );
    assert_eq!(
        extract_field(result_val, "converged"),
        Some(Value::Bool(true)),
        "the scalar-dims solve must converge"
    );
}

/// `cfg(not(has_gmsh))`: skip-stub. Without the gmsh adapter the body realization
/// cannot produce a VolumeMesh, so the capstone is compiled out.
#[cfg(not(has_gmsh))]
#[test]
fn solve_elastic_static_body_e2e_skipped_without_gmsh() {
    eprintln!(
        "skipping FEA body-arg realized-VolumeMesh capstone: has_gmsh cfg not set \
         (stub-mode build)"
    );
}
