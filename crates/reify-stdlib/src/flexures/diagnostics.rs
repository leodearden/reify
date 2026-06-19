//! Flexure PRB-constructor diagnostic classifier.
//!
//! `flexure_diagnose(name, args, result)` is the flexure analogue of
//! `stackup::diagnose` / `fea::diagnose`: a pure post-call classifier invoked
//! from reify-expr's `FunctionCall` arm (task 3871). Unlike the stackup/fea
//! helpers — which return a single `Diagnostic` only on a `Value::Undef` result
//! — `flexure_diagnose` runs on BOTH the success and the `Undef` path and
//! returns a `Vec<Diagnostic>`, because one PRB ctor call can surface several
//! signals at once (PRD `docs/prds/v0_3/compliant-joints.md` §1 / §5.3):
//!   • `W_FlexureYielding`            (Warning) — cached `at_yield == true`
//!   • `W_FlexurePrbOutOfRange`       (Warning) — declared range exceeds ±5°
//!   • `E_FlexureGeometryInvalid`     (Error)   — degenerate geometry on Undef
//!   • `W_FlexureFatigueCheckMissing` (Info)    — standing advisory, every call
//!
//! The once-per-session dedup of the Info fatigue advisory is the emission
//! layer's responsibility (reify-expr, step-10), not this classifier's.

use std::collections::BTreeMap;

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector};
use reify_ir::{PersistentMap, Value};

use super::common::{classify_geometry_invalid, PRB_ANGLE_LIMIT_RAD};

/// PRB rotational small-deflection bound in degrees — the human-facing spelling
/// of [`PRB_ANGLE_LIMIT_RAD`] cited by the `W_FlexurePrbOutOfRange` advisory.
const PRB_ANGLE_LIMIT_DEG: f64 = 5.0;

/// Classify the diagnostics a PRB flexure constructor call should surface
/// (PRD `docs/prds/v0_3/compliant-joints.md` §1 / §5.3).
///
/// Dispatches on the builtin `name`: a non-flexure name short-circuits to an
/// empty `Vec` (so unrelated calls never carry a flexure advisory). For a
/// recognised PRB ctor it then reads `result`:
/// - a constructed joint `Value::Map` surfaces `W_FlexureYielding` (cached
///   `at_yield`) and `W_FlexurePrbOutOfRange` (declared angular range past ±5°);
/// - a `Value::Undef` is re-classified via [`classify_geometry_invalid`] and
///   surfaces `E_FlexureGeometryInvalid` ONLY for degenerate geometry.
///
/// Every recognised PRB ctor call also appends the standing
/// `W_FlexureFatigueCheckMissing` Info advisory; its once-per-eval-session
/// dedup is the reify-expr emission layer's responsibility (step-10), not this
/// classifier's.
pub fn flexure_diagnose(name: &str, args: &[Value], result: &Value) -> Vec<Diagnostic> {
    if !is_flexure_ctor(name) {
        return Vec::new();
    }

    let mut diags = Vec::new();

    match result {
        // Success path: a constructed joint Map. Read the cached compliance
        // record + the joint range to surface the §5.3 operating-stress warnings.
        Value::Map(joint) => {
            if let Some(fields) = compliance_fields(joint) {
                // W_FlexureYielding — the cached stress-check tripped: peak
                // surface stress at the (declared|auto) range endpoint ≥ yield.
                if rec_bool(fields, "at_yield") == Some(true)
                    && let Some(d) = yielding_diagnostic(fields, joint)
                {
                    diags.push(d);
                }
            }
            // W_FlexurePrbOutOfRange — the user declared an angular operating
            // range beyond the ±5° pseudo-rigid-body small-deflection bound. The
            // auto cap is always ≤ 5°, so an endpoint past 5° can only come from a
            // declared range.
            //
            // DELIBERATE SCOPING (angular families only): this fires solely for
            // ANGLE-dimensioned ranges — `angular_range_endpoint` returns `None`
            // for the LENGTH-dimensioned ranges of the displacement families
            // (fixed-guided / prismatic / parallelogram). The ±5° rotational bound
            // is the only small-deflection limit the PRD
            // (docs/prds/v0_3/compliant-joints.md §5) attaches a number to, so a
            // displacement joint driven past its small-deflection fraction is
            // intentionally NOT surfaced here — its PRB-model degradation is
            // instead caught by the populated yielding stress-check above. If a
            // future PRD revision wants displacement over-range warned too, add a
            // LENGTH-range analogue beside this (with its own fractional bound);
            // the angular/displacement asymmetry is a conscious decision, not an
            // oversight.
            if let Some(endpoint) = angular_range_endpoint(joint)
                && endpoint > PRB_ANGLE_LIMIT_RAD + 1e-9
            {
                diags.push(prb_out_of_range_diagnostic(endpoint));
            }
        }
        // Undef path: re-classify the rejection. Emit E_FlexureGeometryInvalid
        // ONLY for degenerate geometry; non-geometry rejections (bad material /
        // axis / arity) stay silent.
        Value::Undef => {
            if let Some(violation) = classify_geometry_invalid(name, args) {
                diags.push(
                    Diagnostic::error(format!(
                        "E_FLEXURE_GEOMETRY_INVALID: {}",
                        violation.describe()
                    ))
                    .with_code(DiagnosticCode::FlexureGeometryInvalid),
                );
            }
        }
        _ => {}
    }

    // W_FlexureFatigueCheckMissing — a standing Info advisory accompanying every
    // PRB flexure construction (PRD §1): the PRB model carries no fatigue-life
    // check. Deduplicated to once-per-eval-session at the reify-expr emission
    // layer (step-10), not here.
    diags.push(
        Diagnostic::info(
            "W_FLEXURE_FATIGUE_CHECK_MISSING: this pseudo-rigid-body flexure has no \
             fatigue-life check; validate cyclic flexures against the material endurance \
             limit (S–N) for the intended duty cycle",
        )
        .with_code(DiagnosticCode::FlexureFatigueCheckMissing),
    );

    diags
}

