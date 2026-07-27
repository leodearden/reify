//! Task #3443 (ο) — `#kernel(...)` pragma propagation integration gate.
//!
//! Proves that `engine.build()` honours a module-level `#kernel(occt)` pragma
//! by routing the terminal `BooleanUnion` to "occt" instead of the lex-min
//! default "manifold" — the two synthetic kernels in this test's registry both
//! support `(BooleanUnion, BRep)`, so without the pragma "manifold" (m < o)
//! always wins.
//!
//! ## Fixture
//!
//! `examples/multi_kernel/pragma_override.ri` contains `#kernel(occt)` and a
//! two-box BRep union (PragmaOverrideUnion).  The fixture is deliberately
//! BRep-terminal (union of two BRep boxes at BRep demand) so no cross-kernel
//! conversion stage is needed — both "manifold" and "occt" are plain BRep
//! boolean kernels in this test's synthetic registry.
//!
//! ## RED → GREEN
//!
//! RED before S4: `build()` ignores `module.kernel_pragma` (not yet threaded
//! through `execute_realization_ops`). Lex-min picks "manifold" for the union,
//! so the pragma-assert fires (assertion: last op routed to "occt").
//!
//! GREEN after S4: `build()` reads `module.kernel_pragma.as_deref()` and
//! passes it as `prefer_kernel` down to `execute_realization_ops` → "occt" is
//! preferred over lex-min "manifold" for the union.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_ir::{CapabilityDescriptor, ExportFormat, Operation, ReprKind};
use reify_test_support::{errors_only, manufacturing_purpose, parse_and_compile_with_stdlib};

/// Name-recording kernel: delegates `execute` / `query` / `export` /
/// `tessellate` to an inner mock and pushes its own `name` onto a shared log
/// on every `execute()` call (which handles every non-conversion op —
/// primitives, booleans, transforms). Mirrors the in-crate `NamedRecordingKernel`
/// (engine_build.rs::tests) which is private to that test module.
struct NamedRecordingKernel {
    name: String,
    inner: reify_test_support::mocks::MockGeometryKernel,
    log: Arc<Mutex<Vec<String>>>,
}

impl reify_ir::GeometryKernel for NamedRecordingKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        self.log.lock().unwrap().push(self.name.clone());
        self.inner.execute(op)
    }

    fn query(
        &self,
        q: &reify_ir::GeometryQuery,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.query(q)
    }

    fn export(
        &self,
        handle: reify_ir::GeometryHandleId,
        format: reify_ir::ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: reify_ir::GeometryHandleId,
        tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        self.inner.tessellate(handle, tolerance)
    }
}

/// Build a synthetic `{manifold, occt}` registry where both kernels support
/// `(PrimitiveBox, BRep)`, `(TransformTranslate, BRep)`, and `(BooleanUnion, BRep)`.
///
/// `TransformTranslate` is required because the fixture uses `translate(box_b_raw, …)`.
/// Both kernels share the same capability set so lex-min ("manifold" < "occt") picks
/// "manifold" on every op without a pragma, while `#kernel(occt)` steers the terminal
/// `BooleanUnion` to "occt". Used by both the pragma and no-pragma test scenarios.
fn build_test_registry() -> BTreeMap<String, CapabilityDescriptor> {
    let desc = CapabilityDescriptor {
        supports: vec![
            (Operation::PrimitiveBox, ReprKind::BRep),
            (Operation::TransformTranslate, ReprKind::BRep),
            (Operation::BooleanUnion, ReprKind::BRep),
        ],
    };
    let mut registry = BTreeMap::new();
    registry.insert("manifold".to_string(), desc.clone());
    registry.insert("occt".to_string(), desc);
    registry
}

/// Build a `{manifold, occt}` kernels map using `NamedRecordingKernel`s
/// sharing the provided `log`. Both kernels are backed by `MockGeometryKernel`.
fn build_test_kernels(
    log: Arc<Mutex<Vec<String>>>,
) -> BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> {
    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert(
        "manifold".to_string(),
        Box::new(NamedRecordingKernel {
            name: "manifold".to_string(),
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            log: Arc::clone(&log),
        }),
    );
    kernels.insert(
        "occt".to_string(),
        Box::new(NamedRecordingKernel {
            name: "occt".to_string(),
            inner: reify_test_support::mocks::MockGeometryKernel::new(),
            log: Arc::clone(&log),
        }),
    );
    kernels
}

