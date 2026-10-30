//! Invariant R: every unit label a display surface prints reads back, through the compiler, to
//! the same dimension and the same SI scale. PRD
//! `docs/prds/v0_6/angle-units-surface-convergence.md` §5 C2; task μ in §12.
//!
//! The four surfaces are `Display for DimensionVector`, `DimensionVector::to_display_units`,
//! `reify_ir::dimension_unit_label` and the `reify_core::unit_ladders()` rungs.

use std::collections::HashMap;
use std::fmt;

use reify_compiler::CompiledModule;
use reify_core::DimensionVector;
use reify_ir::{CompiledExprKind, Value};
use reify_test_support::{compile_source_with_stdlib_allow_parse_errors, errors_only};

/// Invariant R's relative scale bound (PRD §5 C2). Not exact equality: `1g/cm^3` reads back as
/// `999.9999999999999`.
const SCALE_REL_TOL: f64 = 1e-12;

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
    stale_blocks: Vec<DimensionVector>,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Emission {
            surface,
            dimension_name,
            dimension,
            label,
            si_scale,
        } = &self.emission;
        write!(
            f,
            "{surface:?} {dimension_name} ({dimension}) `{label}` at scale {si_scale:e}: "
        )?;
        match &self.breach {
            Breach::Unreadable(message) => write!(f, "does not compile: {message}"),
            Breach::WrongDimension { read } => write!(f, "reads back as dimension {read}"),
            Breach::WrongScale { read } => write!(f, "reads back at scale {read:e}"),
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} emission(s) fail Invariant R; {} KNOWN_BLOCKS row(s) are stale:",
            self.unexpected.len(),
            self.stale_blocks.len()
        )?;
        for violation in &self.unexpected {
            writeln!(f, "  {violation}")?;
        }
        let stale_rows = KNOWN_BLOCKS
            .iter()
            .filter(|block| self.stale_blocks.contains(&block.dimension));
        for block in stale_rows {
            writeln!(
                f,
                "  {} (owner {}): no emission it blocks fails any more; delete this KNOWN_BLOCKS row",
                block.dimension, block.owner
            )?;
        }
        Ok(())
    }
}

fn surface_emissions() -> Vec<Emission> {
    Vec::new()
}

/// What `1{label}` compiles to: its SI value and dimension, or why it does not compile.
type ReadBack = Result<(f64, DimensionVector), String>;

/// Compiles `1{label}` with the stdlib prelude, the path a typed-in value takes: parse, lower,
/// then registry-first unit resolution. Never panics, so one bad label cannot stop the sweep.
fn resolve_unit_literal(label: &str) -> ReadBack {
    let source = format!("structure def S {{ let x = 1{label} }}");
    let module = compile_source_with_stdlib_allow_parse_errors(&source);
    let errors = errors_only(&module);
    if !errors.is_empty() {
        let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
        return Err(messages.join("; "));
    }
    match probe_cell_expr(&module) {
        Some(CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        })) => Ok((*si_value, *dimension)),
        Some(other) => Err(format!(
            "`1{label}` compiled to {other:?}, not a scalar literal"
        )),
        None => Err(format!(
            "`1{label}` compiled without errors but left no cell `x` with a default expression"
        )),
    }
}

fn probe_cell_expr(module: &CompiledModule) -> Option<&CompiledExprKind> {
    let default_expr = module
        .templates
        .iter()
        .find(|template| template.name == "S")?
        .value_cells
        .iter()
        .find(|cell| cell.id.member == "x")?
        .default_expr
        .as_ref()?;
    Some(&default_expr.kind)
}

/// False when either scale is NaN.
fn scales_agree(read: f64, claimed: f64) -> bool {
    (read - claimed).abs() <= SCALE_REL_TOL * read.abs().max(claimed.abs())
}

fn first_breach(emission: &Emission, read_back: &ReadBack) -> Option<Breach> {
    match read_back {
        Err(message) => Some(Breach::Unreadable(message.clone())),
        Ok((_, read_dimension)) if *read_dimension != emission.dimension => {
            Some(Breach::WrongDimension {
                read: *read_dimension,
            })
        }
        Ok((read_scale, _)) if !scales_agree(*read_scale, emission.si_scale) => {
            Some(Breach::WrongScale { read: *read_scale })
        }
        Ok(_) => None,
    }
}

