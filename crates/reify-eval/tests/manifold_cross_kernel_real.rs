//! Real-kernel integration gate for the Manifold execute arm (task 3437, ζ).
//!
//! Proves that `Engine::with_registered_kernels` (the inventory-driven
//! multi-kernel constructor) correctly routes a Mesh-demanded `BooleanUnion`
//! to the Manifold kernel — preceded by an OCCT BRep→Mesh tessellation stage
//! — when given two PARTIALLY-overlapping 10mm BRep boxes and no `#kernel`
//! pragma.  The routing is demanded-repr-driven: `build(ExportFormat::Stl)`
//! causes `compute_demanded_reprs` to mark the terminal realization `Mesh`,
//! which makes the dispatcher select Manifold (Mesh-capable) over OCCT
//! (BRep-capable) for the union.
//!
//! ## OCCT-failure premise: DROPPED
//!
//! This test does NOT rely on OCCT failing.  The dispatcher BFS never
//! considers OCCT's `(BooleanUnion, BRep)` when demanded == Mesh.  Real OCCT
//! meshes ingest into Manifold via the bit-exact vertex weld in
//! `manifold_from_reify_mesh` landed in task #4329.
//!
//! ## Signal class
//!
//! Engine-test-level integration gate.  Lives in `crates/reify-eval/tests/`
//! so `verify.sh`'s OCCT-gated suite picks it up automatically.  Does NOT
//! add any production Rust code — the routing substrate (task 4050/ε), the
//! Manifold execute arm (kernel.rs:245-270), and the vertex weld (#4329) are
//! already on main.
//!
//! ## Reuse
//!
//! - Linker anchor pattern: `crates/reify-kernel-manifold/tests/dispatcher_integration.rs:66-97`
//! - Engine routing pattern: `crates/reify-eval/tests/cross_kernel_handoff.rs:196-381`
//! - OCCT-available gate + include_str! + parse_and_compile_with_stdlib:
//!   `crates/reify-eval/tests/geometry_query_kernel_dispatch.rs:28-52`
//! - manufacturing_purpose injection: `crates/reify-eval/tests/geometry_query_kernel_dispatch.rs:406-420`
//! - Kernel-direct box/translate/tessellate: `crates/reify-kernel-occt/tests/interference_integration.rs:29-58`
//! - unit_cube_manifold non-degeneracy probe: `crates/reify-kernel-manifold/src/kernel.rs:1686-1695`

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{ExportFormat, GeometryError, GeometryKernel, GeometryOp, KernelId, ReprKind, Value};
use reify_kernel_manifold::ManifoldKernel;
use reify_test_support::{errors_only, manufacturing_purpose, parse_and_compile_with_stdlib};

// ── Item 2: engine routing ──────────────────────────────────────────────────

