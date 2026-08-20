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

/// Realized-path §7a grid shape for the 1 m × 100 mm × 100 mm `box(...)` body
/// (`BODY_SOURCE` / `MULTI_CASE_BODY_SOURCE`), from the realized arm's
/// derivation: `ext = aabb = [1.0, 0.1, 0.1] m`, `nz = 6`, `dz = ext_z = 0.1`,
/// `nx = round(ext_x/dz × 6) = 60`, `ny = round(ext_y/dz × 6) = 6` — so the grid
/// is `(60+1)(6+1)(6+1)`. The synthetic path fixes `ny = 1` (2 Y-nodes), which is
/// what makes the Y axis the structural discriminator between the two paths.
#[cfg(has_gmsh)]
const REALIZED_BOX_GRID_AXES: [usize; 3] = [61, 7, 7];

/// `∏ REALIZED_BOX_GRID_AXES` = 2989. Derived, not restated, so the shape and
/// the count can never disagree.
#[cfg(has_gmsh)]
const REALIZED_BOX_GRID_NODES: usize =
    REALIZED_BOX_GRID_AXES[0] * REALIZED_BOX_GRID_AXES[1] * REALIZED_BOX_GRID_AXES[2];

/// Realized-path §7a grid shape for `NON_PRISMATIC_BODY_SOURCE`'s
/// `cylinder(50mm, 200mm)`: AABB `[0.1, 0.1, 0.2] m` ⇒ `nz = 6`,
/// `nx = ny = round(0.1/0.2 × 6) = 3` ⇒ `(3+1)(3+1)(6+1)`.
#[cfg(has_gmsh)]
const REALIZED_CYLINDER_GRID_AXES: [usize; 3] = [4, 4, 7];

/// `∏ REALIZED_CYLINDER_GRID_AXES` = 112.
#[cfg(has_gmsh)]
const REALIZED_CYLINDER_GRID_NODES: usize = REALIZED_CYLINDER_GRID_AXES[0]
    * REALIZED_CYLINDER_GRID_AXES[1]
    * REALIZED_CYLINDER_GRID_AXES[2];

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

/// Install the MULTI-CASE fixtures' compute trampolines + VolumeMesh demand.
///
/// Manual registration, NOT `register_compute_fns` — see the module doc's
/// #4876 boundary-demand rationale. The outer consumer is `solver::multi_case`
/// (VolumeMesh-demanding, NO boundary demand); each per-case sub-solve routes
/// through `solver::elastic_static`, so BOTH trampolines are registered.
#[cfg(has_gmsh)]
fn register_multi_case_trampolines(engine: &mut reify_eval::Engine) {
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
}

/// Install the SINGLE-CASE body fixtures' compute trampoline + VolumeMesh demand.
///
/// Manual registration, NOT `register_compute_fns` — see the module doc's
/// #4876 boundary-demand rationale. Trampoline + VolumeMesh demand ONLY, no
/// boundary demand (that routes through the #4876-SIGSEGV attributed producer,
/// which is exactly the path a curved/seamed B-rep is most likely to crash on).
#[cfg(has_gmsh)]
fn register_elastic_static_body_trampoline(engine: &mut reify_eval::Engine) {
    engine.register_compute_fn(
        "solver::elastic_static",
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline
            as reify_eval::ComputeFn,
    );
    engine.register_volume_mesh_demand("solver::elastic_static");
}

