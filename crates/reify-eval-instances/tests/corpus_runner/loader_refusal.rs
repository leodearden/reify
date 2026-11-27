//! The loader refuses a key no entry may carry, and says which key.

use crate::entry::EntryRecord;

const VALID_HEAD: &str = r#"tests = "A definition's constraint is checked on its instance."
rule = "decision 7"
provenance = "design 14.7"
needs = ["instance", "constraint"]
answer_basis = "stated"
"#;

const VALID_ROW: &str = r#"
[[expect]]
path = "Bolt/constraint[0]"
verdict = "satisfied"
"#;

fn refusal(text: &str) -> String {
    match EntryRecord::from_toml(text) {
        Ok(_) => panic!("the loader accepted an entry with an unknown key:\n{text}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn an_unknown_key_is_refused_by_name() {
    let valid = format!("{VALID_HEAD}{VALID_ROW}");
    if let Err(error) = EntryRecord::from_toml(&valid) {
        panic!("the unseeded entry must load, so the refusals below are about the key: {error}");
    }

    let at_top_level = format!("{VALID_HEAD}colour = \"red\"\n{VALID_ROW}");
    let message = refusal(&at_top_level);
    assert!(
        message.contains("unknown field `colour`"),
        "a top-level unknown key must be refused by name, got: {message}"
    );

    let in_a_row = format!("{VALID_HEAD}{VALID_ROW}colour = \"red\"\n");
    let message = refusal(&in_a_row);
    assert!(
        message.contains("unknown field `colour`"),
        "an unknown key in an [[expect]] row must be refused by name, got: {message}"
    );
}
