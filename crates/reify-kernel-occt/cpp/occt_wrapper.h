#pragma once
#include "rust/cxx.h"
#include <Precision.hxx>
#include <TopoDS_Shape.hxx>
#include <TopTools_IndexedDataMapOfShapeListOfShape.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <cstdint>
#include <memory>
#include <vector>

namespace occt {

/// Opaque wrapper around TopoDS_Shape for crossing the FFI boundary.
///
/// IMMUTABLE POST-CONSTRUCTION INVARIANT: Once `shape` is assigned (e.g.
/// `result->shape = maker.Shape()`), no FFI operation mutates it in place.
/// Every "modification" (translate, boolean, fillet, etc.) returns a fresh
/// OcctShape. The three topology-map caches below are therefore safe to
/// populate lazily and never need invalidation.
///
/// THREAD-SAFETY NOTE: The cache slots are unsynchronized (`mutable` without
/// a mutex). Safety relies on OcctShape being accessed only via the
/// `!Send + !Sync` `OcctKernel`, which pins all accesses to a single thread.
/// If a future change wraps OcctShape in something `Sync`, a synchronization
/// primitive (e.g. `std::call_once`) must be added to the lazy accessors.
///
/// STRONG-EXCEPTION-GUARANTEE: Each lazy accessor builds the map into a local
/// `unique_ptr` and moves it into the cache slot only after MapShapes /
/// MapShapesAndAncestors returns successfully.  On a throw the local dies, the
/// slot stays null, and the build counter stays at 0, so the next call retries
/// cleanly and observability remains honest.  See the per-accessor comments in
/// occt_wrapper.cpp for the rationale; do NOT reorder back to the
/// assign-then-fill pattern.
struct OcctShape {
    TopoDS_Shape shape;

    // --- Lazy topology-map cache slots ---
    // Null until first use; built exactly once per shape lifetime.
    mutable std::unique_ptr<TopTools_IndexedMapOfShape> face_map_cache_;
    mutable std::unique_ptr<TopTools_IndexedMapOfShape> edge_map_cache_;
    mutable std::unique_ptr<TopTools_IndexedDataMapOfShapeListOfShape> edge_face_map_cache_;

    // Build counters: each increments exactly once, on the cache miss that
    // populates the corresponding slot. Zero-cost on cache-hit fast paths.
    mutable uint32_t face_map_builds_ = 0;
    mutable uint32_t edge_map_builds_ = 0;
    mutable uint32_t edge_face_map_builds_ = 0;

