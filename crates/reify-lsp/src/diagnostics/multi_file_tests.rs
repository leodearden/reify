//! A file-backed document that imports a sibling module is diagnosed as a
//! program: the import resolves from the entry file's directory, and the
//! entry's text comes from the editor buffer (`main.ri` is never written).

use std::path::Path;

use reify_ir::Satisfaction;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

use crate::diagnostics::{DiagnosticsResult, EvalState, compute_diagnostics_with_state};

const PULLEY_10MM: &str = "module parts\npub structure def Pulley {\n    param d : Length = 10mm\n}\n";

fn entry_uri(dir: &Path) -> Url {
    Url::from_file_path(dir.join("main.ri")).expect("absolute path")
}

fn diagnose(dir: &Path, entry_source: &str) -> DiagnosticsResult {
    compute_diagnostics_with_state(&mut EvalState::new(), entry_source, &entry_uri(dir))
}

fn messages(result: &DiagnosticsResult) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.message.as_str()).collect()
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
