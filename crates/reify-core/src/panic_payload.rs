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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_payload_str_reads_a_str_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom_str");
        assert_eq!(panic_payload_str(payload.as_ref()), Some("boom_str"));
    }

    #[test]
    fn panic_payload_str_reads_a_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("boom_string"));
        assert_eq!(panic_payload_str(payload.as_ref()), Some("boom_string"));
    }

    #[test]
    fn panic_payload_str_is_none_for_a_non_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(panic_payload_str(payload.as_ref()), None);
    }

    /// `panic_payload_to_string` must extract the payload text from both
    /// `&str` and `String` panic payloads, and fall back to a fixed message
    /// for any other payload type.
    #[test]
    fn panic_payload_to_string_extracts_str_string_and_falls_back() {
        let str_payload: Box<dyn std::any::Any + Send> = Box::new("boom_str");
        assert_eq!(panic_payload_to_string(str_payload.as_ref()), "boom_str");

        let string_payload: Box<dyn std::any::Any + Send> = Box::new(String::from("boom_string"));
        assert_eq!(
            panic_payload_to_string(string_payload.as_ref()),
            "boom_string"
        );

        let other_payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
        assert_eq!(
            panic_payload_to_string(other_payload.as_ref()),
            "<non-string panic payload>"
        );
    }
}
