//! The SIGNATURE FIXTURES: compile-verified `.ri` sources whose calls stand for
//! documented signatures — where each lives, how it is read, and the one
//! definition of "compiles clean" every one of them is held to.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

use crate::chunk_io::repo_root;

/// The executable transcription of stdlib.md's "Key Geometry Operations" and
/// "Curves" sections, repo-relative.
pub(crate) const STDLIB_GEOMETRY_OPS_FIXTURE: &str =
    "crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri";

/// The mirror of every other signature in the chunks' unfenced prose,
/// repo-relative.
pub(crate) const UNFENCED_SIGNATURES_FIXTURE: &str =
    "crates/reify-compiler/tests/fixtures/unfenced_signatures_smoke.ri";

/// Every fixture whose calls can exercise a documented form, each repo-relative
/// — the currency every violation names a fixture in.
pub(crate) const SIGNATURE_FIXTURES: &[&str] =
    &[UNFENCED_SIGNATURES_FIXTURE, STDLIB_GEOMETRY_OPS_FIXTURE];

/// The source of the repo-relative fixture at `path`.
pub(crate) fn read_fixture(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|e| panic!("{path} must be readable ({e}) — it is a signature fixture"))
}

/// The diagnostics that make a signature fixture's calls untrustworthy, rendered
/// one per line: any `Severity::Error`, a call to a name nothing resolves, and a
/// builtin called at an argument shape it does not recognise. Any other warning
/// never counts.
pub(crate) fn fixture_compile_violations(source: &str) -> Vec<String> {
    compile_source_with_stdlib(source)
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.severity == Severity::Error
                || matches!(
                    diagnostic.code,
                    Some(
                        DiagnosticCode::UnresolvedFunction
                            | DiagnosticCode::BuiltinArgShapeUnrecognized
                    )
                )
        })
        .map(|diagnostic| {
            let code = diagnostic
                .code
                .as_ref()
                .map(|code| format!(" {code:?}"))
                .unwrap_or_default();
            format!("{:?}{code}: {}", diagnostic.severity, diagnostic.message)
        })
        .collect()
}

/// Every signature fixture compiles clean: each call in one stands for a
/// documented signature, so an Error, an unresolved name or an unrecognised
/// argument shape there is a documented form the compiler does not accept.
#[test]
fn every_signature_fixture_compiles_clean() {
    let violations: Vec<String> = SIGNATURE_FIXTURES
        .iter()
        .flat_map(|path| {
            fixture_compile_violations(&read_fixture(path))
                .into_iter()
                .map(move |violation| format!("{path}: {violation}"))
        })
        .collect();

    assert!(
        violations.is_empty(),
        "signature fixtures that do not compile clean:\n{}",
        violations.join("\n")
    );
}

#[test]
fn fixture_compile_violations_counts_errors_unresolved_names_and_unrecognised_arg_shapes_only() {
    let reported = [
        (
            "structure def S {\n    let o = some(1mm, 2mm)\n}\n",
            "Error",
        ),
        (
            "structure def S {\n    let x = bogus_fn(1mm)\n}\n",
            "UnresolvedFunction",
        ),
        (
            "structure def S {\n    let xs = generate(3)\n}\n",
            "BuiltinArgShapeUnrecognized",
        ),
    ];
    for (source, class) in reported {
        let violations = fixture_compile_violations(source);
        assert!(
            violations.iter().any(|violation| violation.contains(class)),
            "`{source}` must be reported as {class}, got {violations:#?}"
        );
    }

    assert_eq!(
        fixture_compile_violations("structure def S {\n    let o = some(1mm)\n}\n"),
        Vec::<String>::new(),
        "any other diagnostic — e.g. the missing-`module` warning — never counts"
    );
}
