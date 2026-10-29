//! OpenCASCADE geometry kernel implementation for Reify.
//!
//! Provides two public types:
//!
//! - [`OcctKernel`] — the raw kernel, `!Send + !Sync` (contains `cxx::UniquePtr`).
//!   Useful for single-threaded test scenarios where channel overhead is unwanted.
//!
//! - [`OcctKernelHandle`] — a `Send + Sync` handle that communicates with a
//!   dedicated OS thread owning an `OcctKernel`. Implements [`GeometryKernel`]
//!   and is the recommended API for all production and cross-thread usage.
//!
//! [`GeometryKernel`]: reify_types::GeometryKernel

/// Whether OCCT libraries were found at build time.
///
/// This constant is `true` when the build detected OCCT include/lib
/// directories, and `false` otherwise (stub types are used instead).
/// Downstream crates can check this to skip OCCT-dependent tests
/// at runtime.
///
/// **This constant is never evidence that anything was tested.** When
/// `cfg!(has_occt)` is false, this crate's `#[cfg(all(test, has_occt))]`
/// modules and its `#![cfg(has_occt)]` integration binaries are not compiled
/// at all — the suite reports zero tests REPORTED, not zero tests FAILED, and
/// nothing downstream can tell the difference from a passing run. So a
/// runtime check here cannot guard build-time OCCT presence; it can only ever
/// be tautological (inside a `has_occt` module) or destructive (outside one,
/// where it would break the sanctioned OCCT-free stub build whose contract
/// suite lives in `stubs.rs`). Task 6343 deleted the const-assert that tried.
///
/// Build-time OCCT presence is gated instead by the OCCT arm of
/// `scripts/check-manifold-deps.sh`, which `scripts/verify.sh` emits as a plan
/// entry on every `RUN_RUST=1` verify — before any compile, where it cannot be
/// cfg'd away. That arm also pins the resolved SONAME and records it in the
/// verify log. Its guard is `tests/infra/test_occt_deps_preflight.sh`.
pub const OCCT_AVAILABLE: bool = cfg!(has_occt);

// Re-export register::OCCT_KERNEL_VERSION at the crate root so downstream
// crates (e.g. kernel_version_enforcement integration tests) can reference
// the compiled-in native version string without reaching into the sub-module.
// Placed next to OCCT_AVAILABLE for discoverability.
pub use register::OCCT_KERNEL_VERSION;

#[cfg(has_occt)]
#[allow(dead_code)]
mod ffi;
#[cfg(all(test, has_occt))]
mod ffi_contract_tests;
#[cfg(has_occt)]
pub use ffi::ffi::TopologyCacheBuildCounts;

/// Zero the calling thread's boolean-op-pass count (task 5213).
///
/// Incremented once per completed OCCT boolean `Build()`, at the single
/// `Build()` site every boolean goes through: the binary fuse/cut/common ops,
/// their `*_with_history` siblings (the production realization path) and the
/// single-pass `fuse_shape_list`.  Exposed so
/// tests can assert that a K-instance pattern performs exactly ONE boolean
/// pass rather than K−1 — a deterministic, non-flaky signal for the O(N²)→
/// single-pass change.
///
/// This is a TEST-OBSERVABILITY hook, not a progress-reporting one.  Do not
/// plumb it into GUI/CLI progress reporting: those paths drive OCCT through
/// [`OcctKernelHandle`]'s dedicated worker thread (see the per-thread scope
/// below), so a reporter on any other thread reads a permanent 0.  A
/// cross-thread progress signal would need a separate process-global
/// aggregate, deliberately not added here because nothing consumes one today.
///
/// The counter is PER-THREAD: each thread has its own count, so a
/// reset→operate→read window on one thread needs no serialization against
/// other threads, and this reset zeroes only the calling thread's count.
///
/// The count therefore reflects only booleans performed ON THE CALLING THREAD.
/// Work driven through [`OcctKernelHandle`], which owns a dedicated OCCT worker
/// thread (see `src/handle.rs`), lands in that worker's count and is NOT
/// observable from the caller — use [`OcctKernel`] directly when reading the
/// counter.
#[cfg(has_occt)]
pub fn reset_boolean_pass_count() {
    ffi::ffi::reset_boolean_pass_count();
}

/// Read the calling thread's count of completed OCCT boolean passes.
///
/// See [`reset_boolean_pass_count`] for the increment contract and for the
/// per-thread scope (in particular, why [`OcctKernelHandle`]-driven work is not
/// visible here).
#[cfg(has_occt)]
pub fn boolean_pass_count() -> u64 {
    ffi::ffi::boolean_pass_count()
}

/// Stub boolean-op-pass counter reset (OCCT not available) — no-op.
#[cfg(not(has_occt))]
pub fn reset_boolean_pass_count() {}

/// Stub boolean-op-pass counter read (OCCT not available) — always 0.
#[cfg(not(has_occt))]
pub fn boolean_pass_count() -> u64 {
    0
}

#[cfg(all(has_occt, feature = "test-fixtures"))]
mod boolean_parallelism;
#[cfg(all(has_occt, feature = "test-fixtures"))]
#[doc(hidden)]
pub use boolean_parallelism::{
    BooleanParallelism, boolean_parallelism, parallel_bop_build_count, with_boolean_parallelism,
};
#[cfg(has_occt)]
mod step_document;
#[cfg(all(has_occt, feature = "test-fixtures"))]
#[doc(hidden)]
pub use step_document::xstep_cascade_unit_for_test;
#[cfg(has_occt)]
pub use step_document::{StepBodyError, StepDocument, StepReadError};
// Re-export the result type so callers using the test-fixture wrapper below
// can name it without reaching into the private bridge module.
#[cfg(has_occt)]
pub use ffi::ffi::RevolveSynthesisPostSortResult;

// Same re-export rationale for the #6344 STEP plane-angle guard seam: the
// `export_step_with_injected_fault_for_test` wrapper below returns the probe
// result and takes the fault by value, so integration tests must be able to
// name both.
#[cfg(has_occt)]
pub use ffi::ffi::{StepGuardFault, StepGuardProbeResult};

/// Re-exported so callers can name [`OcctKernel::volume_measurement`]'s return
/// type without reaching into the private bridge module.
#[cfg(has_occt)]
pub use ffi::ffi::VolumeMeasurement;

/// Fixture for integration tests: runs only the post-sort/dedup helper on
/// a synthetic flat-records input, without requiring real OCCT geometry.
///
/// Input is a flat buffer of `(parent_index, parent_subshape_index,
/// result_subshape_index)` triples.  The fixture stable-sorts by
/// `parent_subshape_index`, drops duplicate records (keeping the first under
/// the stable sort), and returns the compacted buffer plus the count of
/// dropped duplicates.
///
/// Gated behind the `test-fixtures` feature so this Rust wrapper is not
/// exposed on the crate's public surface in production builds.
///
/// Note: the gate is **Rust-side reachability only**. The underlying
/// cxx-bridge declaration (`crate::ffi::ffi::revolve_synthesis_post_sort_for_test`)
/// and its C++ implementation in `cpp/occt_wrapper.cpp` are compiled into
/// every build of this crate — `cxx::bridge` does not currently support
/// `cfg`-gating individual `extern "C++"` items without restructuring the
/// whole bridge module. Removing the symbol from production binaries would
/// require splitting the bridge into feature-gated sub-modules; the helper
/// is small (~10 LOC, one std::vector copy) and we have intentionally
/// accepted the binary cost in exchange for keeping the bridge layout flat.
///
/// Integration tests reach this function via the self-dev-dep entry in
/// `Cargo.toml` (`features = ["test-fixtures"]`).
#[cfg(all(has_occt, feature = "test-fixtures"))]
#[doc(hidden)]
pub fn revolve_synthesis_post_sort_for_test(input: &[u32]) -> RevolveSynthesisPostSortResult {
    ffi::ffi::revolve_synthesis_post_sort_for_test(input)
}
mod floor_constants;
pub use floor_constants::RUST_GUARD_MARKER;
pub mod register;
pub mod tessellate_options;
pub use tessellate_options::TessellateOptions;
// Unconditional `WarmStartableRegistration` submission for NodeKind::Realization
// — both has_occt and stub OcctKernel impl WarmStartable. See module docs.
mod warm_register;
// `types` is always compiled so `Curvature` exists in both `has_occt` and
// `!has_occt` builds without a `#[cfg]`-gated duplicate definition.
mod types;
pub use types::Curvature;
pub use types::Transform3;
#[cfg(has_occt)]
mod history_decode;
#[cfg(has_occt)]
mod op_args;
#[cfg(has_occt)]
mod queries;
#[cfg(has_occt)]
mod handle;
#[cfg(has_occt)]
pub use handle::OcctKernelHandle;
#[cfg(has_occt)]
mod sweep_pose;
#[cfg(has_occt)]
use sweep_pose::validate_sweep_pose;
// `reify_mesh_morph::Projector` impl over `OcctKernel`. Gated on
// `feature = "mesh-morph"` so adapters keep the dep-direction-inverted
// layering (see `Cargo.toml`); also `has_occt` because the impl requires the
// real FFI to be useful.
#[cfg(all(has_occt, feature = "mesh-morph"))]
mod projector_impl;
#[cfg(all(has_occt, feature = "mesh-morph"))]
pub use projector_impl::OcctProjector;
// v0.2 persistent-naming-v2 BRepAlgoAPI history records (task 2590).
// Always exported from the crate root so callers in either build mode
// (`has_occt` and stub) can name the types; the stub variant of
// `OcctKernelHandle::boolean_fuse_with_history` returns
// `Err(OperationFailed("OCCT not available"))`.
#[cfg(has_occt)]
use floor_constants::{CPP_LINE_WIRE_MIN_LENGTH_SQ, RUST_LINE_WIRE_MIN_LENGTH_SQ};
/// Compile-time invariant: the Rust primary floor must stay strictly below the C++ floor.
/// Changing either constant in `src/floor_constants.rs` re-evaluates this at `cargo check`.
#[cfg(has_occt)]
const _: () = assert!(RUST_LINE_WIRE_MIN_LENGTH_SQ < CPP_LINE_WIRE_MIN_LENGTH_SQ);

#[cfg(not(has_occt))]
mod stubs;
#[cfg(not(has_occt))]
pub use stubs::{OcctKernel, OcctKernelHandle, TopologyCacheBuildCounts};

#[cfg(has_occt)]
use std::collections::HashMap;

#[cfg(has_occt)]
use history_decode::{
    decode_boolean_history, decode_local_feature_history, decode_loft_history, decode_sweep_history,
};

#[cfg(has_occt)]
use op_args::{
    AXIS_MAG_SQ_MIN, extract_f64, extract_length_f64, validate_all_positive_finite,
    validate_extrude_distance, validate_loft_profile_count, validate_pipe_start_tangent,
    validate_positive_finite, validate_revolve_params, validate_uv_finite,
};

#[cfg(has_occt)]
use reify_ir::{BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE, BRepKind, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE, StepSchema, TessError, ThreeMfOptions, Value, WarmStartable, write_3mf, write_stl_binary};

#[cfg(has_occt)]
/// Send-safe payload for OCCT warm-start state.
///
/// Contains BRep ASCII serializations of all shapes in the kernel, the
/// per-handle [`BRepKind`] classification map, and the next handle ID counter.
/// BRep format is valid UTF-8 text, so `String` is used instead of `Vec<u8>`.
///
/// The `reprs` map is kept in lock-step with `shapes`: only IDs whose BRep
/// serialized successfully are included, so every key in `reprs` is also a
/// key in `shapes`.
struct OcctWarmState {
    /// Map from handle ID to BRep ASCII string of the corresponding shape.
    shapes: HashMap<u64, String>,
    /// Map from handle ID to [`BRepKind`] classification (Face, Solid, Edge, …).
    ///
    /// Restored alongside `shapes` in `with_warm_state` so that post-warm-start
    /// dispatch (e.g. `Centroid` → `query_face_centroid` for `BRepKind::Face`)
    /// works correctly.
    reprs: HashMap<u64, BRepKind>,
    /// The next handle ID to assign (preserves ID namespace across warm-start).
    next_id: u64,
}

#[cfg(has_occt)]
/// Encode a `Point3` as the JSON string `{"x":_,"y":_,"z":_}` used by centroid-style queries.
///
/// Both the `Centroid` and `CenterOfMass` query arms return their result in this format.
/// Routing both through this single helper makes it structurally impossible for the two arms
/// to drift apart — the bit-equality invariant documented in the `CenterOfMass`
/// doc-comment in `reify-types::geometry` ("the result is identical to
/// `Centroid(handle)`") is upheld by construction rather than by convention.
///
/// This helper exists specifically because two query arms must produce identical output;
/// it is not a general-purpose JSON encoder. Other query arms (e.g. `BoundingBox`) keep
/// their inline `format!` because no such cross-arm equality contract applies to them.
fn centroid_json(p: ffi::ffi::Point3) -> String {
    format!("{{\"x\":{},\"y\":{},\"z\":{}}}", p.x, p.y, p.z)
}

// --- BRepAlgoAPI history records (v0.2 persistent-naming-v2, task 2590) ---
//
// The record types live in `reify-types` so that consumers (notably
// `reify_eval::propagate_attributes_via_brepalgoapi_history`) can use them
// without taking a normal-dep on `reify-kernel-occt`. Re-exported here for
// callers that already import from this crate; new call sites should prefer
// `reify_types::{BooleanOpHistoryRecords, HistoryRecord, DeletedRecord}`.
// Task 7b (#2831): LocalFeatureOpHistoryRecords lifted to reify-ir so that
// reify-eval can consume fillet/chamfer history without a normal-dep on
// reify-kernel-occt. Re-exported here so existing callers (stubs.rs,
// tests/common/mod.rs, local_feature_helper_contract.rs, per_edge_fillet.rs)
// continue resolving `reify_kernel_occt::LocalFeatureOpHistoryRecords` unchanged.
pub use reify_ir::{AttributeHistory, BooleanOpHistoryRecords, DeletedRecord, HistoryRecord, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords, SweepOpHistoryRecords};

#[cfg(has_occt)]
/// The sub-shape kinds [`OcctKernel::extract_subshapes`] enumerates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubShapeKind {
    Edge,
    Face,
    Vertex,
}

#[cfg(has_occt)]
impl SubShapeKind {
    fn repr(self) -> BRepKind {
        match self {
            Self::Edge => BRepKind::Edge,
            Self::Face => BRepKind::Face,
            Self::Vertex => BRepKind::Vertex,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Edge => "edge",
            Self::Face => "face",
            Self::Vertex => "vertex",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Edge => "edges",
            Self::Face => "faces",
            Self::Vertex => "vertices",
        }
    }
}

#[cfg(has_occt)]
/// OpenCASCADE geometry kernel (raw, `!Send + !Sync`).
///
/// Contains `cxx::UniquePtr<OcctShape>` handles which are `!Send`, so the
/// kernel cannot cross thread boundaries. For cross-thread usage, use
/// [`OcctKernelHandle`] which runs the kernel on a dedicated OS thread.
pub struct OcctKernel {
    shapes: HashMap<u64, cxx::UniquePtr<ffi::ffi::OcctShape>>,
    /// Per-handle BRepKind, populated alongside `shapes` in `store_with_repr`.
    /// Looked up via the public `repr_of(id)` accessor. Persisted and
    /// restored in lock-step with `shapes` across warm-start (see
    /// `warm_state` / `with_warm_state`): entries missing at restore time
    /// (e.g. because the shape failed to serialize) fall back to
    /// `BRepKind::Solid`.
    reprs: HashMap<u64, BRepKind>,
    /// Idempotency cache for [`Self::extract_edges`]: parent handle id →
    /// previously-minted edge handle list (in canonical
    /// `TopExp::MapShapes(.., TopAbs_EDGE, ..)` order). On a cache hit, the
    /// stored vec is returned verbatim; on a miss, fresh handles are minted
    /// via `store_with_repr` and stored here.
    ///
    /// Required by the v0.2 selector vocabulary v2 (task 2658, step-26):
    /// `selector_vocabulary_v2::adjacent_to_face` re-calls `extract_faces`
    /// internally to recover the face_handle → face_index mapping. Without
    /// idempotency the second call would mint a fresh set of ids and the
    /// caller's pre-extracted handles would no longer match. The cache
    /// makes `extract_*` deterministic per (kernel, parent) pair.
    extracted_edges: HashMap<u64, Vec<GeometryHandleId>>,
    /// Idempotency cache for [`Self::extract_faces`]: parent handle id →
    /// previously-minted face handle list (canonical `TopExp::MapShapes`
    /// order). Sister to `extracted_edges`; same rationale.
    extracted_faces: HashMap<u64, Vec<GeometryHandleId>>,
    /// Idempotency cache for [`Self::extract_vertices`]: parent handle id →
    /// previously-minted vertex handle list (canonical
    /// `TopExp::MapShapes(.., TopAbs_VERTEX, ..)` order). Required by the
    /// PNv2 selector vocabulary v2 (task 3632): `extract_vertices` is called
    /// by downstream consumers (task C CornerVertex/CapCornerVertex seeders)
    /// to recover the canonical vertex index of a caller-supplied handle.
    /// Without idempotency the second call would mint a fresh set of ids and
    /// the caller's pre-extracted handles would no longer match. Cache is
    /// invalidated on `with_warm_state` (same rationale as `extracted_edges`).
    extracted_vertices: HashMap<u64, Vec<GeometryHandleId>>,
    /// Provenance map: child handle id → parent (body) handle id, populated
    /// inside [`Self::extract_edges`] / [`Self::extract_faces`] so a later
    /// `OwnerBody(child)` query can answer "what body did this sub-shape come
    /// from?" without re-extraction.
    ///
    /// Required by the v0.2 selector vocabulary v2 (task 2658, step-30):
    /// `selector_vocabulary_v2::owner_body_of` issues a single `OwnerBody`
    /// query and unwraps the `Value::Int(parent.0 as i64)` reply. Handles
    /// produced directly by `execute` (i.e. body handles themselves) have no
    /// recorded parent and surface as `QueryError::QueryFailed`.
    parent_handle: HashMap<u64, GeometryHandleId>,
    next_id: u64,
    /// Number of shapes that failed deserialization during the last `with_warm_state()` call.
    last_warm_start_failures: usize,
}

// Note: OcctKernel is !Send + !Sync because cxx::UniquePtr<OcctShape> is !Send.
// Use OcctKernelHandle for cross-thread usage — it communicates with a dedicated
// OS thread that owns the kernel.

/// Classify an `OcctShape` by its BODY type: the top-level TopAbs type after
/// the single-body unwrap, so a 1-solid COMPOUND wrapper is
/// [`BRepKind::Solid`]. A type with no [`BRepKind`] is an error, never Solid.
///
/// Only the kind is unwrapped, not the stored shape: a fillet, chamfer, draft
/// or thicken result is still OCCT's COMPOUND wrapper, so C++ code branching
/// on `ShapeType()` sees COMPOUND where `repr_of` says Solid.
#[cfg(has_occt)]
fn brep_kind_of_shape(shape: &ffi::ffi::OcctShape) -> Result<BRepKind, GeometryError> {
    let name = ffi::ffi::body_type_name(shape)
        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
    brep_kind_of_type_name(&name)
}

/// The OCCT TopAbs type name → [`BRepKind`] table behind [`brep_kind_of_shape`].
#[cfg(has_occt)]
fn brep_kind_of_type_name(name: &str) -> Result<BRepKind, GeometryError> {
    Ok(match name {
        "Solid" => BRepKind::Solid,
        "CompSolid" | "Compound" => BRepKind::Compound,
        "Shell" => BRepKind::Shell,
        "Wire" => BRepKind::Wire,
        "Face" => BRepKind::Face,
        "Edge" => BRepKind::Edge,
        "Vertex" => BRepKind::Vertex,
        other => {
            return Err(GeometryError::OperationFailed(format!(
                "cannot classify OCCT shape type {other:?} as a BRepKind"
            )));
        }
    })
}

/// How an op's result [`BRepKind`] is decided: stated, copied from an input,
/// or read off the built shape by [`brep_kind_of_shape`].
#[cfg(has_occt)]
#[derive(Clone, Copy)]
enum ReprPolicy {
    Fixed(BRepKind),
    Inherit(GeometryHandleId),
    Classify,
}

/// What an `execute` arm yields: a new shape to store under its policy, or a
/// handle a delegate already stored.
#[cfg(has_occt)]
enum ArmOutcome {
    New(cxx::UniquePtr<ffi::ffi::OcctShape>, ReprPolicy),
    Stored(GeometryHandle),
}

