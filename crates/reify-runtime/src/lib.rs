// Stub — M1 implementation pending

/// Task scheduling priority.
///
/// Variants are ordered from highest priority (P0Interactive) to lowest
/// (P3Speculative). Derived Ord respects declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// User-interactive: must respond within a frame budget.
    P0Interactive,
    /// Fast background: lightweight computations (expression eval).
    P1Fast,
    /// Slow background: heavier computations (constraint solving).
    P1Slow,
    /// Speculative: pre-computation that may be discarded.
    P3Speculative,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::P0Interactive < Priority::P1Fast);
        assert!(Priority::P1Fast < Priority::P1Slow);
        assert!(Priority::P1Slow < Priority::P3Speculative);

        // Verify equality
        assert_eq!(Priority::P0Interactive, Priority::P0Interactive);

        // Verify Copy
        let p = Priority::P1Fast;
        let p2 = p;
        assert_eq!(p, p2);
    }
}
