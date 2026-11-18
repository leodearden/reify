//! Captures the message a closure panics with, for tests that assert several
//! substrings of one panic — which `#[should_panic(expected = ..)]` cannot.

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
