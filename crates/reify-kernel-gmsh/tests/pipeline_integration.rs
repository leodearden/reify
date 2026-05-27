//! Integration tests for the `mesh_volume` pipeline wrapper and its helpers.
//!
//! This file tests both the pure-Rust helpers (no `cfg(has_gmsh)` gate) and
//! the FFI-backed orchestrating wrapper (`#[cfg(has_gmsh)]` module).
//!
//! Pure-Rust helper tests run on every host — no libgmsh required.
//! Integration tests that call `mesh_surface_to_volume_with_diagnostics` are
//! inside the `with_libgmsh` module, gated with `#[cfg(has_gmsh)]`.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use reify_kernel_gmsh::MeshingOptions;
use reify_kernel_gmsh::auto_size::AutoSizeConfig;
use reify_kernel_gmsh::mesh_volume::{
    apply_repair_if_requested, compute_thickness_warnings, resolve_mesh_size,
};
use reify_kernel_gmsh::repair::RepairConfig;
use reify_kernel_gmsh::through_thickness::ThroughThicknessConfig;
use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

// ---------------------------------------------------------------------------
// Helpers shared across multiple tests in this file
// ---------------------------------------------------------------------------

/// A sliver-laden mesh: one good equilateral-ish triangle + one sliver
/// triangle (near-collinear vertices). Mirrors the mesh constructed in
/// `repair_tests.rs::sliver_triangles_below_area_threshold_are_collapsed`.
fn sliver_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            // Triangle 0 corners (equilateral-ish, area ~0.43)
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.5, 0.866, 0.0, // v2
            // Triangle 1 corners (collinear-ish, area ~5e-9 — sliver)
            5.0, 0.0, 0.0, // v3
            6.0, 0.0, 0.0, // v4
            5.5, 1e-8, 0.0, // v5
        ],
        indices: vec![
            0, 1, 2, // good triangle
            3, 4, 5, // sliver
        ],
        normals: None,
    }
}

// ---------------------------------------------------------------------------
// apply_repair_if_requested — None passes through, Some delegates
// ---------------------------------------------------------------------------

/// `apply_repair_if_requested` with `None` must return the input unchanged:
/// identical vertices and indices (Cow::Borrowed semantics — no copy, no repair).
#[test]
fn apply_repair_if_requested_none_passes_input_through() {
    let mesh = sliver_mesh();
    let result = apply_repair_if_requested(&mesh, None);
    assert_eq!(
        result.vertices, mesh.vertices,
        "None must pass vertices through unchanged"
    );
    assert_eq!(
        result.indices, mesh.indices,
        "None must pass indices through unchanged"
    );
}

/// `apply_repair_if_requested` with `Some(cfg)` must delegate to
/// `repair_surface_mesh` and return the repaired mesh (fewer indices because
/// the sliver triangle was dropped).
#[test]
fn apply_repair_if_requested_some_delegates_to_repair_surface_mesh() {
    let mesh = sliver_mesh();
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-6,
        vertex_merge_epsilon: 1e-9,
    };
    let result = apply_repair_if_requested(&mesh, Some(cfg));
    assert!(
        result.indices.len() < mesh.indices.len(),
        "Some(cfg) must invoke repair and drop the sliver triangle; \
         before: {} indices, after: {} indices",
        mesh.indices.len(),
        result.indices.len()
    );
    // The sliver is dropped — only one triangle (3 indices) survives.
    assert_eq!(
        result.indices.len(),
        3,
        "exactly one triangle should survive; got {} indices",
        result.indices.len()
    );
}

// ---------------------------------------------------------------------------
// resolve_mesh_size — caller-wins, auto-fires, none-defers
// ---------------------------------------------------------------------------

/// A unit cube surface mesh — 8 vertices, 12 triangles (2 per face).
/// Inline duplicate of `mesh_to_volume_tests.rs::unit_cube_mesh`.
fn unit_cube_mesh() -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            0, 2, 1,  0, 3, 2,
            4, 5, 6,  4, 6, 7,
            0, 1, 5,  0, 5, 4,
            3, 7, 6,  3, 6, 2,
            0, 4, 7,  0, 7, 3,
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

