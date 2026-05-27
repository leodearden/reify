//! Tolerance stack-up builtins: Contributor value-shape builders.
//! T1 Phase 1 — math arms in T2/T5.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_stackup("foo", &[]).is_none());
    }

    #[test]
    fn math_stub_names_return_none() {
        assert!(eval_stackup("stackup_worst_case", &[]).is_none());
        assert!(eval_stackup("stackup_rss", &[]).is_none());
        assert!(eval_stackup("monte_carlo_stackup", &[]).is_none());
    }
}
