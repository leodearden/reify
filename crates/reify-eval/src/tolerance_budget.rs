#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_stage_applies_safety_factor() {
        // N=1: per_stage_tolerance(tol, 1) == tol * 0.8.
        // At N=1, the geometric split collapses to tol^(1/1) * 0.8 = tol * 0.8.
        // Use exact float equality — the multiplication is exact for this input.
        assert_eq!(per_stage_tolerance(0.001, 1), 0.001 * 0.8);
    }
}
