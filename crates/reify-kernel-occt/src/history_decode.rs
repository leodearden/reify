//! One decoder per op-history type: [`decode_boolean_history`],
//! [`decode_local_feature_history`], [`decode_sweep_history`] and
//! [`decode_loft_history`]. Each reads every record buffer off its FFI history
//! wrapper, then takes the result shape last.
//!
//! ## Read order (defensive convention)
//!
//! All record buffers are read before the result shape is taken. The C++
//! wrapper stores the result shape and record buffers as separate members, so
//! reading buffers after the take is technically safe with the current wrapper.
//! The decoders preserve the original inline order as a defensive convention:
//! it guards against future C++ wrapper changes that might move a buffer into
//! the result shape and make pre-take reads essential.

use reify_ir::{
    BooleanOpHistoryRecords, DeletedRecord, HistoryRecord, LocalFeatureOpHistoryRecords,
    LoftOpHistoryRecords, SweepOpHistoryRecords,
};

use crate::ffi::ffi;

/// Decode a `BRepAlgoAPI` boolean's history and take its result shape.
pub(crate) fn decode_boolean_history(
    history: cxx::UniquePtr<ffi::BooleanOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, BooleanOpHistoryRecords) {
    decode_six_buffer_history(history, &BOOLEAN_OP_ACCESSORS)
}

/// Decode a fillet/chamfer history and take its result shape.
pub(crate) fn decode_local_feature_history(
    history: cxx::UniquePtr<ffi::LocalFeatureOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, LocalFeatureOpHistoryRecords) {
    decode_six_buffer_history(history, &LOCAL_FEATURE_OP_ACCESSORS)
}

/// Decode an extrude/revolve/sweep history — the six record buffers, both
/// cap-face lists and the three diagnostic counters — and take its result
/// shape.
pub(crate) fn decode_sweep_history(
    mut history: cxx::UniquePtr<ffi::SweepOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, SweepOpHistoryRecords) {
    let face_modified = decode_history_records(ffi::sweep_op_history_face_modified(&history));
    let face_generated = decode_history_records(ffi::sweep_op_history_face_generated(&history));
    let face_deleted = decode_deleted_records(ffi::sweep_op_history_face_deleted(&history));
    let edge_modified = decode_history_records(ffi::sweep_op_history_edge_modified(&history));
    let edge_generated = decode_history_records(ffi::sweep_op_history_edge_generated(&history));
    let edge_deleted = decode_deleted_records(ffi::sweep_op_history_edge_deleted(&history));
    let start_cap_face_indices = ffi::sweep_op_history_start_cap_face_indices(&history)
        .into_iter()
        .collect();
    let end_cap_face_indices = ffi::sweep_op_history_end_cap_face_indices(&history)
        .into_iter()
        .collect();
    let unsynthesized_profile_edge_count =
        ffi::sweep_op_history_unsynthesized_profile_edge_count(&history);
    let duplicate_parent_subshape_index_count =
        ffi::sweep_op_history_duplicate_parent_subshape_index_count(&history);
    let silent_drop_count = ffi::sweep_op_history_silent_drop_count(&history);
    let result_shape = ffi::sweep_op_history_take_result_shape(history.pin_mut());
    let records = SweepOpHistoryRecords {
        silent_drop_count,
        face_modified,
        face_generated,
        face_deleted,
        edge_modified,
        edge_generated,
        edge_deleted,
        start_cap_face_indices,
        end_cap_face_indices,
        unsynthesized_profile_edge_count,
        duplicate_parent_subshape_index_count,
    };
    (result_shape, records)
}

/// Decode a loft history — the six record buffers and both cap-face lists;
/// loft has no diagnostic counters — and take its result shape.
pub(crate) fn decode_loft_history(
    mut history: cxx::UniquePtr<ffi::LoftOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, LoftOpHistoryRecords) {
    let face_modified = decode_history_records(ffi::loft_op_history_face_modified(&history));
    let face_generated = decode_history_records(ffi::loft_op_history_face_generated(&history));
    let face_deleted = decode_deleted_records(ffi::loft_op_history_face_deleted(&history));
    let edge_modified = decode_history_records(ffi::loft_op_history_edge_modified(&history));
    let edge_generated = decode_history_records(ffi::loft_op_history_edge_generated(&history));
    let edge_deleted = decode_deleted_records(ffi::loft_op_history_edge_deleted(&history));
    let start_cap_face_indices = ffi::loft_op_history_start_cap_face_indices(&history)
        .into_iter()
        .collect();
    let end_cap_face_indices = ffi::loft_op_history_end_cap_face_indices(&history)
        .into_iter()
        .collect();
    let result_shape = ffi::loft_op_history_take_result_shape(history.pin_mut());
    let records = LoftOpHistoryRecords {
        face_modified,
        face_generated,
        face_deleted,
        edge_modified,
        edge_generated,
        edge_deleted,
        start_cap_face_indices,
        end_cap_face_indices,
    };
    (result_shape, records)
}

