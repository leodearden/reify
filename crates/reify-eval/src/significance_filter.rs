// significance_filter: output significance filter for ComputeNode results.
// Full rustdoc will be added in step-21 finalisation.

#[cfg(test)]
mod tests {
    use super::{FilterOutcome, is_opted_in};

    // ── Step-1: is_opted_in allowlist tests ──────────────────────────────────

    #[test]
    fn is_opted_in_returns_true_for_elastic_static() {
        assert!(
            is_opted_in("solver::elastic_static"),
            "\"solver::elastic_static\" must be in the v1 opt-in allowlist"
        );
    }

    #[test]
    fn is_opted_in_returns_false_for_modal_and_arbitrary() {
        assert!(
            !is_opted_in("solver::modal"),
            "\"solver::modal\" must NOT be in the opt-in allowlist"
        );
        assert!(
            !is_opted_in("foo::bar"),
            "arbitrary strings must NOT be in the opt-in allowlist"
        );
    }
}