/// The 13 PRB flexure constructor names (beam / notch / hinge / prismatic /
/// compound). Only these surface flexure diagnostics; everything else — plain
/// builtins, the `__flexure_compliance_get` accessor intrinsic (step-12) — is
/// short-circuited to an empty `Vec`.
fn is_flexure_ctor(name: &str) -> bool {
    matches!(
        name,
        "prb_cantilever_beam"
            | "prb_fixed_fixed_beam"
            | "prb_notch_circular"
            | "prb_notch_elliptical"
            | "prb_notch_right_circular"
            | "prb_living_hinge"
            | "prb_cross_spring_pivot"
            | "prb_let_joint"
            | "prb_prismatic_blade"
            | "prb_two_axis_pivot"
            | "prb_parallelogram_flexure"
            | "prb_double_parallelogram_flexure"
            | "prb_cartwheel_flexure"
    )
}

/// Build the `W_FlexureYielding` warning from the cached compliance record.
///
/// The material yield and safety factor are recovered from the record alone —
/// no per-family material-arg extraction needed:
///   `yield_margin = (yield − max_stress) / yield`
///   ⇒ `yield = max_stress / (1 − yield_margin)`, `safety_factor = 1 / (1 − yield_margin)`.
/// In the at-yield regime `yield_margin ≤ 0`, so `(1 − yield_margin) ≥ 1 > 0`
/// (no divide-by-zero; the no-yield sentinel margin `1.0` never reaches here
/// because `at_yield` is always `false` without a yield datum).
fn yielding_diagnostic(
    fields: &PersistentMap<String, Value>,
    joint: &BTreeMap<Value, Value>,
) -> Option<Diagnostic> {
    let max_stress = rec_pressure(fields, "max_stress")?;
    let yield_margin = rec_real(fields, "yield_margin")?;
    let denom = 1.0 - yield_margin;
    if denom <= 0.0 || !denom.is_finite() {
        return None;
    }
    let yield_si = max_stress / denom;
    let safety_factor = 1.0 / denom;

    // Suggested narrower range = the auto SAFE PRB-valid bound stored in the
    // record, formatted per the joint's dimensional family.
    // `prb_validity_range` is a `Value::Range` (since task 4576) — prismatic
    // families carry Range<Length> (task 4587), angular families Range<Angle>.
    let suggestion = match joint_kind(joint) {
        Some("prismatic") => match rec_range_upper_si(
            fields,
            "prb_validity_range",
            DimensionVector::LENGTH,
        ) {
            Some(half_m) => format!("±{:.3} mm", half_m * 1e3),
            None => "the PRB-valid operating range".to_string(),
        },
        _ => match rec_range_upper_si(
            fields,
            "prb_validity_range",
            DimensionVector::ANGLE,
        ) {
            Some(half_rad) => format!("±{:.2}°", half_rad.to_degrees()),
            None => "the PRB-valid operating range".to_string(),
        },
    };

    Some(
        Diagnostic::warning(format!(
            "W_FLEXURE_YIELDING: peak surface stress {:.1} MPa exceeds the material yield \
             strength {:.1} MPa (safety factor {:.2}); narrow the declared operating range \
             to {} to keep the flexure below yield",
            max_stress / 1e6,
            yield_si / 1e6,
            safety_factor,
            suggestion,
        ))
        .with_code(DiagnosticCode::FlexureYielding),
    )
}