/// The ONE realization harness every `#[cfg(has_gmsh)]` body capstone below
/// shares: compile `source`, `build()` it through the real OCCT+gmsh path, and
/// hand back BOTH the engine (so a caller can assert on
/// `realization_kernel_provenance()`) and the `BuildResult`.
///
/// `register` installs the fixture's trampolines and demands — the one thing
/// that genuinely differs between fixtures, and the reason this takes a callback
/// rather than hardcoding a registration.
///
/// `build()` (not `eval()`) realizes geometry through the kernel and runs the
/// post-hydration redispatch — the only path that projects a VolumeMesh into a
/// geometry-consuming `@optimized` consumer.
///
/// The no-`Severity::Error` assertion lives here because every caller wants it
/// and none wants it phrased differently; `label` names the fixture in the
/// failure message.
#[cfg(has_gmsh)]
fn build_realized(
    source: &str,
    label: &str,
    register: impl FnOnce(&mut reify_eval::Engine),
) -> (reify_eval::Engine, reify_eval::BuildResult) {
    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);

    let mut engine = make_occt_engine();
    register(&mut engine);
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    let build_result = engine.build(&compiled, reify_ir::ExportFormat::Step);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the {label} build — a tessellation or \
         mesher failure on this fixture's geometry would surface here. got: {errors:?}"
    );

    (engine, build_result)
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

    // Realized-path §7a grid derivation: see `REALIZED_BOX_GRID_AXES`.
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
        node_count, REALIZED_BOX_GRID_NODES,
        "the body-arg §7a grid node count must equal the realized-AABB heuristic \
         {REALIZED_BOX_GRID_NODES} ({REALIZED_BOX_GRID_AXES:?} for the 1 m × 100 mm × \
         100 mm box), got {node_count}"
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
    use reify_core::{KernelId, ValueCellId};
    use reify_ir::{ReprKind, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping multi_case_body_solve_shares_one_realization_across_cases: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let (engine, build_result) = build_realized(
        MULTI_CASE_BODY_SOURCE,
        "multi-case body",
        register_multi_case_trampolines,
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

    // Realized-path §7a grid (vs the synthetic ny=1 → 854): see
    // `REALIZED_BOX_GRID_AXES` for the derivation.
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
            node_count, REALIZED_BOX_GRID_NODES,
            "case \"{case_name}\": §7a grid node count must equal the realized-AABB \
             heuristic {REALIZED_BOX_GRID_NODES} ({REALIZED_BOX_GRID_AXES:?} for the \
             1 m × 100 mm × 100 mm box), got {node_count}"
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
    use reify_core::{KernelId, ValueCellId};
    use reify_ir::{ReprKind, Value};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping non_prismatic_body_solve_runs_on_realized_volume_mesh: OCCT not \
             available (no BRep kernel to build the cylinder body)"
        );
        return;
    }

    // ── (1) no Error diagnostics — the curved body tessellated and tet-meshed;
    //        `build_realized` carries that assertion for every fixture here.
    let (engine, build_result) = build_realized(
        NON_PRISMATIC_BODY_SOURCE,
        "non-prismatic body",
        register_elastic_static_body_trampoline,
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
/// Registration is [`register_multi_case_trampolines`] — NOT
/// `register_compute_fns`, see the module doc's #4876 boundary-demand rationale.
/// The delta is taken across the build on a FRESH engine, so it is attributable
/// to this build alone.
///
/// Does NOT go through [`build_realized`]: it must read `cache_stats()` on the
/// engine BEFORE the build to establish the zero baseline, which a
/// build-and-return helper cannot expose.
#[cfg(has_gmsh)]
fn build_multi_case_and_count_realizations(
    source: &str,
) -> (usize, reify_eval::BuildResult) {
    use reify_ir::ExportFormat;

    let compiled = reify_test_support::parse_and_compile_with_stdlib(source);

    let mut engine = make_occt_engine();
    register_multi_case_trampolines(&mut engine);
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

/// `cfg(has_gmsh)`: **B9, re-mesh-avoidance half** — a two-case
/// `solve_load_cases` over a shared non-prismatic body realizes and caches the
/// volume mesh EXACTLY ONCE, and adding the second load case adds ZERO entries
/// (PRD `docs/prds/v0_4/fea-result-model.md` boundary-test B9, re-homed from
/// task 4088 to task 4152).
///
/// This is the half of B9 that PASSES today, split out of
/// [`multi_case_non_prismatic_body_caches_one_realization_for_both_cases`]
/// (which stays `#[ignore]`d on #5951 for its `cases`-map/Sampled-field half)
/// so that the counter has GREEN coverage on a real OCCT+gmsh FEA path. Without
/// this split the only executing `realization_entries` assertions in CI are the
/// `MockGeometryKernel` ones in `tests/tolerance_wiring_e2e.rs`, and a
/// regression on the real path — the conversion-intermediate insert in
/// `engine_build.rs` switched to `insert_terminal`, or a second realization edge
/// appearing for the shared body — would make the delta 2 with nothing green to
/// catch it.
///
/// Asserts, over the SAME body:
///   (1) the two-case build's `realization_entries` delta is exactly 1;
///   (2) the shape-robust control — the ONE-case variant yields the SAME delta,
///       so a second load case adds ZERO new realization entries.
///
/// Both builds also assert no `Severity::Error` diagnostics, and the helper
/// cross-checks the counter against `realization_kernel_provenance()` so a
/// counter/provenance divergence is diagnosable rather than a bare mismatch.
///
/// **Read the `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` doc before changing the
/// fixtures.** The counter can only move because they declare a
/// `RepresentationWithin` bound; with no tolerance contract the terminal cache
/// insert is gated off and the delta is 0 for every body shape. That is a
/// deliberate production semantic ("no tolerance contract → no caching"), NOT a
/// bug in the counter.
#[cfg(has_gmsh)]
#[test]
fn non_prismatic_two_case_build_realizes_body_exactly_once() {
    use reify_core::Severity;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping non_prismatic_two_case_build_realizes_body_exactly_once: \
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
}

/// `cfg(has_gmsh)`: **B9, result-shape half** — each of the two cases carries
/// real per-case Sampled fields off the single shared realization
/// (PRD `docs/prds/v0_4/fea-result-model.md` boundary-test B9, re-homed from
/// task 4088 to task 4152).
///
/// Two load cases sharing `(body, material, element_order, mesh_size)` — they
/// share the single `body`/`material` args and the one inline
/// `ElasticOptions()`, and differ ONLY in tip force (1000 N vs 2000 N) — must
/// each come back with their own populated result off the ONE shared mesh.
///
/// **The re-mesh-avoidance assertions of B9 are NOT here.** They pass today and
/// live, un-ignored, in the green sibling
/// [`non_prismatic_two_case_build_realizes_body_exactly_once`] above: the
/// two-case `realization_entries` delta == 1, and the one-case control proving a
/// second load case adds ZERO entries. Only the `cases`-map / Sampled-field half
/// below is blocked, so only that half is `#[ignore]`d — the counter keeps green
/// coverage on a real OCCT+gmsh path either way.
///
/// **Read the `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` doc before changing this
/// fixture.** The counter can only move because that fixture declares a
/// `RepresentationWithin` bound; with no tolerance contract the terminal
/// cache insert is gated off and the delta is 0 for every body shape. That is a
/// deliberate production semantic ("no tolerance contract → no caching"), NOT a
/// bug in the counter.
///
/// Asserts:
///   (3) `result["cases"]` is a 2-entry `Value::Map` keyed "operating"/
///       "overload", neither `Undef`;
///   (4) each case carries REAL Sampled fields — `displacement` a 3-axis
///       Regular grid of all-finite data, `stress` present — and converged;
///   (5) per-case independence still holds
///       (`overload.max_von_mises > operating.max_von_mises`).
///
/// (numbering kept from the original single B9 test, whose (1)/(2) are now the
/// green sibling's)
///
/// # Why this is `#[ignore]`d — (1)/(2) PASS in the sibling, (3)/(4)/(5) cannot
///
/// The assertions below are deliberately preserved VERBATIM rather than
/// weakened. B9's two halves are currently satisfiable only by mutually
/// exclusive fixtures, and that is a production defect (#5951), not a defect in
/// this test or in the counter.
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
/// provenance cross-check finds exactly one `(VolumeMesh, Gmsh)` realization —
/// which is why they now run un-ignored in the sibling above. What fails is that
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
#[ignore = "blocked on #5951 — a demanded-tolerance bound is required for realization_entries to move, but that same bound silently empties the multi-case solve's `cases` map; B9's (1)/(2) run green in non_prismatic_two_case_build_realizes_body_exactly_once, (3)+ cannot"]
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

    // The same two-case build as the green sibling — taken through the shared
    // helper so the realization count is still cross-checked against provenance
    // — but here only for its `build_result`. The delta assertions (1)/(2) live
    // in `non_prismatic_two_case_build_realizes_body_exactly_once`.
    let (_two_case_delta, build_result) =
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

/// Task #6154 — realize `MULTI_CASE_BODY_SOURCE`'s `box(1000mm, 100mm, 100mm)`
/// through the OCCT+gmsh path and hand back the "operating" case's
/// §7a-resampled `displacement` field. `None` ⇒ OCCT is unavailable and the
/// caller skips.
///
/// Deliberately NOT memoized. A full OCCT tessellation + gmsh tet-mesh + solve is
/// the most expensive thing in this file, so a `OnceLock` looks attractive — but
/// in a normal run this has exactly ONE live caller
/// (`realized_box_grid_miss_report_reconciles_with_geometry`; the other is
/// `#[ignore]`d against #6200), so the cache would never be hit and would cost a
/// static, a two-function split, and a `&'static` return for nothing. The
/// realization HARNESS — which is genuinely shared, with the capstones above —
/// is factored into [`build_realized`] instead; that is where the duplication
/// actually was.
#[cfg(has_gmsh)]
fn realized_box_operating_displacement(caller: &str) -> Option<reify_ir::SampledField> {
    use reify_ir::Value;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping {caller}: OCCT not available (no BRep kernel to build the box body)");
        return None;
    }

    let (_engine, build_result) = build_realized(
        MULTI_CASE_BODY_SOURCE,
        "multi-case body",
        register_multi_case_trampolines,
    );

    let result_cell = reify_core::ValueCellId::new("FeaBodyMultiCase", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyMultiCase.result not found in build values"));
    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!("solve_load_cases result must be a Value::Map, got: {other:?}"),
    };
    let case_val = cases_map
        .get(&Value::String("operating".to_string()))
        .unwrap_or_else(|| panic!("cases map must contain \"operating\""));

    Some(sampled_field(case_val, "displacement"))
}

/// Task #6154's CYLINDER sibling of [`realized_box_operating_displacement`] —
/// realize `NON_PRISMATIC_BODY_SOURCE`'s `cylinder(50mm, 200mm)` and hand back
/// its §7a-resampled `displacement` field. Not memoized, for the same reason.
///
/// Its build is not shared with `non_prismatic_body_solve_runs_on_realized_volume_mesh`
/// above — that test asserts on `engine.realization_kernel_provenance()`, which a
/// field-returning helper cannot hand back without keeping a live OCCT/gmsh
/// handle alive for its caller — but the HARNESS is: both go through
/// [`build_realized`] with the same registrar.
#[cfg(has_gmsh)]
fn realized_cylinder_displacement(caller: &str) -> Option<reify_ir::SampledField> {
    use reify_core::ValueCellId;
    use reify_ir::Value;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping {caller}: OCCT not available (no BRep kernel to build the cylinder body)"
        );
        return None;
    }

    let (_engine, build_result) = build_realized(
        NON_PRISMATIC_BODY_SOURCE,
        "non-prismatic body",
        register_elastic_static_body_trampoline,
    );

    let result_cell = ValueCellId::new("FeaBodyNonPrismatic", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyNonPrismatic.result not found in build values"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "non-prismatic body result must be a populated ElasticResult, got: {result_val:?}"
    );

    Some(sampled_field(result_val, "displacement"))
}

