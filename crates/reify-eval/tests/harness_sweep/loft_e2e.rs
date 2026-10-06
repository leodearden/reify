//! End-to-end acceptance for `loft()` from source (task #6188): a circle lofted
//! to a `translate()`d circle compiles clean and realizes, on the real OCCT
//! kernel, the conical frustum the two sections bound. Guards both halves —
//! transform kind inference in the compiler and face-profile sections in OCCT.

use reify_core::{DimensionVector, ModulePath, Severity};

const LOFT_FRUSTUM_SOURCE: &str = r#"structure LoftFrustum {
    let g = loft(circle(500mm), translate(circle(250mm), 0mm, 0mm, 800mm))
    let v = volume(g)
}"#;

/// Parse + compile [`LOFT_FRUSTUM_SOURCE`], asserting both stages are free of
/// Error diagnostics (no `GeometryProfileRequired` on the translated circle).
fn compile_loft_frustum() -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(LOFT_FRUSTUM_SOURCE, ModulePath::single("loft_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {compile_errors:?}"
    );
    compiled
}

/// Extent `max - min` of mesh coordinate `axis` (0 = x, 2 = z), in metres.
fn mesh_extent(vertices: &[f32], axis: usize) -> f64 {
    let (lo, hi) = vertices
        .chunks_exact(3)
        .map(|v| v[axis] as f64)
        .fold((f64::MAX, f64::MIN), |(lo, hi), c| (lo.min(c), hi.max(c)));
    hi - lo
}

fn assert_within(actual: f64, expected: f64, rel_tol: f64, what: &str) {
    let rel = (actual - expected).abs() / expected;
    assert!(
        rel < rel_tol,
        "{what}: got {actual}, expected {expected} within {:.1}% (off by {:.3}%)",
        rel_tol * 100.0,
        rel * 100.0
    );
}

/// Compile side only — no OCCT guard, so a missing kernel cannot skip it.
#[test]
fn loft_frustum_source_compiles_clean() {
    compile_loft_frustum();
}

#[test]
fn loft_between_translated_circle_profiles_realizes_a_frustum() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let compiled = compile_loft_frustum();

    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(planner)),
    );

    let result = engine.tessellate_realizations(&compiled);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors lofting: {errors:?}");

    assert_eq!(
        result.meshes.len(),
        1,
        "expected 1 mesh (single realization), got {}",
        result.meshes.len()
    );
    let mesh = &result.meshes[0].mesh;
    assert!(
        !mesh.vertices.is_empty(),
        "lofted mesh should have vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "lofted mesh should have triangles"
    );
    assert_within(
        mesh_extent(&mesh.vertices, 2),
        0.8,
        0.005,
        "mesh z-extent (m)",
    );
    assert_within(
        mesh_extent(&mesh.vertices, 0),
        1.0,
        0.02,
        "mesh x-extent (m)",
    );

    let volume = super::design_fixture::entity_cell(
        &result.values,
        "<inline loft source>",
        "LoftFrustum",
        "v",
        DimensionVector::VOLUME,
    );
    let (r1, r2, h) = (0.5, 0.25, 0.8);
    let frustum = std::f64::consts::PI * h * (r1 * r1 + r1 * r2 + r2 * r2) / 3.0;
    assert_within(volume, frustum, 0.01, "volume(g) (m³)");
}
