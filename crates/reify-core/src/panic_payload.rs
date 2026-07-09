//! Shared [`catch_unwind`](std::panic::catch_unwind) payload formatting.
//!
//! Task #5121: extracted from two near-duplicate copies of the same downcast
//! idiom — `crates/reify-stdlib/src/orientation.rs`'s test-module panic
//! message extraction, and `crates/reify-eval/src/engine_compute.rs`'s
//! `panic_payload_message` (added by task #5079, whose doc comment flagged
//! this exact cross-crate consolidation as deferred follow-up, esc-5079-2).
//! `reify-core` is a dependency-free leaf crate already depended on by both,
//! so it hosts the single shared copy.

/// Turn a [`catch_unwind`](std::panic::catch_unwind) panic payload into a
/// printable message: try `&str`, then `String`, else fall back to a fixed
/// placeholder.
pub fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