/// Linker anchor + engine routing gate.
///
/// Proves that `Engine::with_registered_kernels` routes a Mesh-demanded
/// `BooleanUnion` to Manifold (with an OCCT BRep→Mesh tessellation stage)
/// when built against two PARTIALLY-overlapping 10mm boxes in the
/// `examples/multi_kernel/manifold_boolean.ri` fixture.
///
/// Assertions (see plan step-1):
/// 1. `manifold_capability_descriptor()` is non-empty (linker anchor).
/// 2. Registry contains both `"occt"` and `"manifold"` after linking.
/// 3. Fixture compiles with no error-severity diagnostics.
/// 4. `build(ExportFormat::Stl)` emits no `NoKernelChain` error diagnostic.
/// 5. The `OverlapUnion` realization node records `produced_repr == Mesh`.
/// 6. `test_terminal_handle("OverlapUnion", Mesh, 1e-6).kernel == KernelId::Manifold`.
#[test]
fn engine_routes_overlapping_box_union_to_manifold_mesh() {
    // ── (1) Linker anchor ─────────────────────────────────────────────────
    // Calling manifold_capability_descriptor() forces the linker to include
    // register.rs from the reify-kernel-manifold rlib.  Without an observable
    // reference the rlib is dead-stripped and inventory::submit! never fires
    // (see dispatcher_integration.rs:66-88 for the full rationale).
    let anchor = reify_kernel_manifold::register::manifold_capability_descriptor();
    assert!(
        !anchor.supports.is_empty(),
        "manifold_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)"
    );

    // ── (2) OCCT gate ─────────────────────────────────────────────────────
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping engine_routes_overlapping_box_union_to_manifold_mesh: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // ── (3) Registry contains both kernels ────────────────────────────────
    let reg = reify_eval::kernel_registry::registry();
    assert!(
        reg.contains_key("occt"),
        "registry must contain \"occt\" after OCCT stub check; found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
    assert!(
        reg.contains_key("manifold"),
        "registry must contain \"manifold\" (linker anchor ensures the \
         inventory::submit! fired); found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );

    // ── (4) Compile the fixture ───────────────────────────────────────────
    // include_str! is a compile-time macro: if the fixture does not exist,
    // this file fails to compile → RED before step-2 creates the fixture.
    let mut compiled = parse_and_compile_with_stdlib(include_str!(
        "../../../examples/multi_kernel/manifold_boolean.ri"
    ));
    assert!(
        errors_only(&compiled).is_empty(),
        "manifold_boolean.ri must compile with no error-severity diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── (5) Inject manufacturing purpose (demanded_tol = Some(1e-6)) ─────
    // The RealizationCache is keyed by (entity, ReprKind, tol) and only
    // populates when demanded_tol = Some(..).  A purpose-free build leaves
    // demanded_tol = None → test_terminal_handle returns None → assertion (6)
    // would be impossible.  Mirror the pattern in
    // geometry_query_kernel_dispatch.rs:406-420.
    compiled
        .compiled_purposes
        .push(manufacturing_purpose("manufacturing", 1e-6));

    // ── (6) Build with real OCCT + Manifold ───────────────────────────────
    // with_registered_kernels instantiates every inventory-registered adapter
    // (OCCT via cfg(has_occt); Manifold unconditionally).  The singular
    // with_registered_kernel picks only OCCT (BRep-preferring lex-min picker),
    // so the PLURAL form is required to load Manifold.
    let mut engine = reify_eval::Engine::with_registered_kernels(Box::new(SimpleConstraintChecker));

    // eval() → activate_purpose → build() — the canonical pattern.
    // build()→eval() clears active_purpose_bindings, so activate_purpose MUST
    // be called AFTER eval() and BEFORE build() (see cross_kernel_handoff.rs:271-278).
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("manufacturing", "OverlapUnion");
    let build = engine.build(&compiled, ExportFormat::Stl);

    // ── (7) No NoKernelChain error diagnostic ─────────────────────────────
    let no_kernel_chain_errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::NoKernelChain) && matches!(d.severity, Severity::Error)
        })
        .collect();
    assert!(
        no_kernel_chain_errors.is_empty(),
        "cross-kernel build must not emit a NoKernelChain error diagnostic \
         (if present, the dispatcher could not find a BooleanUnion→Mesh chain, \
         meaning the Manifold rlib is not linked or registration failed); \
         got: {no_kernel_chain_errors:?}"
    );

    // ── (8) produced_repr == Mesh ─────────────────────────────────────────
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    // The fixture has multiple named lets (box_a, box_b_raw, box_b, body).
    // RealizationNodeId is keyed by (entity, index) in a PersistentMap
    // (hash-ordered, not insertion-ordered).  The terminal "body" binding has
    // the highest index, so max_by_key(index) always picks the right node
    // regardless of map iteration order.
    let overlap_union_node = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == "OverlapUnion")
        .max_by_key(|(id, _)| id.index)
        .map(|(_, r)| r)
        .expect(
            "OverlapUnion terminal realization node must be present in the \
             snapshot graph after build(ExportFormat::Stl)",
        );
    assert_eq!(
        overlap_union_node.produced_repr,
        ReprKind::Mesh,
        "the OverlapUnion realization must record produced_repr == Mesh \
         (the cross-kernel union resolves to the Mesh-capable Manifold kernel); \
         got {:?}",
        overlap_union_node.produced_repr
    );

    // ── (9) terminal handle is KernelId::Manifold ─────────────────────────
    // test_terminal_handle reads the RealizationCache at (entity, Mesh, tol).
    // The cache populates during execute_realization_ops when demanded_tol =
    // Some(1e-6) — which the manufacturing purpose injects (step 5 above).
    let terminal = engine
        .test_terminal_handle("OverlapUnion", ReprKind::Mesh, 1e-6)
        .expect(
            "terminal handle must be cached at (OverlapUnion, Mesh, 1e-6) \
             after build(ExportFormat::Stl) with a manufacturing purpose active",
        );
    assert_eq!(
        terminal.kernel,
        KernelId::Manifold,
        "terminal handle must be tagged KernelId::Manifold \
         (the BooleanUnion dispatches to the Mesh-capable Manifold kernel, \
         not the BRep-capable OCCT kernel); got {:?}",
        terminal.kernel
    );
}

