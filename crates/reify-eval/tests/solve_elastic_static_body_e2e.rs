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

/// The `cases` map of a `solve_load_cases` **MultiCaseResult** value, i.e. the
/// inner map of `Value::Map{"cases" → Map<String, ElasticResult>}`.
///
/// Factored out because every multi-case capstone in this file needs the same
/// two-level unwrap, and the FAILURE text is the part worth having once: a
/// pre-hydration `Failed`/`Undef` at the outer level is the signature of the
/// redispatch not delivering the realized mesh to the `solver::multi_case`
/// trampoline. Three hand-copied versions of that message drift; one does not.
#[cfg(has_gmsh)]
fn multi_case_cases_map(
    result_val: &reify_ir::Value,
) -> std::collections::BTreeMap<reify_ir::Value, reify_ir::Value> {
    use reify_ir::Value;

    match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_load_cases result must be a MultiCaseResult Value::Map, got: {other:?} \
             — a pre-hydration Failed/Undef here means the redispatch did not deliver \
             the realized mesh to the multi_case trampoline"
        ),
    }
}

/// One named case out of [`multi_case_cases_map`]'s result, dumping the key set
/// on a miss (an EMPTY map here is #5951's signature, not a typo'd case name).
#[cfg(has_gmsh)]
fn multi_case_case<'a>(
    cases_map: &'a std::collections::BTreeMap<reify_ir::Value, reify_ir::Value>,
    case_name: &str,
) -> &'a reify_ir::Value {
    cases_map
        .get(&reify_ir::Value::String(case_name.to_string()))
        .unwrap_or_else(|| {
            panic!(
                "cases map must contain \"{case_name}\"; got: {:?}",
                cases_map.keys().collect::<Vec<_>>()
            )
        })
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

    let cases_map = multi_case_cases_map(result_val);
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries (operating, overload), got {}",
        cases_map.len()
    );

    // Realized-path §7a grid (vs the synthetic ny=1 → 854): see
    // `REALIZED_BOX_GRID_AXES` for the derivation.
    for case_name in ["operating", "overload"] {
        let case_val = multi_case_case(&cases_map, case_name);

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

        // ── #6154 grid-miss measurement, riding THIS realization ─────────────
        // A full OCCT+gmsh realization is the most expensive thing in this file
        // and one has just happened here, on the fixture #6154 measures, so its
        // measurement is asserted from this capstone rather than from a second
        // copy of the same build. Every assertion it makes is labelled
        // "realized box"; see `assert_box_grid_miss_measurement` for the whole
        // narrative and for what it deliberately does NOT assert.
        if case_name == "operating" {
            let stress = sampled_field(case_val, "stress");
            assert_box_grid_miss_measurement(&disp, &stress);
        }
    }
}

/// [`MULTI_CASE_BODY_SOURCE`] with ONE no-op `structure` declared AHEAD of the
/// solving structure — and, deliberately, NO `RepresentationWithin` bound
/// (task 5951).
///
/// `FeaLeading` owns no geometry, no compute node and no constraint. Its only
/// effect is to add one iteration to `build()`'s per-template loop before
/// `FeaBodyMultiCase`'s body realizes. `structure FeaMeshTolerance { param
/// nominal : Real = 1.0 }` in [`NON_PRISMATIC_MULTI_CASE_BODY_SOURCE`] is
/// byte-identical in shape — which is exactly the point: that fixture's
/// leading structure, not its tolerance bound, is what emptied the `cases` map.
/// Keeping this fixture bound-free states the defect with the tolerance
/// contract removed entirely, so no future reader can re-derive the wrong
/// attribution from it.
#[cfg(has_gmsh)]
const MULTI_CASE_BODY_WITH_LEADING_TEMPLATE_SOURCE: &str = r#"
structure FeaLeading {
    param nominal : Real = 1.0
}

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