/// Classify a §7a displacement field's out-of-solid grid points and DUMP the
/// full split plus per-axis miss histograms — the measurement artefact #6154
/// owes. Returns `(report, per_axis_histograms)`.
///
/// A raw NaN *count* diagnoses nothing on its own: the sentinel is normative
/// (PRD `v0_4/fea-result-model.md` §3 / §4.1), and a coverage hole and boundary
/// round-off write the identical `NaN`. The index-bucket split is what tells
/// them apart, so it is printed unconditionally on every run rather than being
/// reachable only from a failing assertion.
#[cfg(has_gmsh)]
fn classify_and_dump_grid_misses(
    disp: &reify_ir::SampledField,
    label: &str,
) -> (reify_solver_elastic::GridMissReport, Vec<Vec<usize>>) {
    let report = reify_solver_elastic::classify_grid_misses(disp, 3);
    let axes: Vec<usize> = disp.axis_grids.iter().map(|a| a.len()).collect();
    let mut hist: Vec<Vec<usize>> = axes.iter().map(|&n| vec![0usize; n]).collect();
    for idx in &report.missed_indices {
        for a in 0..3 {
            hist[a][idx[a]] += 1;
        }
    }
    eprintln!(
        "#6154 {label} §7a grid-miss report: axes={axes:?} n_grid={} n_missed={} \
         ({:.1}%) | interior={} face={} edge={} corner={} | partial_nan={}",
        report.n_grid,
        report.n_missed,
        100.0 * report.n_missed as f64 / report.n_grid as f64,
        report.missed_interior,
        report.missed_face,
        report.missed_edge,
        report.missed_corner,
        report.n_partial_nan,
    );
    for (a, name) in ["x", "y", "z"].iter().enumerate() {
        eprintln!("#6154   {label} misses per {name}-index: {:?}", hist[a]);
    }
    (report, hist)
}