    // --- Lazy accessor methods ---
    // Each returns a const reference to the corresponding cached map, building
    // it (and incrementing the counter) on the first call only.
    const TopTools_IndexedMapOfShape& face_map() const;
    const TopTools_IndexedMapOfShape& edge_map() const;
    const TopTools_IndexedDataMapOfShapeListOfShape& edge_face_map() const;
};

/// Opaque vector of TopoDS_Shape for passing N shapes across the CXX FFI boundary.
/// Uses push/build semantics: new_shape_vec() creates, shape_vec_push() adds shapes.
struct OcctShapeVec {
    std::vector<TopoDS_Shape> shapes;
};

/// Create an empty OcctShapeVec.
std::unique_ptr<OcctShapeVec> new_shape_vec();

/// Push a shape into the vector (mutable borrow via Pin).
void shape_vec_push(OcctShapeVec& vec, const OcctShape& shape);

/// Return the number of shapes in the vector.
size_t shape_vec_len(const OcctShapeVec& vec);

/// Return the i-th shape in the vector wrapped in a fresh `OcctShape`.
///
/// Deep-copies the underlying `TopoDS_Shape` (which is itself a thin handle,
/// so the copy is cheap). Throws std::runtime_error if `idx` is out of range.
std::unique_ptr<OcctShape> shape_vec_at(const OcctShapeVec& vec, size_t idx);

// Shared types — defined by cxx bridge. Forward-declared here for function signatures.
struct Point3;
struct BBox;
struct TessResult;
struct TopologyCacheBuildCounts;
struct InertiaTensor3x3;
/// Returned by `revolve_synthesis_post_sort_for_test`; defined by cxx bridge.
struct RevolveSynthesisPostSortResult;

// --- Foundation constants ---

/// Return OCCT's `Precision::Confusion()` value (~1e-7).
///
/// Test-fixture-style helper intentionally compiled into every build of the
/// wrapper.  Cfg-gating cxx::bridge entries is awkward, and the cost of
/// always shipping a tiny constant-returning function is lower than the
/// friction of conditional bridge declarations.  The current sole call site
/// is the `reify-kernel-occt` crate's private test module in `lib.rs`, which
/// pins `reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` against the
/// authoritative OCCT value at runtime.  The symbol is visible to all callers
/// of the wrapper, not hidden by construction.
double precision_confusion();

// --- Primitive construction ---

/// Create a box centered at origin with given dimensions (in meters).
std::unique_ptr<OcctShape> make_box(double width, double height, double depth);

/// Create a cylinder along Z axis (in meters).
std::unique_ptr<OcctShape> make_cylinder(double radius, double height);

/// Create a sphere centered at origin (in meters).
std::unique_ptr<OcctShape> make_sphere(double radius);

// --- Compound assembly ---

/// Assemble N solid shapes into a single TopoDS_Compound for multi-body STEP
/// export (T7 `make_compound`).
///
/// Each shape in `shapes` is added to a fresh compound via
/// `BRep_Builder::MakeCompound` + `Add` (mirrors the test helper
/// `make_nonmanifold_compound_for_test`). The source shapes are copied by
/// reference (TopoDS_Shape copy is a lightweight handle increment), so the
/// originals remain valid after the call.
///
/// Throws `std::runtime_error` if the input vector is empty.
std::unique_ptr<OcctShape> make_compound(const OcctShapeVec& shapes);

// --- Boolean operations ---

std::unique_ptr<OcctShape> boolean_fuse(const OcctShape& left, const OcctShape& right);
std::unique_ptr<OcctShape> boolean_cut(const OcctShape& left, const OcctShape& right);
std::unique_ptr<OcctShape> boolean_common(const OcctShape& left, const OcctShape& right);

// --- BRepAlgoAPI_* history (v0.2 persistent-naming-v2, task 2590) ---

/// Records the per-parent face/edge correspondence emitted by a single
/// `BRepAlgoAPI_*` boolean. Each record is a flat tuple of
/// `(parent_index, parent_subshape_index, result_subshape_index)` for
/// Modified/Generated, or `(parent_index, parent_subshape_index)` for
/// Deleted — all 0-based, packed into `std::vector<uint32_t>`.
///
/// We materialize the records EAGERLY at construction time because
/// `BRepAlgoAPI_*::Modified()/Generated()/IsDeleted()` query maps tied
/// to the algorithm object's lifetime. Once the algorithm goes out of
/// scope the maps are gone, so we can't lazily query them later.
///
/// `result` owns the fused shape; `boolean_op_history_take_result_shape`
/// hands it off to the kernel via `std::move`. Subsequent queries to
/// `result` after take are illegal (it is a moved-from `unique_ptr`).
struct BooleanOpHistory {
    std::unique_ptr<OcctShape> result;
    /// Count of Modified/Generated children that BRepAlgoAPI reported but
    /// that could not be found in the result face_map/edge_map. Should be
    /// zero for a well-formed boolean; non-zero indicates a kernel
    /// correspondence loss or map-type mismatch.
    ///
    /// Single bulk accumulator: `boolean_fuse_with_history` passes this field
    /// as `out_drop_count` to all four `emit_history_for_parent` calls (left
    /// faces, right faces, left edges, right edges), so it aggregates drops
    /// across shape kinds and operands without per-kind/per-operand breakdown.
    /// If a future consumer needs finer-grained diagnostics, split this field
    /// into separate face/edge or left/right counters and update
    /// `emit_history_for_parent`'s signature before adding new call sites.
    uint32_t silent_drop_count = 0;
    std::vector<uint32_t> face_modified;
    std::vector<uint32_t> face_generated;
    std::vector<uint32_t> face_deleted;
    std::vector<uint32_t> edge_modified;
    std::vector<uint32_t> edge_generated;
    std::vector<uint32_t> edge_deleted;
};

/// Run `BRepAlgoAPI_Fuse` on `left` and `right`, materializing the result
/// shape AND the Modified/Generated/Deleted records for each parent's
/// faces and edges into a single `BooleanOpHistory`.
///
/// Each parent's faces are walked in canonical TopExp `face_map()` order
/// (1-based, deduplicated by `IsSame`); the per-record indices are
/// 0-based at the FFI boundary. Result-side indices come from the result
/// shape's cached `face_map()`/`edge_map()`. Modified records map a
/// parent sub-shape to one or more result sub-shapes; Generated records
/// map a parent sub-shape to NEW result sub-shapes (e.g. section walls);
/// Deleted records carry only `(parent_index, parent_subshape_index)`
/// because the parent has no result analogue. Empty result-side lookups
/// (a child shape that BRepAlgoAPI reports but that doesn't appear in
/// the result `face_map`/`edge_map`) are silently skipped.
std::unique_ptr<BooleanOpHistory> boolean_fuse_with_history(const OcctShape& left, const OcctShape& right);

/// Move the result shape out of the history wrapper. Returns the freshly
/// constructed `OcctShape` for the kernel to register; subsequent calls
/// observe an empty `unique_ptr`.
std::unique_ptr<OcctShape> boolean_op_history_take_result_shape(BooleanOpHistory& history);

/// Six accessors returning the flat record buffers as `rust::Vec<uint32_t>`
/// (deep-copied at the FFI boundary). Each Modified/Generated buffer holds
/// flat groups of 3 `uint32_t`s `(parent_index, parent_subshape_index,
/// result_subshape_index)`; Deleted buffers hold groups of 2
/// `(parent_index, parent_subshape_index)`.
rust::Vec<uint32_t> boolean_op_history_face_modified(const BooleanOpHistory& history);
rust::Vec<uint32_t> boolean_op_history_face_generated(const BooleanOpHistory& history);
rust::Vec<uint32_t> boolean_op_history_face_deleted(const BooleanOpHistory& history);
rust::Vec<uint32_t> boolean_op_history_edge_modified(const BooleanOpHistory& history);
rust::Vec<uint32_t> boolean_op_history_edge_generated(const BooleanOpHistory& history);
rust::Vec<uint32_t> boolean_op_history_edge_deleted(const BooleanOpHistory& history);
/// Count of Modified/Generated children silently dropped because they could
/// not be found in the result map. Zero for a well-formed boolean operation.
uint32_t boolean_op_history_silent_drop_count(const BooleanOpHistory& history);

// --- BRepPrimAPI sweep history (v0.2 persistent-naming-v2, task 2573) ---

/// Records the per-parent face/edge correspondence emitted by a single-parent
/// sweep operation (BRepPrimAPI_MakePrism, BRepPrimAPI_MakeRevol). Mirrors
/// `BooleanOpHistory` but adds `start_cap_face_indices` / `end_cap_face_indices`
/// — the FirstShape() / LastShape() result-face indices that aren't part of
/// the standard Modified/Generated maps.
///
/// The `parent_index` field in each record (Modified/Generated/Deleted) is
/// always `0` because sweep operations have a single parent profile; this
/// field is preserved for cross-record uniformity with `BooleanOpHistory`.
///
/// As with `BooleanOpHistory`, we materialize records EAGERLY at construction
/// time because the algorithm's tracking maps are tied to its lifetime —
/// once it goes out of scope the maps are gone.
///
/// Diagnostic counters (full-revolution only): `unsynthesized_profile_edge_count`
/// counts non-degenerate, untracked profile edges that did not produce a
/// `face_generated` record in the synthesis post-pass (see
/// `synthesize_full_revolution_radial_face_records`). When non-zero, one OCCT
/// `Message_Warning` is emitted via `Message::DefaultMessenger()`.
/// `duplicate_parent_subshape_index_count` counts `face_generated` records
/// dropped by `revolve_synthesis_post_sort_and_dedup` because their
/// `parent_subshape_index` duplicated the preceding record after stable-sort.
/// Both counters are zero for well-formed full-revolution inputs and are
/// always zero for prism operations and partial revolves. Integration tests
/// should assert both are zero as a regression-grade health check.
///
/// `result` owns the swept shape; `sweep_op_history_take_result_shape`
/// hands it off to the kernel via `std::move`.
struct SweepOpHistory {
    std::unique_ptr<OcctShape> result;
    /// Count of Modified/Generated children that the sweep primitive observed
    /// but could not map back into the result face/edge map. Should be zero
    /// for a well-formed sweep; non-zero indicates a kernel correspondence
    /// loss or map-type mismatch.
    ///
    /// Single bulk accumulator: each `make_prism_with_history` /
    /// `make_revolve_with_history` / `make_pipe_with_history` call passes this
    /// field as `out_drop_count` to all four `emit_sweep_*` calls (face
    /// Modified, face Generated, edge Modified, edge Generated), so it
    /// aggregates drops across shape kinds without per-kind breakdown.
    /// If a future consumer needs finer-grained diagnostics, split this field
    /// into separate face/edge counters before adding new call sites.
    uint32_t silent_drop_count = 0;
    std::vector<uint32_t> face_modified;
    std::vector<uint32_t> face_generated;
    std::vector<uint32_t> face_deleted;
    std::vector<uint32_t> edge_modified;
    std::vector<uint32_t> edge_generated;
    std::vector<uint32_t> edge_deleted;
    /// 0-based result `face_map` indices of the FirstShape() cap (start of
    /// sweep). For prism this is the profile-as-placed; for partial revolve
    /// this is the profile in its starting orientation. Empty for full-2π
    /// revolutions where FirstShape == LastShape.
    std::vector<uint32_t> start_cap_face_indices;
    /// 0-based result `face_map` indices of the LastShape() cap (end of
    /// sweep). For prism this is the swept-end face; for partial revolve
    /// this is the profile rotated by `angle_rad`. Empty for full-2π revolutions.
    std::vector<uint32_t> end_cap_face_indices;
    /// Count of non-degenerate, untracked profile edges that passed through
    /// `synthesize_full_revolution_radial_face_records` without producing a
    /// `face_generated` record. Covers three exit paths: (4) axial-classifier
    /// (`dot(edge_dir, axis) > 1 - DIR_TOL`), (5) slanted-classifier
    /// (`dot > DIR_TOL`), and (6) inner face-matching loop fall-through.
    /// Degenerate edges (null vertices / zero-length) are NOT counted.
    /// Always 0 for prism operations and for partial revolves; only
    /// incremented by the full-revolution radial-face synthesis post-pass.
    /// Zero for well-formed profiles; non-zero indicates a synthesis gap.
    uint32_t unsynthesized_profile_edge_count = 0;
    /// Count of `face_generated` records dropped by `revolve_synthesis_
    /// post_sort_and_dedup` because their `parent_subshape_index` duplicated
    /// the immediately preceding record's (after stable-sort). Always 0 for
    /// well-formed profiles; non-zero indicates OCCT emitted a duplicate edge
    /// report or a synthesis collision occurred. Only incremented by the
    /// full-revolution radial-face synthesis post-pass.
    uint32_t duplicate_parent_subshape_index_count = 0;
};

/// Run `BRepPrimAPI_MakePrism` on `profile` along the direction vector
/// `(dx, dy, dz)` (must be non-zero), materializing the result shape AND the
/// Modified/Generated/Deleted records for the profile's face/edge sub-shapes
/// into a single `SweepOpHistory`. Also populates the cap-face index lists
/// from `FirstShape()` / `LastShape()` lookups in the result face_map.
///
/// Profile sub-shapes are walked in canonical TopExp `face_map()` /
/// `edge_map()` order (1-based, deduplicated by `IsSame`); per-record
/// indices are 0-based at the FFI boundary. Result-side indices come from
/// the result shape's cached `face_map()` / `edge_map()`. Empty result-side
/// lookups (a child reported by the algorithm but not appearing in the
/// result `face_map`/`edge_map`) are silently skipped.
std::unique_ptr<SweepOpHistory> make_prism_with_history(
    const OcctShape& profile, double dx, double dy, double dz);

/// Run `BRepPrimAPI_MakeRevol` on `profile` about the axis with origin
/// `(ox, oy, oz)` and direction `(ax, ay, az)` for `angle_rad` radians,
/// materializing the result shape AND the Modified/Generated/Deleted
/// records for the profile's face/edge sub-shapes into a single
/// `SweepOpHistory`. Also populates the cap-face index lists from
/// `FirstShape()` / `LastShape()` lookups in the result face_map.
///
/// Cap behavior: under PARTIAL revolution (angle_rad mod 2π ∈ (0, 2π))
/// `FirstShape()` and `LastShape()` reference distinct cap faces and
/// both lists are populated. Under FULL revolution (angle_rad's modulo-2π
/// residual is below `CPP_FULL_REVOLVE_TOL`) `FirstShape()` and
/// `LastShape()` reference the same closed surface; both cap lists
/// remain empty so consumers can encode the no-caps case naturally.
///
/// face_generated under FULL revolution: OCCT 7.5.x's `Generated()` does
/// not record edges that are perpendicular to the rotation axis (radial
/// edges) when angle == 2π — they sweep into flat annular-disk faces that
/// OCCT's tracking map omits.  A C++ post-pass (`synthesize_full_revolution_
/// radial_face_records`) closes this gap geometrically so that `face_generated`
/// contains exactly one record per profile edge (matching the invariant for
/// partial revolutions).  The combined vector is stable-sorted by
/// `parent_subshape_index` after synthesis so the ordering invariant
/// `record_position == parent_subshape_index` is preserved for both partial
/// and full revolutions.  Consumers (e.g., `populate_revolve_attributes` in
/// crates/reify-eval) may treat all face_generated records uniformly —
/// synthesized records are byte-identical to OCCT-reported ones.
///
/// Synthesis diagnostics: when any non-degenerate, untracked profile edge
/// fails to produce a `face_generated` record,
/// `SweepOpHistory::unsynthesized_profile_edge_count` is incremented and one
/// `Message_Warning` is emitted via
/// `Message::DefaultMessenger()` summarizing the count. After synthesis, if any
/// records have a duplicate `parent_subshape_index` after stable-sort, the
/// duplicate is dropped (first occurrence under stable order wins) and
/// `SweepOpHistory::duplicate_parent_subshape_index_count` is incremented per
/// drop. Debug builds additionally assert the post-dedup records are strictly
/// increasing in `parent_subshape_index`.
///
/// Honors the same Shell→Solid + `BRepLib::OrientClosedSolid`
/// post-processing as `make_revolve` (the result shape may be a Solid
/// constructed from the algorithm's Shell output).
///
/// Profile sub-shapes are walked in canonical TopExp order (1-based,
/// deduplicated by `IsSame`); per-record indices are 0-based at the
/// FFI boundary. Result-side indices come from the result shape's
/// cached `face_map()` / `edge_map()`.
std::unique_ptr<SweepOpHistory> make_revolve_with_history(
    const OcctShape& profile,
    double ox, double oy, double oz,
    double ax, double ay, double az,
    double angle_rad);

/// Run `BRepOffsetAPI_MakePipe` on `profile` along `spine` (a wire),
/// materializing the result shape AND the Modified/Generated/Deleted
/// records for the profile's face/edge/vertex sub-shapes into a single
/// `SweepOpHistory`. Also populates the cap-face index lists from
/// `FirstShape()` / `LastShape()` lookups in the result face_map.
///
/// `BRepOffsetAPI_MakePipe` inherits from `BRepPrimAPI_MakeSweep` (via
/// `BRepOffsetAPI_BuildAddSurface`), which inherits from
/// `BRepBuilderAPI_MakeShape` — so the templated `emit_sweep_*` helpers
/// reused from the prism/revolve wrappers work verbatim. Sweep is
/// single-parent like extrude/revolve (the spine is the path along which
/// the profile is swept; only the profile counts as the operand whose
/// sub-shapes propagate to the result), so the existing SweepOpHistory
/// shape fits — `parent_index` is always `0` and the cap-index lists
/// hold the start/end profile placements.
///
/// Diagnostic counters `unsynthesized_profile_edge_count` and
/// `duplicate_parent_subshape_index_count` remain `0` for sweep — those
/// are revolve-synthesis-specific (full-revolution radial-edge synthesis
/// post-pass) and have no analogue in pipe sweeping.
///
/// Profile sub-shapes are walked in canonical TopExp `face_map()` /
/// `edge_map()` order (1-based, deduplicated by `IsSame`); per-record
/// indices are 0-based at the FFI boundary. Result-side indices come
/// from the result shape's cached `face_map()` / `edge_map()`.
std::unique_ptr<SweepOpHistory> make_pipe_with_history(
    const OcctShape& profile, const OcctShape& spine);

/// Move the result shape out of the history wrapper. Returns the freshly
/// constructed `OcctShape` for the kernel to register; subsequent calls
/// observe an empty `unique_ptr`.
std::unique_ptr<OcctShape> sweep_op_history_take_result_shape(SweepOpHistory& history);

// --- BRepOffsetAPI_ThruSections loft history (v0.2 persistent-naming-v2, task 2619) ---

/// Records the per-section face/edge correspondence emitted by a loft
/// (`BRepOffsetAPI_ThruSections`). Loft is **multi-parent**: each profile
/// section indexed `0..N-1` is a distinct parent, and `parent_index` in
/// every record denotes the section index (NOT always 0 like
/// `SweepOpHistory`). Field shape mirrors `SweepOpHistory` for layout
/// uniformity but **without** the diagnostic counters
/// `unsynthesized_profile_edge_count` / `duplicate_parent_subshape_index_count`
/// — those are revolve-synthesis-specific (full-2π radial-edge synthesis
/// post-pass, task 2706) and have no analogue in loft.
///
/// `face_modified` / `face_deleted` / `edge_modified` / `edge_deleted` are
/// expected to be empty for `BRepOffsetAPI_ThruSections` — the algorithm
/// generates a fresh shape rather than transforming one parent. Kept here
/// for layout uniformity with `SweepOpHistory`. `face_generated` carries
/// the per-section edge → result-face correspondence sourced from
/// `loft.GeneratedFace(edge)`.
///
/// `start_cap_face_indices` / `end_cap_face_indices` are populated only
/// when the underlying loft was constructed with `is_solid=true`
/// (closing caps from the first / last profile sections).
///
/// As with `SweepOpHistory`, records are materialized EAGERLY at
/// construction time because the algorithm's tracking maps are tied to
/// its lifetime — once it goes out of scope the maps are gone.
///
/// `result` owns the lofted shape; `loft_op_history_take_result_shape`
/// hands it off to the kernel via `std::move`.
struct LoftOpHistory {
    std::unique_ptr<OcctShape> result;
    std::vector<uint32_t> face_modified;
    std::vector<uint32_t> face_generated;
    std::vector<uint32_t> face_deleted;
    std::vector<uint32_t> edge_modified;
    std::vector<uint32_t> edge_generated;
    std::vector<uint32_t> edge_deleted;
    /// 0-based result `face_map` indices of the FirstShape() cap (first
    /// profile section under `is_solid=true`). Empty when `is_solid=false`.
    std::vector<uint32_t> start_cap_face_indices;
    /// 0-based result `face_map` indices of the LastShape() cap (last
    /// profile section under `is_solid=true`). Empty when `is_solid=false`.
    std::vector<uint32_t> end_cap_face_indices;
};

/// Run `BRepOffsetAPI_ThruSections` on `profiles` (N >= 2 wire profiles),
/// materializing the result shape AND the per-section face correspondence
/// records into a single `LoftOpHistory`. The algorithm exposes
/// `GeneratedFace(edge)` (NOT the generic `Generated()` interface), so
/// the helper walks each profile section's edges and maps each to its
/// generated lateral face in the result.
///
/// `is_solid=true` produces a closed solid (caps populated from
/// `FirstShape()` / `LastShape()`); `is_solid=false` produces an open
/// shell with empty cap-index lists. `is_ruled` is hard-coded to
/// `Standard_False` to match the `loft_profiles` non-history variant
/// (smooth interpolation between sections).
///
/// `parent_index` in every `face_generated` record is the section index
/// (0..N-1 across N profiles); `parent_subshape_index` is the per-section
/// edge index in canonical TopExp `MapShapes(profile, TopAbs_EDGE, _)`
/// order. **Cannot reuse `emit_sweep_modified_deleted_for_parent` /
/// `emit_sweep_generated_cross_type` directly** because those hard-code
/// `parent_index = 0` (single-parent contract); loft's per-section walk
/// is implemented inline below.
///
/// Result-side indices come from the result shape's cached
/// `face_map()` / `edge_map()`. Empty result-side lookups (a child
/// reported by the algorithm but not appearing in the result map) are
/// silently skipped.
///
/// Throws `std::runtime_error` if `profiles.size() < 2` or the
/// `BRepOffsetAPI_ThruSections` algorithm fails (`!IsDone()`).
std::unique_ptr<LoftOpHistory> make_loft_with_history(
    const OcctShapeVec& profiles, bool is_solid);

/// Move the result shape out of the loft-history wrapper. Returns the
/// freshly constructed `OcctShape` for the kernel to register; subsequent
/// calls observe an empty `unique_ptr`.
std::unique_ptr<OcctShape> loft_op_history_take_result_shape(LoftOpHistory& history);

/// Eight accessors returning the flat record buffers as `rust::Vec<uint32_t>`
/// (deep-copied at the FFI boundary). Modified/Generated buffers hold flat
/// groups of 3 `uint32_t`s `(parent_index, parent_subshape_index,
/// result_subshape_index)`; Deleted buffers hold groups of 2
/// `(parent_index, parent_subshape_index)`. Cap-face buffers hold flat
/// `uint32_t` indices into the result `face_map` (no grouping).
///
/// `face_modified` / `face_deleted` / `edge_modified` / `edge_deleted` are
/// expected to be empty for `BRepOffsetAPI_ThruSections`; the accessors
/// are provided for layout uniformity with `sweep_op_history_*`.
rust::Vec<uint32_t> loft_op_history_face_modified(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_face_generated(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_face_deleted(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_edge_modified(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_edge_generated(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_edge_deleted(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_start_cap_face_indices(const LoftOpHistory& history);
rust::Vec<uint32_t> loft_op_history_end_cap_face_indices(const LoftOpHistory& history);

/// Eight accessors returning the flat record buffers as `rust::Vec<uint32_t>`
/// (deep-copied at the FFI boundary). Modified/Generated buffers hold flat
/// groups of 3 `uint32_t`s `(parent_index, parent_subshape_index,
/// result_subshape_index)`; Deleted buffers hold groups of 2
/// `(parent_index, parent_subshape_index)`. Cap-face buffers hold flat
/// `uint32_t` indices into the result `face_map` (no grouping).
rust::Vec<uint32_t> sweep_op_history_face_modified(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_face_generated(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_face_deleted(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_edge_modified(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_edge_generated(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_edge_deleted(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_start_cap_face_indices(const SweepOpHistory& history);
rust::Vec<uint32_t> sweep_op_history_end_cap_face_indices(const SweepOpHistory& history);
/// Count of non-degenerate, untracked profile edges that did not produce a
/// face_generated record during the full-revolution synthesis post-pass.
/// Always 0 for prism operations and for partial revolves.
uint32_t sweep_op_history_unsynthesized_profile_edge_count(const SweepOpHistory& history);
/// Count of face_generated records dropped by the post-sort dedup pass because
/// their parent_subshape_index duplicated the preceding record (after stable-sort).
/// Zero for a well-formed full revolve.
uint32_t sweep_op_history_duplicate_parent_subshape_index_count(const SweepOpHistory& history);
/// Count of Modified/Generated children silently dropped because they could not
/// be found in the result face/edge map. Zero for a well-formed sweep.
uint32_t sweep_op_history_silent_drop_count(const SweepOpHistory& history);

/// Test fixture: run `revolve_synthesis_post_sort_and_dedup` on a synthetic flat
/// `face_generated`-layout input (`parent_index, parent_subshape_index,
/// result_subshape_index` triples). Returns the deduplicated records and the
/// count of dropped duplicates. Exposed for unit-testing the dedup logic
/// without real OCCT geometry.
RevolveSynthesisPostSortResult revolve_synthesis_post_sort_for_test(
    rust::Slice<const uint32_t> input);

/// Forward declaration of the cxx-generated Transform3Props POD struct.
/// Full definition is generated by cxx into `ffi.rs.h` which is included
/// first by `ffi.rs.cc` and by `occt_wrapper.cpp`. The forward declaration
/// here lets the function signatures below compile independently of include
/// ordering in other translation units that include only this header.
struct Transform3Props;

/// Probe whether `a` and `b` are intersecting (non-positive minimum distance).
///
/// Uses BRepExtrema_DistShapeShape: returns true iff dist.Value() <= 0.0.
/// This is the same OCCT primitive as `min_clearance` and `query_distance`,
/// significantly cheaper than a full BRepAlgoAPI_Common boolean because it
/// computes only distance (not intersection geometry) and can early-exit.
/// Face-touching pairs (distance == 0) are reported as intersecting.
/// Tolerance filtering belongs at task 2531's stdlib layer.
bool shapes_intersect(const OcctShape& a, const OcctShape& b);

/// Probe whether `a` and `b` interfere after pre-composing `t_rel` into the
/// cheaper-by-topology side (PRD §6.2 + §9.2, task 3841).
///
/// Returns true iff `dist_with_pre_compose(a, b, t_rel) <= 0.0`.
/// Face-touching pairs (distance == 0) count as interfering — same semantics as
/// `shapes_intersect`. Tolerance filtering belongs at the stdlib layer.
bool interferes_with_transform(
    const OcctShape& a,
    const OcctShape& b,
    const Transform3Props& t_rel);

// --- Local-feature op history (v0.2 persistent-naming-v2, task 2655) ---

/// Records the per-parent face/edge correspondence emitted by a local-feature
/// operation (BRepFilletAPI_MakeFillet, BRepFilletAPI_MakeChamfer). Mirrors
/// `BooleanOpHistory` (single-parent, no caps) — fillet/chamfer have no
/// FirstShape/LastShape concept and no revolve-synthesis counters.
///
/// The `parent_index` field in each record (Modified/Generated/Deleted) is
/// always `0` because local-feature operations have a single parent solid.
///
/// Records are materialized EAGERLY at construction time because the
/// algorithm's tracking maps (Modified/Generated/IsDeleted) are tied to the
/// BRepFilletAPI_Make* object's lifetime — once it goes out of scope the maps
/// are gone.
///
/// `result` owns the modified shape; `local_feature_op_history_take_result_shape`
/// hands it off to the kernel via `std::move`.
struct LocalFeatureOpHistory {
    std::unique_ptr<OcctShape> result;
    /// Count of Modified/Generated children that the algorithm reported but
    /// that could not be found in the result face_map/edge_map. Should be
    /// zero for a well-formed fillet/chamfer; non-zero indicates a kernel
    /// correspondence loss.
    uint32_t silent_drop_count = 0;
    std::vector<uint32_t> face_modified;
    std::vector<uint32_t> face_generated;
    std::vector<uint32_t> face_deleted;
    std::vector<uint32_t> edge_modified;
    std::vector<uint32_t> edge_generated;
    std::vector<uint32_t> edge_deleted;
};

/// Run `BRepFilletAPI_MakeFillet` on `shape` with the given `radius` applied
/// to every edge, materializing the result shape AND the Modified/Generated/Deleted
/// records for each parent face/edge sub-shape into a single `LocalFeatureOpHistory`.
///
/// The fillet algorithm walks every edge of `shape` calling `Add(radius, edge)`,
/// then calls `Build()`. On success it emits:
///   (a) face Modified records (parent face → trimmed result face, same-type);
///   (b) face Generated records (parent EDGE → fillet lateral face, cross-type);
///   (c) face Deleted records;
///   (d) edge Modified/Generated/Deleted records analogous.
///
/// Result-side indices come from the result shape's cached `face_map()`/`edge_map()`.
/// Empty result-side lookups (a child reported by the algorithm but not appearing
/// in the result map) increment `silent_drop_count` and are silently skipped.
std::unique_ptr<LocalFeatureOpHistory> make_fillet_with_history(
    const OcctShape& shape, double radius);

/// Run `BRepFilletAPI_MakeChamfer` on `shape` with the given `distance` applied
/// to every edge, materializing the result shape AND the Modified/Generated/Deleted
/// records into a single `LocalFeatureOpHistory`. Identical structure to
/// `make_fillet_with_history`; uses `BRepFilletAPI_MakeChamfer::Add(distance, edge)`.
std::unique_ptr<LocalFeatureOpHistory> make_chamfer_with_history(
    const OcctShape& shape, double distance);

/// Move the result shape out of the local-feature-history wrapper for
/// registration in the kernel's shape table. Subsequent calls observe
/// an empty `unique_ptr`.
std::unique_ptr<OcctShape> local_feature_op_history_take_result_shape(
    LocalFeatureOpHistory& history);

/// Six accessors returning the flat record buffers as `rust::Vec<uint32_t>`
/// (deep-copied at the FFI boundary). Modified/Generated buffers hold flat groups
/// of 3 `uint32_t`s `(parent_index, parent_subshape_index, result_subshape_index)`;
/// Deleted buffers hold groups of 2 `(parent_index, parent_subshape_index)`.
rust::Vec<uint32_t> local_feature_op_history_face_modified(const LocalFeatureOpHistory& history);
rust::Vec<uint32_t> local_feature_op_history_face_generated(const LocalFeatureOpHistory& history);
rust::Vec<uint32_t> local_feature_op_history_face_deleted(const LocalFeatureOpHistory& history);
rust::Vec<uint32_t> local_feature_op_history_edge_modified(const LocalFeatureOpHistory& history);
rust::Vec<uint32_t> local_feature_op_history_edge_generated(const LocalFeatureOpHistory& history);
rust::Vec<uint32_t> local_feature_op_history_edge_deleted(const LocalFeatureOpHistory& history);
/// Count of Modified/Generated children silently dropped because they could
/// not be found in the result map. Zero for a well-formed fillet/chamfer.
uint32_t local_feature_op_history_silent_drop_count(const LocalFeatureOpHistory& history);

// --- Modifications ---

std::unique_ptr<OcctShape> fillet_all_edges(const OcctShape& shape, double radius);
std::unique_ptr<OcctShape> chamfer_all_edges(const OcctShape& shape, double distance);

// --- Transforms ---

std::unique_ptr<OcctShape> translate_shape(const OcctShape& shape, double dx, double dy, double dz);
std::unique_ptr<OcctShape> rotate_shape(const OcctShape& shape, double ax, double ay, double az, double angle_rad);
std::unique_ptr<OcctShape> scale_shape(const OcctShape& shape, double factor, double cx, double cy, double cz);
std::unique_ptr<OcctShape> rotate_around_shape(const OcctShape& shape, double px, double py, double pz, double ax, double ay, double az, double angle_rad);

/// Apply a general non-rigid affine transform (3×3 linear + translation) to `shape`
/// using gp_GTrsf / BRepBuilderAPI_GTransform (Copy=true; source untouched).
/// Row-major linear part (m00..m22) + translation column (tx, ty, tz).
/// Singular-input guard: rejects |det(linear)| < 1e-12 with an error message containing
/// "singular". Non-uniform scale and shear are valid. Per PRD affine-map-type.md §5 task ε.
std::unique_ptr<OcctShape> gtransform_shape(const OcctShape& shape,
    double m00, double m01, double m02,
    double m10, double m11, double m12,
    double m20, double m21, double m22,
    double tx, double ty, double tz);

// --- Mirror / Pattern ---

std::unique_ptr<OcctShape> mirror_shape(const OcctShape& shape,
    double ox, double oy, double oz,
    double nx, double ny, double nz);

std::unique_ptr<OcctShape> linear_pattern(const OcctShape& shape,
    double dx, double dy, double dz,
    uint32_t count, double spacing);

std::unique_ptr<OcctShape> circular_pattern(const OcctShape& shape,
    double ox, double oy, double oz,
    double ax, double ay, double az,
    uint32_t count, double total_angle);

std::unique_ptr<OcctShape> linear_pattern_2d(const OcctShape& shape,
    double dx1, double dy1, double dz1,
    uint32_t count1, double spacing1,
    double dx2, double dy2, double dz2,
    uint32_t count2, double spacing2);

std::unique_ptr<OcctShape> arbitrary_pattern(const OcctShape& shape,
    const rust::Vec<double>& flat_transforms, uint32_t num_transforms);

// --- Thicken / Shell ---

std::unique_ptr<OcctShape> thicken_shape(const OcctShape& shape, double offset);

std::unique_ptr<OcctShape> shell_shape(const OcctShape& shape, double thickness,
    const rust::Vec<uint32_t>& face_indices);

// --- Draft ---

std::unique_ptr<OcctShape> draft_shape(const OcctShape& shape, double angle_rad,
    const OcctShape& plane_shape);

// --- Wire helpers / Loft ---

/// Create a circular wire profile at a given Z height (for loft profiles).
std::unique_ptr<OcctShape> make_circle_wire(double radius, double z_height);

/// Create a flat circular face (disk) at a given Z height (for extrude profiles).
std::unique_ptr<OcctShape> make_circle_face(double radius, double z_height);

/// Create a straight line wire between two 3D points (for sweep paths).
std::unique_ptr<OcctShape> make_line_wire(double x1, double y1, double z1,
    double x2, double y2, double z2);

// --- Curve constructors ---

/// Create a circular arc wire.
std::unique_ptr<OcctShape> make_arc_wire(
    double cx, double cy, double cz,
    double radius,
    double start_angle, double end_angle,
    double ax, double ay, double az);

/// Create a helix wire on a cylindrical surface.
std::unique_ptr<OcctShape> make_helix_wire(
    double radius, double pitch, double height);

/// Create a polyline wire from N >= 2 points (flat coords array of 3*N doubles).
/// Produces N-1 line edges connecting consecutive points.  Stable kernel FFI
/// primitive: backing wire for planned polygon faces, multi-segment sweep/pipe
/// paths (make_pipe, make_pipe_shell), and BRepAdaptor_CompCurve composite testing.
std::unique_ptr<OcctShape> make_polyline_wire(
    rust::Slice<const double> coords, size_t n_points);

/// Create an interpolated B-spline curve through points (flat coords, n_points triples).
std::unique_ptr<OcctShape> make_interp_curve(
    rust::Slice<const double> coords, size_t n_points);

/// Create a Bézier curve from control points (flat coords, n_points triples).
std::unique_ptr<OcctShape> make_bezier_curve(
    rust::Slice<const double> coords, size_t n_points);

/// Create a NURBS (B-spline) curve from poles, weights, flat knots, and degree.
std::unique_ptr<OcctShape> make_nurbs_curve(
    rust::Slice<const double> pole_coords, size_t n_poles,
    rust::Slice<const double> weights,
    rust::Slice<const double> flat_knots,
    int degree);

/// Loft through N wire profiles (N >= 2) to create a solid.
std::unique_ptr<OcctShape> loft_profiles(const OcctShapeVec& profiles);

// --- Sweep ---

/// Sweep a profile along a wire path (BRepOffsetAPI_MakePipe).
std::unique_ptr<OcctShape> make_pipe(const OcctShape& profile, const OcctShape& spine);

/// Sweep a profile along a spine path, with an auxiliary guide wire
/// constraining orientation (BRepOffsetAPI_MakePipeShell + SetMode).
/// `spine` is the path the section follows; `guide` biases section
/// orientation via SetMode(guide, /*KeepContact=*/Standard_False).
std::unique_ptr<OcctShape> make_pipe_shell(const OcctShape& profile,
                                           const OcctShape& spine,
                                           const OcctShape& guide);

/// Loft through >= 2 section profiles along a guide wire spine, via
/// BRepOffsetAPI_MakePipeShell. The first guide is the spine; each
/// profile is added as a section via `.Add(...)`. If a second guide is
/// present, it is applied via `SetMode(aux, /*KeepContact=*/false)`
/// as an auxiliary-orientation constraint.
std::unique_ptr<OcctShape> loft_guided_profiles(const OcctShapeVec& profiles,
                                                const OcctShapeVec& guides);

// --- Sweep / Extrude / Revolve ---

/// Extrude a profile shape by a direction vector (dx, dy, dz).
/// The direction vector must have non-zero magnitude.
std::unique_ptr<OcctShape> make_prism(const OcctShape& profile, double dx, double dy, double dz);

/// Revolve a profile shape around an axis by angle_rad radians.
/// Axis defined by origin point (ox,oy,oz) and direction (ax,ay,az).
std::unique_ptr<OcctShape> make_revolve(const OcctShape& profile,
    double ox, double oy, double oz,
    double ax, double ay, double az,
    double angle_rad);

/// Create a rectangular face (planar) centered at (cx, cy, cz) with
/// given width (X direction) and height (Y direction) in the XY plane.
std::unique_ptr<OcctShape> make_rect_face(double width, double height,
    double cx, double cy, double cz);

/// Create a triangular face (planar) in the plane Y=cy with vertices
/// (x1, cy, z1), (x2, cy, z2), (x3, cy, z3). Used as a test fixture
/// for revolve history integration tests (task 2636, step-3) that need
/// a non-rectangular profile in the XZ plane.
std::unique_ptr<OcctShape> make_triangle_face(
    double x1, double z1,
    double x2, double z2,
    double x3, double z3,
    double cy);

// --- Wire queries ---

/// Return the normalised start-tangent of a wire (unit vector at the first
/// parameter of the wire's composite curve). Throws std::runtime_error if the
/// shape is not a wire or the start-tangent has zero magnitude.
Point3 wire_start_tangent(const OcctShape& wire);

// --- Queries ---

double query_volume(const OcctShape& shape);
double query_area(const OcctShape& shape);
Point3 query_centroid(const OcctShape& shape);

/// Centroid of a 2D sub-shape (TopoDS_Face), via `BRepGProp::SurfaceProperties`
/// + `GProp_GProps::CentreOfMass`. Used by the `GeometryQuery::Centroid`
/// dispatch when the stored `BRepKind` is `Face` (an extracted face has no
/// enclosed volume, so volume-properties would default to the origin).
Point3 query_face_centroid(const OcctShape& shape);

BBox query_bbox(const OcctShape& shape);

double query_distance(const OcctShape& shape1, const OcctShape& shape2);

/// Minimum BREP distance between `a` and `b` via BRepExtrema_DistShapeShape.
///
/// Semantically identical to `query_distance` today, but a separate symbol for
/// the kinematic-constraints call site (task 2531; see PRD task 7). Decouples
/// future evolution (e.g. tolerance-based early-exit, signed clearance, or
/// per-pair witness points) from the generic query_distance callers.
double min_clearance(const OcctShape& a, const OcctShape& b);

/// Minimum BREP distance between `a` and `b` after pre-composing `t_rel` into
/// the cheaper-by-topology side (PRD §6.2 + §9.2, kinematic-constraints task 3841).
///
/// Uses `BRepBuilderAPI_Transform(…, Standard_False)` — TopLoc_Location
/// encoding, no geometry bake, no PNv2 concerns. The transformed copy is
/// transient (dropped before naming bookkeeping can run).
///
/// Rigid-invariance: dist(T·A, B) == dist(A, T⁻¹·B).
/// When |B| < |A| (topology-size metric: face_map + edge_map extent), the
/// function transforms B by t_rel.Inverted() instead of A by t_rel.
double distance_with_transform(
    const OcctShape& a,
    const OcctShape& b,
    const Transform3Props& t_rel);

/// Apply the rigid transform encoded in `t` to `shape`, returning a fresh
/// `OcctShape` whose topology shares pointers with the source (TopLoc_Location
/// encoding via `BRepBuilderAPI_Transform(…, Standard_False)`) — no geometry
/// bake, no precision loss (sub-placement PRD §5, task 3901).
///
/// Reuses the existing static helpers `build_trsf` (unit-quaternion validation,
/// `gp_Quaternion(qx,qy,qz,qw)` ordering) and `apply_location_trsf`
/// (BRepBuilderAPI_Transform with Copy=Standard_False) — same composition used
/// by `dist_with_pre_compose`. The source shape is never mutated; callers can
/// re-use the source handle to place the same child in multiple frames.
///
/// Throws `std::runtime_error` if `t` carries a non-unit quaternion (|q|² outside
/// [1−1e-6, 1+1e-6]) — the error message starts with "build_trsf: non-unit
/// quaternion" and is surfaced as `Err(cxx::Exception)` through the cxx-bridge.
std::unique_ptr<OcctShape> apply_transform_to_shape(
    const OcctShape& shape,
    const Transform3Props& t);

/// Return the closest point on `shape` to the query point (px, py, pz).
///
/// Algorithm: build a TopoDS_Vertex from the query point via
/// `BRepBuilderAPI_MakeVertex(gp_Pnt(px, py, pz))`, then run
/// `BRepExtrema_DistShapeShape(shape, vertex)`. The witness on `shape` is
/// `dist.PointOnShape1(1)` (operand 1 is the input shape; operand 2 is the
/// query vertex — uninteresting). This ordering mirrors `query_distance` and
/// `min_clearance` for call-site consistency.
///
/// See OcctKernel::closest_point_on_shape in crates/reify-kernel-occt/src/lib.rs for the
/// dist<1e-10 shell-fallback rationale and multi-shell caveat.
Point3 closest_point_on_shape(const OcctShape& shape, double px, double py, double pz);

/// Return the geometric position of `shape` (a `TopoDS_Vertex`) via
/// `BRep_Tool::Pnt`. Mandated by PRD `mesh-morphing-phase-2.md` §3.4
/// `vertex_position`: snap to exact coordinates, no closest-point
/// computation. Throws std::runtime_error if shape is not a vertex.
Point3 vertex_point(const OcctShape& shape);

/// Test whether the query point (px, py, pz) lies on the BREP boundary
/// (face/edge/vertex) of `shape` within `tolerance`.
///
/// Algorithm: build a `TopoDS_Vertex` from the query point via
/// `BRepBuilderAPI_MakeVertex(gp_Pnt(px, py, pz))`, run
/// `BRepExtrema_DistShapeShape(shape, vertex)`, and return
/// `dist.Value() <= tolerance`. Operand ordering mirrors `closest_point_on_shape`
/// and `min_clearance` (input shape first, query vertex second).
///
/// **Interior solid points return true (OCCT overlap behavior):**
/// `BRepExtrema_DistShapeShape` has NO inside/outside knowledge. When the query
/// vertex is strictly inside a `TopoDS_Solid`, OCCT considers the two shapes to
/// overlap and reports `dist.Value() = 0` (NOT the distance to the nearest BREP
/// face). Therefore `point_on_shape` returns `true` for any interior solid point
/// at any positive tolerance. Consequence: this primitive cannot distinguish a
/// point on the BREP surface from a point inside the solid for `TopoDS_Solid`
/// inputs. Callers that need strict surface-only membership must apply a
/// `BRepClass3d_SolidClassifier` pre-filter before this call (see escalation
/// esc-2829-6 / parent task 2324 for the documented escape hatch).
///
/// Callers commonly pass `Precision::Confusion()` (~1e-7) for `tolerance`
/// to match OCCT's default confusion threshold. Pass 0.0 for exact-coincidence
/// queries (returns `true` only when `dist.Value()` is exactly 0).
///
/// **Tolerance precondition:** `tolerance` must be a non-negative finite value.
/// Negative or NaN values cause an immediate `std::runtime_error` rather than
/// silently returning misleading results (negative → always `false` since
/// `dist.Value() >= 0`; NaN → always `false` via IEEE 754).
///
/// **Naming note:** For `TopoDS_Solid` inputs the function returns `true` for
/// interior points (not just surface points) due to the OCCT overlap behavior
/// described above. A higher-level wrapper that applies a `BRepClass3d_SolidClassifier`
/// pre-filter for strict surface-only membership is tracked in escalation esc-2829-6
/// and parent task 2324.
bool point_on_shape(const OcctShape& shape, double px, double py, double pz, double tolerance);

/// Test whether `(px, py, pz)` is inside or on the boundary of a closed solid.
///
/// Uses `BRepClass3d_SolidClassifier(shape).Perform(gp_Pnt(px,py,pz), tolerance)`.
/// Returns `true` when the classifier state is `TopAbs_IN` (strictly inside) or
/// `TopAbs_ON` (on the boundary surface within `tolerance`); returns `false` for
/// `TopAbs_OUT`. This is the conventional closed-solid membership predicate per
/// PRD §8.1 (KGQ-β).
///
/// **Tolerance precondition:** `tolerance` must be a non-negative finite `double`.
/// Negative or NaN values cause the implementation to throw `std::runtime_error`.
bool contains_solid(const OcctShape& shape, double px, double py, double pz, double tolerance);

/// Test whether two shapes are geometrically equivalent within `tolerance`
/// by (1) topology-count matching and (2) sampled-vertex proximity.
///
/// STRICT-VARIANT NOTE: This is the asymmetric sampled-point geo_equiv (PRD §5.1,
/// KGQ-δ).  A future `geo_equiv_strict` using symmetric Hausdorff distance is
/// deferred to v0.4 per PRD §5.1 + Open Question §10.
///
/// Algorithm:
///   (1) Compare per-kind (vertex/edge/face) counts via `TopExp::MapShapes`;
///       a count mismatch returns `false` immediately.
///   (2) For each face / edge in canonical `face_map()` / `edge_map()` order,
///       evaluate `sample_count` uniform parameter points on both shapes and
///       require every `|p_a − p_b| < tolerance`.
///
/// **Tolerance precondition:** `tolerance` must be a *strictly positive* finite
/// `double`.  Zero, negative, or non-finite values cause the implementation to
/// throw `std::runtime_error`.  (A zero tolerance makes `tol_sq = 0` so the
/// `>= tol_sq` comparison is always true — even identical shapes return `false`.)
///
/// Powers the v0.1 stdlib `geo_equiv(a, b, tol) -> Bool` (PRD §9 KGQ-δ).
bool geo_equiv_topo_sample(const OcctShape& a, const OcctShape& b,
                           double tolerance, size_t sample_count);

double query_moment_of_inertia(const OcctShape& shape, double ax, double ay, double az);

/// Compute the full 3×3 inertia tensor about the shape's centroid,
/// scaled by `density` to yield mass-weighted moments (kg·m²).
/// Uses BRepGProp::VolumeProperties + GProp_GProps::MatrixOfInertia().
/// Off-diagonal pairs `(i,j)` and `(j,i)` are averaged so the returned
/// tensor is bit-exactly symmetric (`I_ij == I_ji`).  A relative-tolerance
/// check (1e-9 relative + 1e-12 absolute floor) guards against gross asymmetry
/// from a future OCCT regression or a corrupted shape.
InertiaTensor3x3 query_inertia_tensor(const OcctShape& shape, double density);

/// Return the number of times each topology-map cache slot has been built for
/// `shape`. Each counter is 0 on a fresh shape and increments to 1 on first
/// use via the lazy accessors `face_map()`, `edge_map()`, or `edge_face_map()`.
/// Exposed as an observability hook for deterministic cache-effectiveness tests.
TopologyCacheBuildCounts topology_cache_build_counts(const OcctShape& shape);

/// Return the 0-based global indices of faces sharing at least one edge with
/// the face at `face_index`. Indices follow the canonical
/// `TopExp::MapShapes(..., TopAbs_FACE, ...)` order — a 0-based view of the
/// 1-based `TopTools_IndexedMapOfShape`, deduplicated by `TopoDS_Shape::IsSame`.
/// Excludes the queried face itself; deduplicated; returned in ascending order.
/// Throws std::runtime_error if `face_index` is out of range.
rust::Vec<uint32_t> adjacent_faces(const OcctShape& shape, uint32_t face_index);

/// Return the 0-based global indices of edges shared between the faces at
/// `face_a_index` and `face_b_index`, using `TopoDS_Shape::IsSame` for
/// matching. Indices follow the canonical
/// `TopExp::MapShapes(..., TopAbs_EDGE, ...)` order — a 0-based view of the
/// 1-based `TopTools_IndexedMapOfShape`, deduplicated by `TopoDS_Shape::IsSame`.
/// Returns an empty vector if `face_a_index == face_b_index`. Deduplicated;
/// returned in ascending order. Throws std::runtime_error if either index is
/// out of range.
rust::Vec<uint32_t> shared_edges(const OcctShape& shape, uint32_t face_a_index, uint32_t face_b_index);

/// Return the 0-based global indices of faces that own the edge at `edge_index`
/// (the "ancestor faces" in topology terms). Uses the cached `edge_face_map()`
/// to look up the parent faces of the edge in O(1) (amortised), then maps each
/// parent back through `face_map().FindIndex()` for the canonical 0-based view.
/// Indices follow the canonical `TopExp::MapShapes(..., TopAbs_FACE, ...)`
/// order. Deduplicated; returned in ascending order. For a manifold solid every
/// edge has exactly two ancestor faces, but the kernel does not enforce this —
/// a degenerate or seam edge may surface 1 (e.g. a closed cylinder seam) or
/// > 2 (non-manifold).
/// Throws std::runtime_error if `edge_index` is out of range.
rust::Vec<uint32_t> ancestor_faces_of_edge(const OcctShape& shape, uint32_t edge_index);

/// Return the unique edges of `shape` as an OcctShapeVec, in canonical
/// `TopExp::MapShapes(.., TopAbs_EDGE, ..)` order (deduplicated by
/// `TopoDS_Shape::IsSame`). Reuses the cached `edge_map()`.
std::unique_ptr<OcctShapeVec> get_edges(const OcctShape& shape);

/// Return the unique faces of `shape` as an OcctShapeVec, in canonical
/// `TopExp::MapShapes(.., TopAbs_FACE, ..)` order (deduplicated by
/// `TopoDS_Shape::IsSame`). Reuses the cached `face_map()`.
std::unique_ptr<OcctShapeVec> get_faces(const OcctShape& shape);

/// Return the unique vertices of `shape` as an OcctShapeVec, in
/// `TopExp::MapShapes(.., TopAbs_VERTEX, ..)` order (deduplicated by
/// `TopoDS_Shape::IsSame`). Builds a fresh local map per call — no
/// `vertex_map()` cache slot on OcctShape (the Rust-side `extracted_vertices`
/// cache provides idempotency).
std::unique_ptr<OcctShapeVec> get_vertices(const OcctShape& shape);

/// Compute the total arc length of an edge (or any 1-dimensional sub-shape)
/// in the same length units as the shape's coordinates. Backed by
/// `BRepGProp::LinearProperties` followed by `props.Mass()` — for edges,
/// "mass" under the linear density is the arc length.
double query_edge_length(const OcctShape& shape);

/// Compute the unit tangent of an edge sampled at the midpoint of its
/// curve's parameter range. Backed by `BRepAdaptor_Curve::D1` at
/// `(FirstParameter + LastParameter) / 2`. Direction is sign-arbitrary
/// (the topological orientation of the edge is not honoured); callers
/// that care about specific orientation should compare both `±t`.
///
/// Throws std::runtime_error if the shape is not an edge or yields a
/// degenerate (zero-magnitude) tangent.
Point3 query_edge_tangent(const OcctShape& shape);

/// Compute the unit outward normal at the face's centroid.
///
/// The shape MUST be a TopoDS_Face. The implementation:
///   (a) computes the face centroid via `BRepGProp::SurfaceProperties`
///       + `GProp_GProps::CentreOfMass`,
///   (b) projects the centroid to parametric (u, v) via
///       `ShapeAnalysis_Surface::ValueOfUV`,
///   (c) gets first derivatives via `BRepAdaptor_Surface::D1`,
///   (d) returns `Du × Dv` normalized, flipping for reversed-orientation
///       faces so the result tracks the topological face orientation.
///
/// Throws std::runtime_error if the shape is not a face, has no surface,
/// or yields a degenerate (zero-magnitude) normal.
Point3 query_face_normal(const OcctShape& shape);

/// Angle between the outward normals of `face_a` and `face_b` (in radians),
/// sampled at each face's surface centroid.
///
/// Both inputs MUST be `TopoDS_Face` shapes; a `std::runtime_error` is thrown
/// via `wrap_occt_call` if either is not a face, has no underlying surface, or
/// yields a degenerate (zero-magnitude) normal.
///
/// Algorithm: `acos(clamp(n_a · n_b, -1, 1))` where each `n` is the face's
/// unit outward normal sampled at its centroid. Honours `TopAbs_REVERSED`
/// orientation (same semantics as `query_face_normal`).
///
/// Note: this is the angle between outward normals, not a classical dihedral
/// angle (which is defined only for faces sharing an edge and measures the
/// interior half-plane angle). For adjacent convex faces this equals the
/// exterior angle; for disjoint or non-adjacent faces the value is still
/// well-defined but has no standard geometric name.
///
/// Returns radians in `[0, π]`.
double surface_angle(const OcctShape& face_a, const OcctShape& face_b);

/// Classify the underlying surface of a face by its OCCT
/// `BRepAdaptor_Surface::GetType()` (`GeomAbs_*`) result.
///
/// Returns a canonical surface-kind name string consumed by
/// `reify_types::FaceSurfaceKind::try_from_str` on the Rust side:
/// `"Plane"`, `"Cylinder"`, `"Cone"`, `"Sphere"`, `"Torus"`,
/// `"BezierSurface"`, `"BSplineSurface"`, `"OffsetSurface"`, or
/// `"Other"`. `GeomAbs_SurfaceOfRevolution` /
/// `GeomAbs_SurfaceOfExtrusion` (and any future GeomAbs variant) are
/// reported as `"Other"` because the typed Rust enum intentionally
/// omits them per PRD line 78's `%Plane`/`%Cylinder`/`%Cone`/`%Sphere`/
/// `%Torus` vocabulary.
///
/// Throws `std::runtime_error` if `shape` is not a `TopAbs_FACE`.
rust::String face_surface_kind(const OcctShape& shape);

/// Classify the underlying curve of an edge by its OCCT
/// `BRepAdaptor_Curve::GetType()` (`GeomAbs_*`) result.
///
/// Returns a canonical curve-kind name string consumed by
/// `reify_types::EdgeCurveKind::try_from_str` on the Rust side:
/// `"Line"`, `"Circle"`, `"Ellipse"`, `"Hyperbola"`, `"Parabola"`,
/// `"BezierCurve"`, `"BSplineCurve"`, `"OffsetCurve"`, or `"Other"`.
/// `GeomAbs_OtherCurve` and any future GeomAbs variant fall through
/// to `"Other"`.
///
/// Throws `std::runtime_error` if `shape` is not a `TopAbs_EDGE`.
rust::String edge_curve_kind(const OcctShape& shape);

/// Unit outward normal at the parametric point `(u, v)` on `face`.
///
/// The shape MUST be a `TopoDS_Face`. Algorithm:
///   (a) `BRepAdaptor_Surface::D1(u, v, p, du, dv)` — first derivatives,
///   (b) cross product `n = Du × Dv`,
///   (c) reject if `|n| < CPP_DIR_MAG_MIN` (degenerate point),
///   (d) flip if `face.Orientation() == TopAbs_REVERSED`,
///   (e) normalize.
///
/// Throws `std::runtime_error` if the shape is not a face, has no underlying
/// surface, or yields a degenerate (zero-magnitude) normal at `(u, v)`.
Point3 surface_normal_at(const OcctShape& face, double u, double v);

/// Outward unit normal of a face at the Cartesian world-space point
/// `(px, py, pz)` (metres).
///
/// The shape MUST be a `TopoDS_Face`. Algorithm:
///   (a) project `gp_Pnt(px, py, pz)` onto the face's underlying surface via
///       `ShapeAnalysis_Surface::ValueOfUV(p, 1e-9)` to obtain `(u, v)`,
///   (b) delegate to `face_outward_unit_normal_at_uv(face, u, v, who)` for
///       the `BRepAdaptor_Surface::D1` derivative, `Du × Dv` cross product,
///       magnitude check, `TopAbs_REVERSED` orientation flip, and normalize.
///
/// Reuses the same orientation-aware helper as `query_face_normal` (centroid
/// path) and `surface_normal_at` (caller-supplied (u,v) path), so the
/// REVERSED-flip outward convention and magnitude/error handling are shared.
///
/// Throws `std::runtime_error` if the shape is not a face, projection fails,
/// or the surface yields a degenerate (zero-magnitude) normal at the projected
/// `(u, v)`.
Point3 surface_normal_at_point(const OcctShape& face, double px, double py, double pz);

/// Curvature properties at the parametric point `(u, v)` on a face surface.
/// Defined by the cxx bridge (ffi.rs); forward-declared here for use in the
/// `curvature_at` function signature.
struct CurvatureProps;

/// Gaussian, mean, and principal curvatures at the parametric point `(u, v)`
/// on `face`, plus the principal curvature direction tangent vectors.
///
/// The shape MUST be a `TopoDS_Face`. Algorithm:
///   (a) `BRepAdaptor_Surface::D2(u, v, ...)` — honours `TopoLoc_Location`
///       consistently with `surface_normal_at` (same abstraction),
///   (b) compute first fundamental form E, F, G; reject if det(I) is degenerate,
///   (c) compute orientation-aware outward unit normal (flip for `TopAbs_REVERSED`),
///   (d) compute second fundamental form L, M, N from the outward normal,
///   (e) K = (LN − M²)/det(I), H = (EN − 2FM + GL) / (2·det(I)),
///   (f) principal curvatures κ_max = H + √(H²−K), κ_min = H − √(H²−K)
///       (discriminant clamped to ≥ 0 for FP safety),
///   (g) principal directions via eigenvalue solver (non-umbilical) or
///       du-normalised pair (umbilical fallback, covers sphere and planar faces).
///
/// Computing the orientation-aware normal up-front means K, H, and the
/// principal curvatures carry correct signs for both FORWARD and REVERSED faces
/// without a post-hoc swap — unlike the prior GeomLProp_SLProps path.
///
/// Throws `std::runtime_error` if the shape is not a face, has no underlying
/// surface, or curvature is undefined at `(u, v)` (e.g. at a singular point).
CurvatureProps curvature_at(const OcctShape& face, double u, double v);

/// Signed curvature of an edge at the closest point to the world-space query
/// point `(px, py, pz)`.
///
/// Projects `(px, py, pz)` onto the edge's underlying curve via
/// `GeomAPI_ProjectPointOnCurve`, then evaluates curvature via
/// `BRepLProp_CLProps` at the projected parameter.
///
/// Returns the curvature scalar (SI unit: 1/m = m⁻¹; positive for convex
/// toward the Frenet principal normal).
///
/// Throws `std::runtime_error` if:
/// - `shape` is not a `TopoDS_EDGE`,
/// - the edge has no underlying curve (degenerate),
/// - projection yields no nearest point,
/// - the tangent is undefined at the projected parameter.
double curve_curvature_at(const OcctShape& edge, double px, double py, double pz);

// --- Conformance queries ---

/// Check whether `shape` is watertight (closed, no free edges).
///
/// Backed by `BRepCheck_Analyzer.IsValid()`. Returns `false` immediately for
/// shape types other than SOLID/COMPSOLID/SHELL: COMPOUND is excluded because
/// `IsValid()` reports topological consistency, not closure — a compound of
/// open faces can spuriously pass. FACE/WIRE/EDGE/VERTEX are also excluded
/// as they never enclose a volume. Callers testing a COMPOUND should iterate
/// its sub-shapes individually.
bool is_watertight(const OcctShape& shape);

/// Check whether every edge of `shape` has at most 2 parent faces.
///
/// Backed by walking the cached `edge_face_map` (lazy `TopExp::MapShapesAndAncestors`).
/// Returns `false` iff any edge has 3+ incident faces. Shapes with no face
/// incidence (wires, edges, vertices) trivially return `true`.
bool is_manifold(const OcctShape& shape);

/// Check whether all shells of `shape` are consistently oriented.
///
/// Backed by `ShapeAnalysis_Shell::CheckOrientedShells(shape, alsofree=Standard_False)`.
/// Returns `true` iff every connected edge has opposite (FORWARD/REVERSED)
/// orientations on its two incident faces. Shapes with no shells loaded
/// (wires, isolated faces, vertices) trivially return `true`.
bool is_orientable(const OcctShape& shape);

// --- Test fixture helpers ---
// These functions build deliberately malformed or exotic shapes that are only
// useful for conformance integration tests. They are gated by `#[cfg(has_occt)]`
// (not `cfg(test)`) in the Rust layer so that integration-test crates — which
// compile the library in normal (non-test) mode — can call them.

/// Build three planar faces sharing a common edge, assembled into a compound.
/// The shared edge has 3 incident faces, making the compound non-manifold.
std::unique_ptr<OcctShape> make_nonmanifold_compound_for_test();

/// Build a 10×10×10 mm box with one face removed, wrapped in a solid.
/// The resulting open shell causes BRepCheck_Analyzer::IsValid() to return false.
std::unique_ptr<OcctShape> make_malformed_solid_for_test();

/// Build a shell of two faces sharing a common edge, where both faces use
/// the shared edge in the same orientation — violating the consistency
/// requirement for an oriented shell.
std::unique_ptr<OcctShape> make_nonorientable_shell_for_test();

/// Extract the closed shell of a 10×10×10 mm box via TopExp_Explorer.
/// The returned shape has TopAbs_ShapeType() == TopAbs_SHELL and passes
/// all three conformance predicates (watertight, manifold, orientable).
std::unique_ptr<OcctShape> make_closed_shell_for_test();

/// Build a single straight edge from (0,0,0) to (10mm,0,0).
/// The returned shape has TopAbs_ShapeType() == TopAbs_EDGE.
std::unique_ptr<OcctShape> make_edge_for_test();

/// Build a single vertex at the origin (0,0,0).
/// The returned shape has TopAbs_ShapeType() == TopAbs_VERTEX.
std::unique_ptr<OcctShape> make_vertex_for_test();

/// Build a single vertex at (x, y, z). Parameterised variant of
/// `make_vertex_for_test` for tests that need to pin a non-origin location
/// (e.g. `vertex_point` round-trip verification).
std::unique_ptr<OcctShape> make_vertex_at_for_test(double x, double y, double z);

/// Build a CompSolid containing one 10×10×10 mm box solid.
/// The returned shape has TopAbs_ShapeType() == TopAbs_COMPSOLID.
std::unique_ptr<OcctShape> make_compsolid_for_test();

/// Apply a rotation+translation placement using `BRepBuilderAPI_Transform`
/// with `Copy=Standard_False` — encoding the transform into `TopLoc_Location`
/// rather than baking it into geometry (unlike `translate_shape`/`rotate_shape`
/// which use `Copy=Standard_True`). The result has a non-identity location,
/// exercising the `TopoLoc_Location`-aware path through `BRepAdaptor_Surface`.
///
/// Rotation is around axis `(ax, ay, az)` through the origin by `angle_rad`,
/// followed by translation `(dx, dy, dz)`. Used only by placed-face integration
/// tests verifying that `curvature_at` and `surface_normal_at` agree on faces
/// with non-identity location.
std::unique_ptr<OcctShape> apply_test_placement_for_test(
    const OcctShape& shape,
    double ax, double ay, double az, double angle_rad,
    double dx, double dy, double dz
);

// --- Export ---

/// Export shape to STEP format, returns the STEP file content as a string.
rust::String export_step(const OcctShape& shape);

// --- BRep serialization ---

/// Serialize a shape to OCCT BRep ASCII format.
rust::String serialize_brep(const OcctShape& shape);

/// Deserialize a shape from OCCT BRep ASCII format.
std::unique_ptr<OcctShape> deserialize_brep(const std::string& data);

// --- Tessellation ---

TessResult tessellate_shape(const OcctShape& shape, double tolerance);

} // namespace occt
