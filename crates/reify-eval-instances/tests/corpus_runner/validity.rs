//! The corpus is valid: every entry loads, every path an entry names parses
//! and prints back the same, every compile error of this PRD names its code,
//! its sources parse or not as its needs require
//! (one waiting on the purpose grammar fails on its purpose applications
//! alone), no parsing source has a bare `Scalar`, every disputed row names an
//! entry, and the waiting entries counted under each owner equal
//! waiting.toml. One test over the whole corpus: it collects every problem,
//! each naming its entry or file, and fails once at the end.

use std::collections::BTreeSet;

use crate::bookkeeping::{Built, Disputed, EntryState, Waiting, render_waiting, waiting_counts};
use crate::corpus_tree::{self, EntryId, LoadedEntry, Problem};
use crate::parse_check::{
    bare_scalar_lines, expected_to_parse, first_parse_fault, without_purpose_applications,
};

#[test]
fn the_corpus_is_valid() {
    let root = corpus_tree::corpus_root();
    let mut problems = Vec::new();

    let (built, found) = Built::parse(&bookkeeping_text(&root, corpus_tree::BUILT, &mut problems));
    problems.extend(found);
    let (waiting, found) = Waiting::parse(&bookkeeping_text(
        &root,
        corpus_tree::WAITING,
        &mut problems,
    ));
    problems.extend(found);
    let (disputed, found) = Disputed::parse(&bookkeeping_text(
        &root,
        corpus_tree::DISPUTED,
        &mut problems,
    ));
    problems.extend(found);

    let (entries, found) = corpus_tree::load_corpus(&root);
    problems.extend(found);
    if entries.is_empty() {
        problems.push(Problem::new(root.display(), "the corpus holds no entry"));
    }
    for entry in &entries {
        problems.extend(source_problems(entry, &built));
    }

    let ids: BTreeSet<EntryId> = entries.iter().map(|entry| entry.id.clone()).collect();
    problems.extend(disputed.unknown_entries(&ids));

    for entry in &entries {
        let state = EntryState::of(&entry.record.needs, &built, disputed.lists(&entry.id));
        println!("{:<9}{}  {state}", state.label(), entry.id);
    }

    let computed = waiting_counts(&entries, &built);
    let waiting_problems = waiting.compare(&computed);
    let canonical = if waiting_problems.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nthe waiting.toml these entries give:\n{}",
            render_waiting(&computed)
        )
    };
    problems.extend(waiting_problems);

    let listed: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(
        problems.is_empty(),
        "{} problems in the corpus at {}:\n{}{canonical}",
        problems.len(),
        root.display(),
        listed.join("\n")
    );
}

/// A bookkeeping file's text; an unreadable file is a problem, read as empty.
fn bookkeeping_text(root: &std::path::Path, file: &str, problems: &mut Vec<Problem>) -> String {
    std::fs::read_to_string(root.join(file)).unwrap_or_else(|error| {
        problems.push(Problem::new(file, format!("unreadable: {error}")));
        String::new()
    })
}

/// Each source of the entry parses exactly when its needs say it must. One
/// that must not parse fails only on its purpose applications: with those
/// lines blanked it parses. The text that parses has no bare `Scalar`.
fn source_problems(entry: &LoadedEntry, built: &Built) -> Vec<Problem> {
    let must_parse = expected_to_parse(&entry.record.needs, built);
    let mut problems = Vec::new();
    for file in &entry.sources.files {
        let parsing_text = match (must_parse, first_parse_fault(&file.text)) {
            (true, Some(fault)) => {
                problems.push(Problem::new(
                    &entry.id,
                    format!("{} does not parse: {fault}", file.name),
                ));
                None
            }
            (true, None) => Some(file.text.clone()),
            (false, None) => {
                problems.push(Problem::new(
                    &entry.id,
                    format!(
                        "{} parses, but its entry needs `grammar-purpose`, which built.toml \
                         does not list: add the word once the purpose grammar is built",
                        file.name
                    ),
                ));
                Some(file.text.clone())
            }
            (false, Some(_)) => {
                let blanked = without_purpose_applications(&file.text);
                match first_parse_fault(&blanked) {
                    Some(fault) => {
                        problems.push(Problem::new(
                            &entry.id,
                            format!(
                                "{} does not parse even with its purpose applications \
                                 blanked: {fault}",
                                file.name
                            ),
                        ));
                        None
                    }
                    None => Some(blanked),
                }
            }
        };
        for line in parsing_text
            .as_deref()
            .map(bare_scalar_lines)
            .unwrap_or_default()
        {
            problems.push(Problem::new(
                &entry.id,
                format!("{}:{line} has a bare `Scalar`", file.name),
            ));
        }
    }
    problems
}