/// Pin a [`GridMissReport`] to the field it was derived from, by re-deriving the
/// miss set STRAIGHT OFF `sf.data` — independently of `classify_grid_misses`.
///
/// This is the check the report cannot self-satisfy. Its bucket sums reconcile
/// with `n_missed` *by construction* (one increment of each per miss, in the
/// same iteration), so asserting that identity on real data proves nothing
/// about the data; the identity is already pinned where it belongs, in the
/// crate-local `resample.rs::miss_diag_tests` fixtures. What is worth asserting
/// on a realized field is that the report and the field agree: same cardinality,
/// distinct indices, every reported index genuinely all-`NaN` at its flat
/// offset, and every reported point equal to the axis-grid coordinates there.
/// Together those make the reported set exactly the field's `NaN` set.
///
/// Holds for ANY realized field — it says nothing about how many misses there
/// should be, so it survives #6200 landing and the count going to zero.
#[cfg(has_gmsh)]
fn assert_report_reconciles_with_field(
    sf: &reify_ir::SampledField,
    report: &reify_solver_elastic::GridMissReport,
    stride: usize,
    label: &str,
) {
    use std::collections::HashSet;

    let (nx1, ny1, nz1) =
        (sf.axis_grids[0].len(), sf.axis_grids[1].len(), sf.axis_grids[2].len());
    assert_eq!(report.n_grid, nx1 * nz1 * ny1, "{label}: n_grid must be ∏ axis lengths");

    // (1) Independent recount off the raw buffer — never touches the report.
    let independent =
        sf.data.chunks_exact(stride).filter(|c| c.iter().all(|v| v.is_nan())).count();
    assert_eq!(
        report.n_missed, independent,
        "{label}: the report claims {} out-of-solid grid points, but the raw field \
         buffer holds {independent} all-NaN points",
        report.n_missed,
    );

    // (2) No partially-NaN point: those are diverged solution values, not the
    //     all-or-nothing out-of-solid sentinel, and would make (1) undercount.
    assert_eq!(
        report.n_partial_nan, 0,
        "{label}: {} grid point(s) are PARTIALLY NaN — a non-finite solution value, \
         not an out-of-solid marker. The bucket split describes a field that is \
         already broken upstream of the sampler; fix that before reading it",
        report.n_partial_nan,
    );

    // (3) The reported indices are distinct, so (1)'s cardinality match plus
    //     (4)'s per-index NaN check together force set equality.
    let distinct: HashSet<[usize; 3]> = report.missed_indices.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        report.n_missed,
        "{label}: missed_indices must hold {} DISTINCT indices, got {} distinct of \
         {} entries",
        report.n_missed,
        distinct.len(),
        report.missed_indices.len(),
    );
    assert_eq!(
        report.missed_points.len(),
        report.n_missed,
        "{label}: missed_points must be parallel to missed_indices",
    );

    // (4) Every reported miss is really NaN there, at the coordinates claimed.
    for (idx, p) in report.missed_indices.iter().zip(&report.missed_points) {
        let flat = (idx[0] * ny1 + idx[1]) * nz1 + idx[2];
        let comps = &sf.data[flat * stride..(flat + 1) * stride];
        assert!(
            comps.iter().all(|v| v.is_nan()),
            "{label}: index {idx:?} (flat {flat}) is reported out-of-solid but its \
             {stride} components are {comps:?}",
        );
        assert_eq!(
            *p,
            [sf.axis_grids[0][idx[0]], sf.axis_grids[1][idx[1]], sf.axis_grids[2][idx[2]]],
            "{label}: reported coordinates for index {idx:?} must be the axis-grid \
             coordinates at that index",
        );
    }
}

