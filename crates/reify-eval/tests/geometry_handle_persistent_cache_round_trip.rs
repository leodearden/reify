//! Cross-Engine in-session cache-KEY stability integration tests (GHR-ε).
//!
//! # Scope note (esc-3607-59 relaxation, Leo-ratified)
//!
//! The original spec for this file was a full restart→persistent-cache-hit
//! round-trip test.  That premise was false: no on-disk geometry persistence
//! exists yet (`RealizationCache` is an in-memory per-Engine `HashMap`), and
//! the full disk round-trip — including `PersistentlyCacheable`-for-geometry,
//! the Engine cache-dir constructor, and the `examples/spec-shape-physical.ri`
//! fixture — is re-homed to **GHR-ζ** (task 3608+).
//!
//! Under the esc-3607-59 relaxation the deliverable is narrowed to
//! **in-session cross-Engine cache-key stability** (no disk I/O):
//!
//! - Build the same source in two fresh, independent `Engine` instances.
//! - Assert the `Widget.body` `Value::GeometryHandle` from each has
//!   **byte-identical `content_hash()`** — this IS the in-memory cache key
//!   (`NodeCache.result_hash`), so the assertion directly validates that the
//!   key is stable across Engine restarts.
//! - Assert `PartialEq` holds too (kernel_handle excluded by GHR-β §DD).
//! - Assert that changing a parameter (`box(20mm,…)` vs `box(10mm,…)`)
//!   produces a **different** cache key — upstream_values_hash changed →
//!   correct cache invalidation.
//!
//! The filename is kept as `geometry_handle_persistent_cache_round_trip.rs`
//! for phantom-done gate traceability; this doc-comment explains the scope.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// Source that produces a `Widget.body` GeometryHandle via `box(10mm,20mm,30mm)`.
const SOURCE_10: &str = r#"structure def Widget {
    param body : Solid = box(10mm, 20mm, 30mm)
}"#;

/// Variant with the first dimension changed to 20mm — different upstream hash.
const SOURCE_20: &str = r#"structure def Widget {
    param body : Solid = box(20mm, 20mm, 30mm)
}"#;

/// Build `source` in a fresh `Engine` with a fresh `MockGeometryKernel` and
/// return the `Widget.body` value.
fn build_and_get_body(source: &str) -> Value {
    let compiled = compile_source(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time errors; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time errors; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    result.values.get_or_undef(&ValueCellId::new("Widget", "body"))
}

/// Cross-Engine cache-key stability: the same source compiled and built in two
/// independent Engine instances must produce byte-identical `content_hash()`
/// values for `Widget.body`.
///
/// This validates that the in-memory cache key (= `content_hash()`) is stable
/// across Engine restarts: it derives purely from `realization_ref` + `upstream_values_hash`,
/// both of which are deterministic from the source text, with `kernel_handle`
/// excluded (per GHR-β §DD / geometry-handle-runtime.md §6).
///
/// Compiling the same source string TWICE faithfully models a restart re-parsing
/// the file and confirms realization-index determinism.  Two independent Engine
/// instances confirm the key is stable WITHOUT shared Engine state.
#[test]
fn cross_engine_geometry_handle_cache_key_is_stable() {
    // Compile the same source twice (models restart re-parse; realization indices
    // are compile-deterministic so we get the same RealizationNodeId).
    let value_a = build_and_get_body(SOURCE_10);
    let value_b = build_and_get_body(SOURCE_10);

    // Both must be GeometryHandle, not Undef.
    assert!(
        matches!(value_a, Value::GeometryHandle { .. }),
        "Engine A: expected Value::GeometryHandle for Widget.body, got {:?}",
        value_a
    );
    assert!(
        matches!(value_b, Value::GeometryHandle { .. }),
        "Engine B: expected Value::GeometryHandle for Widget.body, got {:?}",
        value_b
    );

    // Byte-identical cache key across independent Engine instances.
    assert_eq!(
        value_a.content_hash(),
        value_b.content_hash(),
        "in-memory cache key (content_hash) must be byte-identical across Engine instances \
         for the same source — key must be stable under restart (no shared Engine state)",
    );

    // PartialEq also holds (kernel_handle excluded per GHR-β §DD).
    assert_eq!(
        value_a, value_b,
        "GeometryHandle PartialEq must hold across Engine instances \
         (kernel_handle excluded; only realization_ref + upstream_values_hash compared)",
    );
}

/// Changing a box dimension changes `upstream_values_hash`, which must produce
/// a DIFFERENT cache key — verifying that cache invalidation fires when
/// semantically-different geometry is built.
#[test]
fn changed_dimension_produces_different_cache_key() {
    let value_10 = build_and_get_body(SOURCE_10);
    let value_20 = build_and_get_body(SOURCE_20);

    // Both must be GeometryHandle.
    assert!(
        matches!(value_10, Value::GeometryHandle { .. }),
        "box(10mm…): expected Value::GeometryHandle, got {:?}",
        value_10
    );
    assert!(
        matches!(value_20, Value::GeometryHandle { .. }),
        "box(20mm…): expected Value::GeometryHandle, got {:?}",
        value_20
    );

    // Different upstream_values_hash → different cache key.
    assert_ne!(
        value_10.content_hash(),
        value_20.content_hash(),
        "changing box dimension must change upstream_values_hash and \
         therefore produce a DIFFERENT in-memory cache key (cache invalidation must fire)",
    );
}
