//! INV-SF-7 (task #7094): the member-continuation ambiguity must be a HARD
//! COMPILE ERROR at the real entry path, not a warning.
//!
//! `crates/reify-syntax/tests/harness_syntax/member_continuation_ambiguity_tests.rs`
//! pins that the check *fires* and where its span lands. This file pins the
//! consequence: that a source carrying the join does not compile.
//!
//! Severity is asserted explicitly and a `Warning` FAILS these tests. That is
//! the whole point — INV-SF-7 forbids the quiet pick, and a warning on a
//! diagnostic that changes a computed VALUE (`d == 2mm` where the author wrote
//! `5mm`) is a quiet pick with extra steps. `compile_builder/pre_pass.rs:24`
//! downgrades `parsed.errors` to warnings on its own path, so nothing about
//! "the parser produced entries" implies a hard failure by itself; the entry
//! path exercised here has to be checked, not assumed.
//!
//! Entry point: `compile_project_with_entry_source_cfg`
//! (`crates/reify-compiler/src/module_dag.rs:696`), the same function the
//! `reify check` CLI path uses to compile a project from an entry file.

use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleResolver, compile_project_with_entry_source_cfg};
use reify_core::{Diagnostic, Severity};

/// REPRO 1 from task #7094: a leading-operator continuation. Absent the check,
/// this compiles clean with `d == 2mm` — the author wrote `5mm`.
const REPRO_ONE: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

/// REPRO 2 from task #7094: a `(`-led continuation. Absent the check, `x` is
/// the call `a.b(c)` rather than the member access `a.b`.
const REPRO_TWO: &str = "structure S {\n  let x = a.b\n  (c)\n}\n";

/// Does this diagnostic identify a member-continuation ambiguity?
///
/// Asks the rule's own exported discriminator
/// (`reify_syntax::member_continuation::is_member_continuation_message`)
/// rather than restating it here. The two crates CAN share this: reify-syntax
/// is already a dependency of reify-compiler, and the predicate is public
/// precisely so every caller reads the contract from one place.
fn is_member_continuation(d: &Diagnostic) -> bool {
    reify_syntax::member_continuation::is_member_continuation_message(&d.message)
}

/// Compile `source` through the project entry path, in a scratch directory
/// with no sibling modules. None of these fixtures import anything, so the
/// resolver root only has to exist.
fn compile_entry(source: &str) -> Result<(), Vec<Diagnostic>> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let entry_path: &Path = &dir.join("main.ri");
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    compile_project_with_entry_source_cfg(entry_path, source, &resolver, &CfgSet::default())
        .map(|_| ())
}

/// Assert `source` fails to compile, with at least one **Error**-severity
/// member-continuation diagnostic.
fn assert_hard_error(label: &str, source: &str) {
    let diagnostics = match compile_entry(source) {
        Ok(()) => panic!(
            "{label}: compiled successfully. The join is silent again — INV-SF-7 \
             says a source whose value depends on this reading must not compile."
        ),
        Err(diagnostics) => diagnostics,
    };

    let continuation: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| is_member_continuation(d))
        .collect();
    assert!(
        !continuation.is_empty(),
        "{label}: compilation failed, but for some other reason — no \
         member-continuation diagnostic among {diagnostics:#?}"
    );

    // The load-bearing assertion. A `Warning` here means the ambiguity is
    // reported and then ignored, which is the failure mode this test exists
    // to prevent.
    assert!(
        continuation.iter().any(|d| d.severity == Severity::Error),
        "{label}: the member-continuation diagnostic is present but not an \
         Error, so the quiet pick survives with a note attached. Got: \
         {continuation:#?}"
    );
}

#[test]
fn repro_one_fails_compilation_with_an_error_diagnostic() {
    assert_hard_error("REPRO 1 (leading operator)", REPRO_ONE);
}

#[test]
fn repro_two_fails_compilation_with_an_error_diagnostic() {
    assert_hard_error("REPRO 2 (`(`-led)", REPRO_TWO);
}

/// The other side of the contract: a hard error on the ambiguous shape must
/// not become a blanket rejection of multi-line expressions.
///
/// The fixture is the shape from real tracked source —
/// `designs/litter_tray/bottom_deck.ri:64-65`, where the continuation row is
/// indented well past the member's own column, which is the author's signal
/// that the line continues the expression. That shape is ordinary in tracked
/// source; if this test ever fails, the check has started rejecting all of it.
/// The live census is reify-syntax's standing sweep,
/// `no_tracked_ri_source_trips_the_member_continuation_check`.
#[test]
fn a_clean_multi_line_continuation_still_compiles() {
    let source = concat!(
        "structure S {\n",
        "  let ledge_z = 20mm\n",
        "  let floor_thickness = 2mm\n",
        "  let inner_length = 100mm\n",
        "  let inner_width = 80mm\n",
        "  let pedestal_r = 5mm\n",
        "  let pedestal_h = ledge_z - floor_thickness\n",
        "  let capacity = (ledge_z - floor_thickness) * inner_length * inner_width\n",
        "               - 2.0 * 3.14159265 * pedestal_r * pedestal_r * pedestal_h\n",
        "}\n",
    );

    match compile_entry(source) {
        Ok(()) => {}
        Err(diagnostics) => {
            assert!(
                !diagnostics.iter().any(is_member_continuation),
                "the deeper-indented continuation shape from bottom_deck.ri:64-65 \
                 was reported as a member-continuation ambiguity; the check has \
                 become a blanket rejection of multi-line expressions. \
                 Diagnostics: {diagnostics:#?}"
            );
            panic!(
                "the fixture failed to compile for an unrelated reason, so this \
                 test no longer exercises what it claims: {diagnostics:#?}"
            );
        }
    }
}