/// Caller's explicit `mesh_size` must win over the auto-derived value, even
/// when both are supplied. Pin: the caller-wins policy from the design decision.
#[test]
fn resolve_mesh_size_caller_value_wins_over_auto() {
    let cube = unit_cube_mesh();
    let options = MeshingOptions {
        mesh_size: Some(0.42),
        ..Default::default()
    };
    let result = resolve_mesh_size(&cube, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("caller-wins: must succeed");
    assert_eq!(
        size,
        Some(0.42),
        "caller's Some(0.42) must win over auto-size even when auto_size_cfg is Some"
    );
}

/// When both `mesh_size` and `auto_size_cfg` are `None`, the function must
/// return `Ok(None)` — deferring to `mesh_to_volume`'s internal default.
#[test]
fn resolve_mesh_size_no_caller_no_auto_returns_none() {
    let mesh = sliver_mesh();
    let options = MeshingOptions::default(); // mesh_size: None
    let result = resolve_mesh_size(&mesh, &options, None);
    let size = result.expect("none/none: must succeed");
    assert_eq!(
        size, None,
        "no caller override + no auto_size_cfg must return Ok(None)"
    );
}

/// When the caller's `mesh_size` is unset but `auto_size_cfg` is `Some`,
/// the function must call `auto_mesh_size_from_features` and return its result.
/// For a single triangle with all edges of length 0.5 and multiplier=1.0,
/// the auto-derived size is ≈ 0.5.
#[test]
fn resolve_mesh_size_auto_fires_when_caller_unset() {
    // Triangle with all edges exactly 0.5 m long.
    let mesh = Mesh {
        vertices: vec![
            0.0,
            0.0,
            0.0, // v0
            0.5,
            0.0,
            0.0, // v1 — edge v0→v1 = 0.5
            0.25,
            0.4330127_f32,
            0.0, // v2 — equilateral (approx, f32-precision √3/4)
        ],
        indices: vec![0, 1, 2],
        normals: None,
    };
    let options = MeshingOptions::default(); // mesh_size: None
    let result = resolve_mesh_size(&mesh, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("auto_size must succeed for a well-formed triangle");
    let size = size.expect("auto_size must return Some for a non-empty mesh");
    assert!(
        (size - 0.5).abs() < 0.01,
        "auto-derived size should be ≈ 0.5 (smallest edge length × 1.0 multiplier); got {size}"
    );
}

/// When `auto_size_cfg` fires but the mesh has no indices, `auto_mesh_size_from_features`
/// returns `Ok(0.0)`. The wrapper must collapse `0.0` to `None` (per design
/// decision: zero means "auto-size unavailable", defer to kernel default).
#[test]
fn resolve_mesh_size_empty_indices_collapses_to_none() {
    let mesh = Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        indices: vec![], // no triangles → auto returns 0.0
        normals: None,
    };
    let options = MeshingOptions::default();
    let result = resolve_mesh_size(&mesh, &options, Some(AutoSizeConfig::default()));
    let size = result.expect("empty-indices collapse: must succeed");
    assert_eq!(
        size, None,
        "auto returns 0.0 for empty-indices mesh; wrapper must collapse to Ok(None)"
    );
}

/// When `auto_mesh_size_from_features` fails (e.g. out-of-bounds index),
/// `resolve_mesh_size` must propagate the error as
/// `GeometryError::OperationFailed` with the prescribed prefix, and must NOT
/// emit a DEBUG event (the `if let Ok(...)` guard suppresses it on the error
/// path).
///
/// Regression guard for both halves of the error contract:
/// (a) the format-string wrapping prefix `"auto_mesh_size_from_features failed: "`;
/// (b) DEBUG event count == 0 on the error path.
#[test]
fn resolve_mesh_size_propagates_auto_size_error_and_suppresses_debug_event() {
    reify_test_support::prime_tracing_callsite_cache();

    // 3 vertices (9 floats) → n_vertices = 3. Index 99 is out of range;
    // auto_mesh_size_from_features returns Err(AutoSizeError::IndexOutOfBounds).
    // Pattern mirrors auto_size_tests.rs::out_of_bounds_index_returns_err.
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
        ],
        indices: vec![0, 1, 99],
        normals: None,
    };

    let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_kernel_gmsh::mesh_volume")
        .build();
    let debug_arc = Arc::clone(&counters[&tracing::Level::DEBUG]);

    let result = tracing::subscriber::with_default(subscriber, || {
        resolve_mesh_size(
            &mesh,
            &MeshingOptions::default(),
            Some(AutoSizeConfig::default()),
        )
    });

    // (a) Verify error variant and message prefix.
    match result {
        Err(GeometryError::OperationFailed(ref msg)) => {
            assert!(
                msg.contains("auto_mesh_size_from_features failed:"),
                "OperationFailed message must start with the prescribed prefix; \
                 got: {msg:?}"
            );
        }
        Err(other) => panic!(
            "expected Err(GeometryError::OperationFailed(...)) for out-of-bounds \
             index, but got a different GeometryError variant: {other:?}"
        ),
        Ok(v) => panic!(
            "expected Err(GeometryError::OperationFailed(...)) for out-of-bounds \
             index, but got Ok({v:?})"
        ),
    }

    // (b) Verify DEBUG event was suppressed on the error path.
    let debug_count = debug_arc.load(Ordering::Acquire);
    assert_eq!(
        debug_count, 0,
        "error path must suppress the DEBUG 'mesh_size resolved' event; got {debug_count}"
    );
}