/// Task #6154 — RECONCILE the realized box's out-of-solid grid points against
/// what the geometry actually predicts.
///
/// `elastic_static.rs`'s field-population contract used to claim of
/// `displacement`: "Every grid point lies inside the solid (prismatic box), so
/// all samples are finite (no NaN sentinels for the cantilever geometry)". The
/// realized path contradicts it — a third of this prismatic box's grid nodes
/// carry the out-of-solid sentinel. The measured split, when it was taken, and
/// what it proved are recorded once in PRD `docs/prds/v0_4/fea-result-model.md`
/// §11 Q2; this test re-measures and prints it on every run rather than pinning
/// it (see below), so read the dump for today's numbers, not a comment.
///
/// This test carries the measurement — the dump is emitted on every run — and
/// pins the two properties that hold TODAY and are #6154's actual deliverable:
/// the report is a faithful description of the realized field, and the grid it
/// describes is the realized-AABB one. The BOX-SPECIFIC geometric prediction the
/// split exposes as violated (`missed_interior == 0`) is preserved in
/// `realized_box_mesh_tiles_its_own_aabb` below, `#[ignore]`d against #6200 —
/// splitting the two keeps this measurement running (and its dump visible) on
/// every CI run instead of aborting at the first upstream-owned failure.
///
/// Deliberately NOT asserted here:
///   - any all-finite property — the sentinel is normative and must survive;
///   - the total miss COUNT — that count is the thing under investigation, and
///     pinning 1055 before the mechanism is fixed would cement a bug as a
///     contract;
///   - the bucket-sum identity — `classify_grid_misses` satisfies it by
///     construction, so it says nothing about this field (it is pinned in that
///     function's own crate-local fixtures instead).
#[cfg(has_gmsh)]
#[test]
fn realized_box_grid_miss_report_reconciles_with_geometry() {
    let Some(disp) =
        realized_box_operating_displacement("realized_box_grid_miss_report_reconciles_with_geometry")
    else {
        return;
    };
    let (report, _hist) = classify_and_dump_grid_misses(&disp, "realized box");

    // ── (i) the report describes THIS field, re-derived from `disp.data` ─────
    assert_report_reconciles_with_field(&disp, &report, 3, "realized box");

    // ── (ii) the grid is the realized one, not the synthetic 854 ────────────
    // Per-AXIS, not just the 2989 product: the interior/face/edge/corner split
    // is defined by index extremity, so it only means what its name says if the
    // axis lengths are the ones the AABB heuristic derives. A different shape
    // with the same node count would silently re-label every bucket.
    let axes: Vec<usize> = disp.axis_grids.iter().map(|a| a.len()).collect();
    assert_eq!(
        axes,
        REALIZED_BOX_GRID_AXES.to_vec(),
        "§7a grid shape must equal the realized-AABB heuristic \
         {REALIZED_BOX_GRID_AXES:?} for the 1 m × 100 mm × 100 mm box \
         ({REALIZED_BOX_GRID_NODES} nodes); got {axes:?}",
    );
}

