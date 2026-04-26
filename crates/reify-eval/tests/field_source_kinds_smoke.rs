//! Worked-example smoke test for field source kinds.
//!
//! Exercises the three user-writeable field source kinds — `analytical`,
//! `sampled`, and `composed` — as defined by §4.1.4 of
//! `docs/reify-language-spec.md` ("Field Declarations").
//!
//! Four-test plan:
//!   1. `composed_stiffness_ri_parses`              — parse only, no errors
//!   2. `composed_stiffness_compiles_with_stdlib`   — compile, three fields present
//!   3. `composed_stiffness_evals_with_three_field_source_kinds` — eval, correct FieldSourceKind per field
//!   4. `composed_stiffness_constraints_all_satisfied` — structure constraints all Satisfied
//!
//! Uses `examples/fields/composed_stiffness.ri` as the fixture file.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{ModulePath, Severity};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/composed_stiffness.ri"
);

/// Read `examples/fields/composed_stiffness.ri` and verify it parses without errors.
#[test]
fn composed_stiffness_ri_parses() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/composed_stiffness.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
