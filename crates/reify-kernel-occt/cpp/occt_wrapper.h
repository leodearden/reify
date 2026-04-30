#pragma once
#include "rust/cxx.h"
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

// --- Primitive construction ---

/// Create a box centered at origin with given dimensions (in meters).
std::unique_ptr<OcctShape> make_box(double width, double height, double depth);

/// Create a cylinder along Z axis (in meters).
std::unique_ptr<OcctShape> make_cylinder(double radius, double height);

/// Create a sphere centered at origin (in meters).
std::unique_ptr<OcctShape> make_sphere(double radius);

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

/// Test fixture: run `revolve_synthesis_post_sort_and_dedup` on a synthetic flat
/// `face_generated`-layout input (`parent_index, parent_subshape_index,
/// result_subshape_index` triples). Returns the deduplicated records and the
/// count of dropped duplicates. Exposed for unit-testing the dedup logic
/// without real OCCT geometry.
RevolveSynthesisPostSortResult revolve_synthesis_post_sort_for_test(
    rust::Slice<const uint32_t> input);

/// Probe whether `a` and `b` are intersecting (non-positive minimum distance).
///
/// Uses BRepExtrema_DistShapeShape: returns true iff dist.Value() <= 0.0.
/// This is the same OCCT primitive as `min_clearance` and `query_distance`,
/// significantly cheaper than a full BRepAlgoAPI_Common boolean because it
/// computes only distance (not intersection geometry) and can early-exit.
/// Face-touching pairs (distance == 0) are reported as intersecting.
/// Tolerance filtering belongs at task 2531's stdlib layer.
bool shapes_intersect(const OcctShape& a, const OcctShape& b);

// --- Modifications ---

std::unique_ptr<OcctShape> fillet_all_edges(const OcctShape& shape, double radius);
std::unique_ptr<OcctShape> chamfer_all_edges(const OcctShape& shape, double distance);

// --- Transforms ---

std::unique_ptr<OcctShape> translate_shape(const OcctShape& shape, double dx, double dy, double dz);
std::unique_ptr<OcctShape> rotate_shape(const OcctShape& shape, double ax, double ay, double az, double angle_rad);
std::unique_ptr<OcctShape> scale_shape(const OcctShape& shape, double factor, double cx, double cy, double cz);
std::unique_ptr<OcctShape> rotate_around_shape(const OcctShape& shape, double px, double py, double pz, double ax, double ay, double az, double angle_rad);

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

/// Return the unique edges of `shape` as an OcctShapeVec, in canonical
/// `TopExp::MapShapes(.., TopAbs_EDGE, ..)` order (deduplicated by
/// `TopoDS_Shape::IsSame`). Reuses the cached `edge_map()`.
std::unique_ptr<OcctShapeVec> get_edges(const OcctShape& shape);

/// Return the unique faces of `shape` as an OcctShapeVec, in canonical
/// `TopExp::MapShapes(.., TopAbs_FACE, ..)` order (deduplicated by
/// `TopoDS_Shape::IsSame`). Reuses the cached `face_map()`.
std::unique_ptr<OcctShapeVec> get_faces(const OcctShape& shape);

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

/// Build a CompSolid containing one 10×10×10 mm box solid.
/// The returned shape has TopAbs_ShapeType() == TopAbs_COMPSOLID.
std::unique_ptr<OcctShape> make_compsolid_for_test();

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
