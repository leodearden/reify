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

/// Tri-state shell-formulation control — the Rust mirror of the stdlib
/// `ShellForce` enum (`crates/reify-compiler/stdlib/solver_elastic.ri:70`,
/// `param shell_force : ShellForce = ShellForce.Auto`).
///
/// `On` is the proxy for an `@shell` annotation: it forces the shell route and
/// hard-errors on extraction failure (no tet fallback). `Auto` auto-classifies
/// by the thickness/extent ratio and falls back softly. `Off` forces the tet
/// route. See the task δ design decisions (PRD §3 failure-semantics table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellForce {
    /// Force the tet/solid route; never run shell extraction.
    Off,
    /// Auto-classify by `shell_threshold`; soft tet fallback on failure.
    Auto,
    /// Force the shell route (proxy for `@shell`); hard-error on failure.
    On,
}

/// Resolved FEA route for a body: shell-kernel assembly vs. tet/solid assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRoute {
    /// Route through the MITC3 shell kernel (`solve_flat_plate_shell`).
    Shell,
    /// Route through the tet/solid path (`solve_cantilever_fea`, task 4084/α).
    Tet,
}

/// What to do when the upstream `shell-extract::extract` step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Surface the extraction error and abort (no fallback). `ShellForce::On`.
    HardError,
    /// Fall back to tet meshing and emit a warning diagnostic. `Auto`/`Off`.
    TetFallbackWithWarning,
}

/// Classify a body's FEA route from its shell-force setting and geometry.
///
/// - `ShellForce::On`  → always [`ShellRoute::Shell`] (proxy for `@shell`).
/// - `ShellForce::Off` → always [`ShellRoute::Tet`].
/// - `ShellForce::Auto` → [`ShellRoute::Shell`] iff `thickness / extent <
///   shell_threshold`, else [`ShellRoute::Tet`], where `thickness = min(L,W,H)`
///   and `extent = max(L,W,H)`. The comparison is strict `<`, so a ratio exactly
///   equal to the threshold classifies `Tet`.
///
/// A non-positive `extent` (degenerate geometry) classifies `Tet` rather than
/// dividing by zero.
///
/// The fixture body (50 mm × 10 mm × 1 mm, ratio 0.02 < the default threshold
/// 0.2) auto-classifies `Shell` under a bare `ElasticOptions()`.
pub fn classify_shell(
    shell_force: ShellForce,
    length: f64,
    width: f64,
    height: f64,
    shell_threshold: f64,
) -> ShellRoute {
    match shell_force {
        ShellForce::On => ShellRoute::Shell,
        ShellForce::Off => ShellRoute::Tet,
        ShellForce::Auto => {
            let thickness = length.min(width).min(height);
            let extent = length.max(width).max(height);
            if extent > 0.0 && thickness / extent < shell_threshold {
                ShellRoute::Shell
            } else {
                ShellRoute::Tet
            }
        }
    }
}

/// Resolve the extraction-failure policy from the shell-force setting.
///
/// - `ShellForce::On`  → [`FailurePolicy::HardError`] (proxy for `@shell`:
///   the user explicitly demanded a shell solve, so a failed extraction is a
///   hard error with no silent fallback).
/// - `ShellForce::Auto`/`Off` → [`FailurePolicy::TetFallbackWithWarning`]: a
///   failed (or never-attempted) extraction degrades gracefully to the tet path
///   with a warning diagnostic.
///
/// The user-facing extraction-failure CLI fixtures are owned by task ε; this
/// helper is the policy site (unit-tested here, wired by the engine lowering in
/// step-12).
pub fn resolve_extraction_failure(shell_force: ShellForce) -> FailurePolicy {
    match shell_force {
        ShellForce::On => FailurePolicy::HardError,
        ShellForce::Auto | ShellForce::Off => FailurePolicy::TetFallbackWithWarning,
    }
}

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