// ── Item 3 probe A: kernel-direct real OCCT→Manifold path ──────────────────

/// Kernel-direct proof that real OCCT-tessellated meshes ingest into Manifold
/// via the bit-exact vertex weld (task #4329, `manifold_from_reify_mesh`).
///
/// Builds two PARTIALLY-overlapping 10×10×10 boxes via a real
/// `OcctKernelHandle`, tessellates each to a `Mesh`, ingests both into
/// `ManifoldKernel`, runs a boolean union, and re-tessellates the result.
/// Asserts the output Mesh has vertices (the union is non-empty) and well-
/// formed triangle indices (`len % 3 == 0`).
///
/// This is the load-bearing proof of the #4329 weld: pre-weld, real OCCT
/// tessellate() emits per-face un-welded vertices (box→24 vertices) and
/// manifold3d::from_mesh_f64 rejects the non-manifold mesh.  Post-weld the
/// 24 un-welded vertices collapse to 8 canonical corners and from_mesh_f64
/// succeeds.
#[test]
fn real_occt_tessellated_union_ingests_and_unions_through_manifold() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping real_occt_tessellated_union_ingests_and_unions_through_manifold: \
             OCCT not available"
        );
        return;
    }

    // Build two 10×10×10 OCCT BRep boxes with 50% X-overlap (dx=5).
    // Mirror the two_box_kernel fixture from interference_integration.rs.
    let occt = reify_kernel_occt::OcctKernelHandle::spawn();

    let box_a = occt
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_a creation must succeed");

    let box_b_raw = occt
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_b_raw creation must succeed");

    let box_b = occt
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 5.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate must succeed");

    // Tessellate: OCCT emits per-face un-welded vertices (box→24 before weld).
    // The manifold_from_reify_mesh weld (task #4329) reduces 24→8 so
    // from_mesh_f64 accepts the resulting closed-manifold mesh.
    let mesh_a = occt
        .tessellate(box_a.id, 0.1)
        .expect("tessellate box_a must succeed");
    let mesh_b = occt
        .tessellate(box_b.id, 0.1)
        .expect("tessellate box_b must succeed");

    // Ingest into ManifoldKernel — this exercises the #4329 weld path.
    let mut manifold = ManifoldKernel::new();
    let h_a = manifold
        .ingest_mesh(&mesh_a)
        .expect("real OCCT mesh_a must ingest into ManifoldKernel post-#4329 weld");
    let h_b = manifold
        .ingest_mesh(&mesh_b)
        .expect("real OCCT mesh_b must ingest into ManifoldKernel post-#4329 weld");

    // Run the boolean union.
    let u = manifold
        .execute(&GeometryOp::Union {
            left: h_a.id,
            right: h_b.id,
        })
        .expect("ManifoldKernel::execute(Union) must succeed on two ingested OCCT meshes");

    // Tessellate the result back to a Mesh.
    let out = manifold
        .tessellate(u.id, 0.0)
        .expect("tessellate of the union result must succeed");

    assert!(
        !out.vertices.is_empty(),
        "union output mesh must have vertices (non-empty result); \
         got 0 vertices — the boolean union produced a degenerate solid"
    );
    assert_eq!(
        out.indices.len() % 3,
        0,
        "union output mesh must have a multiple of 3 indices (well-formed triangles); \
         got {} indices",
        out.indices.len()
    );
}

