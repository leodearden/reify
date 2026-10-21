//! Dimension-checked-readers γ2 (#6941), per-binding signal: once the four FEA
//! load magnitude fields are dimensioned, the ctor-conformance sites in
//! `dcr_load_ctor_dimension_silent.ri` move from the units-CORRECT bindings to
//! the deliberately bare ones.
//!
//! The fixture's exit code is 1 both before and after the retype, on opposite
//! bindings, so only the per-binding site set carries the signal.

use crate::sweep::survey_corpus;
use crate::workspace_git::WORKSPACE_ROOT;

const FIXTURE: &str = "tests/prd-gate/fixtures/dcr_load_ctor_dimension_silent.ri";

/// The `let` binding declared on 1-based `line` of `source`.
fn binding_on_line(source: &str, line: u32) -> Option<&str> {
    let text = source
        .lines()
        .nth(usize::try_from(line).ok()?.checked_sub(1)?)?;
    let rest = text.trim_start().strip_prefix("let ")?;
    rest.split_whitespace().next()
}

#[test]
fn dcr_load_ctor_sites_are_exactly_the_deliberately_bare_bindings() {
    let root = std::path::Path::new(WORKSPACE_ROOT);
    let run = survey_corpus(root, &[FIXTURE.to_owned()]);
    assert!(
        run.not_surveyed.is_empty(),
        "{FIXTURE} must reach the compile phase: {:?}",
        run.not_surveyed
    );

    let source = std::fs::read_to_string(root.join(FIXTURE)).expect("read the fixture");
    let sites: std::collections::BTreeSet<(Option<&str>, Option<&str>)> = run
        .sites
        .iter()
        .map(|site| (binding_on_line(&source, site.line), site.field.as_deref()))
        .collect();

    let expected = std::collections::BTreeSet::from([
        (Some("bare_point"), Some("force")),
        (Some("bare_pressure"), Some("magnitude")),
        (Some("body_force"), Some("force_density")),
    ]);
    assert_eq!(
        sites, expected,
        "with PointLoad.force : Force, PressureLoad.magnitude : Pressure, \
         TractionLoad.traction : Pressure and BodyForce.force_density : ForceDensity, \
         the units-correct `dimensioned_point` and `traction` bindings conform and \
         exactly the three bare bindings are ctor-conformance sites; \
         (binding, field) pairs found: {sites:?}"
    );
}