/// Task #6154's measurement, held as #6200's acceptance gate.
///
/// For a PRISMATIC body the mesh AABB **is** the solid, so every
/// strictly-index-interior grid point must lie inside some tet. It does not:
/// 36 of the 1475 index-interior nodes are out-of-solid. That is a COVERAGE
/// defect in the realized tet mesh, not a tolerance one — the mesh's own tets
/// sum to 8.4291e-3 m³ against a 1.0000e-2 m³ AABB (84.3% fill, 159 tets, 0
/// interior nodes), and because `union(tets) ≤ Σ|tet vol|` that inequality is a
/// *proof* of missing coverage rather than a hypothesis. Only 1 of the 1055
/// misses has `|margin| < 1e-8` (median margin −8.42e-2), so no `tol` expansion
/// reaches these points, and `volume_mesh_to_solver_mesh` cannot be the culprit
/// (it rejects a whole mesh on a bad index rather than dropping an element, and
/// its only compaction touches tet-unreferenced vertices).
///
/// The defect is therefore upstream of this crate, in the gmsh tetrahedralization
/// path (`crates/reify-kernel-gmsh`), which #6154's scope explicitly excludes.
/// The assertion is kept — not deleted, not weakened to a threshold — so that
/// #6200 has an executable acceptance gate: unblocking it is exactly making this
/// test pass with the `#[ignore]` removed.
#[cfg(has_gmsh)]
#[test]
#[ignore = "blocked on #6200 — realized tet mesh fills only 84.3% of its AABB; missed_interior==36 is the coverage defect, not a tol issue"]
fn realized_box_mesh_tiles_its_own_aabb() {
    let Some(disp) = realized_box_operating_displacement("realized_box_mesh_tiles_its_own_aabb")
    else {
        return;
    };
    let (report, hist) = classify_and_dump_grid_misses(&disp, "realized box");

    assert_eq!(
        report.missed_interior, 0,
        "BOX-SPECIFIC prediction: for a prismatic body the mesh AABB IS the solid, \
         so every strictly-index-interior grid point must lie inside some tet. A \
         non-zero interior count means the realized mesh handed to §7a does not tile \
         its own AABB — that is a COVERAGE defect, not a tolerance one, and widening \
         `tol` would not legitimately fix it (see #6200). Measured: interior={} of \
         n_missed={} (face={}, edge={}, corner={}). Per-axis miss histograms: \
         x={:?} y={:?} z={:?}",
        report.missed_interior,
        report.n_missed,
        report.missed_face,
        report.missed_edge,
        report.missed_corner,
        hist[0],
        hist[1],
        hist[2],
    );
}

