//! Invariant R: every unit label a display surface prints reads back, through the compiler, to
//! the same dimension and the same SI scale. PRD
//! `docs/prds/v0_6/angle-units-surface-convergence.md` §5 C2; task μ in §12.
//!
//! The four surfaces are `Display for DimensionVector`, `DimensionVector::to_display_units`,
//! `reify_ir::dimension_unit_label` and the `reify_core::unit_ladders()` rungs.

use reify_core::DimensionVector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    DimensionDisplay,
    ToDisplayUnits,
    DimensionUnitLabel,
    UnitLadderRung,
}

/// One label a surface prints for a dimension, with the SI value of `1{label}` that the
/// surface associates with it.
#[derive(Debug, Clone, PartialEq)]
struct Emission {
    surface: Surface,
    dimension_name: &'static str,
    dimension: DimensionVector,
    label: String,
    si_scale: f64,
}

/// The first conjunct of Invariant R that an emission fails.
#[derive(Debug, Clone, PartialEq)]
enum Breach {
    Unreadable(String),
    WrongDimension { read: DimensionVector },
    WrongScale { read: f64 },
}

#[derive(Debug)]
struct Violation {
    emission: Emission,
    breach: Breach,
}

#[derive(Debug, Default)]
struct Report {
    unexpected: Vec<Violation>,
}

fn surface_emissions() -> Vec<Emission> {
    Vec::new()
}

fn invariant_r_report(_emissions: &[Emission]) -> Report {
    Report::default()
}

fn injected_rung(
    dimension_name: &'static str,
    dimension: DimensionVector,
    label: &str,
    si_scale: f64,
) -> Emission {
    Emission {
        surface: Surface::UnitLadderRung,
        dimension_name,
        dimension,
        label: label.to_string(),
        si_scale,
    }
}

fn corpus_with(injected: impl IntoIterator<Item = Emission>) -> Vec<Emission> {
    let mut corpus = surface_emissions();
    corpus.extend(injected);
    corpus
}

#[test]
fn a_non_ascii_label_injected_into_the_corpus_fails_the_gate() {
    let corpus = corpus_with([injected_rung(
        "Density",
        DimensionVector::MASS_DENSITY,
        "kg/m³",
        1.0,
    )]);

    let report = invariant_r_report(&corpus);

    let [violation] = report.unexpected.as_slice() else {
        panic!(
            "the gate must report exactly the injected `kg/m³` rung; got {:#?}",
            report.unexpected
        );
    };
    assert_eq!(violation.emission.label, "kg/m³");
    assert!(
        matches!(violation.breach, Breach::Unreadable(_)),
        "a superscript label is a parse failure, not a dimension or scale mismatch; got {:?}",
        violation.breach
    );
}

#[test]
fn each_conjunct_of_invariant_r_fails_the_gate_on_its_own() {
    let corpus = corpus_with([
        // Names no declared unit: the class `L` was in before it was declared.
        injected_rung("Volume", DimensionVector::VOLUME, "qq", 1e-3),
        // Reads back as Length·Mass, so the claimed Mass is the wrong dimension.
        injected_rung("Mass", DimensionVector::MASS, "kg·m", 1.0),
        // Reads back as 1e-3 m, so the claimed scale of 1.0 is wrong.
        injected_rung("Length", DimensionVector::LENGTH, "mm", 1.0),
    ]);

    let report = invariant_r_report(&corpus);

    let [unreadable, wrong_dimension, wrong_scale] = report.unexpected.as_slice() else {
        panic!(
            "the gate must report exactly the three injected rungs; got {:#?}",
            report.unexpected
        );
    };
    assert_eq!(unreadable.emission.label, "qq");
    assert!(
        matches!(unreadable.breach, Breach::Unreadable(_)),
        "an undeclared unit symbol must fail the first conjunct; got {:?}",
        unreadable.breach
    );
    assert_eq!(wrong_dimension.emission.label, "kg·m");
    assert!(
        matches!(wrong_dimension.breach, Breach::WrongDimension { .. }),
        "a readable label of another dimension must fail the second conjunct; got {:?}",
        wrong_dimension.breach
    );
    assert_eq!(wrong_scale.emission.label, "mm");
    assert!(
        matches!(wrong_scale.breach, Breach::WrongScale { .. }),
        "a readable label of the right dimension and the wrong scale must fail the third \
         conjunct; got {:?}",
        wrong_scale.breach
    );
}