/// `cfg(has_gmsh)`: task 5951 acceptance — a template declared AHEAD of the
/// solving structure must not strand the body-arg solve.
///
/// This is the tolerance-INDEPENDENT, shape-INDEPENDENT statement of the
/// defect. The fixture is the green
/// [`multi_case_body_solve_shares_one_realization_across_cases`]'s box source
/// verbatim, plus one no-op leading `structure` and nothing else — same body,
/// same material, same two load cases, no `RepresentationWithin`. It is RED at
/// this commit's grandparent and green here.
///
/// The mechanism, measured: `redispatch_geometry_consuming_compute_nodes` runs
/// once per template and scans ALL compute nodes on every call, so during
/// `FeaLeading`'s iteration it reached the solve while `body` was still a
/// SYMBOLIC `kernel_handle: None` placeholder. It recorded a content-free
/// `realization_inputs`, which tripped the one-shot
/// `realization_inputs.is_empty()` candidate gate — and the LATER, correct pass
/// for `FeaBodyMultiCase`'s own template was then skipped forever. The cell
/// kept the stdlib inline `MultiCaseResult()` fallback: `cases = Map({})`, with
/// NO diagnostic (`solve_multi_case_trampoline`'s pre-hydration body-path guard
/// returns `Failed { diagnostics: vec![] }` by design, and the `ReprKind::BRep`
/// projection arm is identity-only per PRD §4 D1).
///
/// The kernel-agnostic unit-level statement of the same contract lives in the
/// `harness_engine` integration binary's `redispatch_template_order_regression`
/// module; this test closes it on the real OCCT+gmsh path.
///
/// # The field bar is [`assert_box_grid_miss_measurement`]'s, same as the sibling
///
/// This test was authored against a base that predated both #6154's grid-miss
/// helpers and #6200's mesh-coverage fix, so it hand-rolled an `any(is_finite)`
/// / `all(finite || nan)` pair and recorded `len=8967 nonfinite=3165
/// axes=[61,7,7]` for the box. RE-MEASURED post-merge, both cases and both
/// fields now report:
///
/// ```text
/// axes=[61, 7, 7] n_grid=2989 n_missed=0 (0.0%)
///   | interior=0 face=0 edge=0 corner=0 | nonfinite_anomalies=0
/// ```
///
/// i.e. the 3165 sentinel components are gone — that was the coverage defect
/// #6200 fixed, not anything this test owns. The reproducible claim is the
/// helper's `missed_interior == 0`, which is keyed on COVERAGE rather than on a
/// count and so survives the drift the total is subject to; the exact-count form
/// stays where #6154 put it. Both this test and the green no-leading-template
/// sibling [`multi_case_body_solve_shares_one_realization_across_cases`] now
/// state that same contract through the same helper, on the byte-identical body.
#[cfg(has_gmsh)]
#[test]
fn multi_case_body_solve_survives_a_preceding_template() {
    use reify_core::{Severity, ValueCellId};
    use reify_ir::Value;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping multi_case_body_solve_survives_a_preceding_template: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    // Same helper as the B9 tests: fresh engine, manual trampolines,
    // `register_volume_mesh_demand`, `ensure_gmsh_kernel`, and the
    // provenance-vs-counter cross-check. The delta itself is not asserted here —
    // with no tolerance contract it is 0 by the deliberate "no tolerance
    // contract → no caching" semantic, which is the counter's business and not
    // this test's.
    let (_delta, build_result) =
        build_multi_case_and_count_realizations(MULTI_CASE_BODY_WITH_LEADING_TEMPLATE_SOURCE);

    let errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics from the leading-template build, got: {errors:?}"
    );

    let result_cell = ValueCellId::new("FeaBodyMultiCase", "result");
    let result_val = build_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaBodyMultiCase.result not found in build values"));
    // Shared two-level unwrap (#6154's helper), whose own panic text already
    // names the pre-hydration Failed/Undef signature this test is about.
    let cases_map = multi_case_cases_map(result_val);
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries (operating, overload) even though a \
         template precedes the solving structure; got {}. An EMPTY map here is the \
         #5951 strand: a premature per-template redispatch consumed the symbolic \
         handle and latched the node out, leaving the stdlib inline \
         `MultiCaseResult()` fallback — silently, with no Error diagnostic",
        cases_map.len()
    );

    for case_name in ["operating", "overload"] {
        let case_val = multi_case_case(&cases_map, case_name);
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
        assert_ne!(
            disp.data.len() / 3,
            SYNTHETIC_GRID_NODES,
            "case \"{case_name}\": must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
             synthetic grid — the realized VolumeMesh must drive the §7a grid, which \
             is the whole point of the redispatch this test pins"
        );
        // The positive half of the same statement: the realized-AABB heuristic
        // drove the grid, exactly as it does on the green no-leading-template
        // sibling.
        assert_eq!(
            disp.data.len() / 3,
            REALIZED_BOX_GRID_NODES,
            "case \"{case_name}\": §7a grid node count must equal the realized-AABB \
             heuristic {REALIZED_BOX_GRID_NODES} ({REALIZED_BOX_GRID_AXES:?} for the \
             1 m × 100 mm × 100 mm box), the same grid the green sibling produces"
        );

        let stress = sampled_field(case_val, "stress");

        // ── the finiteness bar: #6154's normative bucket split ───────────────
        // NOT `all(is_finite)`: on the REALIZED path the §7a grid spans the tet
        // mesh AABB, so any grid point outside the solid carries the PRD §3
        // sentinel `f64::NAN` BY DESIGN (`compute_targets/elastic_static.rs`'s
        // field-population contract is the producer side). But the branch's
        // hand-rolled `any(is_finite)` / `all(finite || nan)` pair is not the
        // right replacement either — it says nothing about WHERE the sentinel
        // fires. `assert_box_grid_miss_measurement` does, and is what the green
        // no-leading-template sibling
        // [`multi_case_body_solve_shares_one_realization_across_cases`] already
        // asserts on the byte-identical body, so the two now state the same
        // contract the same way: grid pinned per-axis to `REALIZED_BOX_GRID_AXES`,
        // the report reconciled against the raw buffer (which subsumes
        // `all(finite || nan)`, and strengthens it — a MIXED part-NaN point is
        // rejected too), `stress` pinned to mark the SAME grid points as
        // `displacement`, and the BOX-SPECIFIC prediction `missed_interior == 0`:
        // for a prismatic body the mesh AABB IS the solid, so every
        // strictly-index-interior grid point must lie inside some tet.
        //
        // That last one also subsumes `any(is_finite)` here, which is why (unlike
        // B9's cylinder, where it is kept) it is dropped: this grid has
        // 59 × 5 × 5 = 1475 index-interior nodes, so an ALL-sentinel field — the
        // #5951 silent failure mode — lands as `missed_interior == 1475` and
        // fails loudly. A non-zero interior count is otherwise a regression of
        // the mesh-coverage defect #6200 fixed and belongs THERE, not here.
        assert_box_grid_miss_measurement(&disp, &stress);

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
/// ## The bound is NOT why this fixture's `cases` map used to come back empty
///
/// It once was recorded here and on the B9 test that the bound ALSO emptied the
/// solve's `cases` map, from a 2×2 over {cylinder, box} × {bound, no bound}.
/// That 2×2 was confounded and the conclusion is retracted (task #5951): the
/// bound requires a checker structure, so both bounded cells also declare
/// `structure FeaMeshTolerance` AHEAD of the solving structure — and it was that
/// LEADING TEMPLATE, not the bound, that stranded the solve. Adding a bound-free
/// no-op leading structure to the green box fixture reproduces the empty map
/// exactly ([`MULTI_CASE_BODY_WITH_LEADING_TEMPLATE_SOURCE`]). Keep that in mind
/// before attributing any future behaviour here to the tolerance contract:
/// this fixture varies TWO things at once relative to the un-bounded ones.
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

    // ── (5) #6154 CYLINDER control, riding THIS realization ───────────────────
    // The anti-weakening guard for the box's 35% out-of-solid measurement: here
    // the AABB is NOT the solid, so 84 of 112 grid nodes MUST stay `NaN`. Rides
    // this build for the same cost reason as the box's; every assertion is
    // labelled "realized cylinder". Closed form and ownership split:
    // `assert_cylinder_grid_miss_measurement`.
    let cyl_stress = sampled_field(result_val, "stress");
    let _report = assert_cylinder_grid_miss_measurement(&disp, &cyl_stress);
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
/// This half of B9 is split out of
/// [`multi_case_non_prismatic_body_caches_one_realization_for_both_cases`]
/// (which carries the `cases`-map / Sampled-field half and is now green too)
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
/// **The re-mesh-avoidance assertions of B9 are NOT here.** They live in the
/// sibling [`non_prismatic_two_case_build_realizes_body_exactly_once`] above:
/// the two-case `realization_entries` delta == 1, and the one-case control
/// proving a second load case adds ZERO entries. Both halves are green; the
/// split is kept so a counter regression and a result-shape regression are
/// diagnosable apart from each other.
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
///       Regular grid carrying real in-solid samples and no ±inf, `stress`
///       likewise — and converged;
///   (5) per-case independence still holds
///       (`overload.max_von_mises > operating.max_von_mises`).
///
/// (numbering kept from the original single B9 test, whose (1)/(2) are now the
/// green sibling's)
///
/// # (4)'s finiteness bar is the PRD's — neither tightened nor weakened
///
/// (4) deliberately does NOT require every component to be finite. The PRD row
/// this test encodes says so verbatim: `displacement` is "finite at grid points
/// inside the solid, `NaN` outside"
/// (`docs/prds/v0_4/fea-result-model.md:100`, the normative field-contract row),
/// and §3's grid rationale calls that out-of-solid `f64::NAN` a load-bearing
/// sentinel, "skipped uniformly by the reductions' `is_finite()` discipline"
/// (:82). The producer side of the same contract is the `displacement` and
/// `stress` bullets of `compute_targets/elastic_static.rs`'s
/// "Field-population contract" module doc, each of which states the
/// out-of-solid `f64::NAN` for its own component count. On the REALIZED path
/// the §7a grid spans the tet mesh AABB, so for a CURVED body most grid points
/// legitimately fall outside the solid: an `all(is_finite)` bar would measure
/// the sampler's AABB coverage rather than the solve.
///
/// So (4) states the bar the way #6154 built it to be stated — as a
/// BUCKET SPLIT, via [`assert_cylinder_grid_miss_measurement`], whose closed
/// form is re-derived at that helper and not restated here. In outline:
/// `cylinder(50mm, 200mm)` ⇒ [`REALIZED_CYLINDER_GRID_AXES`] `[4, 4, 7]` =
/// [`REALIZED_CYLINDER_GRID_NODES`] 112 nodes, of which the 12-of-16
/// cross-section points failing `dx² + dy² < 50²` at all 7 z-levels give
/// [`CYLINDER_PREDICTED_MISSES`] = 84 out-of-solid nodes (252 of the 336
/// displacement components). That is strictly stronger than the per-field
/// `!is_empty()` / `any(is_finite)` / `all(finite || nan)` triple this test
/// carried before the merge, in three ways: it pins the grid SHAPE the closed
/// form is derived from, it rejects a MIXED part-NaN grid point (the old
/// `all(finite || nan)` admitted one), and above all it hard-asserts the
/// UNDER-firing direction — every one of the 84 predicted-outside nodes must
/// carry the sentinel, so a later "fix" that widens `tol`, clamps, or asserts
/// all-finite fails loudly instead of passing. PRD §11 Q2 rejects all three of
/// those outright; do NOT weaken the sentinel to satisfy a failure here.
///
/// MEASURED on this fixture — which, unlike the single-case capstone
/// [`non_prismatic_body_solve_runs_on_realized_volume_mesh`] that calls the
/// same helper, additionally carries the 100 µm `RepresentationWithin` bound —
/// both cases measure `axes=[4, 4, 7] n_grid=112 n_missed=84 (75.0%) |
/// interior=0 face=40 edge=36 corner=8 | nonfinite_anomalies=0`, on
/// `displacement` and `stress` alike. That is the closed form exactly — and the
/// same split [`realized_cylinder_mesh_covers_its_own_aabb`] pins — so the bound
/// does not move the realized AABB and nothing here needs re-deriving.
///
/// ONE clause of the old triple survives alongside the helper, per field:
/// `data.iter().any(is_finite)`. It is not redundant. The helper asserts only
/// that predicted-outside nodes DO miss; the opposite direction is logged, not
/// asserted, because an EXCESS miss is #6200's mesh-coverage territory and the
/// gmsh/HXT tetrahedralization is not bit-reproducible. An ALL-sentinel field
/// therefore passes the helper — and an all-sentinel field is precisely #5951's
/// silent failure mode. `any(is_finite)` is the weakest live statement of "the
/// solve actually ran" that no mesh drift can redden.
///
/// # History — this was `#[ignore]`d on a false attribution
///
/// This test carried `#[ignore = "blocked on #5951"]` plus a 2×2 measurement
/// table concluding "it is the tolerance contract, not the body shape" that
/// empties the `cases` map, and an instruction not to adjust its assertions.
/// That conclusion was WRONG and is retracted: the 2×2 never controlled for the
/// bounded fixtures also declaring `structure FeaMeshTolerance` AHEAD of the
/// solving structure. The real defect was template ORDERING — a premature
/// per-template `redispatch_geometry_consuming_compute_nodes` pass consumed the
/// still-SYMBOLIC (`kernel_handle: None`) body handle, recorded a content-free
/// `realization_inputs`, and tripped the one-shot
/// `realization_inputs.is_empty()` candidate gate, so the later correct pass was
/// skipped forever. Task #5951 fixed that; the tolerance-free statement of the
/// same defect is [`multi_case_body_solve_survives_a_preceding_template`], and
/// the kernel-agnostic one is the `harness_engine` integration binary's
/// `redispatch_template_order_regression` module.
/// The `all(is_finite)` clause was an embellishment added along with that
/// attribution — B9's normative content is the cache-reuse row
/// (`docs/prds/v0_4/fea-result-model.md:243`) plus the §3 field contract, and
/// neither ever asked for it.
///
/// The `NON_PRISMATIC_MULTI_CASE_BODY_SOURCE` doc's separate claim that the
/// `RepresentationWithin` bound is what makes `realization_entries` observable
/// ("no tolerance contract → no caching") is untouched by this and still true.
#[cfg(has_gmsh)]
#[test]
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
    let cases_map = multi_case_cases_map(result_val);
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries (operating, overload), got {}",
        cases_map.len()
    );

    // ── (4) each case carries REAL Sampled fields, converged ──────────────────
    let mut von_mises: Vec<(&str, f64)> = Vec::new();
    for case_name in ["operating", "overload"] {
        let case_val = multi_case_case(&cases_map, case_name);
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
        assert_ne!(
            disp.data.len() / 3,
            SYNTHETIC_GRID_NODES,
            "case \"{case_name}\": must NOT reproduce the {SYNTHETIC_GRID_NODES}-node \
             synthetic grid — the shared realized VolumeMesh must drive the §7a grid"
        );

        // `stress` must be a real Sampled field too, not merely present — it
        // shares the grid, so it shares the out-of-solid NaN sentinels.
        let stress = sampled_field(case_val, "stress");

        // ── (4)'s finiteness bar: #6154's normative bucket split ─────────────
        // Both fields go through `assert_cylinder_grid_miss_measurement`, the
        // closed form for THIS body: `cylinder(50mm, 200mm)` ⇒ a §7a grid of
        // `REALIZED_CYLINDER_GRID_AXES` = [4, 4, 7], of whose 112 nodes exactly
        // `CYLINDER_PREDICTED_MISSES` = 84 lie outside r = 50 mm. It pins that
        // grid shape, reconciles the report against the raw buffer (where the
        // "no ±inf" clause now lives, strengthened to also reject a MIXED
        // part-NaN point that the old `all(finite || nan)` admitted), pins that
        // `stress` marks the SAME grid points as `displacement`, and
        // hard-asserts the UNDER-firing direction — every one of the 84 must
        // carry the normative sentinel, so a future weakening fails loudly.
        //
        // MEASURED on this fixture, which unlike the capstone's additionally
        // carries the 100 µm `RepresentationWithin` bound: axes [4, 4, 7], 84
        // of 112 nodes missed — the closed form exactly, so the bound does not
        // move the realized AABB. Run once per case deliberately: B9's claim is
        // that EACH case's fields come back real off the ONE shared
        // realization, so each case's pair is measured.
        let _report = assert_cylinder_grid_miss_measurement(&disp, &stress);

        // Kept ALONGSIDE the helper, not subsumed by it — and not a weakening
        // of anything. The helper asserts only that predicted-outside nodes DO
        // carry the sentinel; the opposite direction (a node the geometry puts
        // INSIDE coming back NaN) it deliberately logs rather than asserts,
        // that being #6200's non-bit-reproducible mesh-coverage territory. An
        // ALL-sentinel field therefore walks straight through it — and an
        // all-sentinel field is exactly #5951's silent failure mode. This is
        // the weakest live statement of "the solve actually ran" that no mesh
        // drift can redden: not that any particular node is finite, only that
        // not every one of them missed.
        assert!(
            disp.data.iter().any(|v| v.is_finite()),
            "case \"{case_name}\": displacement must carry at least one finite \
             in-solid sample — an all-sentinel field means the solve never ran \
             on the realized curved mesh (the #5951 strand)"
        );
        assert!(
            stress.data.iter().any(|v| v.is_finite()),
            "case \"{case_name}\": stress must carry at least one finite in-solid \
             sample — an all-sentinel field means the solve never ran on the \
             realized curved mesh (the #5951 strand)"
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

/// Task #6154 — realize `NON_PRISMATIC_BODY_SOURCE`'s `cylinder(50mm, 200mm)`
/// through the OCCT+gmsh path and hand back its §7a-resampled
/// `(displacement, stress)` fields. `None` ⇒ OCCT is unavailable and the caller
/// skips.
///
/// `non_prismatic_body_solve_runs_on_realized_volume_mesh` above already
/// realizes this source, so the LIVE control
/// ([`assert_cylinder_grid_miss_measurement`]) rides that build and this helper
/// is not on its path at all. It exists solely for the `#[ignore]`d
/// [`realized_cylinder_mesh_covers_its_own_aabb`], which is run explicitly, in
/// isolation, and so has no capstone realization to ride.
///
/// The box has no counterpart: its every claim now rides the capstone (see
/// [`assert_box_grid_miss_measurement`]), so the second realization that used to
/// pay for a nominally "independent" box coverage test is gone.
///
/// Not memoized — there is exactly one caller, and a `OnceLock` could never
/// dedupe against the capstone's realization anyway, because that capstone
/// asserts on the `Engine` (`realization_kernel_provenance()`), which a
/// field-returning helper cannot hand back. The realization HARNESS that
/// genuinely IS shared is [`build_realized`].
#[cfg(has_gmsh)]
fn realized_cylinder_fields(
    caller: &str,
) -> Option<(reify_ir::SampledField, reify_ir::SampledField)> {
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

    Some((sampled_field(result_val, "displacement"), sampled_field(result_val, "stress")))
}

/// Classify a §7a field's out-of-solid grid points and DUMP the full split plus
/// per-axis miss histograms — the measurement artefact #6154 owes. Returns
/// `(report, per_axis_histograms)`.
///
/// `stride` is the field's component count: 3 for `displacement`, 9 for
/// `stress`. Both carry the identical PRD §3 sentinel contract (see
/// `elastic_static.rs`'s field-population contract — "neither is exempt"), so
/// both are measurable by the same instrument, and `label` is what tells two
/// dumps of the same realization apart.
///
/// A raw NaN *count* diagnoses nothing on its own: the sentinel is normative
/// (PRD `v0_4/fea-result-model.md` §3 / §4.1), and a coverage hole and boundary
/// round-off write the identical `NaN`. The index-bucket split is what tells
/// them apart, so it is printed unconditionally on every run rather than being
/// reachable only from a failing assertion.
#[cfg(has_gmsh)]
fn classify_and_dump_grid_misses(
    disp: &reify_ir::SampledField,
    stride: usize,
    label: &str,
) -> (reify_solver_elastic::GridMissReport, Vec<Vec<usize>>) {
    let report = reify_solver_elastic::classify_grid_misses(disp, stride);
    let axes: Vec<usize> = disp.axis_grids.iter().map(|a| a.len()).collect();
    let mut hist: Vec<Vec<usize>> = axes.iter().map(|&n| vec![0usize; n]).collect();
    for idx in &report.missed_indices {
        for a in 0..3 {
            hist[a][idx[a]] += 1;
        }
    }
    eprintln!(
        "#6154 {label} §7a grid-miss report: axes={axes:?} n_grid={} n_missed={} \
         ({:.1}%) | interior={} face={} edge={} corner={} | nonfinite_anomalies={}",
        report.n_grid,
        report.n_missed,
        100.0 * report.n_missed as f64 / report.n_grid as f64,
        report.missed_interior,
        report.missed_face,
        report.missed_edge,
        report.missed_corner,
        report.n_nonfinite_anomalies,
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

    // (2) No non-finite point OTHER than the all-NaN sentinel — recounted off
    //     the raw buffer with the SAME `!is_finite()` predicate the instrument
    //     uses. Not `is_nan()`: a diverged solve overflows to `±INF` at least as
    //     readily, and an all-`INF` point looks perfectly ordinary to a NaN-only
    //     test — it is neither a miss nor an anomaly there, so a NaN-only guard
    //     would certify a already-broken field as fully covered, which is the
    //     inversion this check exists to prevent. (1) would undercount it too.
    let independent_anomalies = sf
        .data
        .chunks_exact(stride)
        .filter(|c| !c.iter().all(|v| v.is_nan()) && c.iter().any(|v| !v.is_finite()))
        .count();
    assert_eq!(
        independent_anomalies, 0,
        "{label}: {independent_anomalies} grid point(s) carry a non-finite (NaN or \
         ±INF) component that is NOT the all-or-nothing out-of-solid sentinel — i.e. \
         a diverged solution value. The bucket split then describes a field already \
         broken upstream of the sampler; fix that before reading it",
    );
    assert_eq!(
        report.n_nonfinite_anomalies, independent_anomalies,
        "{label}: the report's anomaly count ({}) must agree with the raw buffer's \
         ({independent_anomalies}) — they are computed independently, so a mismatch \
         means `classify_grid_misses` and this reconciler disagree about what \
         'non-finite' means",
        report.n_nonfinite_anomalies,
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

/// Run the SAME grid-miss measurement over the stride-9 `stress` field and pin
/// that it agrees with `displacement`'s, grid point for grid point.
///
/// `elastic_static.rs`'s field-population contract says `displacement` and
/// `stress` carry the IDENTICAL PRD §3 sentinel and that "neither is exempt",
/// and §7a backs that with ONE `resample_multi_nodal_to_grid` call: each grid
/// point is located once, and both fields are written from that single
/// containment test. So the two miss sets are not merely similar — they are the
/// same set. A difference is therefore a real defect (a per-field write path
/// that drifted, a stride/layout error) of a class a displacement-only check
/// cannot see, and the expensive realization is already paid for, so measuring
/// it costs a second classification pass and nothing else.
///
#[cfg(has_gmsh)]
fn assert_stress_miss_set_matches_displacement(
    stress: &reify_ir::SampledField,
    disp_report: &reify_solver_elastic::GridMissReport,
    label: &str,
) {
    use std::collections::HashSet;

    let stress_label = format!("{label} stress");
    let (report, _hist) = classify_and_dump_grid_misses(stress, 9, &stress_label);
    assert_report_reconciles_with_field(stress, &report, 9, &stress_label);

    let disp_set: HashSet<[usize; 3]> = disp_report.missed_indices.iter().copied().collect();
    let stress_set: HashSet<[usize; 3]> = report.missed_indices.iter().copied().collect();
    let mut stress_only: Vec<[usize; 3]> = stress_set.difference(&disp_set).copied().collect();
    let mut disp_only: Vec<[usize; 3]> = disp_set.difference(&stress_set).copied().collect();
    stress_only.sort_unstable();
    disp_only.sort_unstable();
    assert!(
        stress_only.is_empty() && disp_only.is_empty(),
        "{label}: `displacement` and `stress` must mark the SAME grid points \
         out-of-solid — §7a locates each point once and writes both fields from \
         that one containment test, so a difference means the two write paths \
         disagree about containment (or a stride/layout error is mis-reading one \
         of them). Missed in stress only: {stress_only:?}. Missed in displacement \
         only: {disp_only:?}",
    );
}

/// Task #6154's DELIVERABLE for the BOX — measure the realized box's
/// out-of-solid grid points, dump the split on every run, and pin that the
/// report faithfully describes the field it was derived from.
///
/// `elastic_static.rs`'s field-population contract used to claim of
/// `displacement`: "Every grid point lies inside the solid (prismatic box), so
/// all samples are finite (no NaN sentinels for the cantilever geometry)". The
/// realized path contradicted it — when #6154 measured it, about a third of this
/// prismatic box's grid nodes carried the out-of-solid sentinel. VERDICT: a
/// COVERAGE defect in the realized tet mesh, not a tolerance one; the
/// measurement, its provenance and the closing argument are recorded ONCE, in
/// `docs/prds/v0_4/fea-result-model.md` §11 Q2, and the upstream defect was
/// fixed under #6200. This function pins no count either way — for today's
/// numbers read the dump below, never a comment.
///
/// Called from `multi_case_body_solve_shares_one_realization_across_cases`,
/// which already realizes this exact source. A full OCCT tessellation + gmsh
/// tet-mesh + solve is the most expensive thing in this file, so every #6154
/// claim about the box rides THAT realization rather than paying for a second
/// copy of it; the same is true of the cylinder's, on its own capstone.
///
/// Both `displacement` (stride 3) and `stress` (stride 9) are measured — the
/// field-population contract makes their sentinel contracts identical, so a
/// displacement-only check would leave half of it unmeasured. See
/// [`assert_stress_miss_set_matches_displacement`].
///
/// Deliberately NOT asserted here:
///   - any all-finite property — the sentinel is normative and must survive;
///   - the total miss COUNT — that count is the thing under investigation, and
///     pinning it before the mechanism is fixed would cement a bug as a
///     contract. It would also be FLAKY: the split drifts run to run (measured
///     2026-08-20, this fixture, same binary — four runs agreed and a fifth
///     differed by +5 total misses with interior −3, face +8; test-thread count
///     is ruled out, the full suite reproduced the majority split both at
///     `--test-threads=1` and at the default). *Hypothesis:* gmsh/HXT's parallel
///     tetrahedralization is not bit-reproducible under varying host load —
///     project memory carries an independent measurement of the same effect at
///     ~1–6% tet-count drift;
///   - the bucket-sum identity — `classify_grid_misses` satisfies it by
///     construction, so it says nothing about this field (it is pinned in that
///     function's own crate-local fixtures instead).
///
/// ## What IS asserted: the BOX-SPECIFIC prediction, `missed_interior == 0`
///
/// For a PRISMATIC body the mesh AABB **is** the solid, so every
/// strictly-index-interior grid point must lie inside some tet. When #6154
/// measured it, it did not: the realized tet mesh filled only a fraction of the
/// AABB it spanned, and a few dozen index-interior nodes landed in no tet at
/// all. That was a COVERAGE defect, not a tolerance one — the measured margins
/// were orders of magnitude beyond any defensible `tol`, and
/// `volume_mesh_to_solver_mesh` was exonerated. The defect was upstream of this
/// crate, in the gmsh tetrahedralization path (`crates/reify-kernel-gmsh`),
/// which #6154's scope explicitly excluded, and it was fixed under #6200
/// (merged `6ed34b2fe8`). The claim was carried verbatim through that wait —
/// never deleted, never weakened to a threshold — precisely so #6200 had an
/// executable acceptance gate. With the fix landed its job is to keep the fix
/// fixed.
///
/// It stays `== 0` on `missed_interior` rather than tightening to `n_missed ==
/// 0`, even though the box currently measures zero misses in total. The
/// AABB-shell buckets (face/edge/corner) remain legitimately exposed to boundary
/// round-off, and the total drifts run to run. Tightening would trade a stable
/// invariant for a flake.
///
/// ### `missed_interior` counts GRID points, NOT mesh nodes
///
/// The two are easy to conflate and behave completely differently. #6200's own
/// measurements record that a box's strictly-interior MESH-NODE count is purely
/// RESOLUTION-driven — 0 at the auto mesh size even for a complete, `fill = 1.0`
/// mesh, because `auto_mesh_size_from_features` makes the cross-section exactly
/// one element wide — so an assertion keyed on THAT number would not have gone
/// green from a coverage fix at all. This one is keyed on COVERAGE instead: a
/// grid point is a query location, not a mesh entity, and `missed_interior`
/// reaches 0 exactly when the tets tile the AABB, at ANY mesh resolution and
/// with ANY interior-node count — precisely #6200's acceptance property.
///
/// This assertion used to live in a separate `realized_box_mesh_tiles_its_own_aabb`
/// test that paid for a SECOND full OCCT+gmsh realization of this same fixture
/// (~12 s) for "independence". That independence was illusory — both paths went
/// through [`build_realized`] to the same source and asserted on the same field
/// — so the claim moved here, onto the realization the capstone already
/// performs, and stays live.
#[cfg(has_gmsh)]
fn assert_box_grid_miss_measurement(
    disp: &reify_ir::SampledField,
    stress: &reify_ir::SampledField,
) {
    let (report, hist) = classify_and_dump_grid_misses(disp, 3, "realized box displacement");

    // ── (i) the report describes THIS field, re-derived from `disp.data` ─────
    assert_report_reconciles_with_field(disp, &report, 3, "realized box displacement");

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

    // ── (iii) `stress` carries the same sentinel, on the same grid points ────
    assert_stress_miss_set_matches_displacement(stress, &report, "realized box");

    // ── (iv) BOX-SPECIFIC coverage prediction — see the doc above ────────────
    assert_eq!(
        report.missed_interior, 0,
        "BOX-SPECIFIC prediction: for a prismatic body the mesh AABB IS the solid, \
         so every strictly-index-interior grid point must lie inside some tet. A \
         non-zero interior count means the realized mesh handed to §7a does not tile \
         its own AABB — that is a COVERAGE defect, not a tolerance one, and widening \
         `tol` would not legitimately fix it. This is a REGRESSION of the upstream \
         mesh-coverage defect fixed under #6200; the fix belongs there, not here. \
         Measured: interior={} of n_missed={} (face={}, edge={}, corner={}). \
         Per-axis miss histograms: x={:?} y={:?} z={:?}",
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

/// The 4 cross-section columns whose offsets are `(±1/60, ±1/60) m` — the only
/// ones inside the cylinder's `r = 0.05 m`. Every OTHER `(ix, iy)` column is
/// outside, at all 7 z-levels: `12 × 7 =` [`CYLINDER_PREDICTED_MISSES`].
/// Derivation: [`assert_cylinder_grid_miss_measurement`].
#[cfg(has_gmsh)]
const CYLINDER_INSIDE_COLUMNS: [[usize; 2]; 4] = [[1, 1], [1, 2], [2, 1], [2, 2]];

/// Out-of-solid grid nodes the closed form predicts for the realized cylinder,
/// of [`REALIZED_CYLINDER_GRID_NODES`]. Derivation:
/// [`assert_cylinder_grid_miss_measurement`].
#[cfg(has_gmsh)]
const CYLINDER_PREDICTED_MISSES: usize = 84;

/// Task #6154 — the CYLINDER control: prove the normative out-of-solid `NaN`
/// sentinel still fires exactly where geometry says it must.
///
/// The box measurement shows ~35% of a *prismatic* body's grid points marked
/// out-of-solid where 0% is predicted. The obvious wrong "fix" for that is to
/// weaken the sentinel — widen `tol`, or assert all-finite. This is the guard
/// against it: for a cylinder the AABB is emphatically NOT the solid, so a
/// large, exactly-predictable fraction of grid points MUST stay `NaN`, and any
/// weakening shows up here as a shortfall.
///
/// Rides `non_prismatic_body_solve_runs_on_realized_volume_mesh`'s realization
/// (see [`realized_cylinder_fields`], which is NOT on this path — it serves the
/// `#[ignore]`d coverage test, which has no capstone to ride).
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
/// inside the cylinder.
///
/// The box's violated prediction is a strictly stronger statement, and remains
/// box-specific: for a prismatic body the AABB IS the solid, so *no* grid point
/// of any bucket may miss (`n_missed == 0`). Here 84 of 112 must miss. Holding
/// the cylinder's split next to the box's is what stops a later reader promoting
/// `missed_interior == 0` to a global sampler invariant, or "correcting" the
/// cylinder's entirely-correct 75%.
///
/// ## Only the UNDER-firing direction is asserted live
///
/// A node the closed form puts outside that comes back finite is this crate's
/// own contract broken, so that is a hard failure here. The opposite direction —
/// an EXCESS miss, i.e. a node the geometry puts inside that the realized mesh
/// failed to cover — would be a regression of the upstream mesh-coverage defect
/// that #6200 fixed, which this test explicitly disclaims ownership of; and the
/// realized mesh is not bit-reproducible across runs, so pinning it live would
/// let upstream drift red the merge gate for something that "must not be fixed
/// here". The excess is logged loudly instead, and the exact-count/bucket form
/// of the claim is kept in [`realized_cylinder_mesh_covers_its_own_aabb`].
///
/// Both `displacement` (stride 3) and `stress` (stride 9) are measured, and
/// pinned to mark the same grid points — see
/// [`assert_stress_miss_set_matches_displacement`]. The anti-weakening argument
/// applies verbatim to `stress`: `elastic_static.rs`'s field-population contract
/// makes the two sentinel contracts identical and exempts neither, so a
/// weakening that touched only `stress` would slip past a displacement-only
/// guard.
///
/// Returns the DISPLACEMENT [`GridMissReport`] it dumped, so a caller that wants
/// to add the upstream-owned claims on the SAME field does not have to
/// re-classify (and re-print) it.
#[cfg(has_gmsh)]
fn assert_cylinder_grid_miss_measurement(
    disp: &reify_ir::SampledField,
    stress: &reify_ir::SampledField,
) -> reify_solver_elastic::GridMissReport {
    use std::collections::HashSet;

    let (report, _hist) = classify_and_dump_grid_misses(disp, 3, "realized cylinder displacement");

    let axes: Vec<usize> = disp.axis_grids.iter().map(|a| a.len()).collect();
    assert_eq!(
        axes,
        REALIZED_CYLINDER_GRID_AXES.to_vec(),
        "§7a grid shape for cylinder(50mm, 200mm) must be \
         {REALIZED_CYLINDER_GRID_AXES:?} ({REALIZED_CYLINDER_GRID_NODES} nodes) — the \
         closed form above is derived from exactly this shape; got {axes:?}",
    );
    assert_report_reconciles_with_field(disp, &report, 3, "realized cylinder displacement");
    assert_stress_miss_set_matches_displacement(stress, &report, "realized cylinder");

    // ── (a) SENTINEL guard — UNDER-firing ───────────────────────────────────
    // Owner: this crate's sampler. Every predicted-outside node must carry the
    // sentinel; a node here going finite means it was weakened.
    let missed: HashSet<[usize; 3]> = report.missed_indices.iter().copied().collect();
    let mut finite_but_predicted_outside: Vec<[usize; 3]> = Vec::new();
    for ix in 0..REALIZED_CYLINDER_GRID_AXES[0] {
        for iy in 0..REALIZED_CYLINDER_GRID_AXES[1] {
            if CYLINDER_INSIDE_COLUMNS.contains(&[ix, iy]) {
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
        "SENTINEL WEAKENED (this crate's sampler): the closed form puts these grid \
         nodes outside the cylinder, so each MUST carry the normative out-of-solid \
         `NaN` (PRD v0_4/fea-result-model.md §4.1), but they came back finite: \
         {finite_but_predicted_outside:?}. Do not 'fix' this by relaxing the \
         sentinel — a widened `tol`, a clamp, or an all-finite assertion all land \
         here first, and all of them fabricate values the solver never solved.",
    );

    // ── (b) COVERAGE — OVER-firing. Logged, NOT asserted (upstream-owned) ───
    // With (a) green the predicted 84 are all present, so any EXCESS is a node
    // the geometry places INSIDE the cylinder that the realized mesh failed to
    // cover — a regression of the coverage defect #6200 fixed, reaching this
    // fixture. Reported here; pinned in
    // `realized_cylinder_mesh_covers_its_own_aabb`.
    if report.n_missed > CYLINDER_PREDICTED_MISSES {
        eprintln!(
            "#6154 realized cylinder MESH UNDER-COVERS (upstream mesh-coverage \
             regression; the defect fixed under #6200 is back): closed \
             form predicts {CYLINDER_PREDICTED_MISSES} of \
             {REALIZED_CYLINDER_GRID_NODES} nodes out-of-solid and all of them are \
             present, so the {} extra miss(es) are nodes the geometry places INSIDE \
             the cylinder. NOT a sentinel bug, and not fixable here.",
            report.n_missed - CYLINDER_PREDICTED_MISSES,
        );
    }

    report
}

/// The cylinder's OVER-firing half: the realized mesh covers every grid point
/// the closed form places inside it, so the miss set is EXACTLY the predicted
/// 84, split (interior, face, edge, corner) = (0, 40, 36, 8).
///
/// Measured green on this base, and `#[ignore]`d anyway — the gate is about
/// DRIFT and EXPENSE, not about waiting on anyone. What it pins is an EXACT node
/// count over a gmsh/HXT tetrahedralization that is not bit-reproducible run to
/// run (the box's pre-fix split drifted on this very host, 1055 vs 1060 across
/// five runs; this fixture has only ever been observed on one host and one gmsh
/// build). Live, one drifted node would red the merge queue for a mesh property
/// this crate does not own and this file explicitly says "must not be fixed
/// here". It also costs a full OCCT+gmsh realization of its own.
///
/// [`assert_cylinder_grid_miss_measurement`] keeps the half this crate DOES own
/// — the sentinel must fire on every predicted-outside node — as a hard LIVE
/// assertion on the capstone's realization, and logs any excess. This test
/// re-runs that half on its own realization before adding the two mesh-coverage
/// claims, so all three are pinned on one and the same field.
///
/// So: run it explicitly (`--ignored`) to re-measure the cylinder's coverage.
/// The excess-miss direction it pins would now be a REGRESSION of the upstream
/// coverage defect fixed under #6200, not an instance of it.
#[cfg(has_gmsh)]
#[test]
#[ignore = "expensive: re-measures the realized cylinder's exact coverage; gmsh/HXT tetrahedralization is not bit-reproducible, so the exact count is run-explicit only"]
fn realized_cylinder_mesh_covers_its_own_aabb() {
    let Some((disp, stress)) =
        realized_cylinder_fields("realized_cylinder_mesh_covers_its_own_aabb")
    else {
        return;
    };

    // (a) first, on THIS realization: every predicted-outside node must be
    // missed. Re-run here rather than leant on from the live capstone — that
    // capstone measures a DIFFERENT realization of the fixture, and (c) below
    // needs under- and over-firing pinned on one and the same field to conclude
    // the miss set is exactly the predicted 84.
    let report = assert_cylinder_grid_miss_measurement(&disp, &stress);

    // ── (b) COVERAGE guard — OVER-firing ────────────────────────────────────
    // With (a) green above, every predicted-outside node IS missed, so any
    // excess over the closed form is a node the geometry places INSIDE.
    assert_eq!(
        report.n_missed, CYLINDER_PREDICTED_MISSES,
        "REALIZED CYLINDER MESH UNDER-COVERS: the closed form predicts exactly \
         {CYLINDER_PREDICTED_MISSES} of {REALIZED_CYLINDER_GRID_NODES} nodes \
         out-of-solid, so the {} extra miss(es) are nodes the geometry places INSIDE \
         the cylinder. That is a REGRESSION of the upstream mesh-coverage defect \
         fixed under #6200, now reaching the cylinder fixture — it is NOT a sentinel \
         bug and must not be fixed here.",
        report.n_missed.saturating_sub(CYLINDER_PREDICTED_MISSES),
    );

    // ── (c) BUCKETING — the classifier's own index-extremity arm ────────────
    // (a) puts all 84 predicted misses in the set and (b) admits no others, so
    // on this field the miss SET is exactly the predicted 84; this checks that
    // `classify_grid_misses` labels them the way the closed form does.
    assert_eq!(
        (
            report.missed_interior,
            report.missed_face,
            report.missed_edge,
            report.missed_corner,
        ),
        (0, 40, 36, 8),
        "the closed form splits the {CYLINDER_PREDICTED_MISSES} misses as interior=0 \
         face=40 edge=36 corner=8 by index extremity (derivation: \
         `assert_cylinder_grid_miss_measurement`). The set is already pinned above, \
         so a mismatch here is a bug in `classify_grid_misses`' bucketing, not in the \
         geometry or the mesh."
    );
}
