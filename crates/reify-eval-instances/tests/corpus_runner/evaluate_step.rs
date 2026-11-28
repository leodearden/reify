//! Steps 2-4 of how an entry is checked (docs/prds/instances-on-the-core.md
//! section 5.8): the source as written is compiled and loaded, with a root
//! at each root its rows' paths name; every path the rows name is asked;
//! each answer is compared with its row; and the paths asked again in
//! reverse order on a fresh host must answer alike.
//!
//! Each path is a request of its own, so the order of asking is real. A
//! row after an edit and a `roots` row are problems until the leaves that
//! build edits and default roots extend this step; an `open` row is asked
//! and not compared.

use std::collections::HashMap;

use reify_core::{DefinitionName, InstancePath};
use reify_eval_instances::{DemandError, Host, HostAnswer, MemberKinds};
use reify_eval_source::{DefinitionList, compile_entry};

use crate::answer_check;
use crate::compile_step::entry_file;
use crate::corpus_tree::{LoadedEntry, Problem};
use crate::expectation::Form;
use crate::ready::ready_entries;
use crate::seeded::seeded;

/// What asking one path gave.
type Asked = Result<HostAnswer, DemandError>;

/// Every way `entry`'s answers differ from what its path rows expect, or
/// differ between two orders of asking, each naming the entry.
pub fn evaluate_problems(entry: &LoadedEntry) -> Vec<Problem> {
    let problem = |what: String| Problem::new(&entry.id, what);
    let (rows, mut problems) = path_rows(entry);
    if rows.is_empty() {
        return problems;
    }
    let Some(file) = entry_file(entry) else {
        problems.push(problem("has no one entry file to compile".to_string()));
        return problems;
    };
    let definitions = match compile_entry(&entry.sources.import_root.join(&file.name), &file.text) {
        Ok(output) => output.definitions,
        Err(refusal) => {
            problems.push(problem(format!(
                "the second output refused {}: {refusal:?}",
                file.name
            )));
            return problems;
        }
    };
    let paths = demanded(&rows);
    let roots = roots(&paths);
    let forward = asked(&definitions, &roots, paths.iter().copied());
    let reverse = asked(&definitions, &roots, paths.iter().rev().copied());

    for path in &paths {
        if let Err(error) = &forward[path] {
            problems.push(problem(format!("`{path}` cannot be asked: {error}")));
        }
    }
    for (path, form) in &rows {
        problems.extend(row_mismatch(path, form, &forward).map(&problem));
    }
    for path in &paths {
        let (first, again) = (&forward[path], &reverse[path]);
        if first != again {
            problems.push(problem(format!(
                "`{path}` is answered {first:?} asked in row order, but {again:?} asked in \
                 reverse order"
            )));
        }
    }
    problems
}

/// The rows about a path of the source as written, and a problem for each
/// row this step cannot check. Compile rows are the compile step's.
fn path_rows(entry: &LoadedEntry) -> (Vec<(&InstancePath, &Form)>, Vec<Problem>) {
    let problem = |what: String| Problem::new(&entry.id, what);
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for row in &entry.record.expect {
        match (&row.form, &row.path, row.after_edit) {
            (_, _, Some(edit)) => problems.push(problem(format!(
                "a row after edit {edit} is not checked: this step evaluates the source as written"
            ))),
            (Form::Roots(roots), _, None) => problems.push(problem(format!(
                "a roots row {roots:?} is not checked: this step declares the roots its rows' \
                 paths name"
            ))),
            (Form::Compile | Form::CompileError { .. }, _, None) => {}
            (form, Some(path), None) => rows.push((path, form)),
            (form, None, None) => {
                problems.push(problem(format!("a {form:?} row names no path")));
            }
        }
    }
    (rows, problems)
}

/// Each row's path, then its `same_as` path, each once, in row order.
fn demanded<'e>(rows: &[(&'e InstancePath, &'e Form)]) -> Vec<&'e InstancePath> {
    let mut paths = Vec::new();
    for (path, form) in rows {
        let same_as = match form {
            Form::SameAs(other) => Some(other),
            _ => None,
        };
        for path in [Some(*path), same_as].into_iter().flatten() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

/// The roots `paths` name, each once.
fn roots(paths: &[&InstancePath]) -> Vec<DefinitionName> {
    let mut roots = Vec::new();
    for path in paths {
        if !roots.contains(path.root()) {
            roots.push(path.root().clone());
        }
    }
    roots
}

/// Each of `paths` asked in the order given, on a fresh host.
fn asked<'p>(
    definitions: &DefinitionList,
    roots: &[DefinitionName],
    paths: impl Iterator<Item = &'p InstancePath>,
) -> HashMap<&'p InstancePath, Asked> {
    let mut host = Host::new();
    let loaded = host.load(definitions, &MemberKinds::built(), roots);
    paths
        .map(|path| (path, host.answer(&loaded, path)))
        .collect()
}