// ---------------------------------------------------------------------------
// compute_thickness_warnings — None skips, Some delegates
// ---------------------------------------------------------------------------

/// Surface mesh of an axis-aligned 10×10×0.5 slab.
/// Inline duplicate of `through_thickness_tests.rs::slab_surface_mesh`.
fn slab_surface_mesh() -> Mesh {
    let v = vec![
        0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.5, 10.0, 0.0,
        0.5, 10.0, 10.0, 0.5, 0.0, 10.0, 0.5,
    ];
    let i = vec![
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6,
        3, 0, 4, 3, 4, 7,
    ];
    Mesh {
        vertices: v,
        indices: i,
        normals: None,
    }
}

/// Single-tet volume mesh that spans the slab thickness (mirrors the
/// fixture in `through_thickness_tests.rs::single_layer_tet_through_thin_region_emits_warning`).
fn single_tet_slab_volume() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            10.0, 0.0, 0.0, // 1
            10.0, 10.0, 0.5, // 2
            0.0, 10.0, 0.5, // 3
        ],
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
    }
}

/// `compute_thickness_warnings` with `None` must return an empty Vec
/// regardless of how thin the slab is — the stage is skipped entirely.
#[test]
fn compute_thickness_warnings_none_returns_empty() {
    let surface = slab_surface_mesh();
    let volume = single_tet_slab_volume();
    let warnings = compute_thickness_warnings(&volume, &surface, None);
    assert!(
        warnings.is_empty(),
        "None cfg must skip the through-thickness stage; got {} warning(s)",
        warnings.len()
    );
}

/// `compute_thickness_warnings` with `Some(cfg)` must delegate to
/// `through_thickness_check` and return warnings. For a single-tet slab the
/// layer count is 1, which is below the min_elements_through_thickness=2
/// default, so at least one warning is expected.
#[test]
fn compute_thickness_warnings_some_delegates_to_through_thickness_check() {
    let surface = slab_surface_mesh();
    let volume = single_tet_slab_volume();
    let warnings =
        compute_thickness_warnings(&volume, &surface, Some(ThroughThicknessConfig::default()));
    assert!(
        !warnings.is_empty(),
        "Some(cfg) must delegate to through_thickness_check; \
         single-tet slab should produce at least one warning"
    );
    assert_eq!(
        warnings[0].element_count, 1,
        "single-tet slab must be detected as 1-element-thick; got {}",
        warnings[0].element_count
    );
}

// ---------------------------------------------------------------------------
// repair pre-stage debug event observability
// ---------------------------------------------------------------------------

