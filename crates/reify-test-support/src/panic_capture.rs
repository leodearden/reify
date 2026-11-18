//! Captures the message a closure panics with, for tests that assert several
//! substrings of one panic — which `#[should_panic(expected = ..)]` cannot.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Run `f`, which must panic, and return its panic message. Panics if `f`
/// returns, or if the payload is neither `&str` nor `String`, so no
/// placeholder text can satisfy a caller's `contains` check.
#[track_caller]
pub fn panic_message_of<R>(f: impl FnOnce() -> R) -> String {
    let Err(payload) = catch_unwind(AssertUnwindSafe(f)) else {
        panic!("expected a panic, but the call returned normally");
    };
    let Some(message) = reify_core::panic_payload_str(payload.as_ref()) else {
        panic!(
            "the panic payload is neither a &str nor a String, so it carries no message to assert on"
        );
    };
    message.to_owned()
}

#[cfg(test)]
mod tests {
    use super::panic_message_of;

    #[test]
    fn returns_a_str_payload_verbatim() {
        assert_eq!(
            panic_message_of(|| std::panic::panic_any("boom_str")),
            "boom_str"
        );
    }

    #[test]
    fn returns_a_string_payload_verbatim() {
        assert_eq!(
            panic_message_of(|| std::panic::panic_any(String::from("boom_string"))),
            "boom_string"
        );
    }

    #[test]
    #[should_panic(expected = "returned normally")]
    fn panics_when_the_call_returns_normally() {
        panic_message_of(|| ());
    }

    /// The strictness property: a non-string payload must never come back as
    /// a placeholder that a caller's `contains` check could match.
    #[test]
    #[should_panic(expected = "neither a &str nor a String")]
    fn panics_on_a_non_string_payload() {
        panic_message_of(|| std::panic::panic_any(42_i32));
    }
}