// ── Item 3 probe B: concrete Manifold non-degeneracy ───────────────────────

/// Concrete manifold3d::Manifold non-degeneracy probe for a boolean union.
///
/// Builds two PARTIALLY-overlapping unit cubes via the `unit_cube_manifold`
/// test fixture, runs a boolean union, and asserts the standard non-degeneracy
/// conjuncts on the concrete `manifold3d::Manifold`:
///   `!is_empty && num_tri > 0 && volume > 0.0 && bounding_box.is_some()`
///
/// This probe is Manifold-only (no OCCT) and runs unconditionally.
/// Together with probe A it establishes the load-bearing binary claim
/// "a real Boolean produced a non-degenerate Manifold solid".
///
/// Mirrors `union_meshgl64_exposes_provenance_and_merge_pairing_invariant`
/// in `crates/reify-kernel-manifold/src/kernel.rs:1686-1695`.
#[test]
fn manifold_real_boolean_union_is_nondegenerate_solid() {
    use reify_kernel_manifold::test_fixtures::unit_cube_manifold;

    // Two unit cubes with 50% X-overlap: [0,1]³ and [0.5,1.5]×[0,1]×[0,1].
    let a = unit_cube_manifold([0.0_f32, 0.0, 0.0]);
    let b = unit_cube_manifold([0.5_f32, 0.0, 0.0]);

    let m = a.union(&b);

    assert!(
        !m.is_empty(),
        "union of two overlapping unit cubes must not be empty (is_empty=true \
         indicates an empty manifold — the Boolean produced no solid)"
    );
    assert!(
        m.num_tri() > 0,
        "union of two overlapping unit cubes must have > 0 triangles; got {}",
        m.num_tri()
    );
    assert!(
        m.volume() > 0.0,
        "union of two overlapping unit cubes must have positive volume; got {}",
        m.volume()
    );
    assert!(
        m.bounding_box().is_some(),
        "union of two overlapping unit cubes must have a bounding box \
         (bounding_box() returned None — the solid has no geometry)"
    );
}

// ── Item 4: kernel-seam γ — structured mesh-contract diagnostics (INV-GEO-1) ─