/// When `apply_repair_if_requested` is called with `Some(cfg)`, it must emit
/// exactly one DEBUG event at the `reify_kernel_gmsh::mesh_volume` target
/// (the "repair pre-stage applied" tracing event).
///
/// When called with `None`, no DEBUG event must be emitted.
/// Pins the task description's "emit a debug log when applied" requirement.
#[test]
fn repair_pre_stage_emits_debug_event_when_some_supplied() {
    // Prime the callsite cache so per-test with_default subscribers see events
    // even if a prior test thread hit the callsite with no subscriber active.
    reify_test_support::prime_tracing_callsite_cache();

    let mesh = sliver_mesh();

    // --- (a) Some(cfg): exactly 1 DEBUG event must be emitted ---
    let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_kernel_gmsh::mesh_volume")
        .build();
    let debug_arc = Arc::clone(&counters[&tracing::Level::DEBUG]);

    let _repaired = tracing::subscriber::with_default(subscriber, || {
        apply_repair_if_requested(
            &mesh,
            Some(RepairConfig {
                sliver_area_threshold: 1e-6,
                vertex_merge_epsilon: 1e-9,
            }),
        )
    });

    let debug_count = debug_arc.load(Ordering::Acquire);
    assert_eq!(
        debug_count, 1,
        "Some(cfg) must emit exactly 1 DEBUG event at \
         reify_kernel_gmsh::mesh_volume; got {debug_count}"
    );

    // --- (b) None: zero DEBUG events must be emitted ---
    let (subscriber_none, counters_none) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::DEBUG)
        .target_prefix("reify_kernel_gmsh::mesh_volume")
        .build();
    let debug_arc_none = Arc::clone(&counters_none[&tracing::Level::DEBUG]);

    let _unchanged = tracing::subscriber::with_default(subscriber_none, || {
        apply_repair_if_requested(&mesh, None)
    });

    let debug_count_none = debug_arc_none.load(Ordering::Acquire);
    assert_eq!(
        debug_count_none, 0,
        "None must emit 0 DEBUG events at \
         reify_kernel_gmsh::mesh_volume; got {debug_count_none}"
    );
}

// ---------------------------------------------------------------------------
// Inline DEBUG field-capturing subscriber (used by resolve_mesh_size tests)
// ---------------------------------------------------------------------------
// Mirrors the WarnCapturingSubscriber pattern from
// crates/reify-test-support/src/tracing_support.rs:603-689, narrowed to
// DEBUG level + a caller-supplied target prefix. Kept inline (not added to
// reify-test-support) because only one test in the workspace currently needs
// DEBUG-level field capture — see design decision (1) in the plan.

struct DebugFieldCapturingSubscriber {
    fields: Arc<Mutex<Vec<HashMap<String, String>>>>,
    target_prefix: &'static str,
    span_counter: AtomicU64,
}

struct DebugFieldVisitor {
    fields: HashMap<String, String>,
}

impl tracing::field::Visit for DebugFieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() != "message" {
            self.fields
                .insert(field.name().to_owned(), value.to_owned());
        }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() != "message" {
            self.fields
                .insert(field.name().to_owned(), format!("{value:?}"));
        }
    }
}

