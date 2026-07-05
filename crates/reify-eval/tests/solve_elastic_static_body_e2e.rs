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
/// RED until the full `.ri` → demand → realize → consume chain composes
/// end-to-end (steps 2/4/6/8 build the substrate; this is the integration
/// capstone).
#[cfg(has_gmsh)]
#[test]
#[ignore = "blocked on #5008 — the full .ri→demand→realize→consume chain needs two \
            fixes the task-4870 plan walled off (design_decision[3]: no engine_build.rs \
            edits; report engine gaps as a blocking dependency). (A) The static \
            VolumeMesh-demand pass (engine_build.rs realization_indices_where) resolves \
            @optimized targets via `module.functions`, which is EMPTY for a stdlib \
            consumer like solve_elastic_static (stdlib fns live in `self.functions`), so \
            the demand never fires and `body` realizes (BRep, Occt) not (VolumeMesh, \
            Gmsh). (B) Once A is patched, the realized-mesh solve panics at \
            reify-solver-elastic dirichlet.rs:273 — a coordinate-selected clamp node with \
            no assembled stiffness diagonal (an orphan gmsh surface node). The \
            scalar-dims companion below stays live (the additive overload is proven \
            non-breaking)."]
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
