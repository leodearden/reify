//! A file-backed document that imports a sibling module is diagnosed as a
//! program: the import resolves from the entry file's directory, and the
//! entry's text comes from the editor buffer (`main.ri` is never written).

use std::path::Path;

use reify_ir::Satisfaction;
use tower_lsp::lsp_types::{DiagnosticSeverity, Range, Url};

use crate::diagnostics::{DiagnosticsResult, EvalState, compute_diagnostics_with_state};

const PULLEY_10MM: &str =
    "module parts\npub structure def Pulley {\n    param d : Length = 10mm\n}\n";

fn entry_uri(dir: &Path) -> Url {
    Url::from_file_path(dir.join("main.ri")).expect("absolute path")
}

fn diagnose(dir: &Path, entry_source: &str) -> DiagnosticsResult {
    compute_diagnostics_with_state(&mut EvalState::new(), entry_source, &entry_uri(dir))
}

fn messages(result: &DiagnosticsResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect()
}

#[test]
fn importing_entry_has_no_false_unknown_structure() {
    let guard = reify_test_support::prefixed_tempdir("reify-lsp-multifile-");
    std::fs::write(guard.path().join("parts.ri"), PULLEY_10MM).expect("write parts.ri");
    let entry = "module main\nimport parts\nstructure def Rig {\n    sub p = Pulley()\n    constraint p.d > 5mm\n}\n";

    let result = diagnose(guard.path(), entry);

    for message in messages(&result) {
        assert!(
            !message.contains("references unknown structure")
                && !message.contains("not resolved by this entry point"),
            "the import must resolve, got {message:?}"
        );
    }
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(errors.is_empty(), "expected no errors, got {errors:?}");
    let check = result.check_result.as_ref().expect("eval ran");
    assert!(
        check
            .constraint_results
            .iter()
            .any(|e| e.id.entity == "Rig" && e.satisfaction == Satisfaction::Satisfied),
        "the Rig constraint over the imported Pulley's param must be satisfied, got {:?}",
        check.constraint_results
    );
}

#[test]
fn missing_import_of_file_backed_entry_is_reported_as_unresolvable() {
    let guard = reify_test_support::prefixed_tempdir("reify-lsp-multifile-");
    let entry = "module main\nimport ghost\nstructure def Rig {\n    param x : Length = 1mm\n}\n";

    let result = diagnose(guard.path(), entry);

    assert!(
        result.diagnostics.iter().any(|d| {
            d.severity == Some(DiagnosticSeverity::ERROR)
                && d.message.contains("ghost")
                && d.message.contains("not found")
        }),
        "expected an Error naming the unresolvable `ghost`, got {:?}",
        messages(&result)
    );
}

/// The published message for a violated constraint, as the pipeline words it.
fn violation_message(entry: &reify_eval::ConstraintCheckEntry) -> String {
    match &entry.label {
        Some(label) => format!("constraint violated: {label}"),
        None => format!("constraint {} violated", entry.id),
    }
}

fn violation_range(result: &DiagnosticsResult, entity: &str) -> Vec<Range> {
    let check = result.check_result.as_ref().expect("eval ran");
    let violated: Vec<_> = check
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == entity && e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violated.is_empty(),
        "expected a violated {entity} constraint, got {:?}",
        check.constraint_results
    );
    violated
        .into_iter()
        .map(|entry| {
            let message = violation_message(entry);
            result
                .diagnostics
                .iter()
                .find(|d| d.message == message)
                .unwrap_or_else(|| panic!("no published {message:?}, got {:?}", messages(result)))
                .range
        })
        .collect()
}

/// An imported template's span indexes its own file, so its violation is
/// published unanchored rather than at an arbitrary spot in the entry.
#[test]
fn imported_template_violation_is_not_mislocated_in_the_entry() {
    let guard = reify_test_support::prefixed_tempdir("reify-lsp-multifile-");
    std::fs::write(
        guard.path().join("parts.ri"),
        "module parts\npub structure def Pulley {\n    param d : Length = 1mm\n    constraint d > 5mm\n}\n",
    )
    .expect("write parts.ri");
    // Padding puts the byte offsets of Pulley's constraint in parts.ri inside
    // the entry's text, so a mislocated range would be visible, not clamped.
    let padding = "// padding line\n".repeat(8);
    let entry = format!(
        "{padding}module main\nimport parts\nstructure def Rig {{\n    sub p = Pulley()\n    constraint p.d > 5mm\n}}\n"
    );
    let rig_constraint_line = entry
        .lines()
        .position(|line| line.contains("constraint p.d"))
        .expect("Rig constraint line") as u32;

    let result = diagnose(guard.path(), &entry);

    for range in violation_range(&result, "Pulley") {
        assert_eq!(
            range,
            Range::default(),
            "Pulley's violation must be unanchored"
        );
    }
    for range in violation_range(&result, "Rig") {
        assert_ne!(range, Range::default());
        assert_eq!(range.start.line, rig_constraint_line);
    }
}