impl tracing::Subscriber for DebugFieldCapturingSubscriber {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        m.level() == &tracing::Level::DEBUG && m.target().starts_with(self.target_prefix)
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        // Initial value 1 ensures from_u64 never receives 0 (which panics).
        tracing::span::Id::from_u64(self.span_counter.fetch_add(1, Ordering::Relaxed))
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut v = DebugFieldVisitor {
            fields: HashMap::new(),
        };
        event.record(&mut v);
        self.fields.lock().unwrap().push(v.fields);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// ---------------------------------------------------------------------------
// resolve_mesh_size debug event observability
// ---------------------------------------------------------------------------

/// `resolve_mesh_size` must emit exactly one DEBUG event at the
/// `reify_kernel_gmsh::mesh_volume` target on each successful call,
/// asserting both the event count and the structured `source` field value per
/// branch (caller / kernel_default / auto).
///
/// Pins the structured diagnostic introduced by suggestion-3 ("mesh_size resolved"
/// event with `source` and `mesh_size` fields). The `source` field is the
/// critical regression detector: a bug that hard-codes `source="caller"` for
/// all branches would pass a count-only test but fail here.
///
/// `mesh_size` is intentionally NOT pinned — see design decision (2).
#[test]
fn resolve_mesh_size_emits_debug_event_recording_source_and_value() {
    reify_test_support::prime_tracing_callsite_cache();

    let cube = unit_cube_mesh();

    // --- (a) caller branch: mesh_size=Some(0.42) → source="caller" ---
    let fields_a: Arc<Mutex<Vec<HashMap<String, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let sub_a = DebugFieldCapturingSubscriber {
        fields: Arc::clone(&fields_a),
        target_prefix: "reify_kernel_gmsh::mesh_volume",
        span_counter: AtomicU64::new(1),
    };
    let result_a = tracing::subscriber::with_default(sub_a, || {
        resolve_mesh_size(
            &cube,
            &MeshingOptions {
                mesh_size: Some(0.42),
                ..Default::default()
            },
            None,
        )
    });
    assert!(result_a.is_ok(), "caller branch must succeed");
    {
        let events_a = fields_a.lock().unwrap();
        assert_eq!(
            events_a.len(),
            1,
            "caller branch must emit exactly 1 DEBUG event; got {}",
            events_a.len()
        );
        assert_eq!(
            events_a[0].get("source").map(|s| s.as_str()),
            Some("caller"),
            "caller branch: source field must be \"caller\"; fields={:?}",
            events_a[0]
        );
    }

    // --- (b) kernel_default branch: mesh_size=None, auto_cfg=None → source="kernel_default" ---
    let fields_b: Arc<Mutex<Vec<HashMap<String, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let sub_b = DebugFieldCapturingSubscriber {
        fields: Arc::clone(&fields_b),
        target_prefix: "reify_kernel_gmsh::mesh_volume",
        span_counter: AtomicU64::new(1),
    };
    let result_b = tracing::subscriber::with_default(sub_b, || {
        resolve_mesh_size(&cube, &MeshingOptions::default(), None)
    });
    assert!(result_b.is_ok(), "kernel_default branch must succeed");
    {
        let events_b = fields_b.lock().unwrap();
        assert_eq!(
            events_b.len(),
            1,
            "kernel_default branch must emit exactly 1 DEBUG event; got {}",
            events_b.len()
        );
        assert_eq!(
            events_b[0].get("source").map(|s| s.as_str()),
            Some("kernel_default"),
            "kernel_default branch: source field must be \"kernel_default\"; fields={:?}",
            events_b[0]
        );
    }

    // --- (c) auto branch: mesh_size=None, auto_cfg=Some → source="auto" ---
    // unit_cube_mesh() has non-zero auto-derived size → "auto" (not "auto_collapsed_to_kernel_default").
    let fields_c: Arc<Mutex<Vec<HashMap<String, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let sub_c = DebugFieldCapturingSubscriber {
        fields: Arc::clone(&fields_c),
        target_prefix: "reify_kernel_gmsh::mesh_volume",
        span_counter: AtomicU64::new(1),
    };
    let result_c = tracing::subscriber::with_default(sub_c, || {
        resolve_mesh_size(
            &cube,
            &MeshingOptions::default(),
            Some(AutoSizeConfig::default()),
        )
    });
    assert!(result_c.is_ok(), "auto branch must succeed");
    {
        let events_c = fields_c.lock().unwrap();
        assert_eq!(
            events_c.len(),
            1,
            "auto branch must emit exactly 1 DEBUG event; got {}",
            events_c.len()
        );
        assert_eq!(
            events_c[0].get("source").map(|s| s.as_str()),
            Some("auto"),
            "auto branch: source field must be \"auto\"; fields={:?}",
            events_c[0]
        );
    }

    // --- (d) auto_collapsed_to_kernel_default branch ---
    // mesh_size=None, auto_cfg=Some, but mesh has no indices → auto_mesh_size_from_features
    // returns Ok(0.0) → wrapper collapses to None → source="auto_collapsed_to_kernel_default".
    // Closes the coverage gap on the fourth source-taxonomy label (mesh_volume.rs match arm:
    // `(None, true, None) => "auto_collapsed_to_kernel_default"`).
    let empty_indices_mesh = Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        indices: vec![], // no triangles → auto_mesh_size_from_features returns Ok(0.0)
        normals: None,
    };
    let fields_d: Arc<Mutex<Vec<HashMap<String, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let sub_d = DebugFieldCapturingSubscriber {
        fields: Arc::clone(&fields_d),
        target_prefix: "reify_kernel_gmsh::mesh_volume",
        span_counter: AtomicU64::new(1),
    };
    let result_d = tracing::subscriber::with_default(sub_d, || {
        resolve_mesh_size(
            &empty_indices_mesh,
            &MeshingOptions::default(),
            Some(AutoSizeConfig::default()),
        )
    });
    assert!(
        result_d.is_ok(),
        "auto_collapsed_to_kernel_default branch must succeed"
    );
    {
        let events_d = fields_d.lock().unwrap();
        assert_eq!(
            events_d.len(),
            1,
            "auto_collapsed_to_kernel_default branch must emit exactly 1 DEBUG event; \
             got {}",
            events_d.len()
        );
        assert_eq!(
            events_d[0].get("source").map(|s| s.as_str()),
            Some("auto_collapsed_to_kernel_default"),
            "auto_collapsed_to_kernel_default branch: source field must be \
             \"auto_collapsed_to_kernel_default\"; fields={:?}",
            events_d[0]
        );
    }
}