/// End-to-end observable-signal + regression guard for kernel-seam γ (task
/// 5104, PRD `kernel-seam-contracts.md` §4 site-2): `ManifoldKernel::ingest_mesh`
/// surfaces a structured `GeometryError::MeshContractViolation` for a
/// contract-violating mesh — diagnosed by `Mesh::validate` *before*
/// `Manifold::from_mesh_f64`'s generic `NotManifold` error — while a valid
/// mesh still ingests.
///
/// Unconditional (no OCCT gate): the violating mesh is constructed directly
/// by reversing one triangle's winding in the `unit_cube_mesh` fixture, so
/// this test needs no real OCCT tessellation.
///
/// 1. Negative half: clone `unit_cube_mesh([0,0,0])` and reverse the first
///    triangle's winding (swap its 2nd/3rd index, `(0, 2, 1)` -> `(0, 1, 2)`)
///    so that triangle's directed edges duplicate two neighboring faces'
///    edge directions instead of reversing them — a `ConsistentWinding`
///    violation (the PRD's canonical reversed-winding case). Asserts
///    `ingest_mesh` returns `Err(GeometryError::MeshContractViolation {
///    kernel: "manifold", counts.reversed_edges > 0, .. })`.
/// 2. Positive half ("valid unwelded OCCT still ingests"): the pristine
///    `unit_cube_mesh` fixture ingests as `Ok(_)` — kernel-seam γ's
///    validation (tol = 0.0) adds no rejections beyond `from_mesh_f64`'s
///    existing set.
///
/// RED on pre-γ main: the reversed cube used to return
/// `Err(GeometryError::OperationFailed(_))` (from_mesh_f64's generic
/// rejection) rather than the structured `MeshContractViolation`.
#[test]
fn manifold_ingest_contract_violating_mesh_yields_structured_diagnostic() {
    use reify_kernel_manifold::test_fixtures::unit_cube_mesh;

    // Reverse the first triangle's winding: (0, 2, 1) -> (0, 1, 2). Its
    // directed edges now duplicate two neighboring faces' edge directions
    // instead of reversing them, violating ConsistentWinding.
    let mut bad = unit_cube_mesh([0.0, 0.0, 0.0]);
    bad.indices.swap(1, 2);

    let result = ManifoldKernel::new().ingest_mesh(&bad);
    match result {
        Err(GeometryError::MeshContractViolation {
            kernel: kernel_name,
            counts,
            ..
        }) => {
            assert_eq!(
                kernel_name, "manifold",
                "MeshContractViolation must carry the producing kernel's name",
            );
            assert!(
                counts.reversed_edges > 0,
                "reversing one triangle's winding must be caught as a \
                 ConsistentWinding violation (reversed_edges > 0); got {counts:?}",
            );
        }
        other => panic!(
            "ingest_mesh of a reversed-winding cube must return \
             Err(GeometryError::MeshContractViolation {{ kernel: \"manifold\", .. }}); \
             got {other:?}"
        ),
    }

    // Positive half: the pristine (valid) cube still ingests successfully —
    // kernel-seam γ's validation adds no rejections beyond from_mesh_f64's
    // existing set.
    let ok = ManifoldKernel::new().ingest_mesh(&unit_cube_mesh([0.0, 0.0, 0.0]));
    assert!(
        ok.is_ok(),
        "a valid closed-orientable mesh must still ingest after wiring \
         Mesh::validate(); got {ok:?}",
    );
}

// ── Item 5: engine-build hardening κ — mixed-kernel attribute-resolved ──────
// selector (task 5071, INV-GEO-2 #4351 engine-build consumer-side boundary)