/// `#kernel(occt)` pragma in the fixture must steer the terminal `BooleanUnion`
/// to "occt" even though "manifold" (m < o) would win lex-min without it.
///
/// Assertion: the last `execute()` call in the build — which handles the union
/// — is recorded on "occt" (not "manifold").
///
/// RED before S4: `build()` does not thread `module.kernel_pragma`, so lex-min
/// picks "manifold" for the union and the assertion fires.
#[test]
fn build_step_routes_union_to_pragma_kernel_occt() {
    let mut compiled = parse_and_compile_with_stdlib(include_str!(
        "../../../examples/multi_kernel/pragma_override.ri"
    ));

    // Verify the fixture compiled without errors and that the pragma was parsed.
    assert!(
        errors_only(&compiled).is_empty(),
        "pragma_override.ri must compile without error diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );
    assert_eq!(
        compiled.kernel_pragma.as_deref(),
        Some("occt"),
        "pragma_override.ri must produce kernel_pragma = Some(\"occt\"); \
         got {:?} — check that #kernel(occt) is present and parseable",
        &compiled.kernel_pragma,
    );

    // Inject a manufacturing purpose so demanded_tol = Some(1e-6) (required
    // for test_terminal_handle to populate the RealizationCache).
    compiled
        .compiled_purposes
        .push(manufacturing_purpose("manufacturing", 1e-6));

    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(SimpleConstraintChecker),
        build_test_kernels(Arc::clone(&log)),
        build_test_registry(),
        Some("manifold".to_string()),
    );

    let _eval = engine.eval(&compiled);
    engine.activate_purpose("manufacturing", "PragmaOverrideUnion");
    let build = engine.build(&compiled, ExportFormat::Step);

    // No error diagnostics.
    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build must not emit error diagnostics; got: {errors:#?}"
    );

    // The last execute() call must be on "occt" (the pragma-steered terminal
    // union).  Primitives can route to either kernel (lex-min applies); only
    // the terminal binary boolean op is pragma-steered.
    let calls = log.lock().unwrap().clone();
    assert!(
        !calls.is_empty(),
        "at least one execute() call must be recorded; build may have failed silently"
    );
    assert_eq!(
        calls.last().map(|s| s.as_str()),
        Some("occt"),
        "terminal BooleanUnion must route to pragma-preferred 'occt', not lex-min \
         'manifold'; execute() call log: {calls:?}"
    );
}

/// Control: an identical-geometry source WITHOUT `#kernel(occt)` must route
/// to "manifold" (lex-min, m < o). Proves that the pragma_override fixture is
/// the causal factor for the occt routing in the sibling test above.
#[test]
fn build_step_routes_union_to_lexmin_manifold_without_pragma() {
    // Inline source: same two-box BRep union as the fixture but NO pragma.
    let source = r#"
structure PragmaOverrideUnion {
    param offset : Length = 5mm
    let box_a = box(10mm, 10mm, 10mm)
    let box_b_raw = box(10mm, 10mm, 10mm)
    let box_b = translate(box_b_raw, offset, 0mm, 0mm)
    let body = union(box_a, box_b)
}
"#;

    let mut compiled = parse_and_compile_with_stdlib(source);

    assert!(
        errors_only(&compiled).is_empty(),
        "control source must compile without error diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );
    assert_eq!(
        compiled.kernel_pragma.as_deref(),
        None,
        "control source must have kernel_pragma = None (no #kernel pragma)"
    );

    compiled
        .compiled_purposes
        .push(manufacturing_purpose("manufacturing", 1e-6));

    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(SimpleConstraintChecker),
        build_test_kernels(Arc::clone(&log)),
        build_test_registry(),
        Some("manifold".to_string()),
    );

    let _eval = engine.eval(&compiled);
    engine.activate_purpose("manufacturing", "PragmaOverrideUnion");
    let build = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "control build must not emit error diagnostics; got: {errors:#?}"
    );

    let calls = log.lock().unwrap().clone();
    assert!(
        !calls.is_empty(),
        "at least one execute() call must be recorded"
    );
    assert_eq!(
        calls.last().map(|s| s.as_str()),
        Some("manifold"),
        "no pragma: terminal BooleanUnion must route to lex-min 'manifold'; \
         calls: {calls:?}"
    );
}
