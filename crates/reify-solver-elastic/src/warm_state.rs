//! FEA-solver warm-state shape and OpaqueState conversion (PRD task #14).

#[cfg(test)]
mod tests {
    use super::*;

    /// `CgWarmState::from_displacement(u)` → `into_opaque_state()` →
    /// `from_opaque_state()` round-trips the displacement vector unchanged.
    /// Also pins the `estimated_size_bytes` formula
    /// (`u.len() * size_of::<f64>()`).
    #[test]
    fn cg_warm_state_round_trips_through_opaque_state() {
        let u = vec![1.0_f64, 2.0, 3.0];
        let ws = CgWarmState::from_displacement(u.clone());
        let opaque = ws.into_opaque_state();
        let restored = CgWarmState::from_opaque_state(opaque).expect("downcast");
        assert_eq!(restored.u, u);

        assert_eq!(
            CgWarmState::from_displacement(vec![0.0_f64; 5]).estimated_size_bytes(),
            5 * std::mem::size_of::<f64>(),
        );
    }
}
