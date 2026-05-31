//! Pure, dependency-free cache-key half of the modal-analysis trampoline.
//!
//! The faer-matrix-holding `(K, M)` warm-state cache lives in the `reify-eval`
//! modal trampoline (`modal_ops.rs`); this module holds only the pure
//! `ModalCacheKey` it keys that cache on. (Type lands in task κ step-2.)
