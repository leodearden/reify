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
use reify_core::{DiagnosticCode, Severity};
use reify_ir::{ExportFormat, GeometryKernel, GeometryOp, KernelId, ReprKind, Value};
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
            d.code == Some(DiagnosticCode::NoKernelChain)
                && matches!(d.severity, Severity::Error)
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
    let overlap_union_node = snap
        .graph
        .realizations
        .iter()
        .find(|(id, _)| id.entity == "OverlapUnion")
        .map(|(_, r)| r)
        .expect(
            "OverlapUnion realization node must be present in the snapshot \
             graph after build(ExportFormat::Stl)",
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
