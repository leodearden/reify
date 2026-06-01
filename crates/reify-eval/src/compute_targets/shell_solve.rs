//! Shell-classification + extraction-failure policy and the reify-eval glue
//! that bridges the neutral `reify-solver-elastic` shell driver output into the
//! DSL `ShellChannels` / `ShellStress` value (PRD task δ,
//! `docs/prds/v0_4/shell-extract-engine-bridge.md` §3/§5/§7/§11 OQ-1/OQ-2).
//!
//! This module is the reify-eval-side host for the ShellChannels-production glue
//! (the task names `shell_result.rs`, but `ShellChannels` is defined in
//! reify-eval and the crate dependency direction is `reify-eval →
//! reify-solver-elastic`, so naming it in the solver crate would close a
//! dependency cycle — see the task δ design decisions).

#[cfg(test)]
mod tests {
    use super::*;

    /// RED (task δ step-5): pin the shell-classification routing policy.
    ///
    /// `classify_shell(shell_force, length, width, height, shell_threshold)`
    /// resolves the FEA route for a body:
    /// - `ShellForce::On`  → always `Shell` (proxy for an `@shell` annotation).
    /// - `ShellForce::Off` → always `Tet`.
    /// - `ShellForce::Auto` → `Shell` iff `thickness/extent < shell_threshold`
    ///   (`thickness = min(L,W,H)`, `extent = max(L,W,H)`), else `Tet`.
    ///   The comparison is strict `<`, so a ratio exactly equal to the
    ///   threshold classifies `Tet`.
    #[test]
    fn classify_shell_routes_by_force_and_threshold() {
        // Fixture body 50mm × 10mm × 1mm: thickness=1mm, extent=50mm,
        // ratio = 1/50 = 0.02 < 0.2 → shell under Auto.
        let (l, w, h) = (0.050_f64, 0.010_f64, 0.001_f64);
        let threshold = 0.2_f64;

        // Forced On always routes Shell regardless of geometry.
        assert_eq!(
            classify_shell(ShellForce::On, l, w, h, threshold),
            ShellRoute::Shell,
            "ShellForce::On must force the shell route on a thin plate"
        );
        assert_eq!(
            classify_shell(ShellForce::On, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Shell,
            "ShellForce::On forces Shell even for a thick (cube-ish) body"
        );

        // Forced Off always routes Tet regardless of geometry.
        assert_eq!(
            classify_shell(ShellForce::Off, l, w, h, threshold),
            ShellRoute::Tet,
            "ShellForce::Off must force the tet route even for a thin plate"
        );

        // Auto + thin plate (ratio 0.02 < 0.2) → Shell.
        assert_eq!(
            classify_shell(ShellForce::Auto, l, w, h, threshold),
            ShellRoute::Shell,
            "Auto with thickness/extent ratio 0.02 < 0.2 must classify Shell"
        );

        // Auto + thick body 10×10×8 (ratio 8/10 = 0.8 >= 0.2) → Tet.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Tet,
            "Auto with thickness/extent ratio 0.8 >= 0.2 must classify Tet"
        );

        // Boundary: ratio exactly == threshold is NOT < threshold → Tet.
        // 10×10×2 → ratio 2/10 = 0.2 == threshold.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.002, threshold),
            ShellRoute::Tet,
            "Auto at ratio exactly == threshold must classify Tet (strict <)"
        );
    }

    /// RED (task δ step-5): pin the extraction-failure fallback policy.
    ///
    /// `resolve_extraction_failure(shell_force)` decides what happens when the
    /// upstream shell-extract step fails:
    /// - `ShellForce::On`  → `HardError` (proxy for `@shell`: no fallback).
    /// - `ShellForce::Auto`/`Off` → `TetFallbackWithWarning` (soft fallback).
    #[test]
    fn resolve_extraction_failure_maps_force_to_policy() {
        assert_eq!(
            resolve_extraction_failure(ShellForce::On),
            FailurePolicy::HardError,
            "ShellForce::On must hard-error on extraction failure (no fallback)"
        );
        assert_eq!(
            resolve_extraction_failure(ShellForce::Auto),
            FailurePolicy::TetFallbackWithWarning,
            "ShellForce::Auto must fall back to tet meshing with a warning"
        );
        assert_eq!(
            resolve_extraction_failure(ShellForce::Off),
            FailurePolicy::TetFallbackWithWarning,
            "ShellForce::Off must not hard-error (never attempts shell extraction)"
        );
    }
}
