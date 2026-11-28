use crate::corpus_tree::LoadedEntry;
use crate::seeded::seeded;

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