#[cfg(has_occt)]
impl OcctKernel {
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
            reprs: HashMap::new(),
            extracted_edges: HashMap::new(),
            extracted_faces: HashMap::new(),
            extracted_vertices: HashMap::new(),
            parent_handle: HashMap::new(),
            next_id: 1,
            last_warm_start_failures: 0,
        }
    }

    /// Return the [`BRepKind`] that was assigned to `id` at `store_with_repr`
    /// time, or `None` if `id` is unknown to this kernel.
    ///
    /// After a `warm_state()`/`with_warm_state()` round-trip the classification
    /// is restored from the persisted `reprs` map. Any handle whose BRep
    /// serialized successfully but whose repr entry was absent falls back to
    /// [`BRepKind::Solid`].
    pub fn repr_of(&self, id: GeometryHandleId) -> Option<BRepKind> {
        self.reprs.get(&id.0).copied()
    }

    /// Returns the number of shapes that failed BRep deserialization during
    /// the last [`WarmStartable::with_warm_state`] call (0 if none failed, or
    /// if `with_warm_state` has not yet been called on this kernel).
    ///
    /// INV-GEO-3 production observability accessor: each failure is also
    /// reported via a `tracing::warn!` event at the point of failure (see
    /// `with_warm_state`); this accessor exposes the failure count from
    /// the most recent restore for production consumers that want the
    /// summary figure without instrumenting tracing.
    ///
    /// Edge case: a `with_warm_state` call whose [`OpaqueState`] fails the
    /// type check is a no-op per the trait contract (state silently
    /// ignored) and returns *before* the counter reset — it does not zero
    /// or otherwise change the count left by the previous successful
    /// restore.
    pub fn warm_start_failures(&self) -> usize {
        self.last_warm_start_failures
    }

    /// Number of shapes currently resident in this kernel's native `shapes`
    /// table.
    ///
    /// This is the observability primitive for task 5212's reload
    /// native-memory-bound signal: `shapes` holds one `cxx::UniquePtr<OcctShape>`
    /// per minted handle and is the map that grows unbounded across GUI
    /// whole-file reloads until [`Self::reset`] evicts it. Exposing the count
    /// lets callers (and the boundedness regression test) verify that a
    /// reset genuinely frees the resident shapes and that repeated
    /// execute-batch→reset cycles stay bounded rather than accumulating.
    /// Mirrors the [`Self::warm_start_failures`] diagnostic accessor above.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// Free every resident native shape and clear all derived provenance/
    /// idempotency caches, returning the kernel to an empty-but-reusable
    /// state — WITHOUT rewinding the `next_id` handle counter.
    ///
    /// This is the reload native-memory bound for task 5212. The reify-eval
    /// `Engine` and the `OcctKernel` it owns are long-lived and reused across
    /// GUI whole-file reloads; without this, every reload re-executes the
    /// design's geometry and mints fresh `shapes` while prior-design shapes
    /// stay resident, growing OCCT native memory unbounded over a long
    /// session. Calling `reset()` once per reload (see
    /// `Engine::reset_geometry_for_reload`) evicts the previous build's
    /// shapes so the resident count stays bounded.
    ///
    /// `next_id` is deliberately KEPT monotonic (not reset to 1): the leak is
    /// the `shapes` map, not the 8-byte counter. Keeping handle ids strictly
    /// increasing means any handle that outlives a reload (e.g. a lingering
    /// realization-cache entry) can never alias a freshly-minted shape — it
    /// resolves to a clean `InvalidReference` via the #5211 `get_shape`
    /// IsNull/InvalidReference guard instead of silently reading a different
    /// shape. INV-BUILD-1 (`engine_build.rs`) warns that "re-minting id 1 on
    /// a later build would be served silently in release"; this is the
    /// read-side counterpart. (It is also why `with_warm_state`'s
    /// `*next_id = warm.next_id` swap, which CAN lower the counter, is a
    /// dormant aliasing hazard — see its guard comment.)
    pub fn reset(&mut self) {
        // INV-GEO-3 state-inventory guard: this exhaustive destructure (no
        // `..` spread) forces every OcctKernel field to be classified here as
        // CLEAR (freed on reload) or KEEP (deliberately preserved). Adding a
        // new field to OcctKernel without extending this pattern is a hard
        // compile error (E0027), so the reload clear/keep decision can't be
        // silently skipped. Mirrors the warm_state()/with_warm_state()
        // destructures.
        let Self {
            shapes,
            reprs,
            extracted_edges,
            extracted_faces,
            extracted_vertices,
            parent_handle,
            next_id: _,                  // KEEP: monotonic — see doc comment above.
            last_warm_start_failures: _, // KEEP: last-restore diagnostic, not a resident shape.
        } = self;
        // CLEAR: free the resident native shapes (the reload memory bound)
        // and every derived provenance/idempotency cache keyed to them.
        shapes.clear();
        reprs.clear();
        extracted_edges.clear();
        extracted_faces.clear();
        extracted_vertices.clear();
        parent_handle.clear();
    }

    /// Store a shape with an explicit `BRepKind`.
    ///
    /// `repr` is wrapped in `Some(...)` on the returned [`GeometryHandle`] —
    /// OCCT is a genuine B-rep kernel so sub-shape classification is always
    /// meaningful. The persistent `reprs` map stores the bare [`BRepKind`]
    /// for `repr_of(id)` lookups (warm-state round-trips).
    fn store_with_repr(
        &mut self,
        shape: cxx::UniquePtr<ffi::ffi::OcctShape>,
        repr: BRepKind,
    ) -> GeometryHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.shapes.insert(id, shape);
        self.reprs.insert(id, repr);
        GeometryHandle {
            id: GeometryHandleId(id),
            repr: Some(repr),
        }
    }

    /// The [`BRepKind`] `policy` assigns to `shape`.
    ///
    /// An `Inherit` source always has a recorded kind: every caller has
    /// already read the source's shape, and `store_with_repr` and
    /// `with_warm_state` record a shape and its kind together. A source with
    /// no kind is therefore a broken invariant and an error, never a silent
    /// re-classification.
    fn resolve_repr(
        &self,
        shape: &ffi::ffi::OcctShape,
        policy: ReprPolicy,
    ) -> Result<BRepKind, GeometryError> {
        match policy {
            ReprPolicy::Fixed(kind) => Ok(kind),
            ReprPolicy::Inherit(source) => self.repr_of(source).ok_or_else(|| {
                GeometryError::OperationFailed(format!(
                    "ReprPolicy::Inherit source {source:?} has no recorded BRepKind: \
                     the kernel's shapes and reprs are out of lock-step"
                ))
            }),
            ReprPolicy::Classify => brep_kind_of_shape(shape),
        }
    }

    /// Store `shape` under the [`BRepKind`] its `policy` resolves to.
    fn store_with_policy(
        &mut self,
        shape: cxx::UniquePtr<ffi::ffi::OcctShape>,
        policy: ReprPolicy,
    ) -> Result<GeometryHandle, GeometryError> {
        let repr = self.resolve_repr(&shape, policy)?;
        Ok(self.store_with_repr(shape, repr))
    }

    /// Debug-only invariant guard for [`Self::extract_subshapes`]: asserts
    /// that `h`, just returned from `store_with_repr`, really did get a
    /// strictly fresh id (never reused).
    ///
    /// This protects `warm_state()`'s bounded-payload filter (see its
    /// "Bounded payload" note): the filter treats every `parent_handle` key
    /// as a derived, rebuildable sub-shape id and drops it from persisted
    /// state. That's only safe because `store_with_repr` mints ids by
    /// fetch-and-increment on `next_id` and never reuses one, so a
    /// root/persistent handle id can never end up back here as `h.id.0`.
    /// Assert that freshness explicitly so a future change that breaks it
    /// (e.g. a `store_with_repr` that interns/reuses ids) fails loudly here
    /// instead of silently making the filter drop real root data.
    fn debug_assert_fresh_id(&self, h: &GeometryHandle) {
        debug_assert_eq!(
            h.id.0,
            self.next_id - 1,
            "store_with_repr must mint a strictly fresh id; a non-fresh \
             id here could alias an existing root/persistent handle and \
             make warm_state()'s parent_handle filter silently drop real \
             root data"
        );
    }

    /// Look up a shape by handle ID.
    ///
    /// Propagate with `?`; see reify-ir's `From<GeometryError>` impls for the
    /// mapping.
    fn get_shape(&self, id: GeometryHandleId) -> Result<&ffi::ffi::OcctShape, GeometryError> {
        let ptr = self
            .shapes
            .get(&id.0)
            .ok_or(GeometryError::InvalidReference(id))?;
        let shape = ptr.as_ref().ok_or_else(|| {
            GeometryError::OperationFailed(format!("shape handle {id:?} is null"))
        })?;
        // Reject a NON-null wrapper around null (IsNull) topology. Such a shape
        // passes the null-UniquePtr check above yet dereferences to a null
        // TShape, which crashes the unguarded OCCT geometry-query FFI (e.g.
        // query_volume's ShapeType() fallback -> hardware SIGSEGV). This single
        // chokepoint covers all four mass-property queries plus every other
        // shape.shape deref reached via get_shape (query_area,
        // query_edge_length, ...). Refuse loudly — no silent zero/origin
        // fallback (aligns with sibling 5197's mass-properties policy).
        if ffi::ffi::shape_is_null(shape) {
            return Err(GeometryError::OperationFailed(format!(
                "shape handle {:?} wraps null/empty topology (IsNull)",
                id
            )));
        }
        Ok(shape)
    }

    /// Return the topology-map cache build counts for the shape identified by
    /// `handle`. Each counter is 0 on a fresh shape and increments to 1 when
    /// the corresponding lazy cache slot is first populated.
    ///
    /// Returns [`GeometryError::InvalidReference`] if `handle` is unknown.
    pub fn topology_cache_build_counts(
        &self,
        handle: GeometryHandleId,
    ) -> Result<ffi::ffi::TopologyCacheBuildCounts, GeometryError> {
        let shape = self.get_shape(handle)?;
        Ok(ffi::ffi::topology_cache_build_counts(shape))
    }

    /// The idempotency cache holding `kind`'s extractions.
    fn subshape_cache(&mut self, kind: SubShapeKind) -> &mut HashMap<u64, Vec<GeometryHandleId>> {
        match kind {
            SubShapeKind::Edge => &mut self.extracted_edges,
            SubShapeKind::Face => &mut self.extracted_faces,
            SubShapeKind::Vertex => &mut self.extracted_vertices,
        }
    }

    /// Extract every unique `kind` sub-shape of `parent` in canonical
    /// `TopExp::MapShapes` order, each stamped `kind.repr()` with `parent`
    /// recorded as its owner. Idempotent per (parent, kind): see
    /// [`Self::extract_edges`].
    fn extract_subshapes(
        &mut self,
        parent: GeometryHandleId,
        kind: SubShapeKind,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        if let Some(cached) = self.subshape_cache(kind).get(&parent.0) {
            return Ok(cached.clone());
        }
        // Collect into a Vec<UniquePtr<OcctShape>> in a scope where the
        // immutable borrow of `self` (via get_shape) drops before we mutate
        // self via store_with_repr.
        let materialized: Vec<cxx::UniquePtr<ffi::ffi::OcctShape>> = {
            let shape = self.get_shape(parent)?;
            let vec = match kind {
                SubShapeKind::Edge => ffi::ffi::get_edges(shape),
                SubShapeKind::Face => ffi::ffi::get_faces(shape),
                SubShapeKind::Vertex => ffi::ffi::get_vertices(shape),
            }
            .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
            let len = ffi::ffi::shape_vec_len(&vec);
            let mut buf = Vec::with_capacity(len);
            for i in 0..len {
                let sub = ffi::ffi::shape_vec_at(&vec, i)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                buf.push(sub);
            }
            buf
        };

        let mut ids = Vec::with_capacity(materialized.len());
        for sub in materialized {
            let h = self.store_with_repr(sub, kind.repr());
            self.debug_assert_fresh_id(&h);
            // Provenance, so `OwnerBody(child)` answers without re-extraction.
            self.parent_handle.insert(h.id.0, parent);
            ids.push(h.id);
        }
        self.subshape_cache(kind).insert(parent.0, ids.clone());
        Ok(ids)
    }

    /// The 0-based positions of `selection` in `parent`'s canonical `kind`
    /// enumeration, in selection order; `None` selects every sub-shape.
    ///
    /// `extract_subshapes`, the `get_edges`/`get_faces` getters and the
    /// `OcctShape` edge/face maps share one `TopExp::MapShapes` order, so a
    /// position is exactly the index the kernel-side FFI expects.
    fn map_selection_to_indices(
        &mut self,
        parent: GeometryHandleId,
        selection: Option<&[GeometryHandleId]>,
        kind: SubShapeKind,
        label: &str,
    ) -> Result<Vec<u32>, GeometryError> {
        let parent_subshapes = self.extract_subshapes(parent, kind).map_err(|e| {
            GeometryError::OperationFailed(format!(
                "{label}: failed to enumerate parent {} of {parent:?}: {e:?}",
                kind.plural()
            ))
        })?;
        let Some(selection) = selection else {
            return Ok((0..parent_subshapes.len() as u32).collect());
        };
        let index_of: HashMap<GeometryHandleId, u32> = parent_subshapes
            .iter()
            .enumerate()
            .map(|(i, h)| (*h, i as u32))
            .collect();
        selection
            .iter()
            .map(|h| {
                index_of.get(h).copied().ok_or_else(|| {
                    GeometryError::OperationFailed(format!(
                        "{label}: {} {h:?} does not belong to solid {parent:?}",
                        kind.noun()
                    ))
                })
            })
            .collect()
    }

    /// Extract every unique edge of `handle` as a handle with
    /// `BRepKind::Edge`. Order follows the canonical
    /// `TopExp::MapShapes(.., TopAbs_EDGE, ..)` enumeration.
    ///
    /// **Idempotent per parent**: a second call with the same `handle`
    /// returns the same handle list as the first call (cached in
    /// `extracted_edges`). This is required by the v0.2 selector
    /// vocabulary v2's `adjacent_to_face` (and forthcoming
    /// `ancestor_faces_of_edge`), which re-calls `extract_*` to recover
    /// the canonical sub-shape index of a caller-supplied handle. Without
    /// idempotency the second call would mint a fresh set of ids and the
    /// caller's pre-extracted handles would no longer match. The cache is
    /// invalidated on `with_warm_state` (when the kernel's shape table is
    /// swapped wholesale).
    ///
    /// Returns [`QueryError::InvalidHandle`] if `handle` is unknown.
    pub fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.extract_subshapes(handle, SubShapeKind::Edge)
    }

    /// Extract every unique face of `handle` as a handle with
    /// `BRepKind::Face`. Order follows the canonical
    /// `TopExp::MapShapes(.., TopAbs_FACE, ..)` enumeration.
    ///
    /// **Idempotent per parent**: see [`Self::extract_edges`] for the
    /// rationale and cache-invalidation contract.
    ///
    /// Returns [`QueryError::InvalidHandle`] if `handle` is unknown.
    pub fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.extract_subshapes(handle, SubShapeKind::Face)
    }

    /// Extract every unique vertex of `handle` as a handle with
    /// `BRepKind::Vertex`. Order follows the canonical
    /// `TopExp::MapShapes(.., TopAbs_VERTEX, ..)` enumeration (a fresh local
    /// map is built per call — there is no `vertex_map()` cache slot on
    /// `OcctShape`; the Rust-side `extracted_vertices` cache below provides
    /// idempotency without a C++ map slot).
    ///
    /// **Idempotent per parent**: a second call with the same `handle` on the
    /// same kernel returns the previously-minted `Vec<GeometryHandleId>`
    /// verbatim (cached in `extracted_vertices`). Required by the PNv2
    /// selector vocabulary v2 (task 3632) and downstream task-C seeders that
    /// re-call `extract_vertices` internally to recover the canonical vertex
    /// index. The cache is invalidated on `with_warm_state` (when the kernel's
    /// shape table is swapped wholesale).
    ///
    /// Child handle provenance is recorded in `parent_handle` so a later
    /// `OwnerBody(vertex_id)` query can answer "what body did this vertex come
    /// from?" — matches the `extract_edges`/`extract_faces` contract.
    ///
    /// Returns [`QueryError::InvalidHandle`] if `handle` is unknown.
    pub fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.extract_subshapes(handle, SubShapeKind::Vertex)
    }

    /// Split `target` with an unbounded cutting plane defined by
    /// `plane_origin` and `plane_normal` (metres). Returns a
    /// `Vec<GeometryHandleId>` of the result solids (length 1 for a
    /// non-intersecting plane; ≥ 2 for an intersecting one). Each result
    /// solid is registered with `BRepKind::Solid`.
    ///
    /// Mirrors [`Self::extract_edges`] / [`Self::extract_faces`] but calls
    /// `ffi::split_shape` instead of `ffi::get_edges` / `ffi::get_faces`.
    /// Unlike extraction there is no idempotency cache — splitting the same
    /// solid with the same plane twice legitimately creates fresh handles.
    pub fn execute_split(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<Vec<GeometryHandleId>, GeometryError> {
        let (target, plane_origin, plane_normal) = match op {
            reify_ir::GeometryOp::Split {
                target,
                plane_origin,
                plane_normal,
            } => (*target, *plane_origin, *plane_normal),
            other => {
                return Err(GeometryError::OperationFailed(format!(
                    "execute_split called with non-Split op: {}",
                    other.kind_name()
                )));
            }
        };
        let [ox, oy, oz] = plane_origin;
        let [nx, ny, nz] = plane_normal;
        // Collect UniquePtr<OcctShape> pieces in a scope that ends the
        // immutable borrow on `self` before we call `store_with_repr`.
        let materialized: Vec<cxx::UniquePtr<ffi::ffi::OcctShape>> = {
            let shape = self.get_shape(target)?;
            let vec = ffi::ffi::split_shape(shape, ox, oy, oz, nx, ny, nz)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            let len = ffi::ffi::shape_vec_len(&vec);
            let mut buf = Vec::with_capacity(len);
            for i in 0..len {
                let sub = ffi::ffi::shape_vec_at(&vec, i)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                buf.push(sub);
            }
            buf
        };
        let mut ids = Vec::with_capacity(materialized.len());
        for sub in materialized {
            let h = self.store_with_repr(sub, BRepKind::Solid);
            ids.push(h.id);
        }
        Ok(ids)
    }

    /// Assemble N placed product solids into a single `TopoDS_Compound` handle
    /// for multi-body STEP export (T7).
    ///
    /// Resolves each `GeometryHandleId` to its `OcctShape`, builds an
    /// `OcctShapeVec`, and calls `ffi::ffi::make_compound` which uses
    /// `BRep_Builder::MakeCompound` + `Add`.  Each input shape is
    /// **deep-copied** (via `BRepBuilderAPI_Copy`) inside `make_compound`
    /// before it is added to the compound, so every compound member gets an
    /// independent `TShape` — preventing the STEP writer from deduplicating
    /// multiple instances of the same source solid into a single
    /// `MANIFOLD_SOLID_BREP`.  Source handles remain valid after the call.
    ///
    /// Returns `Err(GeometryError::OperationFailed)` when:
    /// - `handles` is empty, or
    /// - any handle is unknown.
    pub fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        if handles.is_empty() {
            return Err(GeometryError::OperationFailed(
                "make_compound: handles slice must not be empty".into(),
            ));
        }
        // Build the OcctShapeVec while holding the immutable borrow on
        // self.shapes (via get_shape).  shape_vec_push copies each
        // TopoDS_Shape into the vec — the copy is a thin handle increment.
        // The vec itself is a fresh allocation separate from self.shapes,
        // so there is no aliasing conflict.
        let compound_result = {
            let mut vec = ffi::ffi::new_shape_vec();
            for &id in handles {
                let shape = self.get_shape(id)?;
                ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
            }
            // All shapes pushed; the immutable borrow on self.shapes ends here.
            ffi::ffi::make_compound(&vec)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
        };
        // Immutable borrow on self.shapes is released; now safe to call
        // store_with_repr (which takes &mut self).
        Ok(self.store_with_repr(compound_result, BRepKind::Compound))
    }

    /// Fuse N shapes into a single body in ONE OCCT boolean pass (task 5213,
    /// Lever 1 — the single-pass n-ary fuse).
    ///
    /// Resolves each `GeometryHandleId` to its `OcctShape`, builds an
    /// `OcctShapeVec`, and calls `ffi::ffi::fuse_all`, which fuses the whole
    /// list in one `BRepAlgoAPI_Fuse` (SetArguments/SetTools) pass instead of
    /// the O(N²) pairwise-accumulator loop.  Union semantics are preserved
    /// exactly: overlapping inputs merge (no internal walls / double-counted
    /// volume), disjoint inputs come back as a watertight multi-solid.  Source
    /// handles remain valid after the call.
    ///
    /// Returns `Err(GeometryError::OperationFailed)` when:
    /// - `handles` is empty, or
    /// - any handle is unknown, or
    /// - the underlying OCCT fuse fails.
    pub fn fuse_all(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        if handles.is_empty() {
            return Err(GeometryError::OperationFailed(
                "fuse_all: handles slice must not be empty".into(),
            ));
        }
        // Gather the argument shapes into a fresh OcctShapeVec while holding the
        // immutable borrow on self.shapes; the borrow ends before we store.
        let fused = {
            let mut vec = ffi::ffi::new_shape_vec();
            for &id in handles {
                let shape = self.get_shape(id)?;
                ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
            }
            ffi::ffi::fuse_all(&vec).map_err(|e| GeometryError::OperationFailed(e.to_string()))?
        };
        // The single-element path is an identity passthrough of the sole
        // input shape (any kind), so it keeps that input's classification.
        let policy = match handles {
            [only] => ReprPolicy::Inherit(*only),
            _ => ReprPolicy::Classify,
        };
        self.store_with_policy(fused, policy)
    }

    /// Test whether two shapes are intersecting (non-positive minimum distance).
    ///
    /// Uses `BRepExtrema_DistShapeShape` — same primitive as `min_clearance` —
    /// returning `Ok(true)` iff the minimum BREP distance is ≤ 0.0.  This is
    /// significantly cheaper than a full `BRepAlgoAPI_Common` boolean because it
    /// computes only distance (not intersection geometry) and can early-exit.
    /// Face-touching pairs (distance == 0) count as intersecting.
    /// No tolerance filtering at the FFI level; task 2531 layers
    /// any tolerance/exclusion semantics on top.
    ///
    /// Returns `Err(QueryError::InvalidHandle(id))` if either handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(...))` if the OCCT call fails.
    ///
    /// Part of the kinematic-constraints FFI (docs/prds/kinematic-constraints.md
    /// task 7). Wrapped by task 2531's stdlib `interferes`/`interferes_with`.
    pub fn shapes_intersect(
        &self,
        a: GeometryHandleId,
        b: GeometryHandleId,
    ) -> Result<bool, QueryError> {
        let s1 = self.get_shape(a)?;
        let s2 = self.get_shape(b)?;
        ffi::ffi::shapes_intersect(s1, s2).map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Probe whether two shapes interfere after pre-composing `t_rel` into the
    /// cheaper-by-topology side (PRD §6.2 + §9.2, task 3841).
    ///
    /// Returns `Ok(true)` iff the minimum BREP distance after transform is ≤ 0.0.
    /// Face-touching pairs count as interfering — matches `shapes_intersect` semantics.
    ///
    /// Returns `Err(QueryError::InvalidHandle(id))` if either handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(...))` if the OCCT call fails.
    pub fn interferes_with_transform(
        &self,
        a: GeometryHandleId,
        b: GeometryHandleId,
        t_rel: &crate::Transform3,
    ) -> Result<bool, QueryError> {
        let s1 = self.get_shape(a)?;
        let s2 = self.get_shape(b)?;
        queries::interferes_with_transform(s1, s2, t_rel)
    }

    /// Minimum BREP distance between two shapes via BRepExtrema_DistShapeShape.
    ///
    /// Returns `Ok(distance)` — 0.0 for overlapping shapes.
    ///
    /// Returns `Err(QueryError::InvalidHandle(id))` if either handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(...))` if the OCCT call fails.
    ///
    /// Part of the kinematic-constraints FFI (docs/prds/kinematic-constraints.md
    /// task 7). Wrapped by task 2531's stdlib `min_clearance`.
    pub fn min_clearance(
        &self,
        a: GeometryHandleId,
        b: GeometryHandleId,
    ) -> Result<f64, QueryError> {
        let s1 = self.get_shape(a)?;
        let s2 = self.get_shape(b)?;
        ffi::ffi::min_clearance(s1, s2).map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Minimum BREP distance between two shapes after pre-composing `t_rel`
    /// into the cheaper-by-topology side (PRD §6.2 + §9.2, task 3841).
    ///
    /// Rigid-invariance property: `distance_with_transform(a, b, t)` equals
    /// `min_clearance(translate_rotate(a, t), b)` within floating-point tolerance.
    ///
    /// Returns `Err(QueryError::InvalidHandle(id))` if either handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(...))` if the OCCT call fails.
    pub fn distance_with_transform(
        &self,
        a: GeometryHandleId,
        b: GeometryHandleId,
        t_rel: &crate::Transform3,
    ) -> Result<f64, QueryError> {
        let s1 = self.get_shape(a)?;
        let s2 = self.get_shape(b)?;
        queries::distance_with_transform(s1, s2, t_rel)
    }

    /// Apply the rigid transform `t` to the shape identified by `handle`,
    /// returning a *new* handle that points at the transformed result. The
    /// source handle is preserved unmodified so the same child shape can be
    /// placed under several different frames (sub-placement PRD §5, task 3901).
    ///
    /// The transform is encoded via `BRepBuilderAPI_Transform(…, Standard_True)`
    /// (geometry bake) — each placed copy is a topologically independent
    /// `TopoDS_Shape` so that the STEP writer emits a distinct
    /// `MANIFOLD_SOLID_BREP` for every placed body.  The result inherits the
    /// source's [`BRepKind`] because a rigid isometry preserves topology
    /// (a Solid stays a Solid, a Face stays a Face, …).
    ///
    /// # Errors
    /// - `GeometryError::InvalidReference(handle)` if `handle` is unknown to
    ///   this kernel.
    /// - `GeometryError::OperationFailed(msg)` if the quaternion stored in `t`
    ///   is not a unit quaternion (|q|² outside [1−1e-6, 1+1e-6]) — the message
    ///   contains "quaternion". Any other OCCT failure is also surfaced via
    ///   `OperationFailed`.
    pub fn apply_transform_to_handle(
        &mut self,
        handle: GeometryHandleId,
        t: &crate::Transform3,
    ) -> Result<GeometryHandleId, GeometryError> {
        self.store_transformed(handle, t).map(|stored| stored.id)
    }

    /// [`Self::apply_transform_to_handle`], returning the whole stored handle.
    fn store_transformed(
        &mut self,
        source: GeometryHandleId,
        t: &crate::Transform3,
    ) -> Result<GeometryHandle, GeometryError> {
        let shape = self.get_shape(source)?;
        let props = ffi::ffi::Transform3Props::from(t);
        let new_shape = ffi::ffi::apply_transform_to_shape(shape, &props)
            .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
        self.store_with_policy(new_shape, ReprPolicy::Inherit(source))
    }

    /// Test whether two stored shapes share the same underlying `TShape`
    /// (`TopoDS_Shape::IsPartner` — TShape identity ignoring location and
    /// orientation).
    ///
    /// Returns `true` iff both handles resolve to shapes whose `TShape` handle
    /// is identical (they were produced from the same source without a
    /// geometry-baking copy).  A `BRepBuilderAPI_Transform(…, Standard_False)`
    /// result is `IsPartner` with its source; a `Standard_True` (geometry bake)
    /// or independently-built shape is not.
    ///
    /// Returns `Err(GeometryError::InvalidHandle)` if either handle is unknown.
    pub fn shapes_share_tshape(
        &self,
        a: GeometryHandleId,
        b: GeometryHandleId,
    ) -> Result<bool, GeometryError> {
        let shape_a = self.get_shape(a)?;
        let shape_b = self.get_shape(b)?;
        Ok(ffi::ffi::shapes_share_tshape(shape_a, shape_b))
    }

    /// Return the closest point on the shape identified by `handle` to the
    /// query point `(px, py, pz)`.
    ///
    /// Algorithm: `BRepExtrema_DistShapeShape(shape, vertex)` where `vertex` is
    /// a `TopoDS_Vertex` built from the query point via
    /// `BRepBuilderAPI_MakeVertex`. The witness on the input shape is read from
    /// `dist.PointOnShape1(1)`.
    ///
    /// `BRepExtrema_DistShapeShape` has no inside/outside knowledge — it
    /// returns the distance to the nearest BREP boundary face.  For typical
    /// interior points the reported distance is therefore non-zero; only
    /// on-surface (or coincident) query points yield distance 0.  The C++
    /// wrapper applies a defensive `dist < 1e-10` guard and re-runs the
    /// extrema against the first shell found via `TopExp_Explorer`
    /// (`TopAbs_SHELL`).  For single-shell solids this is the outer shell;
    /// for multi-shell solids it may not be.  The re-run returns a witness on
    /// the shell boundary rather than coinciding with the query point.
    ///
    /// Returns `Err(QueryError::InvalidHandle(handle))` if the handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(...))` if the OCCT call fails.
    ///
    /// The return type is `[f64; 3]` rather than the FFI-internal `Point3`
    /// struct: `Point3` is a cxx-bridge type unavailable in stub builds
    /// (`!has_occt`), so the public API uses a plain array for both paths.
    /// Future point-returning `OcctKernel` methods should follow the same
    /// convention.
    pub fn closest_point_on_shape(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<[f64; 3], QueryError> {
        let s = self.get_shape(handle)?;
        let p = ffi::ffi::closest_point_on_shape(s, px, py, pz)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok([p.x, p.y, p.z])
    }

    /// Read the geometric position of the `TopoDS_Vertex` named by `handle`
    /// via `BRep_Tool::Pnt`. Direct accessor — does not invoke
    /// `BRepExtrema_DistShapeShape`.
    ///
    /// Mandated by PRD `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.4
    /// `vertex_position` ("BRep_Tool::Pnt direct; no closest-point") and
    /// consumed by `OcctProjector::vertex_position` in `projector_impl.rs`.
    ///
    /// Returns `Err(QueryError::InvalidHandle(handle))` if the handle is
    /// unknown; `Err(QueryError::QueryFailed(...))` if the shape is not a
    /// `TopoDS_Vertex` or the OCCT call fails. Return type is `[f64; 3]` for
    /// the same stub-build reason as `closest_point_on_shape`.
    pub fn vertex_point(&self, handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        let s = self.get_shape(handle)?;
        let p =
            ffi::ffi::vertex_point(s).map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok([p.x, p.y, p.z])
    }

    /// Angle between the outward normals of the two faces (in radians),
    /// sampled at each face's surface centroid.
    ///
    /// Algorithm: `acos(clamp(n_a · n_b, -1, 1))` where each `n` is the
    /// face's unit outward normal sampled at its centroid via
    /// `BRepAdaptor_Surface::D1`. Honours `TopAbs_REVERSED` orientation
    /// (same semantics as `query_face_normal`). The dot clamp guards against
    /// FP rounding outside `acos`'s domain for parallel/antiparallel inputs.
    ///
    /// Note: this is the angle between outward normals, not a classical
    /// dihedral angle (which requires a shared edge). For adjacent convex
    /// faces this equals the exterior angle; for disjoint faces the value
    /// is still geometrically meaningful.
    ///
    /// Both handles must name `BRepKind::Face` shapes — the `TopAbs_FACE`
    /// check is performed on the C++ side so errors flow through
    /// `QueryFailed(e.to_string())` with a recognizable prefix.
    ///
    /// Returns radians in `[0, π]`.
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if either handle is unknown.
    /// - `QueryError::QueryFailed` — if either shape is not a face, has no
    ///   underlying surface, yields a degenerate normal, or any OCCT call fails.
    pub fn surface_angle(
        &self,
        face_a: GeometryHandleId,
        face_b: GeometryHandleId,
    ) -> Result<f64, QueryError> {
        let s1 = self.get_shape(face_a)?;
        let s2 = self.get_shape(face_b)?;
        ffi::ffi::surface_angle(s1, s2).map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Unit outward normal at the parametric point `(u, v)` on `face`.
    ///
    /// Algorithm: `BRepAdaptor_Surface::D1(u, v)` → `Du × Dv` → magnitude
    /// check → `TopAbs_REVERSED` orientation flip → normalize.
    ///
    /// The return type is `[f64; 3]` rather than the FFI-internal `Point3`
    /// struct: `Point3` is a cxx-bridge type unavailable in stub builds
    /// (`!has_occt`), so the public API uses a plain array for both paths
    /// (same convention as `closest_point_on_shape`).
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if the handle is unknown.
    /// - `QueryError::NonFiniteParameter` — if `(u, v)` contains NaN or ±Inf.
    /// - `QueryError::QueryFailed` — if the shape is not a face, has no
    ///   underlying surface, yields a degenerate normal, or any OCCT call
    ///   fails.
    pub fn surface_normal_at(
        &self,
        handle: GeometryHandleId,
        u: f64,
        v: f64,
    ) -> Result<[f64; 3], QueryError> {
        let s = self.get_shape(handle)?;
        validate_uv_finite(u, v)?;
        let p = ffi::ffi::surface_normal_at(s, u, v)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok([p.x, p.y, p.z])
    }

    /// Outward unit normal of a face at the Cartesian world-space point
    /// `(px, py, pz)` (metres).
    ///
    /// Projects the query point onto the face's underlying surface via
    /// `ShapeAnalysis_Surface::ValueOfUV(point, 1e-9)` to obtain parametric
    /// `(u, v)`, then delegates to `face_outward_unit_normal_at_uv` for the
    /// `BRepAdaptor_Surface::D1` derivative, `TopAbs_REVERSED` orientation
    /// flip, and magnitude check — the same pipeline as `query_face_normal`
    /// (centroid path) and `surface_normal_at` (caller-supplied (u,v) path).
    ///
    /// The return type is `[f64; 3]` (a plain Rust array) rather than the
    /// FFI-internal `Point3`: `Point3` is unavailable in stub builds
    /// (`!has_occt`), so the public API uses a plain array for both paths
    /// (same convention as `surface_normal_at` and `closest_point_on_shape`).
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if the handle is unknown.
    /// - `QueryError::QueryFailed` — if the shape is not a face, projection
    ///   fails, the surface yields a degenerate normal, or any OCCT call fails.
    pub fn surface_normal_at_point(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<[f64; 3], QueryError> {
        let s = self.get_shape(handle)?;
        let p = ffi::ffi::surface_normal_at_point(s, px, py, pz)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok([p.x, p.y, p.z])
    }

    /// Gaussian, mean, and principal curvatures at the parametric point
    /// `(u, v)` on `face`, plus unit-length principal-direction tangents.
    ///
    /// Uses `BRepAdaptor_Surface::D2` — the same surface abstraction as
    /// `surface_normal_at` — so both APIs honour `TopoLoc_Location` and agree
    /// in world frame on faces with non-identity locations. Curvature is derived
    /// from the first/second fundamental forms with an orientation-aware outward
    /// normal; sign convention for `mean` and principal curvatures follows that
    /// outward normal (correct for both FORWARD and REVERSED faces). Gaussian
    /// curvature `K = κ₁·κ₂` is invariant.
    ///
    /// The return type is `Curvature` — a plain Rust struct with `f64` and
    /// `[f64; 3]` fields defined in both `has_occt` and `!has_occt` builds so
    /// callers compile under either mode.
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if the handle is unknown.
    /// - `QueryError::NonFiniteParameter` — if `(u, v)` contains NaN or ±Inf.
    /// - `QueryError::QueryFailed` — if the shape is not a face, has no
    ///   underlying surface, curvature is undefined at `(u, v)`, or any OCCT
    ///   call fails.
    pub fn curvature_at(
        &self,
        handle: GeometryHandleId,
        u: f64,
        v: f64,
    ) -> Result<Curvature, QueryError> {
        let s = self.get_shape(handle)?;
        validate_uv_finite(u, v)?;
        let c =
            ffi::ffi::curvature_at(s, u, v).map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok(Curvature {
            gaussian: c.gaussian,
            mean: c.mean,
            kappa_min: c.kappa_min,
            kappa_max: c.kappa_max,
            dir_min: [c.dir_min.x, c.dir_min.y, c.dir_min.z],
            dir_max: [c.dir_max.x, c.dir_max.y, c.dir_max.z],
        })
    }

    /// Signed curvature of an edge at the closest point on the edge's underlying
    /// curve to the world-space query point `(px, py, pz)` (in metres).
    ///
    /// Delegates to `occt::curve_curvature_at` which:
    ///   (a) extracts the underlying `Geom_Curve` from the edge via
    ///       `BRep_Tool::Curve(edge, f, l)`,
    ///   (b) projects `(px, py, pz)` onto [f, l] via `GeomAPI_ProjectPointOnCurve`,
    ///   (c) evaluates curvature at the projected parameter via `BRepLProp_CLProps`.
    ///
    /// Sign follows the Frenet frame (positive toward principal normal). For a
    /// full circle in the XY plane OCCT returns κ > 0.
    ///
    /// Returns `Err(QueryError::InvalidHandle)` if `handle` is unknown.
    /// Returns `Err(QueryError::QueryFailed)` if the shape is not an edge, has
    /// no underlying curve, projection fails, or the tangent is undefined.
    pub fn curve_curvature_at(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<f64, QueryError> {
        let s = self.get_shape(handle)?;
        if !px.is_finite() || !py.is_finite() || !pz.is_finite() {
            return Err(QueryError::QueryFailed(
                "curve_curvature_at: query point coordinates must be finite".into(),
            ));
        }
        ffi::ffi::curve_curvature_at(s, px, py, pz)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Test whether the query point `(px, py, pz)` lies on the BREP boundary
    /// (face/edge/vertex) of the shape identified by `handle`, within `tolerance`.
    ///
    /// Uses `BRepExtrema_DistShapeShape(shape, vertex)` where the vertex is built
    /// from the query point, returning `dist.Value() <= tolerance`.
    ///
    /// **Interior solid points return `true` (OCCT overlap behavior):**
    /// `BRepExtrema_DistShapeShape` has NO inside/outside knowledge. When the query
    /// vertex is strictly inside a `TopoDS_Solid`, OCCT considers the two shapes to
    /// overlap and reports `dist.Value() = 0` (NOT the distance to the nearest BREP
    /// face). Therefore this method returns `Ok(true)` for any interior solid point
    /// at any positive tolerance, and CANNOT distinguish a point on the BREP surface
    /// from a point inside the solid for `TopoDS_Solid` inputs. Callers needing strict
    /// surface-only membership must apply a `BRepClass3d_SolidClassifier` pre-filter;
    /// the integration test `point_on_shape_interior_solid_point_returns_true` locks
    /// this contract in. See parent task 2324 for stdlib-level wiring decisions.
    ///
    /// Callers commonly pass `Precision::Confusion()` (~1e-7), exposed as
    /// [`reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`], for `tolerance` to match OCCT's
    /// default confusion threshold. Pass 0.0 for exact-coincidence queries
    /// (returns `true` only when `dist.Value()` is exactly 0).
    ///
    /// **Tolerance precondition:** `tolerance` must be a non-negative finite `f64`.
    /// Negative or NaN values map to `Err(QueryError::QueryFailed(_))` rather than
    /// silently producing misleading results.
    ///
    /// **Naming caveat:** The name `point_on_shape` implies surface membership, but
    /// for `TopoDS_Solid` inputs this method cannot distinguish "on BREP surface" from
    /// "inside the solid" (see "Interior solid points" note above). A higher-level
    /// wrapper that applies a `BRepClass3d_SolidClassifier` pre-filter for strict
    /// surface-only membership is tracked in escalation esc-2829-6 / parent task 2324.
    ///
    /// Returns `Err(QueryError::InvalidHandle(_))` if `handle` is unknown, or
    /// `Err(QueryError::QueryFailed(_))` if the OCCT computation fails.
    pub fn point_on_shape(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
        tolerance: f64,
    ) -> Result<bool, QueryError> {
        let s = self.get_shape(handle)?;
        ffi::ffi::point_on_shape(s, px, py, pz, tolerance)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Test whether `(px, py, pz)` is inside or on the boundary of the closed
    /// solid identified by `handle`.
    ///
    /// Uses `BRepClass3d_SolidClassifier(shape).Perform(gp_Pnt, tolerance)` and
    /// returns `true` when `State() == TopAbs_IN || State() == TopAbs_ON` (closed-
    /// solid membership; both interior and boundary points are "contained").
    ///
    /// Pass [`reify_ir::DEFAULT_CONTAINS_TOLERANCE_M`] (= `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`
    /// ≈ OCCT's `Precision::Confusion()`, ~1e-7) for `tolerance` to match OCCT's
    /// default confusion threshold.
    ///
    /// **Tolerance precondition:** `tolerance` must be a non-negative finite `f64`.
    /// Negative or NaN values map to `Err(QueryError::QueryFailed(_))`.
    ///
    /// Returns `Err(QueryError::InvalidHandle(_))` if `handle` is unknown, or
    /// `Err(QueryError::QueryFailed(_))` if the OCCT computation fails.
    pub fn contains(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
        tolerance: f64,
    ) -> Result<bool, QueryError> {
        let s = self.get_shape(handle)?;
        ffi::ffi::contains_solid(s, px, py, pz, tolerance)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Test whether shapes `left` and `right` are geometrically equivalent
    /// within `tolerance` using topology-count matching + sampled-vertex
    /// proximity.
    ///
    /// STRICT-VARIANT NOTE: This is the asymmetric sampled-point geo_equiv
    /// (PRD §5.1, KGQ-δ).  A future `geo_equiv_strict` using symmetric
    /// Hausdorff distance is deferred to v0.4 per PRD §5.1 + Open Question §10.
    ///
    /// The sample count is threaded from [`reify_ir::DEFAULT_GEO_EQUIV_SAMPLE_COUNT`]
    /// (= 8 per PRD §5.2) so that constant remains the single authoritative source.
    ///
    /// Returns `Err(QueryError::InvalidHandle(_))` if either handle is unknown.
    /// Returns `Err(QueryError::QueryFailed(_))` if `tolerance` is non-finite
    /// or negative, or if the OCCT computation fails.
    pub fn geo_equiv(
        &self,
        left: GeometryHandleId,
        right: GeometryHandleId,
        tolerance: f64,
    ) -> Result<bool, QueryError> {
        let s_left = self.get_shape(left)?;
        let s_right = self.get_shape(right)?;
        ffi::ffi::geo_equiv_topo_sample(s_left, s_right, tolerance, reify_ir::DEFAULT_GEO_EQUIV_SAMPLE_COUNT)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    /// Fuse `left` and `right` via `BRepAlgoAPI_Fuse` and return the
    /// fused-result handle alongside the per-parent face/edge history
    /// records (Modified / Generated / Deleted).
    ///
    /// The result is NORMALIZED (unwrapped to the tightest topology-preserving
    /// type, then same-domain-unified) exactly like the plain `boolean_fuse`
    /// arm, and the handle's `BRepKind` is classified from that real shape —
    /// so a disjoint fuse registers as the multi-body
    /// `BRepKind::Compound`, not `Solid`. The history records describe the
    /// parent ↔ result correspondence emitted by
    /// `BRepAlgoAPI_Fuse::Modified()`, `.Generated()`, and
    /// `.IsDeleted()` for each parent's faces and edges; consumers (the
    /// v0.2 propagation helper in `reify-eval`) use them to copy parent
    /// topology attributes onto the result handles.
    ///
    /// Returns `Err(GeometryError::InvalidReference(_))` if either handle
    /// is unknown to this kernel, or `Err(GeometryError::OperationFailed(_))`
    /// if the OCCT call fails.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2590, step-14). Cut and
    /// Common siblings land in task 8 per docs/prds/v0_2/persistent-naming-v2.md.
    pub fn boolean_fuse_with_history(
        &mut self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError> {
        // Run the FFI call inside a scope that drops the immutable borrow
        // before we mutate `self` via `store_with_policy`.
        let (result_shape, records) = {
            let l = self.get_shape(left)?;
            let r = self.get_shape(right)?;
            let history = ffi::ffi::boolean_fuse_with_history(l, r)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_boolean_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Run `BRepAlgoAPI_Cut` on `left` and `right` (left − right), capturing
    /// the per-parent face/edge Modified/Generated/Deleted history records
    /// alongside the result shape.
    ///
    /// The result is normalized like the plain `boolean_cut` arm and its
    /// `BRepKind` is classified from the real shape (see `brep_kind_of_shape`),
    /// so an empty or multi-body result is `BRepKind::Compound`, not `Solid`.
    ///
    /// Returns `Err(GeometryError::InvalidReference(_))` if either handle
    /// is unknown to this kernel, or `Err(GeometryError::OperationFailed(_))`
    /// if the OCCT call fails.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-2).
    pub fn boolean_cut_with_history(
        &mut self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError> {
        let (result_shape, records) = {
            let l = self.get_shape(left)?;
            let r = self.get_shape(right)?;
            let history = ffi::ffi::boolean_cut_with_history(l, r)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_boolean_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Run `BRepAlgoAPI_Common` on `left` and `right` (A ∩ B), capturing
    /// the per-parent face/edge Modified/Generated/Deleted history records
    /// alongside the result shape.
    ///
    /// The result is normalized like the plain `boolean_common` arm and its
    /// `BRepKind` is classified from the real shape (see `brep_kind_of_shape`),
    /// so an empty or multi-body result is `BRepKind::Compound`, not `Solid`.
    ///
    /// Returns `Err(GeometryError::InvalidReference(_))` if either handle
    /// is unknown to this kernel, or `Err(GeometryError::OperationFailed(_))`
    /// if the OCCT call fails.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-4).
    pub fn boolean_common_with_history(
        &mut self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError> {
        let (result_shape, records) = {
            let l = self.get_shape(left)?;
            let r = self.get_shape(right)?;
            let history = ffi::ffi::boolean_common_with_history(l, r)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_boolean_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Shared body for `fillet_with_history` and `chamfer_with_history`.
    ///
    /// Looks up `shape_id`, calls `build` to run the OCCT algorithm and obtain a
    /// `LocalFeatureOpHistory`, decodes all six record buffers, then registers the
    /// result shape under its classified `BRepKind`.  Error mapping (cxx exceptions →
    /// `GeometryError::OperationFailed`) is handled here so public callers stay
    /// concise.
    ///
    /// `build` receives `&OcctShape` (borrowed from `self`).  The closure must
    /// return the raw `Result` from the cxx bridge so that the error type's
    /// `Display` impl can be used to produce a descriptive `OperationFailed`
    /// message.  Because the `UniquePtr<LocalFeatureOpHistory>` returned by
    /// `build` does not borrow from the `OcctShape`, the immutable borrow of
    /// `self` ends as soon as `build` returns, allowing the subsequent
    /// `store_with_repr` call to take `self` mutably.
    fn run_local_feature_with_history<F, E>(
        &mut self,
        shape_id: GeometryHandleId,
        build: F,
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>
    where
        F: FnOnce(
            &ffi::ffi::OcctShape,
        ) -> Result<cxx::UniquePtr<ffi::ffi::LocalFeatureOpHistory>, E>,
        E: std::fmt::Display,
    {
        // DEFENSE-IN-DEPTH (task 2821 / esc-2655-26 #4): `BRepFilletAPI_MakeFillet`
        // and `BRepFilletAPI_MakeChamfer` iterate parent edges of ONE solid; a
        // Face / Edge / Wire / Shell / Compound input would either fail inside
        // OCCT or fillet a multi-body aggregate. The result is Classify-stamped.
        //
        // REACH: every fillet/chamfer entry point — `execute`'s all-edges and
        // curated arms and each `*_with_history` variant — runs through this one
        // guard. A disjoint `union(a, b)` classifies as `BRepKind::Compound`, and
        // a transform inherits its input's repr (`ReprPolicy::Inherit`), so
        // `fillet(translate(union(a, b)))` is rejected like `fillet(union(a, b))`.
        // Pinned by `repr_policy_table.rs`'s D5 block and
        // `occt_repr_policy_fixtures_e2e.rs`.
        match self.repr_of(shape_id) {
            Some(BRepKind::Solid) => {}
            Some(other) => {
                return Err(GeometryError::OperationFailed(format!(
                    "local-feature operation requires a BRepKind::Solid input shape, got {other:?}"
                )));
            }
            // `repr_of` returns `None` iff `shape_id` is unknown to this kernel.
            // The "repr-lost-but-shape-present" case is unreachable: `store_with_repr`
            // and `with_warm_state` keep `self.shapes` and `self.reprs` in lock-step
            // (see `repr_of` doc).  Fall through to `get_shape` below, which returns
            // `InvalidReference` for genuinely unknown handles rather than a false
            // "requires Solid" rejection (esc-2655-26 suggestion #4 / task 2821
            // amendment; comment aligned by task 2905 / esc-2821-61 #1).
            None => {}
        }
        let (result_shape, records) = {
            let shape = self.get_shape(shape_id)?;
            let history =
                build(shape).map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_local_feature_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Apply `BRepFilletAPI_MakeFillet` to every edge of `shape_id` with the
    /// given `radius`, returning the modified-result handle alongside the
    /// per-parent face/edge Modified/Generated/Deleted history records.
    ///
    /// `radius` must be a finite, strictly positive number.
    ///
    /// `parent_index` in every record is `0` (single parent solid).
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-2).
    pub fn fillet_with_history(
        &mut self,
        shape_id: GeometryHandleId,
        radius: f64,
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError> {
        validate_positive_finite(radius, "fillet radius")?;
        self.run_local_feature_with_history(shape_id, |shape| {
            ffi::ffi::make_fillet_with_history(shape, radius)
        })
    }

    /// Apply `BRepFilletAPI_MakeFillet` to ONLY the `edges` of `shape_id` (a
    /// curated subset) with the given `radius`, returning the modified-result
    /// handle alongside the per-parent face/edge Modified/Generated/Deleted
    /// history records.
    ///
    /// Each [`GeometryHandleId`] in `edges` must be an edge of `shape_id` (as
    /// minted by [`Self::extract_edges`]); the `extracted_edges` cache is warmed
    /// here if cold so a freshly-built solid resolves without the caller calling
    /// `extract_edges` first. Each handle is mapped to its 0-based position in
    /// the parent's canonical edge enumeration and passed to the kernel, which
    /// applies the fillet to exactly those edges. An empty selection is rejected
    /// up-front — the all-edges path is [`Self::fillet_with_history`], never
    /// this method silently falling through.
    ///
    /// `radius` must be finite and strictly positive. `parent_index` in every
    /// record is `0`.
    ///
    /// Curated edge-selection seam (task 3205). The persistent-naming history
    /// seam is preserved identically to the all-edges path.
    pub fn fillet_edges_with_history(
        &mut self,
        shape_id: GeometryHandleId,
        radius: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError> {
        validate_positive_finite(radius, "fillet radius")?;
        if edges.is_empty() {
            return Err(GeometryError::OperationFailed(
                "fillet_edges_with_history: edge selection must be non-empty \
                 (the all-edges path is fillet_with_history)"
                    .into(),
            ));
        }
        let edge_indices = self.map_selection_to_indices(
            shape_id,
            Some(edges),
            SubShapeKind::Edge,
            "fillet_edges_with_history",
        )?;
        self.run_local_feature_with_history(shape_id, |shape| {
            ffi::ffi::make_fillet_edges_with_history(shape, radius, &edge_indices)
        })
    }

    /// Apply `BRepOffsetAPI_DraftAngle` to a curated subset of faces of
    /// `target` at `angle_rad` radians, using `plane` as the neutral plane.
    ///
    /// `faces` must be non-empty (defense-in-depth; the all-faces path is
    /// the `GeometryOp::Draft { faces: vec![], .. }` execute arm which calls
    /// `draft_shape` directly). Each handle in `faces` must belong to `target`;
    /// handles are mapped to their 0-based canonical positions via
    /// `extract_faces` (TopExp::MapShapes order, same as the eval-arm seam).
    ///
    /// The result is stored history-free — mirroring the existing `draft_shape`
    /// execute arm, which does not participate in persistent-naming.
    pub fn draft_faces(
        &mut self,
        target: GeometryHandleId,
        angle_rad: f64,
        plane: GeometryHandleId,
        faces: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        if faces.is_empty() {
            return Err(GeometryError::OperationFailed(
                "draft_faces: face selection must be non-empty \
                 (the all-faces path uses draft_shape / empty faces vec)"
                    .into(),
            ));
        }
        let face_indices =
            self.map_selection_to_indices(target, Some(faces), SubShapeKind::Face, "draft_faces")?;
        let shape = self.get_shape(target)?;
        let plane_shape = self.get_shape(plane)?;
        let result_shape =
            ffi::ffi::draft_faces_shape(shape, angle_rad, plane_shape, &face_indices)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
        self.store_with_policy(result_shape, ReprPolicy::Classify)
    }

    /// Apply `BRepOffsetAPI_MakeThickSolid` to the curated face subset
    /// identified by `faces` (a list of `GeometryHandleId` sub-handles
    /// extracted from `target`), returning the resulting hollowed solid.
    ///
    /// This is the curated-handle companion to the numeric
    /// `GeometryOp::Shell { faces_to_remove }` path, and the kernel-side
    /// implementation of `shell_open(solid, thickness, open_faces)`.
    ///
    /// # Errors
    ///
    /// - `OperationFailed` if `faces` is empty (empty selection must be
    ///   rejected rather than silently shelling all faces).
    /// - `OperationFailed` if any handle in `faces` is not a member of
    ///   `target`'s canonical face enumeration (foreign-face guard).
    /// - `OperationFailed` if the underlying OCCT operation fails.
    pub fn shell_solid_faces(
        &mut self,
        target: GeometryHandleId,
        thickness: f64,
        faces: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        if faces.is_empty() {
            return Err(GeometryError::OperationFailed(
                "shell_solid_faces: face selection must be non-empty \
                 (the all-faces path uses shell_shape / numeric faces_to_remove)"
                    .into(),
            ));
        }
        let face_indices = self.map_selection_to_indices(
            target,
            Some(faces),
            SubShapeKind::Face,
            "shell_solid_faces",
        )?;
        let shape = self.get_shape(target)?;
        let result_shape = ffi::ffi::shell_shape(shape, thickness, &face_indices)
            .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
        self.store_with_policy(result_shape, ReprPolicy::Classify)
    }

    /// Apply `BRepFilletAPI_MakeChamfer` to every edge of `shape_id` with the
    /// given `distance`, returning the modified-result handle alongside the
    /// per-parent face/edge Modified/Generated/Deleted history records.
    ///
    /// `distance` must be a finite, strictly positive number.
    ///
    /// `parent_index` in every record is `0` (single parent solid).
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-6).
    pub fn chamfer_with_history(
        &mut self,
        shape_id: GeometryHandleId,
        distance: f64,
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError> {
        validate_positive_finite(distance, "chamfer distance")?;
        self.run_local_feature_with_history(shape_id, |shape| {
            ffi::ffi::make_chamfer_with_history(shape, distance)
        })
    }

    /// Apply `BRepFilletAPI_MakeChamfer` to ONLY the selected `edges` of
    /// `shape_id` (a curated subset) with the given `distance`, returning the
    /// modified-result handle alongside the per-parent face/edge
    /// Modified/Generated/Deleted history records.
    ///
    /// `distance` must be finite and strictly positive. `parent_index` in every
    /// record is `0`.
    ///
    /// Curated edge-selection seam (task 4185, β). The persistent-naming history
    /// seam is preserved identically to the all-edges path. An empty `edges`
    /// slice is rejected — the all-edges path is `chamfer_with_history`.
    pub fn chamfer_edges_with_history(
        &mut self,
        shape_id: GeometryHandleId,
        distance: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError> {
        validate_positive_finite(distance, "chamfer distance")?;
        if edges.is_empty() {
            return Err(GeometryError::OperationFailed(
                "chamfer_edges_with_history: edge selection must be non-empty \
                 (the all-edges path is chamfer_with_history)"
                    .into(),
            ));
        }
        let edge_indices = self.map_selection_to_indices(
            shape_id,
            Some(edges),
            SubShapeKind::Edge,
            "chamfer_edges_with_history",
        )?;
        self.run_local_feature_with_history(shape_id, |shape| {
            ffi::ffi::make_chamfer_edges_with_history(shape, distance, &edge_indices)
        })
    }

    /// Apply `BRepFilletAPI_MakeChamfer` with ASYMMETRIC setbacks (`d1` on one
    /// adjacent face, `d2` on the other) to a curated subset of `edges` of
    /// `shape_id`, via `MakeChamfer::Add(d1, d2, E, F)` per edge. Returns the
    /// modified-result handle alongside the per-parent face/edge
    /// Modified/Generated/Deleted history records.
    ///
    /// Both `d1` and `d2` must be finite and strictly positive. `parent_index`
    /// in every record is `0`.
    ///
    /// An empty `edges` slice means ALL edges (back-compat, mirroring the
    /// symmetric/Fillet contract): the wrapper enumerates every parent edge
    /// index, so the kernel still receives a non-empty selection (there is no
    /// separate asymmetric all-edges FFI). The reference face F for each edge is
    /// chosen kernel-side (first adjacent face); see
    /// `make_chamfer_asymmetric_edges_with_history`.
    ///
    /// Curated edge-selection seam (task 4185, β).
    pub fn chamfer_asymmetric_edges_with_history(
        &mut self,
        shape_id: GeometryHandleId,
        d1: f64,
        d2: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError> {
        validate_positive_finite(d1, "chamfer d1")?;
        validate_positive_finite(d2, "chamfer d2")?;
        // Empty selection = all edges, enumerated so the back-compat holds
        // without a dedicated all-edges FFI.
        let selection = (!edges.is_empty()).then_some(edges);
        let edge_indices = self.map_selection_to_indices(
            shape_id,
            selection,
            SubShapeKind::Edge,
            "chamfer_asymmetric_edges_with_history",
        )?;
        self.run_local_feature_with_history(shape_id, |shape| {
            ffi::ffi::make_chamfer_asymmetric_edges_with_history(shape, d1, d2, &edge_indices)
        })
    }

    /// Extrude `profile` along the +Z direction by `distance` metres via
    /// `BRepPrimAPI_MakePrism`, returning the swept-result handle alongside
    /// the per-parent face/edge Modified/Generated/Deleted history records
    /// AND the FirstShape/LastShape cap-face indices.
    ///
    /// Mirrors the call convention of the `GeometryOp::Extrude` arm of
    /// [`OcctKernel::execute`]: profile is extruded along `(0, 0, distance)`,
    /// validated to be finite and non-zero.
    ///
    /// `parent_index` in every record is `0` (single parent profile).
    /// `start_cap_face_indices` carries the FirstShape() face index (the
    /// profile-as-placed); `end_cap_face_indices` carries the LastShape()
    /// face index (the swept-end face).
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    pub fn extrude_with_history(
        &mut self,
        profile_id: GeometryHandleId,
        distance: f64,
    ) -> Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError> {
        validate_extrude_distance(distance, "extrude")?;
        let (result_shape, records) = {
            let profile_shape = self.get_shape(profile_id)?;
            let history = ffi::ffi::make_prism_with_history(profile_shape, 0.0, 0.0, distance)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_sweep_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Revolve `profile` about the axis with origin `axis_origin` and
    /// direction `axis_dir` by `angle_rad` radians via
    /// `BRepPrimAPI_MakeRevol`, returning the swept-result handle alongside
    /// the per-parent face/edge Modified/Generated/Deleted history records
    /// AND (for partial revolutions) the FirstShape/LastShape cap-face
    /// indices.
    ///
    /// Mirrors the call convention of the `GeometryOp::Revolve` arm of
    /// [`OcctKernel::execute`]: same finite/non-zero/non-degenerate
    /// validation thresholds (AXIS_MAG_SQ_MIN, ANGLE_ABS_MIN).
    ///
    /// Cap behavior:
    /// - PARTIAL revolution (angle_rad mod 2π ∈ (0, 2π)) →
    ///   `start_cap_face_indices` carries the FirstShape() face index
    ///   (profile-as-placed); `end_cap_face_indices` carries the
    ///   LastShape() face index (profile rotated by angle_rad).
    /// - FULL revolution (angle_rad's modulo-2π residual zero, or
    ///   `FirstShape().IsSame(LastShape())`) → both lists empty.
    ///
    /// `parent_index` in every record is `0` (single parent profile).
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-10).
    pub fn revolve_with_history(
        &mut self,
        profile_id: GeometryHandleId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    ) -> Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError> {
        validate_revolve_params(axis_origin, axis_dir, angle_rad)?;
        let (result_shape, records) = {
            let profile_shape = self.get_shape(profile_id)?;
            let history = ffi::ffi::make_revolve_with_history(
                profile_shape,
                axis_origin[0],
                axis_origin[1],
                axis_origin[2],
                axis_dir[0],
                axis_dir[1],
                axis_dir[2],
                angle_rad,
            )
            .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_sweep_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Sweep `profile` along `path` (a wire) via `BRepOffsetAPI_MakePipe`,
    /// returning the swept-result handle alongside the per-parent
    /// face/edge Modified/Generated/Deleted history records AND the
    /// FirstShape/LastShape cap-face indices.
    ///
    /// Mirrors the call convention of the `GeometryOp::Sweep` arm of
    /// [`OcctKernel::execute`]: same handle resolution, the same
    /// [`validate_sweep_pose`] degenerate-pose refusal, and the same
    /// input-dependent `brep_kind_of_shape` stamp. Sharing both is deliberate —
    /// this method is the entry point `OcctKernelHandle`'s worker routes
    /// `GeometryOp::Sweep` to, so the two sites must not disagree about which
    /// poses are legal, nor about what the result IS, the way they did while
    /// each stamped its own unconditional `BRepKind::Solid` (#5713).
    /// Sweep is single-parent
    /// (the profile is the operand; the path is the spine along which
    /// the profile is swept), so `parent_index` is always `0` and the
    /// existing `SweepOpHistoryRecords` struct fits verbatim.
    ///
    /// Cap behavior: `start_cap_face_indices` carries the FirstShape()
    /// face index (profile-as-placed at the spine start);
    /// `end_cap_face_indices` carries the LastShape() face index
    /// (profile at the spine end).
    ///
    /// Diagnostic counters `unsynthesized_profile_edge_count` and
    /// `duplicate_parent_subshape_index_count` are always `0` for
    /// sweep — those are revolve-synthesis-specific (full-revolution
    /// radial-edge synthesis post-pass) and have no analogue in pipe
    /// sweeping.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-4).
    pub fn sweep_with_history(
        &mut self,
        profile_id: GeometryHandleId,
        path_id: GeometryHandleId,
    ) -> Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError> {
        let (result_shape, records) = {
            let profile_shape = self.get_shape(profile_id)?;
            let path_shape = self.get_shape(path_id)?;
            validate_sweep_pose(profile_shape, path_shape)?;
            let history = ffi::ffi::make_pipe_with_history(profile_shape, path_shape)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_sweep_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }

    /// Loft through `profile_ids` (>= 2 wire profiles) via
    /// `BRepOffsetAPI_ThruSections(is_solid=true, is_ruled=false)`,
    /// returning the lofted result handle alongside the per-section
    /// (multi-parent) face-correspondence records and the
    /// FirstShape/LastShape cap-face indices.
    ///
    /// Mirrors the call convention of the `GeometryOp::Loft` arm of
    /// [`OcctKernel::execute`]: same handle resolution, same
    /// `profiles.len() < 2` validation, and same result classification.
    /// Loft is **multi-parent** — each profile
    /// section is a distinct parent indexed `0..N-1`, and
    /// `LoftOpHistoryRecords::face_generated`'s `parent_index` field
    /// denotes the section index (NOT always 0 like sweep / extrude /
    /// revolve), so a separate `LoftOpHistoryRecords` struct (mirroring
    /// `SweepOpHistoryRecords`'s field shape but without the diagnostic
    /// counters) carries the result.
    ///
    /// `start_cap_face_indices` carries the FirstShape() face index (first
    /// profile section under `is_solid=true`); `end_cap_face_indices`
    /// carries the LastShape() face index (last profile section). Both
    /// are populated under the hard-coded `is_solid=true` semantics
    /// matching `GeometryOp::Loft`'s contract.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-6).
    pub fn loft_with_history(
        &mut self,
        profile_ids: &[GeometryHandleId],
    ) -> Result<(GeometryHandle, LoftOpHistoryRecords), GeometryError> {
        validate_loft_profile_count(profile_ids.len())?;
        let (result_shape, records) = {
            // Build an OcctShapeVec from the profile handles. Reuses the
            // same `new_shape_vec` / `shape_vec_push` builder pattern as
            // the non-history `loft_profiles` arm of `execute`.
            let mut vec = ffi::ffi::new_shape_vec();
            for &pid in profile_ids {
                let shape = self.get_shape(pid)?;
                ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
            }
            // is_solid hard-coded to `true` to match `GeometryOp::Loft`'s
            // contract; the FFI exposes the flag for forward-compatibility.
            let history = ffi::ffi::make_loft_with_history(&vec, true)
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
            decode_loft_history(history)
        };
        let handle = self.store_with_policy(result_shape, ReprPolicy::Classify)?;
        Ok((handle, records))
    }
}

#[cfg(has_occt)]
impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(has_occt)]
/// Inherent methods — same bodies as the former `GeometryKernel` impl.
/// Called directly by the kernel thread in `OcctKernelHandle`.
impl OcctKernel {
    pub fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        let outcome = match op {
            GeometryOp::Box {
                size_x,
                size_y,
                size_z,
            } => {
                let x = extract_length_f64(size_x, op, "size_x")?;
                let y = extract_length_f64(size_y, op, "size_y")?;
                let z = extract_length_f64(size_z, op, "size_z")?;
                validate_all_positive_finite(&[x, y, z], BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE)?;
                let built = ffi::ffi::make_box(x, y, z)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Cylinder { radius, height } => {
                let r = extract_length_f64(radius, op, "radius")?;
                let h = extract_length_f64(height, op, "height")?;
                validate_positive_finite(r, "cylinder radius")?;
                validate_positive_finite(h, "cylinder height")?;
                let built = ffi::ffi::make_cylinder(r, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Sphere { radius } => {
                let r = extract_length_f64(radius, op, "radius")?;
                validate_all_positive_finite(&[r], SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE)?;
                let built = ffi::ffi::make_sphere(r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Tube {
                outer_r,
                inner_r,
                height,
            } => {
                let outer = extract_length_f64(outer_r, op, "outer_r")?;
                let inner = extract_length_f64(inner_r, op, "inner_r")?;
                let h = extract_length_f64(height, op, "height")?;
                validate_positive_finite(outer, "tube outer radius")?;
                validate_positive_finite(inner, "tube inner radius")?;
                validate_positive_finite(h, "tube height")?;
                // Both values are already validated finite+positive above,
                // so `>=` is unambiguous here (no NaN possible).
                if inner >= outer {
                    return Err(GeometryError::OperationFailed(
                        "tube inner radius must be strictly less than outer radius".into(),
                    ));
                }
                // Compose: outer cylinder - inner cylinder via boolean_cut.
                // Reuses validated FFI primitives rather than adding a
                // bespoke C++ wrapper (mirrors ExtrudeSymmetric pattern).
                let outer_shape = ffi::ffi::make_cylinder(outer, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let inner_shape = ffi::ffi::make_cylinder(inner, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let built = ffi::ffi::boolean_cut(&outer_shape, &inner_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Cone {
                bottom_radius,
                top_radius,
                height,
            } => {
                let bottom_r = extract_length_f64(bottom_radius, op, "bottom_radius")?;
                let top_r = extract_length_f64(top_radius, op, "top_radius")?;
                let h = extract_length_f64(height, op, "height")?;
                if !(bottom_r.is_finite() && bottom_r >= 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "cone bottom_radius must be finite and non-negative".into(),
                    ));
                }
                if !(top_r.is_finite() && top_r >= 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "cone top_radius must be finite and non-negative".into(),
                    ));
                }
                if bottom_r == 0.0 && top_r == 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "cone both radii are zero — degenerate line, at least one radius must be positive".into(),
                    ));
                }
                validate_positive_finite(h, "cone height")?;
                let built = ffi::ffi::make_cone(bottom_r, top_r, h)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Wedge {
                width,
                depth,
                height,
                top_width,
            } => {
                let w = extract_length_f64(width, op, "width")?;
                let d = extract_length_f64(depth, op, "depth")?;
                let h = extract_length_f64(height, op, "height")?;
                let ltx = extract_length_f64(top_width, op, "top_width")?;
                validate_positive_finite(w, "wedge width")?;
                validate_positive_finite(d, "wedge depth")?;
                validate_positive_finite(h, "wedge height")?;
                if !(ltx.is_finite() && ltx >= 0.0) {
                    return Err(GeometryError::OperationFailed(
                        "wedge top_width must be finite and non-negative".into(),
                    ));
                }
                let built = ffi::ffi::make_wedge(w, d, h, ltx)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Torus {
                major_radius,
                minor_radius,
            } => {
                let major = extract_length_f64(major_radius, op, "major_radius")?;
                let minor = extract_length_f64(minor_radius, op, "minor_radius")?;
                validate_positive_finite(major, "torus major radius")?;
                validate_positive_finite(minor, "torus minor radius")?;
                // Both values are already validated finite+positive above,
                // so `>=` is unambiguous here (no NaN possible). A minor radius
                // at or beyond the major radius is a self-intersecting torus.
                if minor >= major {
                    return Err(GeometryError::OperationFailed(
                        "torus minor radius must be strictly less than major radius".into(),
                    ));
                }
                let built = ffi::ffi::make_torus(major, minor)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::HalfSpace {
                px,
                py,
                pz,
                nx,
                ny,
                nz,
            } => {
                let px = extract_length_f64(px, op, "px")?;
                let py = extract_length_f64(py, op, "py")?;
                let pz = extract_length_f64(pz, op, "pz")?;
                // not length-semantic: dimensionless unit-normal component, not a length.
                let nx = extract_f64(nx)?;
                // not length-semantic: dimensionless unit-normal component, not a length.
                let ny = extract_f64(ny)?;
                // not length-semantic: dimensionless unit-normal component, not a length.
                let nz = extract_f64(nz)?;
                // Validate that the normal is non-zero (gp_Dir requires it).
                let norm_sq = nx * nx + ny * ny + nz * nz;
                if !norm_sq.is_finite() || norm_sq == 0.0 {
                    return Err(GeometryError::OperationFailed(
                        "half_space normal must be a non-zero finite vector".into(),
                    ));
                }
                let built = ffi::ffi::make_half_space(px, py, pz, nx, ny, nz)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Fixed(BRepKind::Solid))
            }
            GeometryOp::Union { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                let fused = ffi::ffi::boolean_fuse(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(fused, ReprPolicy::Classify)
            }
            GeometryOp::Difference { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                let cut = ffi::ffi::boolean_cut(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(cut, ReprPolicy::Classify)
            }
            GeometryOp::Intersection { left, right } => {
                let l = self.get_shape(*left)?;
                let r = self.get_shape(*right)?;
                let common = ffi::ffi::boolean_common(l, r)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(common, ReprPolicy::Classify)
            }
            GeometryOp::Fillet {
                target,
                edges,
                radius,
            } => {
                let r = extract_length_f64(radius, op, "radius")?;
                // Empty `edges` = all edges (2-arg back-compat). Neither
                // primitive's history is surfaced through execute().
                let handle = if edges.is_empty() {
                    self.fillet_with_history(*target, r)?.0
                } else {
                    self.fillet_edges_with_history(*target, r, edges)?.0
                };
                ArmOutcome::Stored(handle)
            }
            GeometryOp::Chamfer {
                target,
                edges,
                distance,
            } => {
                let d = extract_length_f64(distance, op, "distance")?;
                // Empty `edges` = all edges (2-arg back-compat). Neither
                // primitive's history is surfaced through execute().
                let handle = if edges.is_empty() {
                    self.chamfer_with_history(*target, d)?.0
                } else {
                    self.chamfer_edges_with_history(*target, d, edges)?.0
                };
                ArmOutcome::Stored(handle)
            }
            GeometryOp::ChamferAsymmetric {
                target,
                edges,
                d1,
                d2,
            } => {
                let d1 = extract_length_f64(d1, op, "d1")?;
                let d2 = extract_length_f64(d2, op, "d2")?;
                // Asymmetric chamfer always routes to the per-edge primitive
                // (empty `edges` = all-edges, enumerated inside the kernel
                // wrapper). The history is captured for the persistent-naming
                // seam but not surfaced through execute().
                let (handle, _history) =
                    self.chamfer_asymmetric_edges_with_history(*target, d1, d2, edges)?;
                ArmOutcome::Stored(handle)
            }
            GeometryOp::Translate { target, dx, dy, dz } => {
                let shape = self.get_shape(*target)?;
                if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() {
                    return Err(GeometryError::OperationFailed(format!(
                        "translate components must be finite values: dx={dx}, dy={dy}, dz={dz}"
                    )));
                }
                let built = ffi::ffi::translate_shape(shape, *dx, *dy, *dz)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::Rotate {
                target,
                axis,
                angle_rad,
            } => {
                let shape = self.get_shape(*target)?;
                if !axis[0].is_finite()
                    || !axis[1].is_finite()
                    || !axis[2].is_finite()
                    || !angle_rad.is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "rotate parameters must be finite values: axis=[{}, {}, {}], angle_rad={}",
                        axis[0], axis[1], axis[2], angle_rad
                    )));
                }
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "rotation axis must not be zero-length".into(),
                    ));
                }
                let built = ffi::ffi::rotate_shape(shape, axis[0], axis[1], axis[2], *angle_rad)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            } => {
                let shape = self.get_shape(*target)?;
                let sp = extract_length_f64(spacing, op, "spacing")?;
                if *count == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear pattern count must be >= 1".into(),
                    ));
                }
                let built = ffi::ffi::linear_pattern(
                    shape,
                    direction[0],
                    direction[1],
                    direction[2],
                    *count as u32,
                    sp,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            } => {
                let shape = self.get_shape(*target)?;
                // not length-semantic: ANGLE, not LENGTH — PRD 3's surface, not this one.
                let total_angle = extract_f64(angle)?;
                if *count == 0 {
                    return Err(GeometryError::OperationFailed(
                        "circular pattern count must be >= 1".into(),
                    ));
                }
                let built = ffi::ffi::circular_pattern(
                    shape,
                    axis_origin[0],
                    axis_origin[1],
                    axis_origin[2],
                    axis_dir[0],
                    axis_dir[1],
                    axis_dir[2],
                    *count as u32,
                    total_angle,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            } => {
                let shape = self.get_shape(*target)?;
                // Validate plane normal is non-zero
                let mag_sq = plane_normal[0] * plane_normal[0]
                    + plane_normal[1] * plane_normal[1]
                    + plane_normal[2] * plane_normal[2];
                if !plane_normal[0].is_finite()
                    || !plane_normal[1].is_finite()
                    || !plane_normal[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "mirror plane normal must be a finite non-zero vector".into(),
                    ));
                }
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "mirror plane normal must not be zero-length".into(),
                    ));
                }
                let built = ffi::ffi::mirror_shape(
                    shape,
                    plane_origin[0],
                    plane_origin[1],
                    plane_origin[2],
                    plane_normal[0],
                    plane_normal[1],
                    plane_normal[2],
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::Loft { profiles } => {
                validate_loft_profile_count(profiles.len())?;
                let mut vec = ffi::ffi::new_shape_vec();
                for &pid in profiles {
                    let shape = self.get_shape(pid)?;
                    ffi::ffi::shape_vec_push(vec.pin_mut(), shape);
                }
                let built = ffi::ffi::loft_profiles(&vec)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::Draft {
                target,
                faces,
                angle,
                plane,
            } => {
                // not length-semantic: ANGLE, not LENGTH — PRD 3's surface, not this one.
                // The magnitude taken is SI RADIANS and crosses to the FFI
                // unconverted; this line is where the dimension tag is
                // discarded. Contract: `GeometryOp::Draft.angle` (INV-AD-4).
                let angle_rad = extract_f64(angle)?;
                if faces.is_empty() {
                    // 3-arg / empty-selection back-compat: draft ALL draftable
                    // faces (unchanged path). Shape stored after the match.
                    let shape = self.get_shape(*target)?;
                    let plane_shape = self.get_shape(*plane)?;
                    let built = ffi::ffi::draft_shape(shape, angle_rad, plane_shape)
                        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                    ArmOutcome::New(built, ReprPolicy::Classify)
                } else {
                    // Curated per-face selection: draft only the chosen faces,
                    // stored history-free by `draft_faces`.
                    ArmOutcome::Stored(self.draft_faces(*target, angle_rad, *plane, faces)?)
                }
            }
            GeometryOp::Thicken { target, offset } => {
                let shape = self.get_shape(*target)?;
                let off = extract_length_f64(offset, op, "offset")?;
                let built = ffi::ffi::thicken_shape(shape, off)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::OffsetCurve {
                target,
                distance,
                reference,
                direction,
            } => {
                let dist = extract_length_f64(distance, op, "distance")?;
                // Dispatch the three overloads on
                // (reference, direction): a reference surface (overload 2) wins
                // over a direction (overload 3); neither → planar offset
                // (overload 1).
                let result = {
                    let shape = self.get_shape(*target)?;
                    match (reference, direction) {
                        (Some(ref_id), _) => {
                            let ref_shape = self.get_shape(*ref_id)?;
                            ffi::ffi::make_offset_curve_on_surface(shape, dist, ref_shape)
                        }
                        (None, Some(dir)) => ffi::ffi::make_offset_curve_directional(
                            shape, dist, dir[0], dir[1], dir[2],
                        ),
                        (None, None) => ffi::ffi::make_offset_curve(shape, dist),
                    }
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?
                };
                ArmOutcome::New(result, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::ZoneSlab { target, width } => {
                let shape = self.get_shape(*target)?;
                let w = extract_length_f64(width, op, "width")?;
                let built = ffi::ffi::zone_slab_shape(shape, w)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::OffsetSolid { target, distance } => {
                let shape = self.get_shape(*target)?;
                let d = extract_length_f64(distance, op, "distance")?;
                let built = ffi::ffi::offset_solid_shape(shape, d)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::OffsetSurface { target, distance } => {
                let shape = self.get_shape(*target)?;
                let d = extract_length_f64(distance, op, "distance")?;
                let out = ffi::ffi::make_offset_surface(shape, d)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                // `BRepKind::Face` assumes a single-face input/result, true
                // for every current DSL surface producer. See
                // `make_offset_surface`'s header doc comment (occt_wrapper.h)
                // for the shell-input caveat this would need if that ever
                // changes.
                ArmOutcome::New(out, ReprPolicy::Fixed(BRepKind::Face))
            }
            GeometryOp::Shell {
                target,
                thickness,
                faces_to_remove,
                open_face_handles,
            } => {
                let th = extract_length_f64(thickness, op, "thickness")?;
                // Curated-handle path (shell_open): open_face_handles is
                // non-empty → delegate to shell_solid_faces, mirroring the
                // Draft execute arm's curated dispatch.
                if !open_face_handles.is_empty() {
                    ArmOutcome::Stored(self.shell_solid_faces(*target, th, open_face_handles)?)
                } else {
                    // Legacy numeric path: faces_to_remove 0-based indices → FFI.
                    let shape = self.get_shape(*target)?;
                    let face_indices: Vec<u32> =
                        faces_to_remove.iter().map(|&i| i as u32).collect();
                    let built = ffi::ffi::shell_shape(shape, th, &face_indices)
                        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                    ArmOutcome::New(built, ReprPolicy::Classify)
                }
            }
            GeometryOp::Scale { target, factor } => {
                let shape = self.get_shape(*target)?;
                if !factor.is_finite() || *factor == 0.0 {
                    return Err(GeometryError::OperationFailed(format!(
                        "scale factor must be finite and non-zero, got {factor}"
                    )));
                }
                let built = ffi::ffi::scale_shape(shape, *factor, 0.0, 0.0, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                let shape = self.get_shape(*target)?;
                if !point[0].is_finite()
                    || !point[1].is_finite()
                    || !point[2].is_finite()
                    || !axis[0].is_finite()
                    || !axis[1].is_finite()
                    || !axis[2].is_finite()
                    || !angle_rad.is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "rotate_around parameters must be finite: point={:?}, axis={:?}, angle={}",
                        point, axis, angle_rad
                    )));
                }
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "rotate_around axis must not be zero-length".into(),
                    ));
                }
                let built = ffi::ffi::rotate_around_shape(
                    shape, point[0], point[1], point[2], axis[0], axis[1], axis[2], *angle_rad,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::ApplyTransform {
                target,
                rotation,
                translation,
            } => {
                // Validate the 4 quaternion components and 3 translation
                // components are finite. Unit-quaternion enforcement lives in
                // the C++ helper `build_trsf` (called via FFI) — surfacing it
                // here would just duplicate the check; the kernel error path
                // already returns a "quaternion"-tagged message for |q|≠1.
                if !rotation[0].is_finite()
                    || !rotation[1].is_finite()
                    || !rotation[2].is_finite()
                    || !rotation[3].is_finite()
                    || !translation[0].is_finite()
                    || !translation[1].is_finite()
                    || !translation[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "apply_transform parameters must be finite: rotation={:?}, translation={:?}",
                        rotation, translation
                    )));
                }
                let t = crate::Transform3 {
                    qw: rotation[0],
                    qx: rotation[1],
                    qy: rotation[2],
                    qz: rotation[3],
                    tx: translation[0],
                    ty: translation[1],
                    tz: translation[2],
                };
                ArmOutcome::Stored(self.store_transformed(*target, &t)?)
            }
            GeometryOp::AffineApply {
                target,
                linear,
                translation,
            } => {
                let shape = self.get_shape(*target)?;
                let all_finite = linear.iter().all(|row| row.iter().all(|c| c.is_finite()))
                    && translation.iter().all(|c| c.is_finite());
                if !all_finite {
                    return Err(GeometryError::OperationFailed(format!(
                        "affine_apply parameters must be finite: linear={:?}, translation={:?}",
                        linear, translation
                    )));
                }
                let built = ffi::ffi::gtransform_shape(
                    shape,
                    linear[0][0],
                    linear[0][1],
                    linear[0][2],
                    linear[1][0],
                    linear[1][1],
                    linear[1][2],
                    linear[2][0],
                    linear[2][1],
                    linear[2][2],
                    translation[0],
                    translation[1],
                    translation[2],
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::ScaleNonUniform { target, sx, sy, sz } => {
                let shape = self.get_shape(*target)?;
                if !sx.is_finite()
                    || !sy.is_finite()
                    || !sz.is_finite()
                    || *sx == 0.0
                    || *sy == 0.0
                    || *sz == 0.0
                {
                    return Err(GeometryError::OperationFailed(format!(
                        "scale factors must be finite and non-zero, got sx={sx}, sy={sy}, sz={sz}"
                    )));
                }
                let built = ffi::ffi::gtransform_shape(
                    shape, *sx, 0.0, 0.0, 0.0, *sy, 0.0, 0.0, 0.0, *sz, 0.0, 0.0, 0.0,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Inherit(*target))
            }
            GeometryOp::Extrude { profile, distance } => {
                let dist = extract_length_f64(distance, op, "distance")?;
                validate_extrude_distance(dist, "extrude")?;
                let profile_shape = self.get_shape(*profile)?;
                let built = ffi::ffi::make_prism(profile_shape, 0.0, 0.0, dist)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                validate_revolve_params(*axis_origin, *axis_dir, *angle_rad)?;
                let profile_shape = self.get_shape(*profile)?;
                let built = ffi::ffi::make_revolve(
                    profile_shape,
                    axis_origin[0],
                    axis_origin[1],
                    axis_origin[2],
                    axis_dir[0],
                    axis_dir[1],
                    axis_dir[2],
                    *angle_rad,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::Sweep { profile, path } => {
                let profile_shape = self.get_shape(*profile)?;
                let path_shape = self.get_shape(*path)?;
                validate_sweep_pose(profile_shape, path_shape)?;
                let shape = ffi::ffi::make_pipe(profile_shape, path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Classify)
            }
            GeometryOp::Pipe { path, radius } => {
                let r = extract_length_f64(radius, op, "radius")?;
                validate_positive_finite(r, "pipe radius")?;
                let path_shape = self.get_shape(*path)?;
                // Orient the circular profile onto the path's start frame: build
                // the circle at the wire's start point with its plane normal
                // aligned to the start-tangent, then sweep. This supports paths
                // with ANY finite start-tangent (not just +Z).
                let t = ffi::ffi::wire_start_tangent(path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                // Copy the tangent components before the by-value validate guard
                // consumes `t` (Point3 is not Copy).
                let (tx, ty, tz) = (t.x, t.y, t.z);
                validate_pipe_start_tangent(t)?;
                let p = ffi::ffi::wire_start_point(path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let circle_shape =
                    ffi::ffi::make_oriented_circle_face(r, p.x, p.y, p.z, tx, ty, tz)
                        .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let built = ffi::ffi::make_pipe(&circle_shape, path_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::ExtrudeSymmetric { profile, distance } => {
                let dist = extract_length_f64(distance, op, "distance")?;
                validate_extrude_distance(dist, "extrude_symmetric")?;
                // Compose: prism the profile by the full distance, then
                // translate by -distance/2 along the extrusion axis so the
                // resulting solid's centroid (in z) aligns with the
                // profile's centroid. This reuses the validated make_prism
                // + translate_shape FFI calls rather than adding a bespoke
                // C++ wrapper.
                let profile_shape = self.get_shape(*profile)?;
                let prism = ffi::ffi::make_prism(profile_shape, 0.0, 0.0, dist)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                let built = ffi::ffi::translate_shape(&prism, 0.0, 0.0, -dist / 2.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::ExtrudeInfinite { profile, axis, both } => {
                let [dx, dy, dz] = *axis;
                // DEFENSE-IN-DEPTH: the eval producer already validates these; this
                // catches any direct-FFI or test-bypassed paths.
                if !(dx.is_finite() && dy.is_finite() && dz.is_finite()) {
                    return Err(GeometryError::OperationFailed(format!(
                        "extrude_infinite axis components must be finite: [{dx}, {dy}, {dz}]"
                    )));
                }
                let mag_sq = dx * dx + dy * dy + dz * dz;
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(format!(
                        "extrude_infinite axis has zero/degenerate magnitude \
                         (mag_sq={mag_sq}); axis must be non-zero"
                    )));
                }
                let profile_shape = self.get_shape(*profile)?;
                let built = ffi::ffi::make_prism_infinite(profile_shape, dx, dy, dz, *both)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            // No Rust-side argument validation, unlike the LoftGuided / Pipe /
            // ExtrudeInfinite neighbours: `make_pipe_shell` in
            // cpp/occt_wrapper.{h,cpp} owns all three contracts — the section
            // profile's, and the path's and guide's wire-ness — and raises a
            // ContractViolation naming the offending argument and type.
            GeometryOp::SweepGuided {
                profile,
                path,
                guide,
            } => {
                let profile_shape = self.get_shape(*profile)?;
                let path_shape = self.get_shape(*path)?;
                let guide_shape = self.get_shape(*guide)?;
                let shape = ffi::ffi::make_pipe_shell(profile_shape, path_shape, guide_shape)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Classify)
            }
            GeometryOp::LoftGuided { profiles, guides } => {
                if profiles.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "loft_guided requires at least 2 profiles".into(),
                    ));
                }
                if guides.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "loft_guided requires at least 1 guide".into(),
                    ));
                }
                let mut profile_vec = ffi::ffi::new_shape_vec();
                for &pid in profiles {
                    let shape = self.get_shape(pid)?;
                    ffi::ffi::shape_vec_push(profile_vec.pin_mut(), shape);
                }
                let mut guide_vec = ffi::ffi::new_shape_vec();
                for &gid in guides {
                    let shape = self.get_shape(gid)?;
                    ffi::ffi::shape_vec_push(guide_vec.pin_mut(), shape);
                }
                let shape = ffi::ffi::loft_guided_profiles(&profile_vec, &guide_vec)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Classify)
            }
            GeometryOp::LineSegment {
                x1,
                y1,
                z1,
                x2,
                y2,
                z2,
            } => {
                crate::floor_constants::line_segment_rust_guard(x2 - x1, y2 - y1, z2 - z1)
                    .map_err(GeometryError::OperationFailed)?;
                let shape = ffi::ffi::make_line_wire(*x1, *y1, *z1, *x2, *y2, *z2)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                axis,
            } => {
                validate_positive_finite(*radius, "arc radius")?;
                let mag_sq = axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2];
                if mag_sq < AXIS_MAG_SQ_MIN {
                    return Err(GeometryError::OperationFailed(
                        "arc axis must not be zero-length".into(),
                    ));
                }
                let shape = ffi::ffi::make_arc_wire(
                    center[0],
                    center[1],
                    center[2],
                    *radius,
                    *start_angle,
                    *end_angle,
                    axis[0],
                    axis[1],
                    axis[2],
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::Helix {
                radius,
                pitch,
                height,
            } => {
                validate_positive_finite(*radius, "helix radius")?;
                validate_positive_finite(*pitch, "helix pitch")?;
                validate_positive_finite(*height, "helix height")?;
                let shape = ffi::ffi::make_helix_wire(*radius, *pitch, *height)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::InterpCurve { points } => {
                if points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "interp_curve requires at least 2 points".into(),
                    ));
                }
                let coords: Vec<f64> = points.iter().flat_map(|p| p.iter().copied()).collect();
                let shape = ffi::ffi::make_interp_curve(&coords, points.len())
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::BezierCurve { control_points } => {
                if control_points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "bezier_curve requires at least 2 control points".into(),
                    ));
                }
                let coords: Vec<f64> = control_points
                    .iter()
                    .flat_map(|p| p.iter().copied())
                    .collect();
                let shape = ffi::ffi::make_bezier_curve(&coords, control_points.len())
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::NurbsCurve {
                control_points,
                weights,
                knots,
                degree,
            } => {
                if control_points.len() < 2 {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve requires at least 2 control points".into(),
                    ));
                }
                if *degree < 1 {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve degree must be >= 1".into(),
                    ));
                }
                if weights.len() != control_points.len() {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve: weights count must equal control points count".into(),
                    ));
                }
                if knots.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "nurbs_curve: knots vector must not be empty".into(),
                    ));
                }
                let expected_knots = control_points.len() + degree + 1;
                if knots.len() != expected_knots {
                    return Err(GeometryError::OperationFailed(format!(
                        "nurbs_curve: expected {} knots (n_points + degree + 1), got {}",
                        expected_knots,
                        knots.len(),
                    )));
                }
                let coords: Vec<f64> = control_points
                    .iter()
                    .flat_map(|p| p.iter().copied())
                    .collect();
                let shape = ffi::ffi::make_nurbs_curve(
                    &coords,
                    control_points.len(),
                    weights,
                    knots,
                    *degree as i32,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Wire))
            }
            GeometryOp::LinearPattern2D {
                target,
                direction1,
                count1,
                spacing1,
                direction2,
                count2,
                spacing2,
            } => {
                let shape = self.get_shape(*target)?;
                let sp1 = extract_length_f64(spacing1, op, "spacing1")?;
                let sp2 = extract_length_f64(spacing2, op, "spacing2")?;
                // Validate direction vectors are finite (NaN/Inf would cause
                // undefined OCCT behavior).  Consistent with Mirror arm.
                if !direction1[0].is_finite()
                    || !direction1[1].is_finite()
                    || !direction1[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d direction1 must contain finite values".into(),
                    ));
                }
                if !direction2[0].is_finite()
                    || !direction2[1].is_finite()
                    || !direction2[2].is_finite()
                {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d direction2 must contain finite values".into(),
                    ));
                }
                if *count1 == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d count1 must be >= 1".into(),
                    ));
                }
                if *count2 == 0 {
                    return Err(GeometryError::OperationFailed(
                        "linear_pattern_2d count2 must be >= 1".into(),
                    ));
                }
                let built = ffi::ffi::linear_pattern_2d(
                    shape,
                    direction1[0],
                    direction1[1],
                    direction1[2],
                    *count1 as u32,
                    sp1,
                    direction2[0],
                    direction2[1],
                    direction2[2],
                    *count2 as u32,
                    sp2,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::ArbitraryPattern { target, transforms } => {
                let shape = self.get_shape(*target)?;
                if transforms.is_empty() {
                    return Err(GeometryError::OperationFailed(
                        "arbitrary_pattern requires at least one transform".into(),
                    ));
                }
                // Stride-7 per-instance rigid transform: [qw,qx,qy,qz,tx,ty,tz].
                // An identity quaternion ([1,0,0,0]) reproduces the pre-4168
                // pure-translation behavior exactly (see `build_trsf` on the
                // C++ side).
                let flat_transforms: Vec<f64> = transforms
                    .iter()
                    .flat_map(|(rotation, translation)| {
                        [
                            rotation[0],
                            rotation[1],
                            rotation[2],
                            rotation[3],
                            translation[0],
                            translation[1],
                            translation[2],
                        ]
                    })
                    .collect();
                let num_transforms = transforms.len() as u32;
                let built = ffi::ffi::arbitrary_pattern(shape, &flat_transforms, num_transforms)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(built, ReprPolicy::Classify)
            }
            GeometryOp::RectangleProfile { width, height } => {
                let w = extract_length_f64(width, op, "width")?;
                let h = extract_length_f64(height, op, "height")?;
                validate_positive_finite(w, "rectangle_profile width")?;
                validate_positive_finite(h, "rectangle_profile height")?;
                let shape = ffi::ffi::make_rectangle_face(w, h, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Face))
            }
            GeometryOp::CircleProfile { radius } => {
                let r = extract_length_f64(radius, op, "radius")?;
                validate_positive_finite(r, "circle_profile radius")?;
                let shape = ffi::ffi::make_circle_face(r, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Face))
            }
            GeometryOp::PolygonProfile { points } => {
                if points.len() < 3 {
                    return Err(GeometryError::OperationFailed(
                        "polygon_profile requires at least 3 vertices".into(),
                    ));
                }
                // Flatten Vec<[f64; 2]> → Vec<f64> for the FFI (mirrors InterpCurve at lib.rs:2742).
                let coords: Vec<f64> = points.iter().flat_map(|p| p.iter().copied()).collect();
                let shape = ffi::ffi::make_polygon_face(&coords, points.len(), 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Face))
            }
            GeometryOp::EllipseProfile { semi_major, semi_minor } => {
                let a = extract_length_f64(semi_major, op, "semi_major")?;
                let b = extract_length_f64(semi_minor, op, "semi_minor")?;
                validate_positive_finite(a, "ellipse_profile semi_major")?;
                validate_positive_finite(b, "ellipse_profile semi_minor")?;
                let shape = ffi::ffi::make_ellipse_face(a, b, 0.0)
                    .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Face))
            }
            GeometryOp::NurbsSurface {
                control_points,
                weights,
                u_knots,
                v_knots,
                u_degree,
                v_degree,
            } => {
                let n_u = control_points.len();
                if n_u < 2 {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: control_points grid must have at least 2 rows, got {n_u}",
                        crate::floor_constants::RUST_GUARD_MARKER
                    )));
                }
                let n_v = control_points[0].len();
                if n_v < 2 {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: control_points grid must have at least 2 columns, got {n_v}",
                        crate::floor_constants::RUST_GUARD_MARKER
                    )));
                }
                for (i, row) in control_points.iter().enumerate() {
                    if row.len() != n_v {
                        return Err(GeometryError::OperationFailed(format!(
                            "{} nurbs_surface: control_points row {i} has {} columns, expected {n_v}",
                            crate::floor_constants::RUST_GUARD_MARKER,
                            row.len()
                        )));
                    }
                }
                if weights.len() != n_u {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: weights has {} rows, expected {n_u}",
                        crate::floor_constants::RUST_GUARD_MARKER,
                        weights.len()
                    )));
                }
                for (i, wrow) in weights.iter().enumerate() {
                    if wrow.len() != n_v {
                        return Err(GeometryError::OperationFailed(format!(
                            "{} nurbs_surface: weights row {i} has {} columns, expected {n_v}",
                            crate::floor_constants::RUST_GUARD_MARKER,
                            wrow.len()
                        )));
                    }
                }
                if *u_degree < 1 {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: u_degree must be >= 1, got {u_degree}",
                        crate::floor_constants::RUST_GUARD_MARKER
                    )));
                }
                if *v_degree < 1 {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: v_degree must be >= 1, got {v_degree}",
                        crate::floor_constants::RUST_GUARD_MARKER
                    )));
                }
                let expected_u_knots = n_u + u_degree + 1;
                if u_knots.len() != expected_u_knots {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: expected {expected_u_knots} u_knots \
                         (n_u={n_u} + u_degree={u_degree} + 1), got {}",
                        crate::floor_constants::RUST_GUARD_MARKER,
                        u_knots.len()
                    )));
                }
                let expected_v_knots = n_v + v_degree + 1;
                if v_knots.len() != expected_v_knots {
                    return Err(GeometryError::OperationFailed(format!(
                        "{} nurbs_surface: expected {expected_v_knots} v_knots \
                         (n_v={n_v} + v_degree={v_degree} + 1), got {}",
                        crate::floor_constants::RUST_GUARD_MARKER,
                        v_knots.len()
                    )));
                }
                for (i, row) in control_points.iter().enumerate() {
                    for (j, pt) in row.iter().enumerate() {
                        if !pt[0].is_finite() || !pt[1].is_finite() || !pt[2].is_finite() {
                            return Err(GeometryError::OperationFailed(format!(
                                "{} nurbs_surface: control_points[{i}][{j}] contains non-finite value",
                                crate::floor_constants::RUST_GUARD_MARKER
                            )));
                        }
                    }
                }
                let pole_coords: Vec<f64> = control_points
                    .iter()
                    .flat_map(|row| row.iter().flat_map(|p| p.iter().copied()))
                    .collect();
                let flat_weights: Vec<f64> = weights
                    .iter()
                    .flat_map(|row| row.iter().copied())
                    .collect();
                let shape = ffi::ffi::make_nurbs_surface(
                    &pole_coords,
                    n_u,
                    n_v,
                    &flat_weights,
                    u_knots,
                    v_knots,
                    *u_degree as i32,
                    *v_degree as i32,
                )
                .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
                ArmOutcome::New(shape, ReprPolicy::Fixed(BRepKind::Face))
            }
            // Split is a multi-output topology selector; it cannot return a
            // single GeometryHandle. Callers must use execute_split() instead.
            GeometryOp::Split { .. } => {
                return Err(GeometryError::OperationFailed(
                    "GeometryOp::Split is multi-output; use execute_split() instead of execute()"
                        .into(),
                ));
            }
            // Surface is a Mesh-repr terminal anchor fed by a Voxel→Mesh
            // conversion edge (PRD docs/prds/v0_3/voxel-to-mesh-surfacing.md
            // C-1); it must never reach OcctKernel::execute(). A permanent
            // defensive Err, not a todo!() — reaching this arm is a
            // dispatcher bug, not unfinished work.
            GeometryOp::Surface { .. } => {
                return Err(GeometryError::OperationFailed(
                    "GeometryOp::Surface is a Mesh-repr terminal anchor fed by a Voxel→Mesh \
                     conversion edge; it must not reach OcctKernel::execute() — see PRD C-1"
                        .into(),
                ));
            }
        };
        match outcome {
            ArmOutcome::New(shape, policy) => self.store_with_policy(shape, policy),
            ArmOutcome::Stored(handle) => Ok(handle),
        }
    }

    /// The one op-to-history lowering: execute `op` and return the attribute
    /// history its OCCT builder recorded.
    ///
    /// Extrude, Revolve, Sweep, Loft, Union, Difference, Intersection, Fillet,
    /// Chamfer and ChamferAsymmetric route to their `*_with_history` primitives
    /// (Fillet and Chamfer to the all-edges or curated-edges form by
    /// `edges.is_empty()`; ChamferAsymmetric's one primitive reads an empty
    /// selection as all edges), decoding length fields exactly as
    /// [`Self::execute`] does. Every other op runs through [`Self::execute`]
    /// with [`AttributeHistory::None`].
    pub fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        match op {
            GeometryOp::Extrude { profile, distance } => {
                let d = extract_length_f64(distance, op, "distance")?;
                self.extrude_with_history(*profile, d)
                    .map(|(h, recs)| (h, AttributeHistory::Extrude(recs)))
            }
            GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => self
                .revolve_with_history(*profile, *axis_origin, *axis_dir, *angle_rad)
                .map(|(h, recs)| (h, AttributeHistory::Revolve(recs))),
            GeometryOp::Sweep { profile, path } => self
                .sweep_with_history(*profile, *path)
                .map(|(h, recs)| (h, AttributeHistory::Sweep(recs))),
            GeometryOp::Loft { profiles } => self
                .loft_with_history(profiles)
                .map(|(h, recs)| (h, AttributeHistory::Loft(recs))),
            GeometryOp::Union { left, right } => self
                .boolean_fuse_with_history(*left, *right)
                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
            GeometryOp::Difference { left, right } => self
                .boolean_cut_with_history(*left, *right)
                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
            GeometryOp::Intersection { left, right } => self
                .boolean_common_with_history(*left, *right)
                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
            GeometryOp::Fillet {
                target,
                edges,
                radius,
            } => {
                let r = extract_length_f64(radius, op, "radius")?;
                let lowered = if edges.is_empty() {
                    self.fillet_with_history(*target, r)
                } else {
                    self.fillet_edges_with_history(*target, r, edges)
                };
                lowered.map(|(h, recs)| (h, AttributeHistory::LocalFeature(recs)))
            }
            GeometryOp::Chamfer {
                target,
                edges,
                distance,
            } => {
                let d = extract_length_f64(distance, op, "distance")?;
                let lowered = if edges.is_empty() {
                    self.chamfer_with_history(*target, d)
                } else {
                    self.chamfer_edges_with_history(*target, d, edges)
                };
                lowered.map(|(h, recs)| (h, AttributeHistory::LocalFeature(recs)))
            }
            GeometryOp::ChamferAsymmetric {
                target,
                edges,
                d1,
                d2,
            } => {
                let d1 = extract_length_f64(d1, op, "d1")?;
                let d2 = extract_length_f64(d2, op, "d2")?;
                self.chamfer_asymmetric_edges_with_history(*target, d1, d2, edges)
                    .map(|(h, recs)| (h, AttributeHistory::LocalFeature(recs)))
            }
            _ => self.execute(op).map(|h| (h, AttributeHistory::None)),
        }
    }

    /// A shape's volume together with which arm produced it.
    ///
    /// `GeometryQuery::Volume` answers with the number alone, which leaves a
    /// caller unable to tell an exact integral from the tessellation fallback.
    /// This reports both, from the same single arm-selection site, so the two
    /// agree bit-for-bit.
    ///
    /// `tessellation_fallback == true` reads NARROWLY today: on OCCT 7.8 every
    /// shape measured to reach that arm is a face-less compound, whose exact
    /// integral and tessellation arm both sum nothing, so the flag means "this
    /// shape has no measurable volume" rather than "an approximation was
    /// substituted for an exact number".
    ///
    /// It is correspondingly the caller's signal that the rest of the
    /// mass-property family — `Centroid`, `CenterOfMass`, `MomentOfInertia`,
    /// `InertiaTensor` — is returning its degenerate origin/zero default for
    /// this shape rather than a measurement: those queries have no fallback arm
    /// of their own.
    pub fn volume_measurement(
        &self,
        id: GeometryHandleId,
    ) -> Result<VolumeMeasurement, QueryError> {
        let shape = self.get_shape(id)?;
        ffi::ffi::query_volume_measurement(shape)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }

    pub fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        match query {
            GeometryQuery::Volume(id) => {
                let shape = self.get_shape(*id)?;
                let vol = ffi::ffi::query_volume(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(vol))
            }
            GeometryQuery::SurfaceArea(id) => {
                let shape = self.get_shape(*id)?;
                let area = ffi::ffi::query_area(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(area))
            }
            GeometryQuery::Centroid(id) => {
                // Dispatch by stored BRepKind so an extracted Face handle
                // (no enclosed volume — `query_centroid`'s VolumeProperties
                // path would default to the origin) routes through the
                // surface-area centroid. Every other repr (Solid, Compound,
                // Wire, Edge, Shell, …) keeps the legacy volume-based path,
                // preserving existing semantics validated by tests like
                // `center_of_mass_on_non_solid_wire_returns_origin`
                // (`tests/harness_occt_kernel/mass_properties.rs`) and
                // `sweep_guided_orientation_differs_from_plain_sweep`
                // (`tests/harness_occt_measurement/sweep_guided_integration.rs`).
                let shape = self.get_shape(*id)?;
                let pt = if self.repr_of(*id) == Some(BRepKind::Face) {
                    ffi::ffi::query_face_centroid(shape)
                        .map_err(|e| QueryError::QueryFailed(e.to_string()))?
                } else {
                    ffi::ffi::query_centroid(shape)
                        .map_err(|e| QueryError::QueryFailed(e.to_string()))?
                };
                // Return centroid as a JSON string since Value has no tuple variant
                Ok(Value::String(centroid_json(pt)))
            }
            GeometryQuery::BoundingBox(id) => {
                let shape = self.get_shape(*id)?;
                let bb = ffi::ffi::query_bbox(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(format!(
                    "{{\"xmin\":{},\"ymin\":{},\"zmin\":{},\"xmax\":{},\"ymax\":{},\"zmax\":{}}}",
                    bb.xmin, bb.ymin, bb.zmin, bb.xmax, bb.ymax, bb.zmax
                )))
            }
            GeometryQuery::Distance { from, to } => {
                let s1 = self.get_shape(*from)?;
                let s2 = self.get_shape(*to)?;
                let dist = ffi::ffi::query_distance(s1, s2)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(dist))
            }
            GeometryQuery::MomentOfInertia { handle, axis } => {
                let shape = self.get_shape(*handle)?;
                let moi = ffi::ffi::query_moment_of_inertia(shape, axis[0], axis[1], axis[2])
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(moi))
            }
            // For uniform-density solids, the center of mass equals the geometric centroid.
            // density is intentionally ignored here (see GeometryQuery::CenterOfMass doc).
            GeometryQuery::CenterOfMass { handle, density: _ } => {
                let shape = self.get_shape(*handle)?;
                let pt = ffi::ffi::query_centroid(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(centroid_json(pt)))
            }
            GeometryQuery::InertiaTensor { handle, density } => {
                let shape = self.get_shape(*handle)?;
                let t = ffi::ffi::query_inertia_tensor(shape, *density)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(vec![
                    Value::List(vec![
                        Value::Real(t.m11),
                        Value::Real(t.m12),
                        Value::Real(t.m13),
                    ]),
                    Value::List(vec![
                        Value::Real(t.m21),
                        Value::Real(t.m22),
                        Value::Real(t.m23),
                    ]),
                    Value::List(vec![
                        Value::Real(t.m31),
                        Value::Real(t.m32),
                        Value::Real(t.m33),
                    ]),
                ]))
            }
            GeometryQuery::AdjacentFaces { shape, face_index } => {
                let s = self.get_shape(*shape)?;
                let idx_u32: u32 = (*face_index).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "adjacent_faces: face_index {} exceeds u32::MAX",
                        face_index
                    ))
                })?;
                let neighbors = ffi::ffi::adjacent_faces(s, idx_u32)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(
                    neighbors
                        .into_iter()
                        .map(|i| Value::Int(i as i64))
                        .collect(),
                ))
            }
            GeometryQuery::SharedEdges {
                shape,
                face_a,
                face_b,
            } => {
                let s = self.get_shape(*shape)?;
                let a_u32: u32 = (*face_a).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "shared_edges: face_a {} exceeds u32::MAX",
                        face_a
                    ))
                })?;
                let b_u32: u32 = (*face_b).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "shared_edges: face_b {} exceeds u32::MAX",
                        face_b
                    ))
                })?;
                let edges = ffi::ffi::shared_edges(s, a_u32, b_u32)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(
                    edges.into_iter().map(|i| Value::Int(i as i64)).collect(),
                ))
            }
            GeometryQuery::IsWatertight(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_watertight(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsManifold(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_manifold(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsOrientable(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_orientable(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsClosed(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_closed(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsConnected(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_connected(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::IsBounded(id) => {
                let shape = self.get_shape(*id)?;
                let v = ffi::ffi::is_bounded(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Bool(v))
            }
            GeometryQuery::EdgeLength(id) => {
                let shape = self.get_shape(*id)?;
                let len = ffi::ffi::query_edge_length(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::Real(len))
            }
            GeometryQuery::EdgeTangent(id) => {
                let shape = self.get_shape(*id)?;
                let t = ffi::ffi::query_edge_tangent(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                // Encode as the same JSON-string format as Centroid /
                // FaceNormal so downstream filters share a single parse
                // routine.
                Ok(Value::String(format!(
                    "{{\"x\":{},\"y\":{},\"z\":{}}}",
                    t.x, t.y, t.z
                )))
            }
            GeometryQuery::FaceNormal(id) => {
                let [x, y, z] = self.face_outward_unit_normal(*id)?;
                // Encode as the same JSON-string format as Centroid so
                // downstream filters share a single parse routine.
                Ok(Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}")))
            }
            GeometryQuery::FaceSurfaceKind(id) => {
                let shape = self.get_shape(*id)?;
                // FFI returns a canonical name string from OCCT's GeomAbs_*
                // enumeration (`"Plane"` / `"Cylinder"` / …). Decoded
                // downstream by `FaceSurfaceKind::try_from_str` in
                // `selector_vocabulary_v2::faces_by_surface_kind`.
                let name = ffi::ffi::face_surface_kind(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(name))
            }
            GeometryQuery::EdgeCurveKind(id) => {
                let shape = self.get_shape(*id)?;
                // FFI returns a canonical name string from OCCT's GeomAbs_*
                // enumeration (`"Line"` / `"Circle"` / …). Decoded
                // downstream by `EdgeCurveKind::try_from_str` in
                // `selector_vocabulary_v2::edges_by_curve_kind`.
                let name = ffi::ffi::edge_curve_kind(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::String(name))
            }
            GeometryQuery::AncestorFacesOfEdge { shape, edge_index } => {
                let s = self.get_shape(*shape)?;
                let idx_u32: u32 = (*edge_index).try_into().map_err(|_| {
                    QueryError::QueryFailed(format!(
                        "ancestor_faces_of_edge: edge_index {} exceeds u32::MAX",
                        edge_index
                    ))
                })?;
                let parents = ffi::ffi::ancestor_faces_of_edge(s, idx_u32)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::List(
                    parents.into_iter().map(|i| Value::Int(i as i64)).collect(),
                ))
            }
            GeometryQuery::OwnerBody(id) => {
                // Look up the recorded parent populated inside
                // `extract_edges` / `extract_faces`. Handles produced
                // directly by `execute` (i.e. body handles) have no
                // recorded parent and surface as QueryFailed.
                let parent = self.parent_handle.get(&id.0).copied().ok_or_else(|| {
                    QueryError::QueryFailed(format!(
                        "owner_body: handle {id:?} has no recorded parent (was extract_edges / extract_faces called?)"
                    ))
                })?;
                Ok(Value::Int(parent.0 as i64))
            }
            GeometryQuery::ClosestPointOnShape { handle, px, py, pz } => {
                // Reuse the direct-method wrapper for InvalidHandle / QueryFailed
                // surfacing, then re-encode the [f64; 3] as JSON-Point3 so the wire
                // format matches Centroid / FaceNormal / EdgeTangent — the
                // eval-side dispatcher reuses `parse_xyz_value` on this string.
                let [x, y, z] = self.closest_point_on_shape(*handle, *px, *py, *pz)?;
                Ok(Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}")))
            }
            GeometryQuery::PointOnShape {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => self
                .point_on_shape(*handle, *px, *py, *pz, *tolerance)
                .map(Value::Bool),
            GeometryQuery::Contains {
                handle,
                px,
                py,
                pz,
                tolerance,
            } => self
                .contains(*handle, *px, *py, *pz, *tolerance)
                .map(Value::Bool),
            GeometryQuery::GeoEquiv {
                left,
                right,
                tolerance,
            } => self.geo_equiv(*left, *right, *tolerance).map(Value::Bool),
            GeometryQuery::SurfaceAngle { face_a, face_b } => {
                self.surface_angle(*face_a, *face_b).map(Value::Real)
            }
            GeometryQuery::FaceNormalAt { handle, px, py, pz } => {
                let [x, y, z] = self.surface_normal_at_point(*handle, *px, *py, *pz)?;
                Ok(Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}")))
            }
            // KGQ-μ: curve curvature at a world point — implementation added by step-4.
            GeometryQuery::CurveCurvatureAt { handle, px, py, pz } => {
                self.curve_curvature_at(*handle, *px, *py, *pz).map(Value::Real)
            }
            // KGQ-μ: surface principal curvatures at (u,v) — returns nested
            // Value::List [[kappa_max, 0.0], [0.0, kappa_min]] (InertiaTensor wire format).
            GeometryQuery::SurfaceCurvatureAt { handle, u, v } => {
                let c = self.curvature_at(*handle, *u, *v)?;
                // Diagonal principal-curvature matrix: trace/2 = mean H, det = Gaussian K.
                // Encoding: outer List of rows, each row a List of Values.
                let row0 = Value::List(vec![Value::Real(c.kappa_max), Value::Real(0.0)]);
                let row1 = Value::List(vec![Value::Real(0.0), Value::Real(c.kappa_min)]);
                Ok(Value::List(vec![row0, row1]))
            }
            // ζ / C4: PROMOTE measure_mesh_deviation into a repr-gated query.
            // Composes the two existing primitives without touching ffi.rs:
            //   1. tessellate `actual` at the variant's deflection tolerance
            //   2. measure_mesh_deviation(nominal, &actual_mesh)
            // Both methods are &self; the composition is clean and promotion-only.
            GeometryQuery::MaxDeviation {
                actual,
                nominal,
                tolerance,
            } => {
                let actual_mesh = self
                    .tessellate(*actual, *tolerance)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                let dev = self.measure_mesh_deviation(*nominal, &actual_mesh)?;
                Ok(Value::Real(dev))
            }
            // ε: analytic-datum projection. get_shape → ffi → compose the
            // projected datum Value per the GeomAbs `kind` byte. Edge +
            // tolerance dispatch land in steps 4 and 10.
            GeometryQuery::FaceAnalyticDatum(id) => {
                let shape = self.get_shape(*id)?;
                let d = ffi::ffi::face_analytic_datum(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                analytic_surface_datum_to_value(&d)
            }
            GeometryQuery::EdgeAnalyticDatum(id) => {
                let shape = self.get_shape(*id)?;
                let d = ffi::ffi::edge_analytic_datum(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                analytic_curve_datum_to_value(&d)
            }
            GeometryQuery::ShapeLocalTolerance(id) => {
                let shape = self.get_shape(*id)?;
                let tol = ffi::ffi::shape_local_tolerance(shape)
                    .map_err(|e| QueryError::QueryFailed(e.to_string()))?;
                Ok(Value::length(tol))
            }
        }
    }

    /// Outward unit normal at the centroid of `face` as a typed `[f64; 3]`.
    ///
    /// Single source of truth shared by the `GeometryQuery::FaceNormal`
    /// production arm and the `face_outward_unit_normal_for_test` shim, so
    /// the equivalence is structural rather than enforced by a cross-check
    /// test.
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if the handle is unknown.
    /// - `QueryError::QueryFailed` — if the shape is not a face, has no
    ///   underlying surface, or yields a degenerate normal.
    fn face_outward_unit_normal(&self, id: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        let s = self.get_shape(id)?;
        let p =
            ffi::ffi::query_face_normal(s).map_err(|e| QueryError::QueryFailed(e.to_string()))?;
        Ok([p.x, p.y, p.z])
    }

    /// Store the `body_index`-th solid of a STEP `product` as a Solid handle.
    ///
    /// The handle is in the product's LOCAL frame, in metres; the component
    /// placements in [`StepDocument::tree`] are the occurrence poses.
    /// `body_index` is 0-based, in the order `ProductKind::Part::solid_count`
    /// counts. Nothing is stored on error.
    pub fn import_step_body(
        &mut self,
        doc: &StepDocument,
        product: &reify_ir::ProductRef,
        body_index: u32,
    ) -> Result<GeometryHandle, StepBodyError> {
        doc.body_shape(product, body_index)
            .map(|shape| self.store_with_repr(shape, BRepKind::Solid))
    }

    /// Linear deflection used by the CLI `export(handle, Stl, writer)` arm.
    ///
    /// 0.1 mm: a reasonable general-purpose tessellation default for the
    /// binary-STL path.  The engine's `DEFAULT_TESSELLATION_TOLERANCE` (0.0001)
    /// lives in `reify-eval`, a higher crate that this kernel cannot import.
    /// Demanded-tolerance plumbing into `export()` is task δ's responsibility.
    const DEFAULT_STL_TESSELLATION_TOLERANCE: f64 = 0.1;

    pub fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        let shape = self.get_shape(handle)?;

        match format {
            ExportFormat::Step => {
                // Plain export uses the DEFAULT schema (AP214), set explicitly
                // on the FFI call so a prior AP203 export cannot leak its
                // process-global schema into this one. AP214 never falls back,
                // so `ap242_fell_back` is unread here.
                let result = ffi::ffi::export_step(shape, StepSchema::default().as_str())
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                writer
                    .write_all(result.content.as_bytes())
                    .map_err(|e| ExportError::IoError(e.to_string()))
            }
            ExportFormat::Stl => {
                let mesh = self
                    .tessellate(handle, Self::DEFAULT_STL_TESSELLATION_TOLERANCE)
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                write_stl_binary(&mesh, writer)
                    .map_err(|e| ExportError::IoError(e.to_string()))
            }
            ExportFormat::ThreeMF => {
                let mesh = self
                    .tessellate(handle, Self::DEFAULT_STL_TESSELLATION_TOLERANCE)
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                // default() → include_materials/include_colors both false → no warnings.
                // Warnings are intentionally discarded: export() has no warning channel.
                // Task δ wires include_materials/include_colors via occurrence params
                // and surfaces W_3MF_NO_MATERIALS as a build diagnostic.
                write_3mf(&mesh, ThreeMfOptions::default(), writer)
                    .map(|_warnings| ())
                    .map_err(|e| ExportError::IoError(e.to_string()))
            }
            ExportFormat::Obj => Err(ExportError::FormatError(format!(
                "unsupported export format: {:?}",
                format
            ))),
        }
    }

    /// Export honoring [`ExportOptions`] — the OCCT override of the trait
    /// default. The STEP arm threads `options.step_schema` into the FFI so a
    /// declared `STEPVersion` selects the written schema, and surfaces the
    /// honest AP242→AP214 degradation as [`ExportWarning::StepAp242Fallback`].
    /// Non-STEP formats ignore options and delegate to [`export`](Self::export)
    /// (the schema is meaningless for STL/3MF), returning no warnings.
    pub fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        let shape = self.get_shape(handle)?;

        match format {
            ExportFormat::Step => {
                let result = ffi::ffi::export_step(shape, options.step_schema.as_str())
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                writer
                    .write_all(result.content.as_bytes())
                    .map_err(|e| ExportError::IoError(e.to_string()))?;
                Ok(if result.ap242_fell_back {
                    vec![ExportWarning::StepAp242Fallback]
                } else {
                    Vec::new()
                })
            }
            // ThreeMF: thread color + include flags from options into ThreeMfOptions.
            // Tessellate via the same tolerance as export(), call write_3mf with the
            // full options, and map ThreeMfWarning::NoMaterials→ExportWarning::ThreeMfNoMaterials.
            ExportFormat::ThreeMF => {
                let mesh = self
                    .tessellate(handle, Self::DEFAULT_STL_TESSELLATION_TOLERANCE)
                    .map_err(|e| ExportError::FormatError(e.to_string()))?;
                let thr_warnings =
                    write_3mf(
                        &mesh,
                        ThreeMfOptions {
                            color: options.color,
                            include_materials: options.include_materials,
                            include_colors: options.include_colors,
                        },
                        writer,
                    )
                    .map_err(|e| ExportError::IoError(e.to_string()))?;
                Ok(thr_warnings
                    .into_iter()
                    .map(|w| match w {
                        reify_ir::ThreeMfWarning::NoMaterials => {
                            ExportWarning::ThreeMfNoMaterials
                        }
                    })
                    .collect())
            }
            // Other non-STEP formats: options carry nothing they consume.
            _ => self.export(handle, format, writer).map(|()| Vec::new()),
        }
    }

    pub fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        let shape = self.get_shape(handle)?;

        let result = ffi::ffi::tessellate_shape(shape, tolerance)
            .map_err(|e| TessError::TessellationFailed(e.to_string()))?;

        Ok(Mesh {
            vertices: result.vertices,
            indices: result.indices,
            normals: if result.normals.is_empty() {
                None
            } else {
                Some(result.normals)
            },
        })
    }

    /// Sampled max facet-chord deviation (SI metres) of `mesh` from the exact
    /// BRep stored under `handle`.
    ///
    /// Mirrors [`OcctKernel::tessellate`]: resolves the shape via
    /// [`OcctKernel::get_shape`] (returns `QueryError::InvalidHandle` on miss),
    /// builds a [`ffi::ffi::TessResult`] from the mesh (normals unused by the
    /// metric), then calls the FFI, mapping any error to
    /// `QueryError::QueryFailed`.
    ///
    /// See [`ffi::ffi::measure_mesh_deviation`] for the sampling algorithm.
    ///
    /// No tolerance argument — cannot echo the configured deflection
    /// (structural anti-circularity, PRD §8.3 CRITICAL).
    pub fn measure_mesh_deviation(
        &self,
        handle: GeometryHandleId,
        mesh: &Mesh,
    ) -> Result<f64, QueryError> {
        let shape = self.get_shape(handle)?;

        let tess_result = ffi::ffi::TessResult {
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            normals: Vec::new(), // normals unused by the metric
        };

        ffi::ffi::measure_mesh_deviation(shape, &tess_result)
            .map_err(|e| QueryError::QueryFailed(e.to_string()))
    }
}

/// Compose the projected datum [`Value`] from an `AnalyticSurfaceDatum` FFI
/// record (geometric-relations ε `FaceAnalyticDatum` dispatch).
///
/// The `kind` byte — set in `occt_wrapper.cpp::face_analytic_datum` — selects
/// the Value variant: `0` Plane → [`Value::Plane`], `1` Cylinder / `2` Cone →
/// [`Value::Axis`], `3` Sphere → centre [`Value::Point`]. Origin components are
/// SI-metre lengths ([`Value::length`]); the direction / normal is a
/// dimensionless unit [`Value::Direction`]. The two encodings MUST agree — see
/// the kind-byte table in the C++ source.
#[cfg(has_occt)]
fn analytic_surface_datum_to_value(
    d: &ffi::ffi::AnalyticSurfaceDatum,
) -> Result<Value, QueryError> {
    let origin = Value::Point(vec![
        Value::length(d.origin.x),
        Value::length(d.origin.y),
        Value::length(d.origin.z),
    ]);
    let dir = Value::Direction {
        x: d.direction.x,
        y: d.direction.y,
        z: d.direction.z,
    };
    match d.kind {
        0 => Ok(Value::Plane {
            origin: Box::new(origin),
            normal: Box::new(dir),
        }),
        1 | 2 => Ok(Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(dir),
        }),
        3 => Ok(origin),
        other => Err(QueryError::QueryFailed(format!(
            "face_analytic_datum: unknown surface-datum kind byte {other}"
        ))),
    }
}

/// Compose the projected datum [`Value`] from an `AnalyticCurveDatum` FFI
/// record (geometric-relations ε `EdgeAnalyticDatum` dispatch).
///
/// All three analytic curve kinds — `0` Line, `1` Circle, `2` Ellipse (set in
/// `occt_wrapper.cpp::edge_analytic_datum`) — project to a [`Value::Axis`]: the
/// infinite line for a line, the centre + circle/ellipse axis for an arc.
/// Origin components are SI-metre lengths; the direction is a dimensionless
/// unit [`Value::Direction`]. Radius / major+minor ride in the FFI scalars and
/// are not part of the projected Axis.
#[cfg(has_occt)]
fn analytic_curve_datum_to_value(
    d: &ffi::ffi::AnalyticCurveDatum,
) -> Result<Value, QueryError> {
    let origin = Value::Point(vec![
        Value::length(d.origin.x),
        Value::length(d.origin.y),
        Value::length(d.origin.z),
    ]);
    let dir = Value::Direction {
        x: d.direction.x,
        y: d.direction.y,
        z: d.direction.z,
    };
    match d.kind {
        // Line (0) / Circle (1) / Ellipse (2) all project to an Axis datum.
        0..=2 => Ok(Value::Axis {
            origin: Box::new(origin),
            direction: Box::new(dir),
        }),
        other => Err(QueryError::QueryFailed(format!(
            "edge_analytic_datum: unknown curve-datum kind byte {other}"
        ))),
    }
}

#[cfg(has_occt)]
impl WarmStartable for OcctKernel {
    fn warm_state(&self) -> Option<OpaqueState> {
        // INV-GEO-3 state-inventory guard: exhaustive destructure (no `..`
        // spread) so a newly-added OcctKernel field forces a compile-time
        // persist/clear/runtime classification here instead of being
        // silently omitted from warm-start state.
        //   PERSIST (serialized into OcctWarmState): shapes, reprs, next_id
        //     — excluding ids that are keys in parent_handle (see below)
        //   CLEAR-on-restore (derived provenance, rebuilt by extract_*;
        //     see with_warm_state): extracted_edges, extracted_faces,
        //     extracted_vertices, parent_handle
        //   RUNTIME-only (not part of warm-start state): last_warm_start_failures
        //
        // Bounded payload: `shapes` is persisted below, but ids that are
        // keys in `parent_handle` are skipped. Those ids name sub-shape
        // blobs previously minted by extract_edges/extract_faces/
        // extract_vertices; since parent_handle and the extracted_* caches
        // are CLEAR-on-restore, a serialized sub-shape would round-trip as
        // an ordinary shapes/reprs entry but come out orphaned on the
        // consumer side: still queryable by id (repr_of, Volume, ...) but
        // unreachable via OwnerBody and never reused by a later extract_*
        // call, which mints fresh ids against the cleared cache. Left
        // unfiltered, this would be unbounded across repeated warm-start
        // cycles, not just single-cycle bloat: each cycle would serialize
        // prior orphans, restore them, and the next extract_* would mint
        // yet more fresh ids on top of the cleared cache, so the shape
        // table would monotonically grow with phantom, unreachable
        // entries. `parent_handle`'s key set is exactly the derived,
        // rebuildable sub-shape ids — roots created by `execute` (Box,
        // Cylinder, Union, Sphere, ...) are never keys, so only
        // root/persistent shapes round-trip.
        //
        // `next_id` is not part of this bounded-vs-unbounded concern: it is
        // persisted verbatim (see the `OcctWarmState` construction below)
        // and, by design, keeps advancing across warm-start cycles — every
        // `store_with_repr` call, root or (pre-filter) sub-shape alike,
        // mints its id via fetch-and-increment and never reuses one, so
        // `next_id` is monotonic regardless of this filter. That's expected
        // and harmless: it's a single 8-byte scalar, not a per-entry
        // collection, so it never contributes to the multi-cycle payload
        // growth this filter exists to bound.
        //
        // Consumer contract: a `GeometryHandleId` returned by extract_edges/
        // extract_faces/extract_vertices must not be held across a
        // `with_warm_state()` boundary. After restore, such an id is only
        // valid again once re-minted by a fresh extract_* call on its root;
        // querying the old id directly (Volume, OwnerBody, repr_of, ...)
        // now fails with `QueryError::InvalidHandle` instead of the
        // pre-filter behavior of returning a stale-but-queryable orphan.
        // Callers must re-extract sub-shapes from their root after every
        // warm-start restore (see `warm_state_reextract_cycle` in the test
        // module below). Verified at the time this filter was added (task
        // #5162): no in-tree production caller exercises this cycle today.
        // `OcctKernelHandle` (crates/reify-kernel-occt/src/handle.rs)
        // forwards `warm_state`/`with_warm_state` over its request channel,
        // but nothing outside this crate's own tests sends
        // `OcctRequest::WarmState`/`WithWarmState`; and `reify-eval`'s only
        // production warm-state wiring (`CacheStore::donate_warm_state`/
        // `get_warm_state`, driven from `engine_compute.rs`/
        // `engine_edit.rs`) is scoped to `NodeId::Compute` solver state, not
        // `OcctKernel`/`OcctKernelHandle` geometry state — `engine_build.rs`
        // has no `warm_state`/`OpaqueState` reference at all. A future
        // caller that wires OCCT warm-start into the eval graph must follow
        // the re-extraction contract above.
        let Self {
            shapes,
            reprs,
            next_id,
            extracted_edges,
            extracted_faces,
            extracted_vertices,
            parent_handle,
            last_warm_start_failures: _,
        } = self;
        // Completeness cross-check for the bounded-payload filter below: it is
        // only correct if every extracted_edges/extracted_faces/
        // extracted_vertices cached id is also a `parent_handle` key. That
        // holds today because extract_edges/extract_faces/extract_vertices
        // insert into `parent_handle` immediately after minting each id (see
        // their `store_with_repr` call sites), but nothing at the type level
        // enforces that pairing — `debug_assert_fresh_id` only guards id
        // freshness, not this coupling. Assert it explicitly (read-only; these
        // fields are otherwise unused here, not persisted) so a future code
        // path that repopulates one of the extracted_* caches, or stores a
        // sub-shape, without a matching `parent_handle` insert fails loudly
        // here instead of silently re-opening the unbounded multi-cycle
        // growth this filter exists to prevent.
        debug_assert!(
            [extracted_edges, extracted_faces, extracted_vertices]
                .into_iter()
                .flat_map(|cache| cache.values().flatten())
                .all(|id| parent_handle.contains_key(&id.0)),
            "every extracted_edges/extracted_faces/extracted_vertices cached \
             id must be a parent_handle key, or warm_state()'s bounded-payload \
             filter would silently leak an un-filtered orphan back into \
             persisted warm-start state"
        );
        if shapes.is_empty() {
            return None;
        }
        let mut warm_shapes = HashMap::new();
        let mut warm_reprs: HashMap<u64, BRepKind> = HashMap::new();
        let mut total_bytes: usize = 0;
        for (&id, shape) in shapes {
            if parent_handle.contains_key(&id) {
                // Derived sub-shape id (rebuildable via extract_*); skip so
                // it doesn't round-trip as an orphaned entry. See the
                // "Bounded payload" note above.
                continue;
            }
            let Some(shape_ref) = shape.as_ref() else {
                continue; // Skip null shapes (best-effort, like serialization failures)
            };
            match ffi::ffi::serialize_brep(shape_ref) {
                Ok(brep) => {
                    total_bytes += brep.len();
                    warm_shapes.insert(id, brep);
                    // Mirror repr entry only for ids that successfully serialized,
                    // keeping shapes and reprs in lock-step.
                    if let Some(&repr) = reprs.get(&id) {
                        warm_reprs.insert(id, repr);
                    }
                }
                Err(_) => {
                    // Skip shapes that fail to serialize (best-effort)
                    continue;
                }
            }
        }
        if warm_shapes.is_empty() {
            return None;
        }
        let staged_count = warm_shapes.len();
        // size_estimate is a payload-bytes lower bound, not a true heap footprint.
        // Per-repr entry: 16 bytes (u64 key + enum byte, payload only). True cost in
        // hashbrown's open-addressing layout is ~32–48 bytes/entry, but the existing
        // +64 struct overhead already underestimates the shapes HashMap the same way —
        // callers treat this figure as approximate, not as a precise allocator reading.
        let size_estimate = total_bytes + staged_count * 16 + 64;
        Some(OpaqueState::new(
            OcctWarmState {
                shapes: warm_shapes,
                reprs: warm_reprs,
                next_id: *next_id,
            },
            size_estimate,
        ))
    }

    fn with_warm_state(&mut self, state: OpaqueState) {
        let warm = match state.downcast::<OcctWarmState>() {
            Some(w) => w,
            None => return, // Wrong type, silently ignore per trait contract
        };
        // Stage deserialization into a temporary map first. If all entries fail
        // to deserialize, we preserve the kernel's pre-existing state untouched.
        self.last_warm_start_failures = 0;
        let mut staged = HashMap::new();
        for (id, brep) in warm.shapes {
            cxx::let_cxx_string!(brep_cxx = brep.as_str());
            match ffi::ffi::deserialize_brep(&brep_cxx) {
                Ok(shape) => {
                    staged.insert(id, shape);
                }
                Err(e) => {
                    tracing::warn!(
                        shape_id = id,
                        error = %e,
                        "warm-start deserialization failed"
                    );
                    self.last_warm_start_failures += 1;
                    continue;
                }
            }
        }
        // Atomic swap: only replace kernel state if at least one shape was
        // successfully deserialized. Otherwise the kernel state is untouched.
        //
        // Coverage boundary: the INV-GEO-3 exhaustive-destructure drift guard
        // below only fires on this swap-occurs path. A future field that must
        // be cleared even on the no-op (total-failure) branch is NOT caught
        // by that guard — it would need its own explicit handling above this
        // `if`, plus a behavioral pin (see
        // `with_warm_state_total_failure_preserves_dirty_state_and_provenance`,
        // which asserts the no-op path leaves every current field, including
        // `parent_handle`, untouched on a dirty kernel).
        if !staged.is_empty() {
            // Rebuild repr map: for every successfully staged id, take the
            // persisted repr if present, falling back to BRepKind::Solid.
            // debug_assert guards against a future producer that serializes
            // warm.shapes and warm.reprs out of sync: if reprs is non-empty it
            // must contain an entry for every shape id (the lock-step invariant
            // maintained by warm_state()). The production fallback remains safe.
            let mut new_reprs = HashMap::new();
            for &id in staged.keys() {
                debug_assert!(
                    warm.reprs.is_empty() || warm.reprs.contains_key(&id),
                    "warm.reprs and warm.shapes are out of sync: \
                     id {id} present in shapes but missing from reprs"
                );
                let repr = warm.reprs.get(&id).copied().unwrap_or(BRepKind::Solid);
                new_reprs.insert(id, repr);
            }
            // INV-GEO-3 state-inventory guard: this exhaustive destructure (no
            // `..` spread) forces every OcctKernel field to be classified here
            // as PERSIST (overwritten from the restored state) or CLEAR-on-restore
            // (derived/cached provenance invalidated by the wholesale shape
            // swap). Adding a new field to OcctKernel without extending this
            // pattern is a hard compile error (E0027), so the consumer-side
            // clear/rebuild decision can't be silently skipped.
            let Self {
                shapes,
                reprs,
                next_id,
                extracted_edges,
                extracted_faces,
                extracted_vertices,
                parent_handle,
                last_warm_start_failures: _, // finalized above; not part of the swap
            } = self;
            // PERSIST: overwritten wholesale from the restored warm state.
            *shapes = staged;
            *reprs = new_reprs;
            *next_id = warm.next_id;
            // FORWARD-LOOKING HAZARD (task 5212, no production caller today):
            // this wholesale `*shapes = staged` swap paired with
            // `*next_id = warm.next_id` can LOWER next_id to a value at or
            // below ids already handed out this session. If OCCT geometry
            // warm-start is ever wired into the GUI whole-file reload path, a
            // later `store_with_repr` could then re-mint an id that a stale
            // handle (e.g. a lingering realization-cache entry) still names,
            // aliasing it onto a freshly-minted shape — silent data corruption
            // / UAF — instead of failing cleanly. This is exactly why
            // `OcctKernel::reset()` deliberately keeps next_id MONOTONIC (see
            // its doc comment and INV-BUILD-1): a warm-start reload wiring must
            // reconcile next_id (take the max, never lower it), not overwrite.
            // CLEAR-on-restore: derived provenance/idempotency caches keyed to
            // the pre-restore shape table. Cached child ids may not correspond
            // to any face/edge/vertex of the freshly-restored shapes, and
            // parent_handle's child → parent entries are likewise stale
            // (rebuilt on demand by a later extract_edges/faces/vertices call).
            extracted_edges.clear();
            extracted_faces.clear();
            extracted_vertices.clear();
            parent_handle.clear();
        }
    }
}

#[cfg(all(test, has_occt))]
impl OcctKernel {
    /// Inject a null `UniquePtr<OcctShape>` at `id` (present in the map, null pointer). Private,
    /// unlike the `*_for_test` constructors: no insert path can produce a null `UniquePtr`.
    fn insert_null_shape(&mut self, id: u64) {
        self.shapes.insert(id, cxx::UniquePtr::null());
        if id >= self.next_id {
            self.next_id = id + 1;
        }
    }
}

/// The crate's one test-fixture API, visible to the integration tests in
/// `tests/` and to this crate's own `cfg(test)` modules alike. Gated behind the
/// `test-fixtures` cargo feature so that downstream consumers building
/// `reify-kernel-occt` in normal mode do not see these methods on
/// `OcctKernel`'s public API surface.
///
/// Building any test target of this crate activates the self-dev-dep
/// `reify-kernel-occt = { path = ".", features = ["test-fixtures", ...] }` in
/// `Cargo.toml`, and cargo unifies `test-fixtures` into the lib-test build too.
/// Workspace siblings (e.g. `reify-eval`) that depend on this crate via
/// `reify-kernel-occt.workspace = true` do NOT enable the feature, so the
/// helpers remain hidden from production builds.
///
/// Naming: the `_for_test` suffix is retained for self-documentation; the
/// real isolation comes from the cfg gate above.
#[cfg(all(has_occt, feature = "test-fixtures"))]
impl OcctKernel {
    /// Run the FULL production STEP export — same mutex, same
    /// `wrap_occt_call("export_step")` label, same INV-AD-4 plane-angle
    /// refusal guard — with exactly one fault injected into the transferred
    /// STEP model, returning the guard's audit counts alongside the file text.
    ///
    /// This is the ONLY way to reach the guard's failure arms; why no real
    /// input can, and what each fault corrupts, are both on
    /// [`StepGuardFault`].
    ///
    /// # Errors
    ///
    /// - `ExportError::InvalidHandle` — if the handle is unknown.
    /// - `ExportError::FormatError` — the guard REFUSED the export (the
    ///   interesting case), or the fault could not be injected into this
    ///   fixture. Both surface with the production `"export_step: "`
    ///   attribution, so a test asserting refusal text is asserting exactly
    ///   what a user would see.
    #[doc(hidden)]
    pub fn export_step_with_injected_fault_for_test(
        &self,
        handle: GeometryHandleId,
        schema: &str,
        fault: StepGuardFault,
    ) -> Result<StepGuardProbeResult, ExportError> {
        let shape = self.get_shape(handle)?;
        ffi::ffi::export_step_with_injected_fault_for_test(shape, schema, fault)
            .map_err(|e| ExportError::FormatError(e.to_string()))
    }

    /// The same injected export, REPORTING the guard's finding in
    /// [`StepGuardProbeResult::refusal`] instead of returning `Err`.
    ///
    /// It differs from `export_step_with_injected_fault_for_test` in its
    /// `StepGuardDisposition` and in nothing else; what that word covers is on
    /// the enum itself, in `cpp/occt_wrapper.cpp`.
    ///
    /// WHY BOTH EXIST. The refusing hook is the only place the production
    /// behaviour is observable — a guard violation must surface as
    /// `ExportError::FormatError` with Reify's own attribution. But on that
    /// path the audit counts are reachable only as digits inside an English
    /// sentence, so every negative test had to scan the message for them. This
    /// one hands the same numbers back as `u32` fields; a test uses both and
    /// asserts the two texts agree.
    ///
    /// `refusal` is empty iff the export was accepted. When it is non-empty no
    /// file was written and `content` is empty — reporting a refusal does not
    /// weaken it into a warning.
    ///
    /// # Errors
    ///
    /// - `ExportError::InvalidHandle` — if the handle is unknown.
    /// - `ExportError::FormatError` — the fault could not be injected into
    ///   this fixture. That is a fixture defect rather than a finding about
    ///   the model, so it still surfaces as an error here: a fixture with
    ///   nothing to corrupt must not read as a guard hit.
    #[doc(hidden)]
    pub fn step_guard_probe_for_test(
        &self,
        handle: GeometryHandleId,
        schema: &str,
        fault: StepGuardFault,
    ) -> Result<StepGuardProbeResult, ExportError> {
        let shape = self.get_shape(handle)?;
        ffi::ffi::step_guard_probe_for_test(shape, schema, fault)
            .map_err(|e| ExportError::FormatError(e.to_string()))
    }

    /// Outward unit normal at the centroid of `face` as a typed `[f64; 3]`.
    ///
    /// Test-side counterpart to `kernel.query(GeometryQuery::FaceNormal(id))`
    /// that bypasses the `Value::String` JSON encoding. Useful for tests that
    /// filter faces by their outward-normal direction (e.g. cylinder cap vs.
    /// side) without coupling to the FaceNormal JSON shape.
    ///
    /// Calls the same `query_face_normal` FFI as the production query path,
    /// so semantics (REVERSED-aware outward direction, unit length, centroid
    /// sampling) are identical.
    ///
    /// # Errors
    ///
    /// - `QueryError::InvalidHandle` — if the handle is unknown.
    /// - `QueryError::QueryFailed` — if the shape is not a face, has no
    ///   underlying surface, or yields a degenerate normal.
    pub fn face_outward_unit_normal_for_test(
        &self,
        face: GeometryHandleId,
    ) -> Result<[f64; 3], QueryError> {
        self.face_outward_unit_normal(face)
    }

    /// Create a circle face at the given z-height via the OCCT FFI and store
    /// it in the kernel, returning its `GeometryHandleId`.
    ///
    /// Exposed for integration tests that need a `TopAbs_FACE` fixture without
    /// going through the production `GeometryOp` API, which has no standalone
    /// face constructor.
    pub fn store_circle_face_for_test(&mut self, radius: f64, z: f64) -> GeometryHandleId {
        let shape = ffi::ffi::make_circle_face(radius, z)
            .expect("make_circle_face should succeed in test fixture");
        let h = self.store_with_repr(shape, BRepKind::Face);
        h.id
    }

    /// Create a circle wire of `radius` at height `z` and store it as a `BRepKind::Wire`.
    pub fn store_circle_wire_for_test(&mut self, radius: f64, z: f64) -> GeometryHandleId {
        let shape = ffi::ffi::make_circle_wire(radius, z)
            .expect("make_circle_wire should succeed in test fixture");
        self.store_with_repr(shape, BRepKind::Wire).id
    }

    /// Create an open cylindrical lateral face and store it as a `BRepKind::Face`.
    pub fn store_cylindrical_face_for_test(
        &mut self,
        radius: f64,
        height: f64,
    ) -> GeometryHandleId {
        let shape = ffi::ffi::make_cylindrical_face(radius, height)
            .expect("make_cylindrical_face should succeed in test fixture");
        self.store_with_repr(shape, BRepKind::Face).id
    }

    /// Store the EMPTY `TopoDS_Compound` fixture as a `BRepKind::Compound`.
    pub fn store_empty_compound_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_empty_compound_for_test()
            .expect("make_empty_compound_for_test should succeed");
        self.store_with_repr(shape, BRepKind::Compound).id
    }

    /// Store a non-null `OcctShape` wrapping null topology; `get_shape` refuses it on
    /// every read, whatever its kind. Stamped `BRepKind::Solid`, the kind its tests
    /// always stored it as.
    pub fn store_null_topology_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_null_shape_for_test();
        self.store_with_repr(shape, BRepKind::Solid).id
    }

    /// Create a `width × height` rectangular face centered at the origin in
    /// the XY plane via the OCCT FFI and store it in the kernel, returning
    /// its `GeometryHandleId` (registered with `BRepKind::Face`).
    ///
    /// Exposed for integration tests of sweep history primitives
    /// (`extrude_with_history`, `revolve_with_history`) that need a planar
    /// face profile. The production `GeometryOp` API has no standalone face
    /// constructor — see `crates/reify-kernel-occt/src/lib.rs` `store_*_for_test`
    /// section docstring for the rationale.
    pub fn store_rect_face_for_test(&mut self, width: f64, height: f64) -> GeometryHandleId {
        let shape = ffi::ffi::make_rect_face(width, height, 0.0, 0.0, 0.0)
            .expect("make_rect_face should succeed in test fixture");
        let h = self.store_with_repr(shape, BRepKind::Face);
        h.id
    }

    /// Create a `width × height` rectangular face in the XZ plane centered
    /// at `(cx, cy, cz)`, store it in the kernel, and return its
    /// `GeometryHandleId` (registered with `BRepKind::Face`).
    ///
    /// Variant of `store_rect_face_for_test` purpose-built for the
    /// `revolve_with_history` integration test: the rect is placed in the
    /// XZ plane (a plane containing the Z-axis), so revolving it about Z
    /// produces a non-degenerate torus-like solid: create rect in XY plane at
    /// origin → rotate 90° about X → translate to `(cx, cy, cz)`.
    /// `width` becomes the radial dimension (along X), `height` the axial
    /// dimension (along Z).
    pub fn store_rect_face_at_for_test(
        &mut self,
        width: f64,
        height: f64,
        cx: f64,
        cy: f64,
        cz: f64,
    ) -> GeometryHandleId {
        let face = ffi::ffi::make_rect_face(width, height, 0.0, 0.0, 0.0)
            .expect("make_rect_face should succeed in test fixture");
        let rotated = ffi::ffi::rotate_shape(&face, 1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2)
            .expect("rotate_shape should succeed in test fixture");
        let translated = ffi::ffi::translate_shape(&rotated, cx, cy, cz)
            .expect("translate_shape should succeed in test fixture");
        let h = self.store_with_repr(translated, BRepKind::Face);
        h.id
    }

    /// Create a triangular face in the plane Y=cy with vertices
    /// (x1, cy, z1), (x2, cy, z2), (x3, cy, z3), store it in the kernel,
    /// and return its `GeometryHandleId` (registered with `BRepKind::Face`).
    ///
    /// Used by the revolve history regression test (task 2636, step-3) to
    /// exercise a non-rectangular profile with one radial edge (bottom edge
    /// perpendicular to Z) and two slanted edges.  With cy=0, the face lies
    /// in the XZ plane, suitable for revolving about the Z axis.
    #[allow(clippy::too_many_arguments)] // 3 vertex coords + cy is intrinsic to triangle-face geometry; struct wrapping adds noise.
    pub fn store_triangle_face_at_for_test(
        &mut self,
        x1: f64,
        z1: f64,
        x2: f64,
        z2: f64,
        x3: f64,
        z3: f64,
        cy: f64,
    ) -> GeometryHandleId {
        let shape = ffi::ffi::make_triangle_face(x1, z1, x2, z2, x3, z3, cy)
            .expect("make_triangle_face should succeed in test fixture");
        let h = self.store_with_repr(shape, BRepKind::Face);
        h.id
    }

    /// Build a non-manifold compound (3 faces sharing 1 edge) and store it.
    ///
    /// Returns the `GeometryHandleId` of the stored compound. Used by
    /// `conformance_integration` tests to verify that `IsManifold` returns `false`
    /// when an edge has 3+ incident faces.
    pub fn store_nonmanifold_compound_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_nonmanifold_compound_for_test()
            .expect("make_nonmanifold_compound_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Compound);
        h.id
    }

    /// Build a malformed solid (10×10×10 mm box missing one face) and store it.
    ///
    /// The solid's open shell causes `BRepCheck_Analyzer::IsValid()` to return
    /// `false`, exercising the analyzer branch of `is_watertight` (as opposed to
    /// the shape-type guard branch). Used by `conformance_integration` tests.
    pub fn store_malformed_solid_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_malformed_solid_for_test()
            .expect("make_malformed_solid_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Solid);
        h.id
    }

    /// Build a non-orientable shell (2 faces using a shared edge with the same
    /// orientation) and store it.
    ///
    /// `ShapeAnalysis_Shell::CheckOrientedShells` returns `Standard_True` (problems
    /// found) for this shell, so `is_orientable` returns `false`. Used by
    /// `conformance_integration` tests.
    pub fn store_nonorientable_shell_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_nonorientable_shell_for_test()
            .expect("make_nonorientable_shell_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Shell);
        h.id
    }

    /// Extract the closed shell of a 10×10×10 mm box and store it.
    ///
    /// The returned shape has `TopAbs_ShapeType() == TopAbs_SHELL` and passes
    /// all three conformance predicates (`is_watertight`, `is_manifold`,
    /// `is_orientable`). Used by `conformance_integration` tests to exercise
    /// the `TopAbs_SHELL` guard arm in `is_watertight`.
    pub fn store_closed_shell_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_closed_shell_for_test()
            .expect("make_closed_shell_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Shell);
        h.id
    }

    /// Build a closed, non-manifold shell and store it.
    ///
    /// The shell consists of two 10 mm³ box volumes sharing a single interior
    /// partition face at X = 10 mm. All 20 unique edges have at least 2 incident
    /// faces (no free edges → closed), but the 4 partition edges each have 3
    /// incident faces (non-manifold).
    ///
    /// Expected predicates: `is_closed=true`, `is_manifold=false`,
    /// `is_watertight=false`.
    ///
    /// Used by `conformance_integration` tests to distinguish the weaker
    /// `is_closed` predicate (no free edges) from `is_watertight` (Closed ∧
    /// Manifold). The C++ fixture validates the closed-non-manifold topology via
    /// a defensive structural self-check.
    pub fn store_closed_nonmanifold_shell_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_closed_nonmanifold_shell_for_test()
            .expect("make_closed_nonmanifold_shell_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Shell);
        h.id
    }

    /// Build a single straight edge along the X axis and store it.
    ///
    /// The returned shape has `TopAbs_ShapeType() == TopAbs_EDGE`. Used by
    /// `conformance_integration` tests to exercise the `TopAbs_EDGE`
    /// short-circuit path in `is_watertight`.
    pub fn store_edge_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_edge_for_test().expect("make_edge_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Edge);
        h.id
    }

    /// Build a single vertex at the origin and store it.
    ///
    /// The returned shape has `TopAbs_ShapeType() == TopAbs_VERTEX`. Used by
    /// `conformance_integration` tests to exercise the `TopAbs_VERTEX`
    /// short-circuit path in `is_watertight`.
    pub fn store_vertex_for_test(&mut self) -> GeometryHandleId {
        let shape = ffi::ffi::make_vertex_for_test().expect("make_vertex_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Vertex);
        h.id
    }

    /// Build a single vertex at `(x, y, z)` and store it.
    ///
    /// Parameterised companion to `store_vertex_for_test` for tests that need
    /// to pin a non-origin location (e.g. `vertex_point` round-trip
    /// verification in `tests/vertex_point_integration.rs`).
    // G-allow: task #3535 (done) vertex_point FFI test fixture; permanent integration-test support only (no production consumer intended)
    pub fn store_vertex_at_for_test(&mut self, x: f64, y: f64, z: f64) -> GeometryHandleId {
        let shape = ffi::ffi::make_vertex_at_for_test(x, y, z)
            .expect("make_vertex_at_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Vertex);
        h.id
    }

    /// Build a CompSolid containing one 10×10×10 mm box solid and store it.
    ///
    /// The returned shape has `TopAbs_ShapeType() == TopAbs_COMPSOLID`, which
    /// passes the shape-type guard in `is_watertight` and reaches
    /// `BRepCheck_Analyzer::IsValid()`. Used by `conformance_integration` tests.
    pub fn store_compsolid_for_test(&mut self) -> GeometryHandleId {
        let shape =
            ffi::ffi::make_compsolid_for_test().expect("make_compsolid_for_test should succeed");
        let h = self.store_with_repr(shape, BRepKind::Compound);
        h.id
    }

    /// Apply a rotation+translation placement to `src_handle`'s shape using
    /// `BRepBuilderAPI_Transform(..., Copy=false)`, encoding the transform into
    /// `TopLoc_Location` rather than baking it into geometry.
    ///
    /// This produces a shape with a non-identity `TopLoc_Location`, which
    /// exercises the location-aware evaluation path in `BRepAdaptor_Surface`.
    /// Unlike the production `translate_shape`/`rotate_shape` FFI calls (which
    /// use `Copy=true` and bake the transform), this fixture preserves the
    /// non-identity location so it reaches the BRepAdaptor composition path.
    ///
    /// Used by placed-face integration tests to verify that `curvature_at` and
    /// `surface_normal_at` agree after the BRepAdaptor unification.
    #[allow(clippy::too_many_arguments)]
    pub fn store_placed_for_test(
        &mut self,
        src_handle: GeometryHandleId,
        ax: f64,
        ay: f64,
        az: f64,
        angle_rad: f64,
        dx: f64,
        dy: f64,
        dz: f64,
    ) -> GeometryHandleId {
        let src = self
            .get_shape(src_handle)
            .expect("store_placed_for_test: src_handle must be valid");
        let placed =
            ffi::ffi::apply_test_placement_for_test(src, ax, ay, az, angle_rad, dx, dy, dz)
                .expect("apply_test_placement_for_test should succeed");
        self.store_with_policy(placed, ReprPolicy::Inherit(src_handle))
            .expect("store_placed_for_test: src_handle's repr is recorded with its shape")
            .id
    }

    /// Build a half-space via the OCCT FFI and store it in the kernel, returning
    /// `Ok(GeometryHandleId)` on success or `Err(GeometryError)` on failure.
    ///
    /// The boundary plane passes through `(px, py, pz)` with outward normal
    /// `(nx, ny, nz)` pointing toward the retained material side.
    /// A zero-length normal returns `Err` (gp_Dir construction error).
    ///
    /// Used by `tests/half_space_integration.rs` to exercise `make_half_space`
    /// before `GeometryOp::HalfSpace` is wired (step-8).
    pub fn make_half_space_for_test(
        &mut self,
        px: f64,
        py: f64,
        pz: f64,
        nx: f64,
        ny: f64,
        nz: f64,
    ) -> Result<GeometryHandleId, GeometryError> {
        let shape = ffi::ffi::make_half_space(px, py, pz, nx, ny, nz)
            .map_err(|e| GeometryError::OperationFailed(e.to_string()))?;
        let h = self.store_with_repr(shape, BRepKind::Solid);
        Ok(h.id)
    }
}

/// Tests that need only the public API plus the `*_for_test` constructors live in
/// `tests/harness_occt_kernel/`, and tests that drive only the cxx bridge live in
/// `ffi_contract_tests.rs`. What stays here must observe a private `OcctKernel` invariant.
#[cfg(all(test, has_occt))]
mod tests {
    use super::*;
    use crate::floor_constants::RUST_GUARD_MARKER;

    /// Run one warm-start capture/restore/re-extract cycle, shared by
    /// `warm_state_persisted_shape_count_stays_bounded_across_cycles` and
    /// `warm_state_persisted_shape_count_bounded_with_post_extraction_root`
    /// so each test body keeps only its distinct setup and final assertion.
    ///
    /// Captures `kernel`'s warm state, records the persisted
    /// `(shapes.len(), reprs.len())` counts and the persisted `shapes` key
    /// set (so callers can pin surviving-id identity, not just cardinality),
    /// restores into a fresh kernel, runs `mid_cycle` on the restored
    /// (pre-re-extraction) kernel for any cycle-specific checks (e.g.
    /// asserting root survival) — `mid_cycle` takes `&mut OcctKernel` so a
    /// caller can also re-extract sub-shapes on a root of its own (e.g. a
    /// second, post-extraction root) instead of just the box — then
    /// re-seeds `parent_handle` by re-running extract_faces/extract_edges/
    /// extract_vertices on `box_root` against the cleared idempotency cache.
    /// `box_root` is taken as a parameter — the caller's actual box handle
    /// id, captured from its `execute` return value — instead of this
    /// helper assuming `GeometryHandleId(1)`, so it stays correct if a
    /// caller's id-assignment order ever changes. Returns the
    /// restored+re-extracted kernel plus the counts and id set captured
    /// before restore.
    fn warm_state_reextract_cycle(
        kernel: OcctKernel,
        box_root: GeometryHandleId,
        mid_cycle: impl FnOnce(&mut OcctKernel),
    ) -> (OcctKernel, usize, usize, std::collections::HashSet<u64>) {
        let state = kernel.warm_state().expect("kernel should have warm state");
        let warm = state
            .downcast_ref::<OcctWarmState>()
            .expect("warm state should downcast_ref to OcctWarmState");
        let shapes_len = warm.shapes.len();
        let reprs_len = warm.reprs.len();
        let shape_ids: std::collections::HashSet<u64> = warm.shapes.keys().copied().collect();

        let mut next = OcctKernel::new();
        next.with_warm_state(state);

        mid_cycle(&mut next);

        next.extract_faces(box_root)
            .expect("extract_faces on the restored box should succeed");
        next.extract_edges(box_root)
            .expect("extract_edges on the restored box should succeed");
        next.extract_vertices(box_root)
            .expect("extract_vertices on the restored box should succeed");

        (next, shapes_len, reprs_len, shape_ids)
    }

    /// RED step-1 (task 5162): `warm_state()` currently persists sub-shape
    /// blobs minted by `extract_faces`/`extract_edges`/`extract_vertices`
    /// wholesale, even though those ids are orphaned on restore (their
    /// `parent_handle` provenance is CLEAR-on-restore). Across repeated
    /// warm-start cycles — capture, restore into a fresh kernel, re-extract
    /// to re-seed `parent_handle` against the cleared idempotency cache —
    /// this is unbounded, not just single-cycle bloat: each cycle's orphans
    /// round-trip through the next, and the following extraction mints yet
    /// more fresh ids on top.
    ///
    /// A single root box (id 1) has 6 faces + 12 edges + 8 vertices = 26
    /// sub-shapes, so on current main the persisted count grows
    /// [1, 27, 53, 79] across 4 cycles — the "stays constant at the root
    /// count" assertion below trips because cycles after the first (27, 53,
    /// 79) differ from `counts[0]` (1). After the fix, only the root
    /// persists every cycle: [1, 1, 1, 1].
    #[test]
    fn warm_state_persisted_shape_count_stays_bounded_across_cycles() {
        let mut kernel = OcctKernel::new();
        let box_handle = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();
        let box_id = box_handle.id;

        let mut counts = Vec::new();
        let mut repr_counts = Vec::new();
        for _ in 0..4 {
            // Re-seeds parent_handle against the cleared idempotency cache,
            // mirroring the extract-after-restore pattern from
            // `tests/harness_occt_kernel/lifecycle.rs`'s
            // `owner_body_survives_warm_start`; no mid-cycle checks needed here.
            let (next, shapes_len, reprs_len, _shape_ids) =
                warm_state_reextract_cycle(kernel, box_id, |_next| {});
            counts.push(shapes_len);
            repr_counts.push(reprs_len);
            kernel = next;
        }

        // The stronger "stays constant at the root count" assertion below
        // subsumes a "does not grow cycle-over-cycle" check (windows(2).all(|w|
        // w[1] <= w[0])) — any growth is itself a violation of all-equal, and
        // the printed `counts`/`repr_counts` vector already pinpoints where
        // growth would occur, so only the all-equal assertion is kept here.
        assert!(
            counts.iter().all(|&c| c == counts[0]),
            "persisted shape count must stay constant at the root count across cycles: {counts:?}"
        );
        // shapes/reprs are filtered in lock-step (see warm_state()'s "Bounded
        // payload" note), so reprs must stay just as bounded — guards against
        // a future change that filters shapes but forgets reprs, which the
        // consumer-side debug_assert in with_warm_state only catches in debug
        // builds.
        assert!(
            repr_counts.iter().all(|&c| c == repr_counts[0]),
            "persisted repr count must stay constant at the root count across cycles: {repr_counts:?}"
        );
    }

    /// Sibling of `warm_state_persisted_shape_count_stays_bounded_across_cycles`
    /// covering the scenario its debug_assert-only guarantee doesn't pin
    /// behaviorally: a *second* root created via `execute` AFTER
    /// `extract_faces`/`extract_edges`/`extract_vertices` has already
    /// populated `parent_handle` with the first root's derived sub-shape
    /// ids. `store_with_repr` mints ids by fetch-and-increment and never
    /// reuses one, so this post-extraction root's id is always strictly
    /// greater than every existing `parent_handle` key and can never
    /// collide with — and must never be caught by — the bounded-payload
    /// filter (the exact invariant `debug_assert_fresh_id` protects). This
    /// test pins that both roots survive every warm-start cycle while all
    /// sub-shapes are filtered, rather than relying solely on the debug
    /// assertion. The mid-cycle closure re-extracts the cylinder's own
    /// faces/edges/vertices on every restored kernel (mirroring the shared
    /// helper's box-only re-extraction), so the filter is exercised against
    /// a *second* root's derived ids each cycle — not just the box's —
    /// rather than the two-root count vacuously holding because the
    /// cylinder never has any sub-shape ids to filter in the first place.
    #[test]
    fn warm_state_persisted_shape_count_bounded_with_post_extraction_root() {
        let mut kernel = OcctKernel::new();
        let box_handle = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();
        let box_id = box_handle.id;
        kernel
            .extract_faces(box_id)
            .expect("extract_faces on the box should succeed");
        kernel
            .extract_edges(box_id)
            .expect("extract_edges on the box should succeed");
        kernel
            .extract_vertices(box_id)
            .expect("extract_vertices on the box should succeed");

        // Second root, created only after parent_handle already holds the
        // box's derived sub-shape ids above.
        let cyl = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let cyl_id = cyl.id;
        const ROOT_COUNT: usize = 2; // box (id 1) + post-extraction cylinder
        // Pins *identity*, not just cardinality: a filter that dropped a
        // root and kept an equal number of unrelated sub-shapes would still
        // satisfy the `counts == ROOT_COUNT` check below, so also assert
        // every cycle's persisted `warm.shapes` key set is exactly the two
        // root ids.
        let expected_ids: std::collections::HashSet<u64> =
            [box_id.0, cyl_id.0].into_iter().collect();

        let mut counts = Vec::new();
        for _ in 0..4 {
            // Mid-cycle: both roots must resolve after every restore, and the
            // cylinder's own faces/edges/vertices are re-extracted here (the
            // shared helper only re-extracts the box root afterward) so the
            // bounded-payload filter is exercised against a second root's
            // derived ids too, not just the box's. Re-seeding parent_handle
            // for the box happens inside the shared helper after this closure.
            let (next, shapes_len, _, shape_ids) =
                warm_state_reextract_cycle(kernel, box_id, |next| {
                    next.query(&GeometryQuery::Volume(box_id))
                        .expect("box root should survive warm-start round-trip");
                    next.query(&GeometryQuery::Volume(cyl_id)).expect(
                        "post-extraction cylinder root should survive warm-start round-trip",
                    );
                    next.extract_faces(cyl_id)
                        .expect("extract_faces on the restored cylinder should succeed");
                    next.extract_edges(cyl_id)
                        .expect("extract_edges on the restored cylinder should succeed");
                    next.extract_vertices(cyl_id)
                        .expect("extract_vertices on the restored cylinder should succeed");
                });
            counts.push(shapes_len);
            assert_eq!(
                shape_ids, expected_ids,
                "persisted shapes key set must be exactly the root ids \
                 (box={box_id:?}, cylinder={cyl_id:?}), not merely matching \
                 cardinality: got {shape_ids:?}"
            );
            kernel = next;
        }

        assert!(
            counts.iter().all(|&c| c == ROOT_COUNT),
            "persisted shape count must stay pinned at the two-root count \
             ({ROOT_COUNT}) across cycles — a post-extraction root must never \
             be dropped, and sub-shapes must stay filtered: {counts:?}"
        );
    }

    /// `reset()` part (a): frees every resident native shape and clears all
    /// derived provenance/idempotency caches, so a reused kernel carries no
    /// build-N state into a whole-file reload. All three `extract_*` caches
    /// (+ `parent_handle`) are populated first so each emptiness assertion is
    /// non-vacuous. Direct private-field access is valid here — `tests` is a
    /// descendant module of the crate root where `OcctKernel` is defined
    /// (same pattern as the warm-state tests below, e.g. `kernel.next_id`).
    #[test]
    fn reset_clears_shape_table_and_derived_caches() {
        let mut kernel = OcctKernel::new();
        let box_handle = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();
        let box_id = box_handle.id;
        kernel
            .extract_faces(box_id)
            .expect("extract_faces on the box should succeed");
        kernel
            .extract_edges(box_id)
            .expect("extract_edges on the box should succeed");
        kernel
            .extract_vertices(box_id)
            .expect("extract_vertices on the box should succeed");

        // Precondition: shapes + reprs + every derived cache are populated.
        assert!(
            kernel.shape_count() > 0,
            "box + extracted sub-shapes should be resident before reset"
        );
        assert!(!kernel.reprs.is_empty(), "reprs populated before reset");
        assert!(
            !kernel.extracted_faces.is_empty(),
            "extracted_faces populated before reset"
        );
        assert!(
            !kernel.extracted_edges.is_empty(),
            "extracted_edges populated before reset"
        );
        assert!(
            !kernel.extracted_vertices.is_empty(),
            "extracted_vertices populated before reset"
        );
        assert!(
            !kernel.parent_handle.is_empty(),
            "parent_handle populated before reset"
        );

        kernel.reset();

        assert_eq!(
            kernel.shape_count(),
            0,
            "reset must free every resident native shape"
        );
        assert!(kernel.reprs.is_empty(), "reset must clear reprs");
        assert!(
            kernel.extracted_faces.is_empty(),
            "reset must clear extracted_faces"
        );
        assert!(
            kernel.extracted_edges.is_empty(),
            "reset must clear extracted_edges"
        );
        assert!(
            kernel.extracted_vertices.is_empty(),
            "reset must clear extracted_vertices"
        );
        assert!(
            kernel.parent_handle.is_empty(),
            "reset must clear parent_handle"
        );
    }

    /// Shared body for the three `warm_state_completeness_debug_assert_fires_
    /// on_orphaned_extracted_*_entry` tests below: corrupt one `extracted_*`
    /// cache via `corrupt` with an id that has no matching `parent_handle`
    /// entry, then invoke `warm_state()` so its completeness cross-check
    /// panics. Only compiled under `#[cfg(debug_assertions)]` — see the
    /// firing tests' doc comment for why `#[should_panic]` needs that gate;
    /// gating the helper the same way avoids an unused-function warning in
    /// release-mode test builds, where none of its three callers compile.
    #[cfg(debug_assertions)]
    fn assert_completeness_guard_fires_on(corrupt: impl FnOnce(&mut OcctKernel)) {
        let mut kernel = OcctKernel::new();
        corrupt(&mut kernel);
        let _ = kernel.warm_state();
    }

    /// `warm_state()`'s completeness cross-check (see its doc comment) is a
    /// debug-only guard: every id cached in `extracted_edges`/
    /// `extracted_faces`/`extracted_vertices` must also be a `parent_handle`
    /// key, or the bounded-payload filter could silently leak an un-filtered
    /// orphan back into persisted warm-start state. `extract_edges`/
    /// `extract_faces`/`extract_vertices` always insert into both caches
    /// together (see their `store_with_repr` call sites), so this coupling
    /// never breaks through the public API — each of the three tests below
    /// reaches past it via one crate-private `extracted_*` field to confirm
    /// the guard actually fires for that cache, rather than trusting an
    /// untested assertion to be correct. They are three separate `#[test]`
    /// functions rather than one loop over the caches because a
    /// `#[should_panic]` test stops at the first panic — a shared loop body
    /// would only ever prove the first cache's corruption fires and leave
    /// the guard's `.flat_map([...])` coverage of the other two caches
    /// unverified.
    ///
    /// `#[cfg(debug_assertions)]` is required because `debug_assert!`
    /// compiles to a no-op in release builds — `#[should_panic]` would
    /// falsely "pass" in a release build where the panic never fires (dual-test
    /// pattern from `crates/reify-eval/src/kernel_registry.rs`'s
    /// `emit_kernel_selection_panics_when_total_is_zero`). No release-mode
    /// counterpart is added: `shapes`/`reprs` filtering never consults
    /// `extracted_*`, so a release build's silently-elided assert has no
    /// distinct fall-through behavior to pin beyond what the bounded-payload
    /// tests above already cover. If a future change ever makes the
    /// bounded-payload filter itself consult `extracted_*` (instead of
    /// keying solely off `parent_handle`), add a release-mode behavioral
    /// test alongside that change confirming a coupling violation still
    /// cannot leak an orphan into persisted state — this debug-only guard
    /// would no longer be sufficient on its own.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "must be a parent_handle key")]
    fn warm_state_completeness_debug_assert_fires_on_orphaned_extracted_faces_entry() {
        // Directly corrupt extracted_faces: cache an id with no matching
        // parent_handle entry, violating the coupling extract_faces always
        // maintains through the public API.
        assert_completeness_guard_fires_on(|kernel| {
            kernel
                .extracted_faces
                .insert(1, vec![GeometryHandleId(9_999)]);
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "must be a parent_handle key")]
    fn warm_state_completeness_debug_assert_fires_on_orphaned_extracted_edges_entry() {
        // Same as the extracted_faces variant above, but corrupts
        // extracted_edges — proves the completeness guard's iteration over
        // all three caches actually includes this one too.
        assert_completeness_guard_fires_on(|kernel| {
            kernel
                .extracted_edges
                .insert(1, vec![GeometryHandleId(9_999)]);
        });
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "must be a parent_handle key")]
    fn warm_state_completeness_debug_assert_fires_on_orphaned_extracted_vertices_entry() {
        // Same as the extracted_faces variant above, but corrupts
        // extracted_vertices — proves the completeness guard's iteration
        // over all three caches actually includes this one too.
        assert_completeness_guard_fires_on(|kernel| {
            kernel
                .extracted_vertices
                .insert(1, vec![GeometryHandleId(9_999)]);
        });
    }

    #[test]
    fn with_warm_state_preserves_state_on_total_deserialization_failure() {
        // Create a kernel with a box
        let mut kernel = OcctKernel::new();
        let box_h = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();

        // Verify box works
        let vol = kernel.query(&GeometryQuery::Volume(box_h.id)).unwrap();
        match &vol {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Construct a corrupted OcctWarmState manually
        let mut corrupted_shapes = HashMap::new();
        corrupted_shapes.insert(1, "INVALID_BREP_DATA".to_string());
        corrupted_shapes.insert(2, "ALSO_GARBAGE".to_string());
        let corrupted_warm = OcctWarmState {
            shapes: corrupted_shapes,
            reprs: HashMap::new(),
            next_id: 99,
        };
        let corrupted_state = OpaqueState::new(corrupted_warm, 64);

        // Apply corrupted warm state — should NOT destroy existing state
        kernel.with_warm_state(corrupted_state);

        // The original box should still be queryable
        let vol_after = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol_after {
            Value::Real(v) => assert!(
                (v - 6000.0).abs() < 1.0,
                "box volume should survive corrupted warm state, got {v}"
            ),
            other => panic!("expected Real, got {:?}", other),
        }

        // next_id should NOT have been advanced to 99 — new sphere should get ID 2
        let sphere_h = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(5.0),
            })
            .unwrap();
        assert_eq!(
            sphere_h.id,
            GeometryHandleId(2),
            "next_id should not advance on total deserialization failure"
        );
    }

    #[test]
    fn with_warm_state_total_failure_preserves_dirty_state_and_provenance() {
        // Dirty consumer: box + cylinder, with extraction provenance already
        // populated (parent_handle non-empty) before the restore attempt.
        let mut kernel = OcctKernel::new();
        kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();
        kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let cyl_faces = kernel
            .extract_faces(GeometryHandleId(2))
            .expect("extract_faces on the cylinder should succeed");

        // Snapshot every field the atomic-swap guard in `with_warm_state` is
        // responsible for leaving untouched when every incoming shape fails
        // to deserialize (the `staged.is_empty()` no-op path) — the exact
        // opposite of the clear-on-successful-restore behavior this PR
        // establishes for the swap-occurs path, so it needs its own pin.
        let shape_ids_before: std::collections::HashSet<u64> =
            kernel.shapes.keys().copied().collect();
        let reprs_before = kernel.reprs.clone();
        let next_id_before = kernel.next_id;
        let parent_handle_before = kernel.parent_handle.clone();

        // Construct a warm state with only undeserializable shape blobs.
        let mut corrupted_shapes = HashMap::new();
        corrupted_shapes.insert(1, "INVALID_BREP_DATA".to_string());
        corrupted_shapes.insert(2, "ALSO_GARBAGE".to_string());
        let corrupted_warm = OcctWarmState {
            shapes: corrupted_shapes,
            reprs: HashMap::new(),
            next_id: 999,
        };
        kernel.with_warm_state(OpaqueState::new(corrupted_warm, 64));

        // Nothing should have changed: staged ends up empty, so the atomic
        // swap — and the parent_handle/extracted_* clears that ride along
        // with it — must not run at all.
        let shape_ids_after: std::collections::HashSet<u64> =
            kernel.shapes.keys().copied().collect();
        assert_eq!(
            shape_ids_after, shape_ids_before,
            "shapes must be untouched when every entry fails to deserialize"
        );
        assert_eq!(
            kernel.reprs, reprs_before,
            "reprs must be untouched when every entry fails to deserialize"
        );
        assert_eq!(
            kernel.next_id, next_id_before,
            "next_id must be untouched when every entry fails to deserialize"
        );
        assert_eq!(
            kernel.parent_handle, parent_handle_before,
            "parent_handle provenance must survive a fully-failed warm-start restore \
             untouched, not just a successful one — the no-op path must not clear it"
        );
        assert_eq!(
            kernel.warm_start_failures(),
            2,
            "both corrupt entries should be counted even though the swap was skipped"
        );

        // And the provenance is still live end-to-end: OwnerBody for a
        // pre-existing cylinder face still resolves correctly post-no-op.
        assert_eq!(
            kernel
                .query(&GeometryQuery::OwnerBody(cyl_faces[0]))
                .unwrap(),
            Value::Int(2),
            "provenance should remain queryable after a fully-failed warm-start restore"
        );
    }

    #[test]
    fn with_warm_state_partial_deserialization_replaces_state() {
        // Create a helper kernel to get a valid cylinder BRep string
        let mut helper = OcctKernel::new();
        helper
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let helper_state = helper.warm_state().expect("helper should have warm state");
        let helper_warm = helper_state
            .downcast::<OcctWarmState>()
            .expect("should downcast");
        let valid_cylinder_brep = helper_warm
            .shapes
            .get(&1)
            .expect("handle 1 should exist")
            .clone();

        // Create main kernel with a box
        let mut kernel = OcctKernel::new();
        kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();

        // Verify it's a box (volume ~6000)
        let vol_before = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match &vol_before {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "box vol: {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Seed a stale parent_handle entry, mirroring
        // `tests/harness_occt_kernel/lifecycle.rs`'s
        // `owner_body_survives_warm_start`, so this PARTIAL-deserialization
        // restore (some shapes fail, `staged` non-empty) is pinned to clear
        // parent_handle too — not just the full-swap (all-shapes-succeed)
        // path that test covers.
        let box_faces = kernel
            .extract_faces(GeometryHandleId(1))
            .expect("extract_faces on the box should succeed");
        assert!(
            !box_faces.is_empty(),
            "box extraction should yield at least one face to seed a stale parent_handle entry"
        );
        let stale_child = box_faces[0];

        // Precondition: before the restore, OwnerBody correctly resolves the
        // box face back to its parent (handle 1).
        assert_eq!(
            kernel.query(&GeometryQuery::OwnerBody(stale_child)).unwrap(),
            Value::Int(1),
            "precondition: stale_child should resolve to its box parent before warm-start"
        );

        // Construct partially-corrupted warm state:
        // handle 1 = valid cylinder BRep, handle 2 = corrupt data
        let mut partial_shapes = HashMap::new();
        partial_shapes.insert(1, valid_cylinder_brep);
        partial_shapes.insert(2, "CORRUPT".to_string());
        let partial_warm = OcctWarmState {
            shapes: partial_shapes,
            reprs: HashMap::new(),
            next_id: 10,
        };
        let partial_state = OpaqueState::new(partial_warm, 64);

        // Apply partially-corrupted warm state
        kernel.with_warm_state(partial_state);

        // parent_handle must be cleared even on a PARTIAL-deserialization
        // restore — the clear rides along the same `if !staged.is_empty()`
        // branch as the full-swap path pinned by
        // `tests/harness_occt_kernel/lifecycle.rs`'s
        // `owner_body_survives_warm_start`. stale_child's underlying shape
        // (an old box face) is gone regardless post-swap, but OwnerBody
        // looks up `parent_handle` directly (see the query handler), so
        // this assertion is the only thing pinning that the map itself —
        // not just the shape table — was cleared on this branch.
        let post_restore = kernel.query(&GeometryQuery::OwnerBody(stale_child));
        assert!(
            matches!(post_restore, Err(QueryError::QueryFailed(_))),
            "stale parent_handle must be cleared on a partial warm-start restore too; got {:?}",
            post_restore
        );

        // Handle 1 should now be a cylinder (not a box)
        let vol_after = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol_after {
            Value::Real(v) => {
                // Cylinder volume = pi * r^2 * h = pi * 25 * 20 ≈ 1570.8
                assert!(
                    (v - 1570.8).abs() < 1.0,
                    "handle 1 should be cylinder volume ~1570.8, got {v}"
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }

        // Handle 2 should NOT exist (corrupt, was not restored)
        let result = kernel.query(&GeometryQuery::Volume(GeometryHandleId(2)));
        assert!(
            result.is_err(),
            "handle 2 should not exist (corrupt BRep was skipped)"
        );

        // next_id should be updated to 10 (swap occurred)
        let new_h = kernel
            .execute(&GeometryOp::Sphere {
                radius: Value::Real(3.0),
            })
            .unwrap();
        assert_eq!(
            new_h.id,
            GeometryHandleId(10),
            "next_id should be updated to 10 after partial restore"
        );
    }

    /// Build a warm state with 1 valid cylinder BRep (id 1) + 1 corrupt
    /// entry (id 2, `"CORRUPT_DATA"`) — deterministically drives the
    /// `deserialize_brep` `Err` branch for exactly one shape. Shared fixture
    /// for `with_warm_state_partial_failure_counts_and_restores_valid` and
    /// `with_warm_state_partial_failure_emits_tracing_warn`, which differ
    /// only in what they assert about the resulting failure (the counter
    /// vs. the tracing event).
    fn one_valid_one_corrupt_state() -> OpaqueState {
        // Create a helper kernel to get a valid cylinder BRep string
        let mut helper = OcctKernel::new();
        helper
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        let helper_state = helper.warm_state().expect("helper should have warm state");
        let helper_warm = helper_state
            .downcast::<OcctWarmState>()
            .expect("should downcast");
        let valid_brep = helper_warm
            .shapes
            .get(&1)
            .expect("handle 1 should exist")
            .clone();

        // Construct warm state: 1 valid + 1 corrupt
        let mut shapes = HashMap::new();
        shapes.insert(1, valid_brep);
        shapes.insert(2, "CORRUPT_DATA".to_string());
        let warm = OcctWarmState {
            shapes,
            reprs: HashMap::new(),
            next_id: 10,
        };
        OpaqueState::new(warm, 64)
    }

    #[test]
    fn with_warm_state_partial_failure_counts_and_restores_valid() {
        let state = one_valid_one_corrupt_state();

        let mut kernel = OcctKernel::new();
        kernel.with_warm_state(state);

        // The valid shape should be restored
        let vol = kernel
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1570.8).abs() < 1.0,
                    "handle 1 should be cylinder volume ~1570.8, got {v}"
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }

        // The failure counter should report 1 failed deserialization
        assert_eq!(
            kernel.warm_start_failures(),
            1,
            "should report 1 failed deserialization"
        );
    }

    /// INV-GEO-3: a shape that fails BRep deserialization during
    /// `with_warm_state` must emit a `tracing::warn!` event (production
    /// observability), not just increment the silent `last_warm_start_failures`
    /// counter. Mirrors the 1-valid + 1-corrupt fixture from
    /// `with_warm_state_partial_failure_counts_and_restores_valid`, but asserts on the
    /// tracing event instead of the counter: exactly 1 WARN, whose message
    /// contains "warm-start deserialization failed" and whose structured
    /// `shape_id` field identifies the failed shape (id 2).
    #[test]
    fn with_warm_state_partial_failure_emits_tracing_warn() {
        use reify_test_support::warn_capturing_subscriber;

        // Inoculate against tracing's per-callsite Interest cache — see
        // `prime_tracing_callsite_cache` in reify-test-support for why.
        reify_test_support::prime_tracing_callsite_cache();

        let state = one_valid_one_corrupt_state();
        let mut kernel = OcctKernel::new();

        let (subscriber, capture) = warn_capturing_subscriber();
        tracing::subscriber::with_default(subscriber, || {
            kernel.with_warm_state(state);
        });

        capture.assert_count_and_any_message_contains(1, "warm-start deserialization failed");
        capture.assert_any_event_field_contains("shape_id", "2");
    }

    #[test]
    fn get_shape_null_ptr_returns_error_not_panic() {
        let mut kernel = OcctKernel::new();
        // Create a valid box (id=1)
        kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();

        // Inject a null shape at id=42
        kernel.insert_null_shape(42);

        // Valid shape should still be accessible
        assert!(kernel.get_shape(GeometryHandleId(1)).is_ok());

        // Null shape should return Err(OperationFailed), not panic
        let result = kernel.get_shape(GeometryHandleId(42));
        match result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("null"),
                    "error should mention null, got: {msg}"
                );
            }
            Err(other) => panic!("expected OperationFailed, got {:?}", other),
            Ok(_) => panic!("expected error for null shape, got Ok"),
        }

        // Non-existent key should still return InvalidReference
        match kernel.get_shape(GeometryHandleId(999)) {
            Err(GeometryError::InvalidReference(id)) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            Err(other) => panic!("expected InvalidReference, got {:?}", other),
            Ok(_) => panic!("expected error for missing shape, got Ok"),
        }
    }

    /// The `OperationFailed` cause `get_shape` reports for a corrupted handle.
    fn get_shape_cause(kernel: &OcctKernel, h: GeometryHandleId) -> String {
        match kernel.get_shape(h) {
            Err(GeometryError::OperationFailed(msg)) => msg,
            Err(other) => panic!("get_shape({h:?}): expected OperationFailed, got {other:?}"),
            Ok(_) => panic!("get_shape({h:?}): expected OperationFailed, got Ok"),
        }
    }

    /// One handle of each corrupted-but-present kind — IsNull topology and a
    /// null `UniquePtr` — paired with the cause `get_shape` reports for it.
    fn corrupted_handles(kernel: &mut OcctKernel) -> Vec<(GeometryHandleId, String)> {
        let null_topology = kernel.store_null_topology_for_test();
        let null_pointer = GeometryHandleId(4000);
        kernel.insert_null_shape(null_pointer.0);
        [null_topology, null_pointer]
            .into_iter()
            .map(|h| (h, get_shape_cause(kernel, h)))
            .collect()
    }

    fn assert_query_cause<T: std::fmt::Debug>(r: Result<T, QueryError>, cause: &str, what: &str) {
        match r {
            Err(QueryError::QueryFailed(m)) if m == cause => {}
            other => panic!("{what}: expected QueryFailed({cause:?}), got {other:?}"),
        }
    }

    /// Both corrupted-handle kinds report `get_shape`'s cause through the
    /// query entry points, not `InvalidHandle`.
    #[test]
    fn queries_on_a_corrupted_handle_report_the_cause() {
        let mut kernel = OcctKernel::new();
        let good_box = kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(20.0),
                size_y: Value::Real(10.0),
                size_z: Value::Real(5.0),
            })
            .expect("Box creation must succeed")
            .id;
        for (h, cause) in corrupted_handles(&mut kernel) {
            assert_query_cause(
                kernel.query(&GeometryQuery::Volume(h)),
                &cause,
                &format!("Volume({h:?})"),
            );
            assert_query_cause(
                kernel.volume_measurement(h),
                &cause,
                &format!("volume_measurement({h:?})"),
            );
            assert_query_cause(
                kernel.extract_edges(h),
                &cause,
                &format!("extract_edges({h:?})"),
            );
            assert_query_cause(
                kernel.shapes_intersect(good_box, h),
                &cause,
                &format!("shapes_intersect(box, {h:?})"),
            );
        }
    }

    fn assert_export_cause<T: std::fmt::Debug>(
        r: Result<T, ExportError>,
        written: &[u8],
        cause: &str,
        what: &str,
    ) {
        match r {
            Err(ExportError::FormatError(m)) if m == cause => {}
            other => panic!("{what}: expected FormatError({cause:?}), got {other:?}"),
        }
        assert!(
            written.is_empty(),
            "{what}: wrote {} bytes before failing",
            written.len()
        );
    }

    /// Export, tessellation and mesh-deviation of either corrupted-handle kind
    /// report `get_shape`'s cause, through both export entry points and for
    /// every format — not `InvalidHandle`, which means the id is unknown.
    #[test]
    fn export_and_tessellate_of_a_corrupted_handle_report_the_cause_not_invalid_handle() {
        let mut kernel = OcctKernel::new();
        for (h, cause) in corrupted_handles(&mut kernel) {
            for format in [ExportFormat::Step, ExportFormat::Stl, ExportFormat::ThreeMF] {
                let mut buf = Vec::new();
                let r = kernel.export(h, format, &mut buf);
                assert_export_cause(r, &buf, &cause, &format!("export({h:?}, {format:?})"));

                let mut buf = Vec::new();
                let r = kernel.export_with_options(h, format, &ExportOptions::default(), &mut buf);
                assert_export_cause(
                    r,
                    &buf,
                    &cause,
                    &format!("export_with_options({h:?}, {format:?})"),
                );
            }
            match kernel.tessellate(h, 0.1) {
                Err(TessError::TessellationFailed(m)) if m == cause => {}
                other => panic!(
                    "tessellate({h:?}): expected TessellationFailed({cause:?}), got {other:?}"
                ),
            }
            let empty_mesh = Mesh {
                vertices: vec![],
                indices: vec![],
                normals: None,
            };
            assert_query_cause(
                kernel.measure_mesh_deviation(h, &empty_mesh),
                &cause,
                &format!("measure_mesh_deviation({h:?})"),
            );
        }
    }

    /// `execute_split` reports exactly what `get_shape` found: an unknown
    /// target is `InvalidReference`, a corrupted one its `OperationFailed`
    /// cause — never a corrupted handle mislabelled as unknown.
    #[test]
    fn execute_split_reports_the_get_shape_cause() {
        let split = |target| GeometryOp::Split {
            target,
            plane_origin: [0.0; 3],
            plane_normal: [0.0, 0.0, 1.0],
        };
        let mut kernel = OcctKernel::new();

        match kernel.execute_split(&split(GeometryHandleId(999))) {
            Err(GeometryError::InvalidReference(id)) => assert_eq!(id, GeometryHandleId(999)),
            other => panic!("unknown target: expected InvalidReference(999), got {other:?}"),
        }

        for (h, cause) in corrupted_handles(&mut kernel) {
            match kernel.execute_split(&split(h)) {
                Err(GeometryError::OperationFailed(m)) if m == cause => {}
                other => panic!(
                    "execute_split({h:?}): expected OperationFailed({cause:?}), got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn warm_state_skips_null_shapes() {
        let mut kernel = OcctKernel::new();

        // Create a valid box (id=1)
        kernel
            .execute(&GeometryOp::Box {
                size_x: Value::Real(10.0),
                size_y: Value::Real(20.0),
                size_z: Value::Real(30.0),
            })
            .unwrap();

        // Inject a null shape at id=2
        kernel.insert_null_shape(2);

        // warm_state should NOT panic and should return Some (valid shape exists)
        let state = kernel.warm_state();
        assert!(state.is_some(), "should serialize the valid shape");

        // Roundtrip: restore in a new kernel
        let mut kernel_b = OcctKernel::new();
        kernel_b.with_warm_state(state.unwrap());

        // Valid box should be preserved (volume ~6000)
        let vol = kernel_b
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}"),
            other => panic!("expected Real, got {:?}", other),
        }

        // Null shape (id=2) should NOT be present in restored kernel
        let result = kernel_b.query(&GeometryQuery::Volume(GeometryHandleId(2)));
        assert!(result.is_err(), "null shape should not survive warm-start");
    }

    #[test]
    fn brep_serialization_roundtrip() {
        // Create a box shape
        let shape = ffi::ffi::make_box(10.0, 20.0, 30.0).unwrap();

        // Serialize to BRep
        let brep = ffi::ffi::serialize_brep(&shape).unwrap();
        assert!(
            !brep.is_empty(),
            "BRep serialization should produce non-empty output"
        );

        // Deserialize from BRep
        cxx::let_cxx_string!(brep_cxx = brep.as_str());
        let restored = ffi::ffi::deserialize_brep(&brep_cxx).unwrap();

        // Query volume of deserialized shape
        let vol = ffi::ffi::query_volume(&restored).unwrap();
        assert!(
            (vol - 6000.0).abs() < 1.0,
            "expected volume ~6000, got {vol}"
        );
    }

    // --- validate_pipe_start_tangent helper unit tests ---

    #[test]
    fn validate_pipe_start_tangent_rejects_non_finite_components() {
        // Calls the pure helper directly with NaN / Infinity inputs and
        // asserts each non-finite component produces a "non-finite" error.
        let non_finite_cases = [
            ffi::ffi::Point3 {
                x: f64::NAN,
                y: 0.0,
                z: 1.0,
            },
            ffi::ffi::Point3 {
                x: 0.0,
                y: f64::INFINITY,
                z: 1.0,
            },
            ffi::ffi::Point3 {
                x: 0.0,
                y: 0.0,
                z: f64::NEG_INFINITY,
            },
        ];
        for t in non_finite_cases {
            let coords = (t.x, t.y, t.z);
            let result = super::validate_pipe_start_tangent(t);
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("non-finite"),
                        "expected error containing 'non-finite' for {coords:?}, got: {msg}"
                    );
                }
                Ok(()) => panic!("expected Err for non-finite tangent ({coords:?}), got Ok"),
                Err(other) => panic!(
                    "expected OperationFailed for non-finite tangent ({coords:?}), got {:?}",
                    other
                ),
            }
        }
    }

    #[test]
    fn validate_pipe_start_tangent_accepts_finite_non_z_tangents() {
        // Once the +Z-only restriction is lifted, the narrowed guard accepts
        // ANY finite tangent (the vector still feeds gp_Dir, so only the
        // finite/NaN check remains). Exercises +X, +Y, -Z, and a normalized
        // diagonal — all finite unit vectors — asserting each returns Ok(()).
        // Fails against the pre-task +Z guard, which rejects every non-+Z case.
        let inv_sqrt3 = 1.0 / 3.0_f64.sqrt();
        let cases = [
            (
                "+X",
                ffi::ffi::Point3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            ),
            (
                "+Y",
                ffi::ffi::Point3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                },
            ),
            (
                "-Z",
                ffi::ffi::Point3 {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
            ),
            (
                "normalized diagonal",
                ffi::ffi::Point3 {
                    x: inv_sqrt3,
                    y: inv_sqrt3,
                    z: inv_sqrt3,
                },
            ),
        ];
        for (label, t) in cases {
            let coords = (t.x, t.y, t.z);
            let result = super::validate_pipe_start_tangent(t);
            assert!(
                result.is_ok(),
                "expected Ok(()) for finite {label} tangent {coords:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn validate_pipe_start_tangent_rejects_nan_magnitude() {
        // Exercises the non-finite guard with NaN inputs: any NaN component is
        // caught by the is_finite() check and should produce a "non-finite" error.
        // Covers the NaN-in-y case ({0, NaN, 0}) which is absent from
        // validate_pipe_start_tangent_rejects_non_finite_components.
        let nan_cases = [
            ffi::ffi::Point3 {
                x: f64::NAN,
                y: 0.0,
                z: 1.0,
            },
            ffi::ffi::Point3 {
                x: 0.0,
                y: f64::NAN,
                z: 0.0,
            },
        ];
        for t in nan_cases {
            let coords = (t.x, t.y, t.z);
            let result = super::validate_pipe_start_tangent(t);
            match result {
                Err(GeometryError::OperationFailed(msg)) => {
                    assert!(
                        msg.contains("non-finite"),
                        "expected error containing 'non-finite' for NaN tangent {coords:?}, got: {msg}"
                    );
                }
                Ok(()) => panic!("expected Err for NaN tangent ({coords:?}), got Ok"),
                Err(other) => panic!(
                    "expected OperationFailed for NaN tangent ({coords:?}), got {:?}",
                    other
                ),
            }
        }
    }

    // --- make_line_wire degeneracy threshold tests (task-383 S2) ---

    #[test]
    fn rust_guard_above_floor_does_not_fire() {
        // Exercises the above-floor case via `OcctKernel::execute`:
        //   dist_sq = 1.1 × RUST_LINE_WIRE_MIN_LENGTH_SQ — the `[rust-guard]` marker must NOT
        //   appear in any error. `Ok` is not required: CPP_LINE_WIRE_MIN_LENGTH_SQ is 100×
        //   above the Rust floor, so the C++ layer still rejects this input; an
        //   `OperationFailed` without the marker is acceptable.
        // See `floor_constants::tests::below_floor_rejects_with_rust_guard_marker` and
        //   `tests/curve_constructors_integration.rs::line_segment_coincident_points_returns_error`
        //   for the below-floor complements.

        // Above-floor case: dist_sq = 1.1 × RUST_LINE_WIRE_MIN_LENGTH_SQ.
        // The Rust guard must NOT fire; any error here is from the C++ layer.
        let above_dx = (1.1 * crate::RUST_LINE_WIRE_MIN_LENGTH_SQ).sqrt();
        debug_assert!(
            above_dx * above_dx > crate::RUST_LINE_WIRE_MIN_LENGTH_SQ,
            "above_dx² must be strictly > RUST_LINE_WIRE_MIN_LENGTH_SQ after fp round-trip"
        );
        let mut kernel = OcctKernel::new();
        let above_result = kernel.execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: above_dx,
            y2: 0.0,
            z2: 0.0,
        });
        match above_result {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    !msg.contains(RUST_GUARD_MARKER),
                    "above-floor case must not fire the Rust guard; marker '[rust-guard]' must \
                     be absent in error (C++ rejection expected here), got: {msg:?}. This \
                     indicates the Rust guard was widened to fire above RUST_LINE_WIRE_MIN_LENGTH_SQ."
                );
            }
            Ok(_) => { /* stronger than needed but acceptable */ }
            Err(other) => panic!(
                "above-floor case should be Ok or OperationFailed, got {:?}",
                other
            ),
        }
    }

    // ── C4 kernel LENGTH tripwire (task #5751) ───────────────────────────────
    //
    // Boundary rows 13/14 of `docs/prds/v0_6/units-length-gate-completion.md`
    // through the real OCCT path. The tripwire is a DETECTOR, never a gate.
    //
    // Every kernel here is built with `OcctKernel::new()` on the TEST THREAD
    // rather than through `OcctKernelHandle`, whose worker thread would not
    // see the thread-local arm.

    /// The §2/D3 probe case, made attributable.
    ///
    /// `fillet(box(10mm,10mm,10mm), 1)` today produces a span-less
    /// `OCCT make_fillet_with_history: unexpected: BRepFilletAPI_MakeFillet
    /// failed`. Calling `extract_length_f64` directly means no OCCT execution
    /// happens at all, so the assertion cannot be perturbed by a downstream
    /// `BRepFilletAPI_MakeFillet` failure.
    #[test]
    fn occt_extract_length_f64_bare_value_names_op_kind_and_field() {
        reify_test_support::prime_tracing_callsite_cache();

        let op = GeometryOp::Fillet {
            target: GeometryHandleId(1),
            edges: vec![],
            radius: Value::Real(1.0),
        };
        let (subscriber, capture) = reify_test_support::warn_capturing_subscriber();
        let got = tracing::subscriber::with_default(subscriber, || {
            extract_length_f64(&Value::Real(1.0), &op, "radius")
        });

        capture.assert_count(1);
        let msg = &capture.messages()[0];
        assert!(msg.contains("Fillet"), "{msg}");
        assert!(msg.contains("radius"), "{msg}");
        capture.assert_any_event_field_contains("op_kind", "Fillet");
        capture.assert_any_event_field_contains("field", "radius");

        // Detector, not gate: the extraction still succeeds with the same
        // payload `extract_f64` would have produced.
        assert_eq!(got.expect("a bare Real still extracts"), 1.0);
    }

    /// **C4 accept/reject parity (step-9A).**
    ///
    /// For every `Value` shape, `extract_length_f64` and `extract_f64` agree on
    /// the accept/reject DISPOSITION, and where both accept, on the `f64`
    /// payload bit-for-bit.
    ///
    /// Error STRINGS are deliberately excluded from the parity claim: C4 itself
    /// requires the length-field error to change from the bare
    /// `"expected numeric value"` to one naming op kind and field. An error
    /// message is not accept/reject behaviour — the same inputs are still
    /// `Err`, only the string improves — so this asserts the upgrade instead.
    #[test]
    fn occt_length_tripwire_never_changes_accept_reject() {
        let op = GeometryOp::Fillet {
            target: GeometryHandleId(1),
            edges: vec![],
            radius: Value::Real(2.0),
        };
        let cases = vec![
            Value::Int(2),
            Value::Real(2.0),
            Value::length(2.0),
            Value::Scalar {
                si_value: 2.0,
                dimension: reify_core::DimensionVector::MASS,
            },
            Value::String("x".into()),
            Value::Undef,
        ];
        for v in cases {
            let gated = extract_length_f64(&v, &op, "radius");
            let plain = extract_f64(&v);
            assert_eq!(
                gated.is_ok(),
                plain.is_ok(),
                "disposition diverged for {v:?}"
            );
            match (gated, plain) {
                (Ok(a), Ok(b)) => assert_eq!(a.to_bits(), b.to_bits(), "payload diverged for {v:?}"),
                (Err(GeometryError::OperationFailed(s)), Err(_)) => {
                    assert!(s.contains("Fillet"), "{v:?}: {s}");
                    assert!(s.contains("radius"), "{v:?}: {s}");
                    assert!(
                        !s.contains("expected numeric value"),
                        "{v:?}: still the bare legacy string: {s}"
                    );
                }
                (g, p) => panic!("unexpected pairing for {v:?}: {g:?} vs {p:?}"),
            }
        }
    }

    #[test]
    fn brep_kind_decoder_rejects_an_unrecognised_shape_type_name() {
        let known = [
            ("Solid", BRepKind::Solid),
            ("CompSolid", BRepKind::Compound),
            ("Compound", BRepKind::Compound),
            ("Shell", BRepKind::Shell),
            ("Wire", BRepKind::Wire),
            ("Face", BRepKind::Face),
            ("Edge", BRepKind::Edge),
            ("Vertex", BRepKind::Vertex),
        ];
        for (name, kind) in known {
            assert_eq!(
                brep_kind_of_type_name(name).ok(),
                Some(kind),
                "known TopAbs name {name:?}"
            );
        }

        for unknown in ["Shape", "Bogus"] {
            match brep_kind_of_type_name(unknown) {
                Err(GeometryError::OperationFailed(msg)) => assert!(
                    msg.contains(unknown),
                    "error for {unknown:?} must name it, got: {msg}"
                ),
                other => panic!(
                    "unrecognised TopAbs name {unknown:?} must be an \
                     OperationFailed error, not silently classified: {other:?}"
                ),
            }
        }
    }

    /// The storing entry every `execute` arm ends in refuses, and stores
    /// nothing, when a policy cannot name a kind: a null shape under
    /// `Classify`, or an `Inherit` source the kernel has no kind for.
    #[test]
    fn store_with_policy_refuses_a_shape_it_cannot_name_a_kind_for() {
        let mut kernel = OcctKernel::new();
        let unknown_source = GeometryHandleId(999_999);
        let cases = [
            (
                "null shape under Classify",
                ffi::ffi::make_null_shape_for_test(),
                ReprPolicy::Classify,
                "null shape",
            ),
            (
                "Inherit from an unknown source",
                ffi::ffi::make_box(0.01, 0.01, 0.01).expect("make_box"),
                ReprPolicy::Inherit(unknown_source),
                "out of lock-step",
            ),
        ];
        for (case, shape, policy, cause) in cases {
            match kernel.store_with_policy(shape, policy) {
                Err(GeometryError::OperationFailed(msg)) => assert!(
                    msg.contains(cause),
                    "{case}: the error must name its cause {cause:?}, got: {msg}"
                ),
                other => panic!("{case}: expected OperationFailed, got {other:?}"),
            }
            assert_eq!(kernel.shape_count(), 0, "{case}: nothing may be stored");
        }
    }
}