// ---------------------------------------------------------------------------
// mesh_surface_to_volume_with_diagnostics — cfg(has_gmsh) wrapper tests
// ---------------------------------------------------------------------------

#[cfg(has_gmsh)]
mod with_libgmsh {
    use super::{slab_surface_mesh, unit_cube_mesh};

    use reify_kernel_gmsh::MeshingOptions;
    use reify_kernel_gmsh::auto_size::AutoSizeConfig;
    use reify_kernel_gmsh::mesh_volume::mesh_surface_to_volume_with_diagnostics;
    use reify_kernel_gmsh::repair::RepairConfig;
    use reify_kernel_gmsh::through_thickness::ThroughThicknessConfig;
    use reify_ir::ElementOrderTag;

    /// All diagnostic stages skipped (all `None` configs). Must produce a
    /// non-empty volume mesh and no through-thickness warnings.
    #[test]
    fn mesh_surface_to_volume_with_diagnostics_all_none_round_trips_unit_cube() {
        let cube = unit_cube_mesh();
        let report = mesh_surface_to_volume_with_diagnostics(
            &cube,
            &MeshingOptions::default(),
            ElementOrderTag::P1,
            None,
            None,
            None,
        )
        .expect("all-None wrapper must succeed for a closed unit cube");

        assert!(
            report.volume.tet_indices.len() / 4 > 0,
            "all-None wrapper must produce at least one tet; tet count = {}",
            report.volume.tet_indices.len() / 4
        );
        assert!(
            report.through_thickness_warnings.is_empty(),
            "all-None wrapper must produce no through-thickness warnings; got {:?}",
            report
                .through_thickness_warnings
                .iter()
                .map(|w| &w.message)
                .collect::<Vec<_>>()
        );
    }

