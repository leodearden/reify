// shell_result.rs — Rust runtime container for the structured shell stress
// result (PRD task T16, `docs/prds/v0_4/structural-analysis-shells.md` §
// "Stress through thickness").
//
// Sibling to the stdlib-level `ShellStress` structure_def in
// `crates/reify-compiler/stdlib/solver_elastic.ri:std/solver/elastic`. This
// file ships the data-only contract (define the shape + tet constructor);
// engine-integration tasks T18-T20 are responsible for actually populating
// these fields from the MITC3+ kernel and wiring the `to_global(stress,
// frame)` dispatch helper.

use reify_types::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::Value;

    /// `ShellStress::homogeneous(field)` is the canonical tet-result constructor.
    /// It must set all three stress channels to the same field value and leave
    /// `frame` as `Value::Undef` (tet stress is in the global frame already;
    /// no per-element local frame exists for solid elements).
    ///
    /// This test pins the tet-result population contract:
    ///   result.top    == input field
    ///   result.mid    == input field
    ///   result.bottom == input field
    ///   result.frame  is undef
    #[test]
    fn shell_stress_homogeneous_replicates_field_across_channels() {
        let field = Value::Real(42.0);
        let result = ShellStress::homogeneous(field.clone());

        assert_eq!(
            result.top, field,
            "homogeneous: top should equal the input field"
        );
        assert_eq!(
            result.mid, field,
            "homogeneous: mid should equal the input field"
        );
        assert_eq!(
            result.bottom, field,
            "homogeneous: bottom should equal the input field"
        );
        assert!(
            result.frame.is_undef(),
            "homogeneous: frame should be Value::Undef for tet results, got: {:?}",
            result.frame
        );
    }

    /// Explicit construction must preserve distinct per-channel values, proving
    /// that `ShellStress` can represent the fully differentiated per-layer
    /// stress distribution produced by the MITC3+ shell kernel.
    ///
    /// This test pins the explicit/per-channel shape needed for shell results:
    /// each of top/mid/bottom/frame round-trips through the struct unchanged.
    #[test]
    fn shell_stress_explicit_construction_preserves_per_channel_values() {
        let top = Value::Real(1.0);
        let mid = Value::Real(2.0);
        let bottom = Value::Real(3.0);
        let frame = Value::Real(4.0);

        let result = ShellStress {
            top: top.clone(),
            mid: mid.clone(),
            bottom: bottom.clone(),
            frame: frame.clone(),
        };

        assert_eq!(result.top, top, "explicit: top round-trips");
        assert_eq!(result.mid, mid, "explicit: mid round-trips");
        assert_eq!(result.bottom, bottom, "explicit: bottom round-trips");
        assert_eq!(result.frame, frame, "explicit: frame round-trips");
    }
}
