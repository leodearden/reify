//! One decoder per op-history type: [`decode_boolean_history`],
//! [`decode_local_feature_history`], [`decode_sweep_history`] and
//! [`decode_loft_history`]. Each decoder names its type's FFI readers and
//! assembles its records. The six record buffers every type carries are decoded
//! by one [`RecordBufferReaders::read`], and the read order is held once, by
//! [`read_then_take`].
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
    read_then_take(history, ffi::boolean_op_history_take_result_shape, |h| {
        let buffers = RecordBufferReaders {
            face_modified: ffi::boolean_op_history_face_modified,
            face_generated: ffi::boolean_op_history_face_generated,
            face_deleted: ffi::boolean_op_history_face_deleted,
            edge_modified: ffi::boolean_op_history_edge_modified,
            edge_generated: ffi::boolean_op_history_edge_generated,
            edge_deleted: ffi::boolean_op_history_edge_deleted,
        }
        .read(h);
        BooleanOpHistoryRecords {
            face_modified: buffers.face_modified,
            face_generated: buffers.face_generated,
            face_deleted: buffers.face_deleted,
            edge_modified: buffers.edge_modified,
            edge_generated: buffers.edge_generated,
            edge_deleted: buffers.edge_deleted,
            silent_drop_count: ffi::boolean_op_history_silent_drop_count(h),
        }
    })
}

/// Decode a fillet/chamfer history and take its result shape.
pub(crate) fn decode_local_feature_history(
    history: cxx::UniquePtr<ffi::LocalFeatureOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, LocalFeatureOpHistoryRecords) {
    read_then_take(
        history,
        ffi::local_feature_op_history_take_result_shape,
        |h| {
            let buffers = RecordBufferReaders {
                face_modified: ffi::local_feature_op_history_face_modified,
                face_generated: ffi::local_feature_op_history_face_generated,
                face_deleted: ffi::local_feature_op_history_face_deleted,
                edge_modified: ffi::local_feature_op_history_edge_modified,
                edge_generated: ffi::local_feature_op_history_edge_generated,
                edge_deleted: ffi::local_feature_op_history_edge_deleted,
            }
            .read(h);
            LocalFeatureOpHistoryRecords {
                face_modified: buffers.face_modified,
                face_generated: buffers.face_generated,
                face_deleted: buffers.face_deleted,
                edge_modified: buffers.edge_modified,
                edge_generated: buffers.edge_generated,
                edge_deleted: buffers.edge_deleted,
                silent_drop_count: ffi::local_feature_op_history_silent_drop_count(h),
            }
        },
    )
}

/// Decode an extrude/revolve/sweep history — the six record buffers, both
/// cap-face lists and the three diagnostic counters — and take its result
/// shape.
pub(crate) fn decode_sweep_history(
    history: cxx::UniquePtr<ffi::SweepOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, SweepOpHistoryRecords) {
    read_then_take(history, ffi::sweep_op_history_take_result_shape, |h| {
        let buffers = RecordBufferReaders {
            face_modified: ffi::sweep_op_history_face_modified,
            face_generated: ffi::sweep_op_history_face_generated,
            face_deleted: ffi::sweep_op_history_face_deleted,
            edge_modified: ffi::sweep_op_history_edge_modified,
            edge_generated: ffi::sweep_op_history_edge_generated,
            edge_deleted: ffi::sweep_op_history_edge_deleted,
        }
        .read(h);
        SweepOpHistoryRecords {
            face_modified: buffers.face_modified,
            face_generated: buffers.face_generated,
            face_deleted: buffers.face_deleted,
            edge_modified: buffers.edge_modified,
            edge_generated: buffers.edge_generated,
            edge_deleted: buffers.edge_deleted,
            start_cap_face_indices: ffi::sweep_op_history_start_cap_face_indices(h),
            end_cap_face_indices: ffi::sweep_op_history_end_cap_face_indices(h),
            unsynthesized_profile_edge_count:
                ffi::sweep_op_history_unsynthesized_profile_edge_count(h),
            duplicate_parent_subshape_index_count:
                ffi::sweep_op_history_duplicate_parent_subshape_index_count(h),
            silent_drop_count: ffi::sweep_op_history_silent_drop_count(h),
        }
    })
}

