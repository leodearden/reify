//! PTODO lane δ-C grammar: decides whether a task cite in a comment line
//! names the OWNER of work still to be done, as opposed to mentioning it as
//! provenance. Normative grammar and measurements:
//! `docs/prds/reify-audit-ptodo-detector.md` §8.1 (δ-C), §20.

/// `true` when the cite spanning `line[head..end]` is in owner position.
/// `head` is the cite's first byte (the `#` of a canonical cite, the `t` of a
/// legacy `task N`); `end` is one past its last digit.
pub(crate) fn is_owner_position(line: &str, head: usize, end: usize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verdict for the first occurrence of `cite` in `line`.
    fn owner(line: &str, cite: &str) -> bool {
        let head = line.find(cite).expect("cite present in line");
        is_owner_position(line, head, head + cite.len())
    }

    #[test]
    fn owner_phrase_on_the_left() {
        for (line, cite) in [
            ("// production that does not exist; owned by task 4595.", "task 4595"),
            ("/// family. Owned by #6006 (registry τ4)", "#6006"),
            ("// resolution semantics are deferred to task 3477/3558.", "task 3477"),
            ("/// Auto-type resolution is deferred to task #3477", "#3477"),
            ("/// during lowering — their full round-trip is tracked by task 3573.", "task 3573"),
            ("/// gap documented in mechanism.rs and tracked in task 2538).", "task 2538"),
            ("// type mismatch (ignored pending task 875)", "task 875"),
            ("/// hydration is blocked on (#2947)", "#2947"),
            ("// rho: re-home (waits on #5505 and #5500).", "#5505"),
        ] {
            assert!(owner(line, cite), "{cite} should be owner-position in: {line}");
        }
    }

    #[test]
    fn future_passive_on_the_left() {
        for (line, cite) in [
            ("// Defensive: names that will be added by task 2699 but", "task 2699"),
            ("// to be wired in #7097", "#7097"),
        ] {
            assert!(owner(line, cite), "{cite} should be owner-position in: {line}");
        }
    }

    #[test]
    fn future_subject_on_the_right() {
        for (line, cite) in [
            ("//!   Task 4262 will add the descriptor-keyed store", "Task 4262"),
            ("// and γ (task #5305) will legitimately", "#5305"),
            ("/// The follow-on docs task (#2538) will", "#2538"),
        ] {
            assert!(owner(line, cite), "{cite} should be owner-position in: {line}");
        }
    }

    #[test]
    fn landing_clause() {
        for (line, cite) in [
            ("// node until task 4122 lands selector→mesh-node resolution", "task 4122"),
            ("/// This is the exact case that fires when #5639 lands and", "#5639"),
            ("// after engine wiring (PRD #2947-#2949) lands", "#2949"),
            ("/// Until #6874 lands, this binding", "#6874"),
        ] {
            assert!(owner(line, cite), "{cite} should be owner-position in: {line}");
        }
    }

    /// Only the cite the rule reaches is the owner; its neighbour on the same
    /// line stays provenance.
    #[test]
    fn verdict_is_per_occurrence() {
        assert!(!owner("// rho: re-home (waits on #5505 and #5500).", "#5500"));
        assert!(!owner("// after engine wiring (PRD #2947-#2949) lands", "#2947"));
    }

    #[test]
    fn provenance_and_guards_stay_legal() {
        for (line, cite) in [
            ("// introduced by #4578", "#4578"),
            ("// landed in #4578", "#4578"),
            ("/// marker declared in this module (task 4578, per", "task 4578"),
            ("// see #4578", "#4578"),
            ("// per #4578", "#4578"),
            ("/// #4578's ruling", "#4578"),
            ("// Task 3441: snapshot this template", "Task 3441"),
            ("/// was a second override until task 4168, and", "task 4168"),
            ("/// shipped this form until #6890 renamed it", "#6890"),
            ("/// until task 6663 DID reinterpret", "task 6663"),
            ("//! Task #6082 lands `x.rs` here rather than", "#6082"),
            ("/// Grammar CAPABILITY, owned by the #5907 series", "#5907"),
            ("/// This test carried `#[ignore = \"blocked on #5951\"]`", "#5951"),
            ("// mark_pending #4578", "#4578"),
            ("// self.pending #4578", "#4578"),
            ("// #4578 willfully", "#4578"),
            ("/// ALL FAILURES AT ONCE (reviewer amendment, task 5623) comes from", "task 5623"),
        ] {
            assert!(!owner(line, cite), "{cite} must stay provenance in: {line}");
        }
    }

    #[test]
    fn non_ascii_context_is_safe() {
        assert!(owner("// κ — owned by #4578 ζ", "#4578"));
        assert!(owner("// ζ: once task 4578 lands — κ", "task 4578"));
        assert!(!owner("// κ — introduced by #4578 ζ", "#4578"));
        assert!(!owner("// —#4578— ζ", "#4578"));
    }
}