/// Build the `W_FlexurePrbOutOfRange` warning citing the ±5° bound and the
/// bookmarked nonlinear-FEA escalation path (PRD §5).
fn prb_out_of_range_diagnostic(endpoint_rad: f64) -> Diagnostic {
    Diagnostic::warning(format!(
        "W_FLEXURE_PRB_OUT_OF_RANGE: declared operating range ±{:.2}° exceeds the ±{:.0}° \
         pseudo-rigid-body small-deflection bound; beyond this the linear PRB model loses \
         fidelity — validate the joint with a nonlinear FEA sweep \
         (see docs/prds/v0_3/compliant-joints.md §5)",
        endpoint_rad.to_degrees(),
        PRB_ANGLE_LIMIT_DEG,
    ))
    .with_code(DiagnosticCode::FlexurePrbOutOfRange)
}

/// The half-width SI magnitude of an angular (ANGLE-dimensioned) joint range's
/// upper bound, or `None` for an absent / non-angular / unbounded range (the
/// displacement families carry a LENGTH-dimensioned range, which returns `None`).
fn angular_range_endpoint(joint: &BTreeMap<Value, Value>) -> Option<f64> {
    match joint_field(joint, "range")? {
        Value::Range { upper: Some(up), .. } => match up.as_ref() {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::ANGLE && si_value.is_finite() => {
                Some(si_value.abs())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Look up a string-keyed entry in a joint `Value::Map`.
fn joint_field<'a>(joint: &'a BTreeMap<Value, Value>, key: &str) -> Option<&'a Value> {
    joint.get(&Value::String(key.to_string()))
}

/// Read the joint's `kind` discriminant ("revolute" / "prismatic" / …).
fn joint_kind(joint: &BTreeMap<Value, Value>) -> Option<&str> {
    match joint_field(joint, "kind")? {
        Value::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// The cached `FlexureCompliance` record's fields, if the joint carries one
/// under the reserved hidden `__flexure_compliance` key.
fn compliance_fields(joint: &BTreeMap<Value, Value>) -> Option<&PersistentMap<String, Value>> {
    match joint_field(joint, "__flexure_compliance")? {
        Value::StructureInstance(d) => Some(&d.fields),
        _ => None,
    }
}

/// Read a `Value::Bool` field from a compliance record.
fn rec_bool(fields: &PersistentMap<String, Value>, key: &str) -> Option<bool> {
    match fields.get(&key.to_string())? {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

/// Read a finite bare `Value::Real` field from a compliance record.
fn rec_real(fields: &PersistentMap<String, Value>, key: &str) -> Option<f64> {
    match fields.get(&key.to_string())? {
        Value::Real(r) if r.is_finite() => Some(*r),
        _ => None,
    }
}

/// Read a finite PRESSURE-dimensioned `Value::Scalar` field's `si_value` from a
/// compliance record.
fn rec_pressure(fields: &PersistentMap<String, Value>, key: &str) -> Option<f64> {
    match fields.get(&key.to_string())? {
        Value::Scalar {
            si_value,
            dimension,
        } if *dimension == DimensionVector::PRESSURE && si_value.is_finite() => Some(*si_value),
        _ => None,
    }
}

/// Read the upper-bound SI value from a `Value::Range` field whose upper bound is
/// a `dimension`-dimensioned finite Scalar. Returns `None` for absent, non-range,
/// unbounded, non-finite, or wrong-dimension fields.
fn rec_range_upper_si(
    fields: &PersistentMap<String, Value>,
    key: &str,
    dimension: DimensionVector,
) -> Option<f64> {
    match fields.get(&key.to_string())? {
        Value::Range { upper: Some(up), .. } => match up.as_ref() {
            Value::Scalar { si_value, dimension: dim }
                if *dim == dimension && si_value.is_finite() =>
            {
                Some(*si_value)
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::flexure_diagnose;
    use reify_core::{Diagnostic, DiagnosticCode, Severity};
    use reify_ir::Value;

    /// First diagnostic in `diags` carrying `code` (panics with the observed
    /// code set if none match).
    fn find(diags: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
        diags
            .iter()
            .find(|d| d.code == Some(code))
            .unwrap_or_else(|| {
                let codes: Vec<_> = diags.iter().map(|d| d.code).collect();
                panic!("expected a diagnostic with code {code:?}; got codes {codes:?}")
            })
    }

    fn has_code(diags: &[Diagnostic], code: DiagnosticCode) -> bool {
        diags.iter().any(|d| d.code == Some(code))
    }

    fn count_code(diags: &[Diagnostic], code: DiagnosticCode) -> usize {
        diags.iter().filter(|d| d.code == Some(code)).count()
    }

    /// Yielding-cantilever call args: t=0.05 mm, L=2 mm, w=5 mm, steel, neutral
    /// 0°, declared ±10° (σ(10°) ≈ 447 MPa > 310 MPa yield, and 10° > the ±5°
    /// PRB bound), mirroring examples/flexures/yield_warning.ri.
    fn yielding_args() -> Vec<Value> {
        let ten_deg = 10.0_f64 * std::f64::consts::PI / 180.0;
        vec![
            Value::length(0.002),
            Value::length(0.005),
            Value::length(0.00005),
            steel(),
            origin(),
            axis_y(),
            Value::angle(0.0),
            Value::angle(ten_deg),
        ]
    }

    #[test]
    fn flexure_diagnose_yielding_cantilever_emits_yielding_and_prb_out_of_range() {
        let args = yielding_args();
        let result = crate::eval_builtin("prb_cantilever_beam", &args);
        assert!(!result.is_undef(), "yielding cantilever is still a valid joint");
        let diags = flexure_diagnose("prb_cantilever_beam", &args, &result);

        // (a) W_FlexureYielding (Warning). The regression contract is the CODE
        // (asserted via `find`) and the SEVERITY; the exact message prose
        // (surface stress / yield / "safety factor" / "narrow ... range") is
        // user-facing wording free to evolve, so it is intentionally not asserted
        // here — pinning several narrative substrings only invites spurious
        // failures on a harmless rewording.
        let yielding = find(&diags, DiagnosticCode::FlexureYielding);
        assert_eq!(
            yielding.severity,
            Severity::Warning,
            "FlexureYielding is a Warning"
        );

        // (b) W_FlexurePrbOutOfRange (Warning). Beyond code + severity, assert
        // only the single genuinely contractual substring: the bookmarked PRD
        // escalation path the user must follow next. The ±5°-bound / "nonlinear
        // FEA" narrative wording is deliberately left unasserted (brittle prose).
        let oor = find(&diags, DiagnosticCode::FlexurePrbOutOfRange);
        assert_eq!(
            oor.severity,
            Severity::Warning,
            "FlexurePrbOutOfRange is a Warning"
        );
        assert!(
            oor.message.contains("compliant-joints"),
            "out-of-range message bookmarks the PRD escalation path: {}",
            oor.message
        );

        // (d) W_FlexureFatigueCheckMissing (Info) accompanies any prb_* call —
        // code + severity are the contract.
        let fatigue = find(&diags, DiagnosticCode::FlexureFatigueCheckMissing);
        assert_eq!(
            fatigue.severity,
            Severity::Info,
            "FlexureFatigueCheckMissing is Info (advisory)"
        );
    }

    #[test]
    fn flexure_diagnose_geometry_invalid_only_for_degenerate_geometry() {
        // (c.1) Degenerate geometry: thickness (3 mm) ≥ length (2 mm) ⇒ Undef ⇒
        // E_FlexureGeometryInvalid (Error).
        let degenerate = vec![
            Value::length(0.002), // L = 2 mm
            Value::length(0.005),
            Value::length(0.003), // t = 3 mm ≥ L (degenerate)
            steel(),
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &degenerate);
        assert!(result.is_undef(), "degenerate geometry returns Undef");
        let diags = flexure_diagnose("prb_cantilever_beam", &degenerate, &result);
        let geo = find(&diags, DiagnosticCode::FlexureGeometryInvalid);
        assert_eq!(
            geo.severity,
            Severity::Error,
            "FlexureGeometryInvalid is an Error"
        );
        assert!(
            geo.message.to_lowercase().contains("geometry"),
            "geometry-invalid message describes the degeneracy: {}",
            geo.message
        );

        // (c.2) Non-geometry Undef: VALID geometry (t=0.5 mm < L=20 mm) but a
        // bad material ⇒ Undef. classify_geometry_invalid must NOT fire.
        let bad_material = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            Value::Real(1.0), // not a material StructureInstance
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &bad_material);
        assert!(result.is_undef(), "bad material returns Undef");
        let diags = flexure_diagnose("prb_cantilever_beam", &bad_material, &result);
        assert!(
            !has_code(&diags, DiagnosticCode::FlexureGeometryInvalid),
            "valid geometry + bad material does NOT emit FlexureGeometryInvalid"
        );
    }

    #[test]
    fn flexure_diagnose_valid_call_emits_fatigue_info() {
        // Plain safe 6-arg cantilever (no declared range): not yielding, in
        // range (auto ±5° cap), so only the standing fatigue advisory fires.
        let args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
        ];
        let result = crate::eval_builtin("prb_cantilever_beam", &args);
        assert!(!result.is_undef());
        let diags = flexure_diagnose("prb_cantilever_beam", &args, &result);

        // (d) Exactly one Info fatigue advisory per call.
        assert_eq!(
            count_code(&diags, DiagnosticCode::FlexureFatigueCheckMissing),
            1,
            "valid prb_* call emits one fatigue Info"
        );
        // A safe, in-range call emits neither yielding nor out-of-range.
        assert!(
            !has_code(&diags, DiagnosticCode::FlexureYielding),
            "safe geometry does not yield"
        );
        assert!(
            !has_code(&diags, DiagnosticCode::FlexurePrbOutOfRange),
            "auto ±5° range is within the PRB bound"
        );
    }

    #[test]
    fn flexure_diagnose_non_flexure_name_is_empty() {
        // (e) A non-flexure builtin name yields no diagnostics, regardless of
        // the result value.
        assert!(flexure_diagnose("box", &[], &Value::Undef).is_empty());
        assert!(flexure_diagnose("stackup_rss", &[], &Value::Undef).is_empty());

        // Even when handed a real joint Map under a non-flexure name, the
        // classifier short-circuits to empty (it dispatches on the name).
        let joint = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(0.02),
                Value::length(0.005),
                Value::length(0.0005),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        assert!(
            flexure_diagnose("not_a_flexure", &[], &joint).is_empty(),
            "non-flexure name short-circuits to empty even with a joint result"
        );
    }

    #[test]
    fn flexure_diagnose_displacement_family_geometry_invalid() {
        // The displacement-family ctors (fixed-fixed beam, prismatic blade) reject
        // degenerate geometry (thickness ≥ length) with Undef; flexure_diagnose
        // re-classifies that Undef as E_FlexureGeometryInvalid (Error), exactly
        // like the cantilever path (step-7) — confirming classify_geometry_invalid
        // covers these names too.
        let degenerate = vec![
            Value::length(0.002), // L = 2 mm
            Value::length(0.005),
            Value::length(0.003), // t = 3 mm ≥ L (degenerate)
            steel(),
            origin(),
            axis_y(),
        ];
        for name in ["prb_fixed_fixed_beam", "prb_prismatic_blade"] {
            let result = crate::eval_builtin(name, &degenerate);
            assert!(result.is_undef(), "{name}: degenerate geometry returns Undef");
            let diags = flexure_diagnose(name, &degenerate, &result);
            let geo = find(&diags, DiagnosticCode::FlexureGeometryInvalid);
            assert_eq!(
                geo.severity,
                Severity::Error,
                "{name}: FlexureGeometryInvalid is an Error"
            );
            assert!(
                geo.message.to_lowercase().contains("geometry"),
                "{name}: geometry-invalid message describes the degeneracy: {}",
                geo.message
            );
        }
    }

    #[test]
    fn flexure_diagnose_notch_geometry_invalid() {
        // The notch family rejects degenerate geometry t ≥ 2r (web ≥ notch
        // diameter) with Undef; flexure_diagnose re-classifies that Undef as
        // E_FlexureGeometryInvalid (Error) for all three notch variants —
        // confirming classify_geometry_invalid covers the notch arg layout
        // (radius at index 0, web thickness at index 1), distinct from the
        // slender-beam (thickness at index 2) layout.
        let degenerate = vec![
            Value::length(0.001), // r = 1 mm (2r = 2 mm)
            Value::length(0.002), // t = 2 mm ≥ 2r (degenerate web)
            Value::length(0.005),
            steel(),
            origin(),
            axis_y(),
        ];
        for name in [
            "prb_notch_circular",
            "prb_notch_elliptical",
            "prb_notch_right_circular",
        ] {
            let result = crate::eval_builtin(name, &degenerate);
            assert!(result.is_undef(), "{name}: degenerate t≥2r returns Undef");
            let diags = flexure_diagnose(name, &degenerate, &result);
            let geo = find(&diags, DiagnosticCode::FlexureGeometryInvalid);
            assert_eq!(
                geo.severity,
                Severity::Error,
                "{name}: FlexureGeometryInvalid is an Error"
            );
            assert!(
                geo.message.to_lowercase().contains("geometry"),
                "{name}: geometry-invalid message describes the degeneracy: {}",
                geo.message
            );
            assert!(
                geo.message.to_lowercase().contains("notch"),
                "{name}: notch geometry message cites the notch degeneracy: {}",
                geo.message
            );
        }
    }

    /// `prb_validity_range` is a `Value::Range` (since task 4576), so the
    /// yielding-suggestion reader must extract the half-width from the Range's
    /// upper bound, not from a bare `Value::Real`.  The format is family-gated:
    /// angular joints get `±X°`, prismatic joints get `±X mm`.
    #[test]
    fn yielding_diagnostic_formats_range_per_family() {
        // (a) Revolute (cantilever): W_FlexureYielding suggestion cites degrees.
        // yielding_args() declares ±10° which exceeds delta_yield for that geometry.
        let cantilever_args = yielding_args();
        let result = crate::eval_builtin("prb_cantilever_beam", &cantilever_args);
        assert!(!result.is_undef(), "yielding cantilever is a valid joint");
        let diags = flexure_diagnose("prb_cantilever_beam", &cantilever_args, &result);
        let msg = &find(&diags, DiagnosticCode::FlexureYielding).message;
        assert!(msg.contains('°'), "revolute suggestion cites degrees: {msg}");
        assert!(!msg.contains(" mm"), "revolute suggestion does not cite mm: {msg}");

        // (b) Prismatic (blade): W_FlexureYielding suggestion cites mm.
        // L=20mm, w=5mm, t=0.5mm, steel → delta_yield ≈ 0.807mm (cantilever formula).
        // 8-arg form: neutral=0, declared endpoint=2mm > delta_yield → at_yield=true.
        let prismatic_args = vec![
            Value::length(0.02),
            Value::length(0.005),
            Value::length(0.0005),
            steel(),
            origin(),
            axis_y(),
            Value::length(0.0),   // neutral offset
            Value::length(0.002), // declared endpoint = 2 mm > delta_yield
        ];
        let result = crate::eval_builtin("prb_prismatic_blade", &prismatic_args);
        assert!(!result.is_undef(), "yielding prismatic is a valid joint");
        let diags = flexure_diagnose("prb_prismatic_blade", &prismatic_args, &result);
        let msg = &find(&diags, DiagnosticCode::FlexureYielding).message;
        assert!(msg.contains(" mm"), "prismatic suggestion cites mm: {msg}");
        assert!(!msg.contains('°'), "prismatic suggestion does not cite degrees: {msg}");
    }

    /// Disposition 5 (task 4547): the `flexure_compliance(joint: Length)` accessor
    /// (body `__flexure_compliance_get(joint)`) is a documented type-lie — the
    /// intrinsic silently returns a sentinel-zero record for ANY bare `Length`
    /// arg, since only a joint `Value::Map` carrying `__flexure_compliance`
    /// resolves a real record. `flexure_diagnose` surfaces that lie at eval time
    /// via a dedicated `__flexure_compliance_get` arm (placed BEFORE the
    /// `is_flexure_ctor` short-circuit), emitting `W_FlexureNonJointArg`
    /// (Warning) when `args[0]` is not a joint Map carrying the compliance record.
    #[test]
    fn flexure_diagnose_compliance_get_non_joint_arg_warns() {
        // A bare LENGTH (5 mm) is NOT a joint Map carrying __flexure_compliance:
        // the accessor returns a sentinel-zero record for it, masking the misuse.
        let non_joint = Value::length(0.005);
        let result = crate::eval_builtin("__flexure_compliance_get", &[non_joint.clone()]);
        let diags = flexure_diagnose("__flexure_compliance_get", &[non_joint], &result);
        let d = find(&diags, DiagnosticCode::FlexureNonJointArg);
        assert_eq!(
            d.severity,
            Severity::Warning,
            "FlexureNonJointArg is a Warning"
        );
    }

    /// Companion: a real joint Map (carrying the cached `__flexure_compliance`
    /// StructureInstance produced by a PRB ctor) is the legitimate accessor arg,
    /// so NO `FlexureNonJointArg` fires.
    #[test]
    fn flexure_diagnose_compliance_get_real_joint_arg_is_clean() {
        let joint = crate::eval_builtin(
            "prb_cantilever_beam",
            &[
                Value::length(0.02),
                Value::length(0.005),
                Value::length(0.0005),
                steel(),
                origin(),
                axis_y(),
            ],
        );
        assert!(
            matches!(joint, Value::Map(_)),
            "prb ctor yields a joint Map carrying __flexure_compliance"
        );
        let result = crate::eval_builtin("__flexure_compliance_get", &[joint.clone()]);
        let diags = flexure_diagnose("__flexure_compliance_get", &[joint], &result);
        assert!(
            !has_code(&diags, DiagnosticCode::FlexureNonJointArg),
            "a real joint arg does not trigger FlexureNonJointArg"
        );
    }
}