/// How the answers miss the row at `path`; none where they meet it, or
/// where a path could not be asked, which is its own problem.
fn row_mismatch(
    path: &InstancePath,
    form: &Form,
    answers: &HashMap<&InstancePath, Asked>,
) -> Option<String> {
    let Ok(answer) = &answers[path] else {
        return None;
    };
    match form {
        Form::SameAs(other) => {
            let Ok(other_answer) = &answers[other] else {
                return None;
            };
            (answer != other_answer).then(|| {
                format!(
                    "`{path}` is answered {answer:?}, but `{other}`, which it is expected to be \
                     the same as, is answered {other_answer:?}"
                )
            })
        }
        form => answer_check::mismatch(form, answer).map(|what| format!("`{path}`: {what}")),
    }
}

#[test]
fn every_ready_entry_answers_as_it_expects() {
    let (ready, mut problems) = ready_entries();
    let with_paths: Vec<&LoadedEntry> = ready
        .iter()
        .filter(|entry| entry.record.expect.iter().any(|row| row.path.is_some()))
        .collect();
    for entry in &with_paths {
        problems.extend(evaluate_problems(entry));
    }
    println!(
        "evaluated {} ready entries with path rows",
        with_paths.len()
    );

    let listed: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(
        problems.is_empty(),
        "{} evaluation problems in the corpus:\n{}",
        problems.len(),
        listed.join("\n")
    );
}

const SOURCE: &str = "structure def A { param t : Length = 1mm }\n";

/// Rows whose answers no later leaf changes: the root is present, and a
/// name A does not declare is absent and the same as another such name.
const EXPECTS_AS_ANSWERED: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "PRD 12.1 rule 10"
needs = ["instance"]
answer_basis = "seeded"

[[expect]]
path = "A"
presence = "present"

[[expect]]
path = "A.nothing"
presence = "absent"

[[expect]]
path = "A.nothing"
same_as = "A.other"
"#;

const EXPECTS_UNDECLARED_PRESENT: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "PRD 12.1 rule 10"
needs = ["instance"]
answer_basis = "seeded"

[[expect]]
path = "A.nothing"
presence = "present"
"#;

const EXPECTS_ROOT_SAME_AS_UNDECLARED: &str = r#"tests = "seeded"
rule = "seeded"
provenance = "PRD 12.1 rule 10"
needs = ["instance"]
answer_basis = "seeded"

[[expect]]
path = "A"
same_as = "A.nothing"
"#;

/// The problems `entry` gives, as text.
fn listed(entry: &LoadedEntry) -> Vec<String> {
    evaluate_problems(entry)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The one problem `entry` gives, as text.
fn sole_problem(entry: &LoadedEntry) -> String {
    let listed = listed(entry);
    assert_eq!(
        listed.len(),
        1,
        "`{}` gives exactly one problem, got {listed:#?}",
        entry.id
    );
    listed.into_iter().next().unwrap_or_default()
}

#[test]
fn an_entry_answered_as_it_expects_gives_no_problem() {
    let entry = seeded("seeded-answered-as-expected", EXPECTS_AS_ANSWERED, SOURCE);
    let problems = listed(&entry);
    assert!(
        problems.is_empty(),
        "every row answered as expected is no problem: {problems:#?}"
    );
}

#[test]
fn a_row_answered_otherwise_is_a_problem_naming_the_path_and_both_sides() {
    let entry = seeded(
        "seeded-present-expected-absent-answered",
        EXPECTS_UNDECLARED_PRESENT,
        SOURCE,
    );
    let problem = sole_problem(&entry);
    let lowered = problem.to_lowercase();
    assert!(
        problem.contains(&entry.id.to_string())
            && problem.contains("`A.nothing`")
            && lowered.contains("present")
            && lowered.contains("absent"),
        "the problem names the entry, the path, the expectation and the answer: {problem}"
    );
}

#[test]
fn a_same_as_row_answered_unequal_is_a_problem_naming_both_paths() {
    let entry = seeded(
        "seeded-same-as-expected-unequal-answered",
        EXPECTS_ROOT_SAME_AS_UNDECLARED,
        SOURCE,
    );
    let problem = sole_problem(&entry);
    assert!(
        problem.contains(&entry.id.to_string())
            && problem.contains("`A`")
            && problem.contains("`A.nothing`"),
        "the problem names the entry and both paths: {problem}"
    );
}