/// Decode a flat `Vec<u32>` of `(parent_index, parent_subshape_index,
/// result_subshape_index)` triples — as emitted by the C++ wrapper —
/// into typed `HistoryRecord`s.
///
/// Trailing partial groups (length not a multiple of 3) are discarded
/// via `chunks_exact`; the wrapper guarantees full triples, so this is
/// a defense-in-depth no-op rather than a permitted code path.
fn decode_history_records(flat: Vec<u32>) -> Vec<HistoryRecord> {
    flat.chunks_exact(3)
        .map(|c| HistoryRecord {
            parent_index: c[0] as u8,
            parent_subshape_index: c[1],
            result_subshape_index: c[2],
        })
        .collect()
}

/// Decode a flat `Vec<u32>` of `(parent_index, parent_subshape_index)`
/// pairs into typed `DeletedRecord`s.  Mirrors `decode_history_records`.
fn decode_deleted_records(flat: Vec<u32>) -> Vec<DeletedRecord> {
    flat.chunks_exact(2)
        .map(|c| DeletedRecord {
            parent_index: c[0] as u8,
            parent_subshape_index: c[1],
        })
        .collect()
}

/// Raw decoded values from the six-buffer history layout shared by
/// `boolean_fuse_with_history` and `run_local_feature_with_history`.
///
/// Both `BooleanOpHistoryRecords` and `LocalFeatureOpHistoryRecords` have
/// identical field shapes (`silent_drop_count: u32`, four `Vec<HistoryRecord>`,
/// two `Vec<DeletedRecord>`).  This struct is the common intermediate: the
/// shared decoder fills it and returns it; call sites convert to their specific
/// output type via `From<SixBufferHistoryDecoded>`.
struct SixBufferHistoryDecoded {
    silent_drop_count: u32,
    face_modified: Vec<HistoryRecord>,
    face_generated: Vec<HistoryRecord>,
    face_deleted: Vec<DeletedRecord>,
    edge_modified: Vec<HistoryRecord>,
    edge_generated: Vec<HistoryRecord>,
    edge_deleted: Vec<DeletedRecord>,
}

/// Function-pointer accessors for the eight FFI entry points that comprise
/// the shared six-buffer-history protocol.
///
/// Two const instances exist, one per history type:
/// [`BOOLEAN_OP_ACCESSORS`] and [`LOCAL_FEATURE_OP_ACCESSORS`].
/// Both are consumed by [`decode_six_buffer_history`].
///
/// The eight fields map 1-to-1 to the cxx-bridge free functions declared in
/// `crates/reify-kernel-occt/src/ffi.rs`:
/// - six record-buffer readers (`fn(&H) -> Vec<u32>`)
/// - one silent-drop-count reader (`fn(&H) -> u32`)
/// - one shape-taker (`fn(Pin<&mut H>) -> UniquePtr<OcctShape>`)
struct SixBufferHistoryAccessors<H> {
    face_modified: fn(&H) -> Vec<u32>,
    face_generated: fn(&H) -> Vec<u32>,
    face_deleted: fn(&H) -> Vec<u32>,
    edge_modified: fn(&H) -> Vec<u32>,
    edge_generated: fn(&H) -> Vec<u32>,
    edge_deleted: fn(&H) -> Vec<u32>,
    silent_drop_count: fn(&H) -> u32,
    take_result_shape: fn(std::pin::Pin<&mut H>) -> cxx::UniquePtr<ffi::OcctShape>,
}