/// Decode a loft history — the six record buffers and both cap-face lists;
/// loft has no diagnostic counters — and take its result shape.
pub(crate) fn decode_loft_history(
    history: cxx::UniquePtr<ffi::LoftOpHistory>,
) -> (cxx::UniquePtr<ffi::OcctShape>, LoftOpHistoryRecords) {
    read_then_take(history, ffi::loft_op_history_take_result_shape, |h| {
        let buffers = RecordBufferReaders {
            face_modified: ffi::loft_op_history_face_modified,
            face_generated: ffi::loft_op_history_face_generated,
            face_deleted: ffi::loft_op_history_face_deleted,
            edge_modified: ffi::loft_op_history_edge_modified,
            edge_generated: ffi::loft_op_history_edge_generated,
            edge_deleted: ffi::loft_op_history_edge_deleted,
        }
        .read(h);
        LoftOpHistoryRecords {
            face_modified: buffers.face_modified,
            face_generated: buffers.face_generated,
            face_deleted: buffers.face_deleted,
            edge_modified: buffers.edge_modified,
            edge_generated: buffers.edge_generated,
            edge_deleted: buffers.edge_deleted,
            start_cap_face_indices: ffi::loft_op_history_start_cap_face_indices(h),
            end_cap_face_indices: ffi::loft_op_history_end_cap_face_indices(h),
        }
    })
}

/// Build `history`'s records with `read`, which reads every buffer and counter,
/// and only then take its result shape (the module doc's read order).
fn read_then_take<H, R>(
    mut history: cxx::UniquePtr<H>,
    take_result_shape: fn(std::pin::Pin<&mut H>) -> cxx::UniquePtr<ffi::OcctShape>,
    read: impl FnOnce(&H) -> R,
) -> (cxx::UniquePtr<ffi::OcctShape>, R)
where
    H: cxx::memory::UniquePtrTarget,
{
    let records = read(&history);
    let result_shape = take_result_shape(history.pin_mut());
    (result_shape, records)
}

/// The six record buffers every op-history type carries, decoded.
struct RecordBuffers {
    face_modified: Vec<HistoryRecord>,
    face_generated: Vec<HistoryRecord>,
    face_deleted: Vec<DeletedRecord>,
    edge_modified: Vec<HistoryRecord>,
    edge_generated: Vec<HistoryRecord>,
    edge_deleted: Vec<DeletedRecord>,
}

/// One op-history type's readers for its six record buffers. Each decoder
/// builds its type's table inline, so a type's readers are named only inside
/// its decoder.
struct RecordBufferReaders<H> {
    face_modified: fn(&H) -> Vec<u32>,
    face_generated: fn(&H) -> Vec<u32>,
    face_deleted: fn(&H) -> Vec<u32>,
    edge_modified: fn(&H) -> Vec<u32>,
    edge_generated: fn(&H) -> Vec<u32>,
    edge_deleted: fn(&H) -> Vec<u32>,
}

impl<H> RecordBufferReaders<H> {
    /// Read and decode all six buffers: `*_modified` and `*_generated` as
    /// [`HistoryRecord`] triples, `*_deleted` as [`DeletedRecord`] pairs.
    fn read(&self, history: &H) -> RecordBuffers {
        RecordBuffers {
            face_modified: decode_history_records((self.face_modified)(history)),
            face_generated: decode_history_records((self.face_generated)(history)),
            face_deleted: decode_deleted_records((self.face_deleted)(history)),
            edge_modified: decode_history_records((self.edge_modified)(history)),
            edge_generated: decode_history_records((self.edge_generated)(history)),
            edge_deleted: decode_deleted_records((self.edge_deleted)(history)),
        }
    }
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