/// Shared build scaffold for the two `mixed_kernel_attribute_selector*` tests
/// below.
///
/// Performs, in order: the Manifold linker-anchor assertion, the
/// registry-contains-both-kernels checks, compiling
/// `examples/multi_kernel/attribute_selectors.ri`, injecting a
/// `manufacturing` purpose (`demanded_tol = Some(1e-6)`), and
/// `eval → activate_purpose → build(ExportFormat::Stl)`. Returns the compiled
/// module, the (post-build) engine — so callers can still read `snapshot()`,
/// `test_terminal_handle`, `topology_attribute_table()`, and
/// `tessellate_realizations` — and the `BuildResult`.
///
/// Callers MUST check `reify_kernel_occt::OCCT_AVAILABLE` and skip
/// (`eprintln!` + early `return`) BEFORE calling this helper: an early
/// `return` inside a helper only exits the helper, not the caller's `#[test]`
/// body, so the OCCT gate cannot be folded in here and stays at each call
/// site.
///
/// This still duplicates part of the scaffold in
/// `engine_routes_overlapping_box_union_to_manifold_mesh` above (lines
/// 60-130); generalizing that pre-existing test (landed under a different
/// task, #3437 ζ) to share this helper is out of this task's locked-module
/// scope and left for a follow-up.
fn build_mixed_kernel_attribute_selectors_fixture() -> (
    reify_compiler::CompiledModule,
    reify_eval::Engine,
    reify_eval::BuildResult,
) {
    // ── Linker anchor ────────────────────────────────────────────────────
    let anchor = reify_kernel_manifold::register::manifold_capability_descriptor();
    assert!(
        !anchor.supports.is_empty(),
        "manifold_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)"
    );

    // ── Registry contains both kernels ──────────────────────────────────
    let reg = reify_eval::kernel_registry::registry();
    assert!(
        reg.contains_key("occt"),
        "registry must contain \"occt\" after OCCT stub check; found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
    assert!(
        reg.contains_key("manifold"),
        "registry must contain \"manifold\" (linker anchor ensures the \
         inventory::submit! fired); found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );

    // ── Compile the fixture ──────────────────────────────────────────────
    // include_str! is a compile-time macro: if the fixture does not exist,
    // this file fails to compile → RED before step-2 creates it.
    let mut compiled = parse_and_compile_with_stdlib(include_str!(
        "../../../examples/multi_kernel/attribute_selectors.ri"
    ));
    // Belt-and-suspenders: parse_and_compile_with_stdlib already panics
    // internally on any error-severity diagnostic (reify-test-support
    // helpers.rs:390-395), so this can never observe a non-empty result here.
    // Kept as an explicit, self-documenting guard — matching the convention
    // at `engine_routes_overlapping_box_union_to_manifold_mesh` above — in
    // case that internal contract is ever relaxed.
    assert!(
        errors_only(&compiled).is_empty(),
        "attribute_selectors.ri must compile with no error-severity diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── Inject manufacturing purpose (demanded_tol = Some(1e-6)) ─────────
    // The RealizationCache is keyed by (entity, ReprKind, tol) and only
    // populates when demanded_tol = Some(..); test_terminal_handle needs that
    // cache entry. Mirrors engine_routes_overlapping_box_union_to_manifold_mesh
    // above.
    compiled
        .compiled_purposes
        .push(manufacturing_purpose("manufacturing", 1e-6));

    // ── Build with real OCCT + Manifold ──────────────────────────────────
    let mut engine = reify_eval::Engine::with_registered_kernels(Box::new(SimpleConstraintChecker));

    // eval() → activate_purpose → build() — the canonical pattern (build()'s
    // internal eval() clears active_purpose_bindings, so activate_purpose
    // MUST be called after the explicit eval() and before build()).
    let _eval = engine.eval(&compiled);
    engine.activate_purpose("manufacturing", "MixedKernelAttributeSelectors");
    let build = engine.build(&compiled, ExportFormat::Stl);

    (compiled, engine, build)
}

/// Engine-build CONSUMER-side boundary witness for INV-GEO-2 (#4351): pairs
/// the producer-facing `cross_kernel_attribute_collision_e2e.rs` tests.
///
/// Builds `examples/multi_kernel/attribute_selectors.ri` — an OCCT cylinder
/// (`post`, realization index 0, never terminal) coexisting with a
/// Mesh-demanded cross-kernel union (`body`, the terminal — highest-index —
/// realization) in one module — and asserts:
///
/// 1. no error-severity diagnostics anywhere in the build;
/// 2. the terminal union realization records `produced_repr == Mesh` and its
///    cached terminal handle is tagged `KernelId::Manifold`;
/// 3. the terminal union renders — `tessellate_realizations().meshes` carries
///    a non-empty mesh (vertices AND indices) at its entity path;
/// 4. per-kernel independence — the post-build `topology_attribute_table()`
///    holds AT LEAST one `KernelId::Occt` entry (seeded by the OCCT cylinder
///    / boxes) AND at least one `KernelId::Manifold` entry (the union's
///    ingest-forwarded attributes) — coexisting without one overwriting the
///    other, the cross-kernel `GeometryHandleId` collision class #4351
///    eliminates (pre-#4351 both kernels' attributes were addressable only
///    by a bare numeric id, so a same-numbered pair would collide onto one
///    table slot).
///
/// Does NOT flip the INV-GEO-2 registry row in `docs/invariants.md` — that
/// row flips only when kernel-seam-contracts lands its conformance property
/// tests; this test records the engine-build-side consumer share of that
/// invariant instead.
///
/// RED (before step-2 creates the fixture): `include_str!` of the
/// not-yet-created `examples/multi_kernel/attribute_selectors.ri` fails to
/// compile this test file (mirrors the RED convention documented at
/// `engine_routes_overlapping_box_union_to_manifold_mesh`, lines 97-98 above).
#[test]
fn mixed_kernel_attribute_selectors_builds_renders_and_reads_per_kernel_attributes() {
    // ── OCCT gate ─────────────────────────────────────────────────────────
    // Must run BEFORE calling the shared fixture helper below — see the
    // helper's doc-comment for why the gate cannot be folded into it.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mixed_kernel_attribute_selectors_builds_renders_and_reads_per_kernel_attributes: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let (compiled, mut engine, build) = build_mixed_kernel_attribute_selectors_fixture();

    // ── (a) no error-severity diagnostics anywhere in the build ───────────
    let build_errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        build_errors.is_empty(),
        "mixed-kernel attribute-selector build must not emit any error-severity \
         diagnostic; got: {build_errors:?}"
    );

    // ── (b) terminal union realization: produced_repr == Mesh ─────────────
    // RealizationNodeId is keyed by (entity, index) in a PersistentMap
    // (hash-ordered, not insertion-ordered); the terminal "body" binding has
    // the highest index (declared last in the fixture), so max_by_key(index)
    // always picks it regardless of map iteration order.
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == "MixedKernelAttributeSelectors")
        .collect();
    assert!(
        !nodes.is_empty(),
        "expected at least one realization node for entity \
         MixedKernelAttributeSelectors; got none"
    );
    nodes.sort_by_key(|(id, _)| id.index);
    let (terminal_id, terminal_node) = *nodes.last().expect("nodes is non-empty (asserted above)");
    let terminal_path = terminal_id.to_string();
    assert_eq!(
        terminal_node.produced_repr,
        ReprKind::Mesh,
        "the terminal (union) realization must record produced_repr == Mesh \
         (the cross-kernel union resolves to the Mesh-capable Manifold kernel); \
         got {:?}",
        terminal_node.produced_repr
    );

    // ── (b') terminal handle is KernelId::Manifold ─────────────────────────
    let terminal = engine
        .test_terminal_handle("MixedKernelAttributeSelectors", ReprKind::Mesh, 1e-6)
        .expect(
            "terminal handle must be cached at (MixedKernelAttributeSelectors, Mesh, 1e-6) \
             after build(ExportFormat::Stl) with a manufacturing purpose active",
        );
    assert_eq!(
        terminal.kernel,
        KernelId::Manifold,
        "terminal handle must be tagged KernelId::Manifold (the BooleanUnion \
         dispatches to the Mesh-capable Manifold kernel, not the BRep-capable \
         OCCT kernel); got {:?}",
        terminal.kernel
    );

    // ── (d) per-kernel independence (INV-GEO-2 consumer witness) ──────────
    // Read the POST-BUILD table before tessellate_realizations() below resets
    // it again (tessellate_realizations re-executes the whole module through
    // its own topology_attribute_table = TopologyAttributeTable::default()
    // reset — engine_build.rs:5586 — so it must not be read after that call).
    //
    // Non-vacuousness: an empty table on EITHER side would vacuously satisfy
    // the "no collision" claim for the wrong reason (a broken seeding/
    // forwarding path), not because there is genuinely no collision risk —
    // hence both counts are asserted >= 1, not just "not colliding".
    let table = engine.topology_attribute_table();
    let total_count = table.iter().count();
    let occt_count = table
        .iter()
        .filter(|(h, _)| h.kernel == KernelId::Occt)
        .count();
    let manifold_count = table
        .iter()
        .filter(|(h, _)| h.kernel == KernelId::Manifold)
        .count();
    assert!(
        occt_count >= 1,
        "expected topology_attribute_table to hold at least one KernelId::Occt \
         entry after the mixed-kernel build (seeded by the OCCT cylinder/boxes); \
         got 0 Occt entries (table len = {total_count})"
    );
    assert!(
        manifold_count >= 1,
        "expected topology_attribute_table to hold at least one \
         KernelId::Manifold entry after the mixed-kernel build (the union's \
         ingest-forwarded attributes); got 0 Manifold entries (table len = \
         {total_count}) — if Occt entries exist but Manifold doesn't, the \
         cross-kernel attribute-forwarding substrate (#4637) regressed"
    );

    // ── (c) renders — terminal mesh is non-empty (binary — no numeric bound)
    let tess = engine.tessellate_realizations(&compiled);
    let terminal_mesh = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a MeshSurface at entity_path \
                 {terminal_path:?}; got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !terminal_mesh.mesh.vertices.is_empty() && !terminal_mesh.mesh.indices.is_empty(),
        "terminal (union) mesh must be non-empty; got {} vertices, {} indices",
        terminal_mesh.mesh.vertices.len(),
        terminal_mesh.mesh.indices.len()
    );
}