fn violations(emissions: &[Emission]) -> Vec<Violation> {
    let mut read_backs: HashMap<&str, ReadBack> = HashMap::new();
    emissions
        .iter()
        .filter_map(|emission| {
            let read_back = read_backs
                .entry(emission.label.as_str())
                .or_insert_with(|| resolve_unit_literal(&emission.label));
            first_breach(emission, read_back).map(|breach| Violation {
                emission: emission.clone(),
                breach,
            })
        })
        .collect()
}

/// A violation that is known and owned, so it does not fail the gate.
struct KnownBlock {
    dimension: DimensionVector,
    owner: &'static str,
}

impl KnownBlock {
    /// Keyed on the composed Display spelling as well as the dimension, so a curated rung or
    /// `dimension_unit_label` for the same dimension stays gated.
    fn absorbs(&self, violation: &Violation) -> bool {
        violation.emission.dimension == self.dimension
            && violation.emission.label == self.dimension.to_string()
    }
}

/// Rational-exponent Display spelling, blocked on #9236, which deletes this row.
const KNOWN_BLOCKS: &[KnownBlock] = &[KnownBlock {
    dimension: DimensionVector::FRACTURE_TOUGHNESS,
    owner: "#9236",
}];

fn invariant_r_report(emissions: &[Emission]) -> Report {
    let (blocked, unexpected): (Vec<Violation>, Vec<Violation>) = violations(emissions)
        .into_iter()
        .partition(|violation| KNOWN_BLOCKS.iter().any(|block| block.absorbs(violation)));
    let stale_blocks = KNOWN_BLOCKS
        .iter()
        .filter(|block| !blocked.iter().any(|violation| block.absorbs(violation)))
        .map(|block| block.dimension)
        .collect();
    Report {
        unexpected,
        stale_blocks,
    }
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
        panic!("the gate must report exactly the injected `kg/m³` rung; got:\n{report}");
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
        panic!("the gate must report exactly the three injected rungs; got:\n{report}");
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

#[test]
fn a_known_blocked_label_is_not_reported_as_unexpected() {
    let fracture_toughness = DimensionVector::FRACTURE_TOUGHNESS;
    let corpus = [Emission {
        surface: Surface::DimensionDisplay,
        dimension_name: "FractureToughness",
        dimension: fracture_toughness,
        label: fracture_toughness.to_string(),
        si_scale: 1.0,
    }];

    let report = invariant_r_report(&corpus);

    assert!(
        report.unexpected.is_empty() && report.stale_blocks.is_empty(),
        "FractureToughness's composed Display label is a known block, so it is neither an \
         unexpected violation nor evidence of a stale block; got:\n{report}"
    );
}

#[test]
fn a_known_block_does_not_absorb_another_label_of_its_dimension() {
    let corpus = [injected_rung(
        "FractureToughness",
        DimensionVector::FRACTURE_TOUGHNESS,
        "qq",
        1.0,
    )];

    let report = invariant_r_report(&corpus);

    let [violation] = report.unexpected.as_slice() else {
        panic!("a curated label of a blocked dimension must stay gated; got:\n{report}");
    };
    assert_eq!(violation.emission.label, "qq");
}

#[test]
fn a_known_block_whose_label_no_longer_fails_is_reported_stale() {
    let corpus: Vec<Emission> = surface_emissions()
        .into_iter()
        .filter(|emission| emission.dimension != DimensionVector::FRACTURE_TOUGHNESS)
        .collect();

    let report = invariant_r_report(&corpus);

    assert_eq!(
        report.stale_blocks,
        [DimensionVector::FRACTURE_TOUGHNESS],
        "a block with no failing emission must be reported stale"
    );
    assert!(
        report.unexpected.is_empty(),
        "removing FractureToughness from the corpus must leave nothing unexpected; got:\n{report}"
    );
}