    /// Caller's explicit `mesh_size` wins over the auto-derived value, observable
    /// via tet count: a finer mesh_size (0.25) produces more tets than auto (≈1.0).
    ///
    /// Three runs:
    /// - A: `mesh_size=None` + `auto_size_cfg=Some(...)` → auto fires, coarse mesh.
    /// - B: `mesh_size=Some(0.25)` + `auto_size_cfg=Some(...)` → caller wins, fine mesh.
    /// - C: `mesh_size=Some(0.25)` + `auto_size_cfg=None` → caller wins (no auto), fine mesh.
    ///
    /// `tets_b > tets_a` pins relative density; `tets_b == tets_c` pins caller-wins
    /// independently of HXT refinement monotonicity — C isolates the policy without
    /// relying on auto_size being a different value than the caller's size.
    #[test]
    fn caller_mesh_size_wins_over_auto_size_observable_in_tet_count() {
        let cube = unit_cube_mesh();

        // Run A: no explicit mesh_size — auto fires, derives ≈ 1.0 → coarse mesh.
        let report_a = mesh_surface_to_volume_with_diagnostics(
            &cube,
            &MeshingOptions {
                mesh_size: None,
                ..Default::default()
            },
            ElementOrderTag::P1,
            None,
            Some(AutoSizeConfig::default()),
            None,
        )
        .expect("run A (auto-size) must succeed for a closed unit cube");

        // Run B: explicit mesh_size=0.25 wins over auto → fine mesh.
        // deterministic=true ensures HXT produces a reproducible tet count so that
        // the tets_b == tets_c comparison below is not fragile across runs.
        let report_b = mesh_surface_to_volume_with_diagnostics(
            &cube,
            &MeshingOptions {
                mesh_size: Some(0.25),
                deterministic: true,
                ..Default::default()
            },
            ElementOrderTag::P1,
            None,
            Some(AutoSizeConfig::default()),
            None,
        )
        .expect("run B (caller-wins) must succeed for a closed unit cube");

        // Run C: same mesh_size=0.25 but auto_size_cfg=None → auto is not supplied.
        // tets_b == tets_c proves the caller-wins policy independently: supplying
        // auto_size_cfg=Some(...) alongside mesh_size=Some(0.25) must produce the
        // same mesh as not supplying auto at all.
        // deterministic=true must match run B's options so the gmsh state is identical.
        let report_c = mesh_surface_to_volume_with_diagnostics(
            &cube,
            &MeshingOptions {
                mesh_size: Some(0.25),
                deterministic: true,
                ..Default::default()
            },
            ElementOrderTag::P1,
            None,
            None, // no auto_size_cfg — pure caller-explicit path
            None,
        )
        .expect("run C (caller-explicit, no auto) must succeed for a closed unit cube");

        let tets_a = report_a.volume.tet_indices.len() / 4;
        let tets_b = report_b.volume.tet_indices.len() / 4;
        let tets_c = report_c.volume.tet_indices.len() / 4;

        assert!(
            tets_b > tets_a,
            "finer mesh_size=0.25 (run B, {} tets) must produce more tets than \
             auto-derived ≈1.0 (run A, {} tets); caller-wins policy not observed",
            tets_b,
            tets_a
        );
        assert_eq!(
            tets_b, tets_c,
            "run B (caller=0.25, auto=Some) and run C (caller=0.25, auto=None) must \
             produce identical tet counts ({} vs {}); auto_size_cfg must be ignored \
             when caller's mesh_size is set",
            tets_b, tets_c
        );
    }

    /// All three diagnostic stages active on a coarsely-meshed thin slab (0.5 m
    /// thick, mesh_size=5.0) must produce at least one through-thickness warning.
    /// Pins: post-stage fires when Some(cfg) supplied; detected thickness ≈ 0.5.
    #[test]
    fn pipeline_all_stages_active_emits_through_thickness_warning_on_under_resolved_slab() {
        let slab = slab_surface_mesh();
        // Large mesh_size ensures HXT produces very few layers through 0.5 m slab.
        let options = MeshingOptions {
            mesh_size: Some(5.0),
            deterministic: true,
            ..Default::default()
        };
        let report = mesh_surface_to_volume_with_diagnostics(
            &slab,
            &options,
            ElementOrderTag::P1,
            Some(RepairConfig::default()),
            Some(AutoSizeConfig::default()),
            Some(ThroughThicknessConfig::default()),
        )
        .expect("all-active pipeline must succeed for a closed slab");

        assert!(
            report.volume.tet_indices.len() / 4 > 0,
            "all-active pipeline must produce at least one tet"
        );
        assert!(
            !report.through_thickness_warnings.is_empty(),
            "coarse slab (mesh_size=5.0, thickness=0.5) must produce at least one \
             through-thickness warning; got none"
        );
        // Under-resolution: element count below 2 (the min_elements default)
        assert!(
            report.through_thickness_warnings[0].element_count < 2,
            "under-resolved slab must be detected; element_count={}, expected <2",
            report.through_thickness_warnings[0].element_count
        );
        // Thickness detection: approximately 0.5 m (within ±1e-3)
        let detected_thickness = report.through_thickness_warnings[0].thickness;
        assert!(
            (detected_thickness - 0.5).abs() < 1e-3,
            "detected thickness must be ≈ 0.5 m (within ±1e-3); got {detected_thickness}"
        );
    }
}