/// Companion to
/// `mixed_kernel_attribute_selectors_builds_renders_and_reads_per_kernel_attributes`:
/// asserts the `@face("top")` ad-hoc selector against the OCCT cylinder
/// `post` resolves to `Value::Frame` on the BUILD path, inside the SAME
/// mixed-kernel module.
///
/// `eval_expr`'s `SelectorKind::Face` arm always leaves `top_frame` at
/// `Value::Undef` at eval time (no kernel is available then — the
/// documented deferral); resolution happens later, during `engine.build()`'s
/// `post_process_ad_hoc_selectors` → `try_eval_ad_hoc_selector` pass, which
/// is exactly what this test drives by reading `BuildResult.values` (mirrors
/// `engine_build_post_processes_ad_hoc_face_selector_to_frame` in
/// `ad_hoc_selector_smoke_tests.rs:442-490`). This is the consumer `@face`
/// read against an OCCT-scoped attribute inside a mixed-kernel build — the
/// companion assertion family to the dual-kernel/render/routing test above.
///
/// This test pays for its own full call to
/// `build_mixed_kernel_attribute_selectors_fixture()`, duplicating the real
/// OCCT+Manifold build that
/// `mixed_kernel_attribute_selectors_builds_renders_and_reads_per_kernel_attributes`
/// above already performs — deliberate, so each boundary (dual-kernel
/// table/render/routing vs. selector→Frame resolution) reports as its own
/// atomic, independently-committable `#[test]`. The extra build only runs in
/// the OCCT-gated integration suite (`OCCT_AVAILABLE`), never in the default
/// stub-mode build/test loop. A `OnceLock`-cached shared fixture would halve
/// the build cost if that suite's wall-clock ever becomes a concern.
///
/// RED (before step-4 adds `top_frame = post @ face("top")` to the
/// fixture): the example has no `top_frame` binding, so the
/// `BuildResult.values` lookup misses (`None`) and the `.unwrap_or_else`
/// below panics.
#[test]
fn mixed_kernel_attribute_selector_resolves_frame_on_build_path() {
    // ── OCCT gate ─────────────────────────────────────────────────────────
    // Must run BEFORE calling the shared fixture helper below — see the
    // helper's doc-comment for why the gate cannot be folded into it.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mixed_kernel_attribute_selector_resolves_frame_on_build_path: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let (_compiled, _engine, build) = build_mixed_kernel_attribute_selectors_fixture();

    let build_errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        build_errors.is_empty(),
        "mixed-kernel attribute-selector build must not emit any error-severity \
         diagnostic; got: {build_errors:?}"
    );

    // ── @face("top") resolves to Value::Frame (non-Undef) on the build path
    let top_frame_id = ValueCellId::new("MixedKernelAttributeSelectors", "top_frame");
    let top_frame_val = build.values.get(&top_frame_id).unwrap_or_else(|| {
        panic!(
            "MixedKernelAttributeSelectors.top_frame not found in build result \
             values (looked up {top_frame_id:?}); the fixture must declare \
             `let top_frame = post @ face(\"top\")` (task 5071 step-4)"
        )
    });
    assert!(
        matches!(top_frame_val, Value::Frame { .. }),
        "MixedKernelAttributeSelectors.top_frame should resolve to \
         Value::Frame {{ .. }} (non-Undef) after post_process_ad_hoc_selectors \
         wires @face(\"top\") against the OCCT cylinder `post`; got {:?}",
        top_frame_val
    );
}