/// Task #6154 — the CYLINDER control: prove the normative out-of-solid `NaN`
/// sentinel still fires exactly where geometry says it must.
///
/// The box measurement above shows 35.3% of a *prismatic* body's grid points
/// marked out-of-solid where 0% is predicted. The obvious wrong "fix" for that
/// is to weaken the sentinel — widen `tol`, or assert all-finite. This test is
/// the guard against it: for a cylinder the AABB is emphatically NOT the solid,
/// so a large, exactly-predictable fraction of grid points MUST stay `NaN`, and
/// any weakening shows up here as a shortfall.
///
/// ## Closed form (re-derived here, not cited)
///
/// `cylinder(50mm, 200mm)` ⇒ AABB `[0.1, 0.1, 0.2] m`; the realized §7a arm
/// derives `nz = 6`, `nx = ny = round(0.1/0.2 · 6) = 3`, so the grid is
/// `4 × 4 × 7 = 112` nodes. Each of the x/y axes spans the full 0.1 m diameter
/// in 3 intervals, so its 4 samples sit at `±0.05` and `±1/60 ≈ ±0.01667` m from
/// the axis. A cross-section point is inside iff `dx² + dy² < 0.05²`:
///
/// | dx, dy | radius | inside? | count |
/// |---|---|---|---|
/// | (±0.05, ±0.05)     | 0.0707 | no  | 4 |
/// | (±0.05, ±0.01667)  | 0.0527 | no  | 8 |
/// | (±0.01667, ±0.01667) | 0.0236 | yes | 4 |
///
/// So 12 of 16 cross-section points are outside, at every one of the 7 z-levels:
/// **84 of 112 nodes** (75%), i.e. 252 of the 336 displacement components.
///
/// Index-bucketing those 84 (extreme = index `0` or `len-1` on that axis):
/// the 4 `(±0.05, ±0.05)` points are extreme on x AND y, the 8 mixed ones on
/// exactly one of x/y; z is extreme at 2 of its 7 levels. That gives
/// `corner = 4·2 = 8`, `edge = 4·5 + 8·2 = 36`, `face = 8·5 = 40`,
/// **`interior = 0`**.
///
/// ## Why `missed_interior == 0` here too — and why that is NOT the box's claim
///
/// This task's plan anticipated `missed_interior > 0` for the cylinder ("the
/// AABB is not the solid, so index-interior points are legitimately outside").
/// Re-deriving rather than citing shows that is false for THIS grid: the only
/// index-interior cross-section offsets are `±1/60 m`, which are a comfortable
/// 0.0236 m < 0.05 m from the axis — every index-interior node is genuinely
/// inside the cylinder. The assertion is therefore written the way the geometry
/// actually falls, not the way the plan guessed.
///
/// The box's violated prediction is a strictly stronger statement, and remains
/// box-specific: for a prismatic body the AABB IS the solid, so *no* grid point
/// of any bucket may miss (`n_missed == 0`). Here 84 of 112 must miss. Pinning
/// the cylinder's full split next to the box's is what stops a later reader
/// promoting `missed_interior == 0` to a global sampler invariant, or
/// "correcting" the cylinder's entirely-correct 75%.
#[cfg(has_gmsh)]
#[test]
fn realized_cylinder_grid_miss_report_matches_closed_form() {
    use std::collections::HashSet;

    let Some(disp) =
        realized_cylinder_displacement("realized_cylinder_grid_miss_report_matches_closed_form")
    else {
        return;
    };
    let (report, _hist) = classify_and_dump_grid_misses(&disp, "realized cylinder");

    let axes: Vec<usize> = disp.axis_grids.iter().map(|a| a.len()).collect();
    assert_eq!(
        axes,
        REALIZED_CYLINDER_GRID_AXES.to_vec(),
        "§7a grid shape for cylinder(50mm, 200mm) must be \
         {REALIZED_CYLINDER_GRID_AXES:?} ({REALIZED_CYLINDER_GRID_NODES} nodes) — the \
         closed form below is derived from exactly this shape; got {axes:?}",
    );
    assert_report_reconciles_with_field(&disp, &report, 3, "realized cylinder");

    // The 4 cross-section columns whose offsets are (±1/60, ±1/60) m — the only
    // ones inside r = 0.05 m. Every OTHER (ix, iy) column is outside, at all 7
    // z-levels: 12 × 7 = 84 predicted misses.
    const INSIDE_COLUMNS: [[usize; 2]; 4] = [[1, 1], [1, 2], [2, 1], [2, 2]];
    const PREDICTED_MISSES: usize = 84;

    // ── (a) SENTINEL guard — UNDER-firing ───────────────────────────────────
    // Owner: this crate's sampler. Every predicted-outside node must carry the
    // sentinel; a node here going finite means it was weakened.
    let missed: HashSet<[usize; 3]> = report.missed_indices.iter().copied().collect();
    let mut finite_but_predicted_outside: Vec<[usize; 3]> = Vec::new();
    for ix in 0..REALIZED_CYLINDER_GRID_AXES[0] {
        for iy in 0..REALIZED_CYLINDER_GRID_AXES[1] {
            if INSIDE_COLUMNS.contains(&[ix, iy]) {
                continue;
            }
            for iz in 0..REALIZED_CYLINDER_GRID_AXES[2] {
                if !missed.contains(&[ix, iy, iz]) {
                    finite_but_predicted_outside.push([ix, iy, iz]);
                }
            }
        }
    }
    assert!(
        finite_but_predicted_outside.is_empty(),
        "SENTINEL WEAKENED (this crate's sampler): the closed form above puts these \
         grid nodes outside the cylinder, so each MUST carry the normative \
         out-of-solid `NaN` (PRD v0_4/fea-result-model.md §4.1), but they came back \
         finite: {finite_but_predicted_outside:?}. Do not 'fix' this by relaxing the \
         sentinel — a widened `tol`, a clamp, or an all-finite assertion all land \
         here first, and all of them fabricate values the solver never solved.",
    );

    // ── (b) COVERAGE guard — OVER-firing ────────────────────────────────────
    // Owner: the realized cylinder tet mesh (upstream of this crate). With (a)
    // green the predicted 84 are all present, so any EXCESS is a node the
    // geometry says is inside but the mesh failed to cover — the box's defect.
    assert_eq!(
        report.n_missed, PREDICTED_MISSES,
        "REALIZED CYLINDER MESH UNDER-COVERS (upstream, see #6200): the closed form \
         predicts exactly {PREDICTED_MISSES} of {REALIZED_CYLINDER_GRID_NODES} nodes \
         out-of-solid and (a) above confirms all {PREDICTED_MISSES} are present, so \
         the {} extra miss(es) are nodes the geometry places INSIDE the cylinder. \
         That is the same coverage defect measured on the box, now reaching the \
         cylinder fixture — it is NOT a sentinel bug and must not be fixed here.",
        report.n_missed.saturating_sub(PREDICTED_MISSES),
    );

    // ── (c) BUCKETING — the classifier's own index-extremity arm ────────────
    // With (a)+(b) green the miss SET is exactly the predicted 84; this checks
    // that `classify_grid_misses` labels them the way the closed form does.
    assert_eq!(
        (
            report.missed_interior,
            report.missed_face,
            report.missed_edge,
            report.missed_corner,
        ),
        (0, 40, 36, 8),
        "the closed form above splits the {PREDICTED_MISSES} misses as interior=0 \
         face=40 edge=36 corner=8 by index extremity. The set is already pinned by \
         (a)+(b), so a mismatch here is a bug in `classify_grid_misses`' bucketing, \
         not in the geometry or the mesh."
    );
}

// (`REALIZED_CYLINDER_GRID_AXES` / `REALIZED_CYLINDER_GRID_NODES`, defined once
// at file scope alongside the box constants, replace the local copy that used
// to live here.)