/// Decode the six record buffers plus `silent_drop_count` from an FFI
/// history wrapper `H`, take the result shape, and return both as a pair.
/// The caller's output type `R` must implement `From<SixBufferHistoryDecoded>`.
///
/// ## Six-buffer protocol
///
/// Both `boolean_fuse_with_history` (via `BooleanOpHistory`) and
/// `run_local_feature_with_history` (via `LocalFeatureOpHistory`) share the
/// same decode shape:
///
/// 1. Six buffer reads via FFI accessors: `face_modified`, `face_generated`,
///    `face_deleted`, `edge_modified`, `edge_generated`, `edge_deleted`.
/// 2. `silent_drop_count` — number of records the C++ wrapper dropped because
///    their result geometry was not found in the output map.
/// 3. `take_result_shape` — called after all record buffer reads (see the
///    module doc's "Read order").
///
/// ## Cross-references
///
/// - [`decode_history_records`]: decodes flat `Vec<u32>` triples into
///   `Vec<HistoryRecord>` (used for `*_modified` and `*_generated` buffers).
/// - [`decode_deleted_records`]: decodes flat `Vec<u32>` pairs into
///   `Vec<DeletedRecord>` (used for `*_deleted` buffers).
/// - [`BOOLEAN_OP_ACCESSORS`]: accessor table for `BooleanOpHistory`.
/// - [`LOCAL_FEATURE_OP_ACCESSORS`]: accessor table for `LocalFeatureOpHistory`.
fn decode_six_buffer_history<H, R>(
    mut history: cxx::UniquePtr<H>,
    accessors: &SixBufferHistoryAccessors<H>,
) -> (cxx::UniquePtr<ffi::OcctShape>, R)
where
    H: cxx::memory::UniquePtrTarget,
    R: From<SixBufferHistoryDecoded>,
{
    let face_modified = decode_history_records((accessors.face_modified)(&history));
    let face_generated = decode_history_records((accessors.face_generated)(&history));
    let face_deleted = decode_deleted_records((accessors.face_deleted)(&history));
    let edge_modified = decode_history_records((accessors.edge_modified)(&history));
    let edge_generated = decode_history_records((accessors.edge_generated)(&history));
    let edge_deleted = decode_deleted_records((accessors.edge_deleted)(&history));
    let silent_drop_count = (accessors.silent_drop_count)(&history);
    let result_shape = (accessors.take_result_shape)(history.pin_mut());
    let decoded = SixBufferHistoryDecoded {
        silent_drop_count,
        face_modified,
        face_generated,
        face_deleted,
        edge_modified,
        edge_generated,
        edge_deleted,
    };
    (result_shape, decoded.into())
}

impl From<SixBufferHistoryDecoded> for BooleanOpHistoryRecords {
    fn from(raw: SixBufferHistoryDecoded) -> Self {
        Self {
            silent_drop_count: raw.silent_drop_count,
            face_modified: raw.face_modified,
            face_generated: raw.face_generated,
            face_deleted: raw.face_deleted,
            edge_modified: raw.edge_modified,
            edge_generated: raw.edge_generated,
            edge_deleted: raw.edge_deleted,
        }
    }
}

impl From<SixBufferHistoryDecoded> for LocalFeatureOpHistoryRecords {
    fn from(raw: SixBufferHistoryDecoded) -> Self {
        Self {
            silent_drop_count: raw.silent_drop_count,
            face_modified: raw.face_modified,
            face_generated: raw.face_generated,
            face_deleted: raw.face_deleted,
            edge_modified: raw.edge_modified,
            edge_generated: raw.edge_generated,
            edge_deleted: raw.edge_deleted,
        }
    }
}

/// Accessor table for `BooleanOpHistory`.  Consumed by
/// [`decode_six_buffer_history`] in [`decode_boolean_history`].
const BOOLEAN_OP_ACCESSORS: SixBufferHistoryAccessors<ffi::BooleanOpHistory> =
    SixBufferHistoryAccessors {
        face_modified: ffi::boolean_op_history_face_modified,
        face_generated: ffi::boolean_op_history_face_generated,
        face_deleted: ffi::boolean_op_history_face_deleted,
        edge_modified: ffi::boolean_op_history_edge_modified,
        edge_generated: ffi::boolean_op_history_edge_generated,
        edge_deleted: ffi::boolean_op_history_edge_deleted,
        silent_drop_count: ffi::boolean_op_history_silent_drop_count,
        take_result_shape: ffi::boolean_op_history_take_result_shape,
    };

/// Accessor table for `LocalFeatureOpHistory`.  Consumed by
/// [`decode_six_buffer_history`] in [`decode_local_feature_history`].
const LOCAL_FEATURE_OP_ACCESSORS: SixBufferHistoryAccessors<ffi::LocalFeatureOpHistory> =
    SixBufferHistoryAccessors {
        face_modified: ffi::local_feature_op_history_face_modified,
        face_generated: ffi::local_feature_op_history_face_generated,
        face_deleted: ffi::local_feature_op_history_face_deleted,
        edge_modified: ffi::local_feature_op_history_edge_modified,
        edge_generated: ffi::local_feature_op_history_edge_generated,
        edge_deleted: ffi::local_feature_op_history_edge_deleted,
        silent_drop_count: ffi::local_feature_op_history_silent_drop_count,
        take_result_shape: ffi::local_feature_op_history_take_result_shape,
    };
