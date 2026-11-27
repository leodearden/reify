//! Step 1 of how an entry is checked (docs/prds/instances-on-the-core.md
//! section 5.8): its entry file is compiled by the second output, and the
//! outcome is compared with the entry's compile rows.

use std::path::PathBuf;

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_eval_source::compile_entry;

use crate::bookkeeping::{Built, Disputed, EntryState};
use crate::corpus_tree::{self, EntryId, EntrySources, LoadedEntry, Problem, SourceFile};
use crate::entry::EntryRecord;
use crate::expectation::Form;

/// Every way `entry`'s compile outcome differs from what its compile rows
/// expect, each naming the entry. The rows that decide are those about the
/// source as written (no `after_edit`): each `compile_error` row needs an
/// error of its code (any error, for a row with no code), and an entry with
/// no `compile_error` row needs no error at all.
pub fn compile_problems(entry: &LoadedEntry) -> Vec<Problem> {
    let problem = |what: String| Problem::new(&entry.id, what);
    let Some(file) = entry_file(entry) else {
        return vec![problem("has no one entry file to compile".to_string())];
    };
    let rows: Vec<&Form> = entry
        .record
        .expect
        .iter()
        .filter(|row| row.after_edit.is_none())
        .map(|row| &row.form)
        .collect();
    let expected_codes: Vec<Option<DiagnosticCode>> = rows
        .iter()
        .filter_map(|form| match form {
            Form::CompileError { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    if !expected_codes.is_empty() && rows.iter().any(|form| matches!(form, Form::Compile)) {
        return vec![problem(
            "expects both acceptance and a compile error of the source as written".to_string(),
        )];
    }

    let output = match compile_entry(&entry.sources.import_root.join(&file.name), &file.text) {
        Ok(output) => output,
        Err(refusal) => {
            return vec![problem(format!(
                "the second output refused {}: {refusal:?}",
                file.name
            ))];
        }
    };
    let errors: Vec<&Diagnostic> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();

    if expected_codes.is_empty() {
        return if errors.is_empty() {
            Vec::new()
        } else {
            vec![problem(format!(
                "expected the source to be accepted, but it reported {}",
                rendered(&errors)
            ))]
        };
    }
    if errors.is_empty() {
        let expected: Vec<String> = expected_codes.iter().map(rendered_code).collect();
        return vec![problem(format!(
            "expected a compile error ({}), but the source was accepted",
            expected.join(", ")
        ))];
    }
    expected_codes
        .iter()
        .flatten()
        .filter(|code| !errors.iter().any(|error| error.code == Some(**code)))
        .map(|code| {
            problem(format!(
                "expected a compile error of code `{code:?}`, but it reported {}",
                rendered(&errors)
            ))
        })
        .collect()
}

/// The file an entry is compiled from: its `entry_file`, or its one file.
pub fn entry_file(entry: &LoadedEntry) -> Option<&SourceFile> {
    let files = &entry.sources.files;
    match &entry.record.entry_file {
        Some(name) => files.iter().find(|file| file.name == *name),
        None => match files.as_slice() {
            [only] => Some(only),
            _ => None,
        },
    }
}

fn rendered_code(code: &Option<DiagnosticCode>) -> String {
    match code {
        Some(code) => format!("`{code:?}`"),
        None => "with no code".to_string(),
    }
}

/// The errors reported, each as its code and message.
fn rendered(errors: &[&Diagnostic]) -> String {
    let each: Vec<String> = errors
        .iter()
        .map(|error| format!("{} {:?}", rendered_code(&error.code), error.message))
        .collect();
    format!("errors {}", each.join("; "))
}

/// A one-file entry in theme `other`, from its expectations and its source.
fn seeded(name: &str, expectations: &str, source: &str) -> LoadedEntry {
    let record = EntryRecord::from_toml(expectations)
        .unwrap_or_else(|error| panic!("the seeded entry `{name}` loads: {error}"));
    LoadedEntry {
        id: EntryId {
            theme: "other".to_string(),
            name: name.to_string(),
        },
        record,
        sources: EntrySources {
            files: vec![SourceFile {
                name: "seeded.ri".to_string(),
                text: source.to_string(),
            }],
            import_root: PathBuf::from("/nonexistent/seeded-corpus-entry"),
        },
    }
}

const EXPECTS_PRIVATE_LET_ERROR: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "scenario C5"
needs = ["compile-error"]
answer_basis = "seeded"

[[expect]]
compile_error = "a private let read from another definition"
code = "PrivateLetReadFromAnotherDefinition"
"#;

const EXPECTS_UNKNOWN_SUB_ERROR: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "scenario C5"
needs = ["compile-error"]
answer_basis = "seeded"

[[expect]]
compile_error = "a sub naming no definition"
code = "UnknownSubStructure"
"#;

const EXPECTS_ACCEPTANCE: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "scenario C5"
needs = ["instance"]
answer_basis = "seeded"

[[expect]]
compile = "ok"
"#;

const ACCEPTED_SOURCE: &str = "structure def A { param t : Length = 1mm }\n";
const UNKNOWN_SUB_SOURCE: &str = "structure def A { sub p : Nope }\n";

/// The one problem `entry` gives, as text.
fn sole_problem(entry: &LoadedEntry) -> String {
    let problems = compile_problems(entry);
    let listed: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert_eq!(
        listed.len(),
        1,
        "`{}` gives exactly one problem, got {listed:#?}",
        entry.id
    );
    listed.into_iter().next().unwrap_or_default()
}

/// Scenario C5 (PRD section 10): the runner's compile step fails an entry
/// whose compile outcome is not the one it expects, and passes one whose
/// outcome is.
#[test]
fn scenario_c5() {
    let accepted = seeded(
        "seeded-error-expected-source-accepted",
        EXPECTS_PRIVATE_LET_ERROR,
        ACCEPTED_SOURCE,
    );
    let problem = sole_problem(&accepted);
    assert!(
        problem.contains(&accepted.id.to_string()) && problem.contains("accepted"),
        "an expected error over an accepted source names the entry and says it was accepted: \
         {problem}"
    );

    let other_code = seeded(
        "seeded-error-expected-other-code-reported",
        EXPECTS_PRIVATE_LET_ERROR,
        UNKNOWN_SUB_SOURCE,
    );
    let problem = sole_problem(&other_code);
    assert!(
        problem.contains(&other_code.id.to_string())
            && problem.contains("PrivateLetReadFromAnotherDefinition")
            && problem.contains("UnknownSubStructure"),
        "an error of another code names the entry, the code expected and the code reported: \
         {problem}"
    );

    let control = seeded(
        "seeded-error-expected-and-reported",
        EXPECTS_UNKNOWN_SUB_ERROR,
        UNKNOWN_SUB_SOURCE,
    );
    let problems: Vec<String> = compile_problems(&control)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        problems.is_empty(),
        "the error expected, reported, is no problem: {problems:#?}"
    );

    let rejected = seeded(
        "seeded-acceptance-expected-error-reported",
        EXPECTS_ACCEPTANCE,
        UNKNOWN_SUB_SOURCE,
    );
    let problem = sole_problem(&rejected);
    assert!(
        problem.contains(&rejected.id.to_string()) && problem.contains("UnknownSubStructure"),
        "an error over a source expected to be accepted names the entry and the code reported: \
         {problem}"
    );
}

#[test]
fn every_ready_entry_compiles_as_it_expects() {
    let root = corpus_tree::corpus_root();
    let mut problems = Vec::new();
    let mut bookkeeping = |file: &str| {
        std::fs::read_to_string(root.join(file)).unwrap_or_else(|error| {
            problems.push(Problem::new(file, format!("unreadable: {error}")));
            String::new()
        })
    };
    let (built, built_problems) = Built::parse(&bookkeeping(corpus_tree::BUILT));
    let (disputed, disputed_problems) = Disputed::parse(&bookkeeping(corpus_tree::DISPUTED));
    let (entries, load_problems) = corpus_tree::load_corpus(&root);
    problems.extend(built_problems);
    problems.extend(disputed_problems);
    problems.extend(load_problems);

    let ready: Vec<&LoadedEntry> = entries
        .iter()
        .filter(|entry| {
            matches!(
                EntryState::of(&entry.record.needs, &built, disputed.lists(&entry.id)),
                EntryState::Ready
            )
        })
        .collect();
    for entry in &ready {
        problems.extend(compile_problems(entry));
    }
    println!(
        "compiled {} ready entries of {} in the corpus",
        ready.len(),
        entries.len()
    );

    let listed: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(
        problems.is_empty(),
        "{} compile problems in the corpus at {}:\n{}",
        problems.len(),
        root.display(),
        listed.join("\n")
    );
}
