//! The loader refuses what no entry may carry, and says what it was.

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
        Ok(_) => panic!("the loader accepted an entry it must refuse:\n{text}"),
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

#[test]
fn a_path_the_text_form_cannot_read_is_refused_naming_it() {
    let valid = format!("{VALID_HEAD}{VALID_ROW}");
    if let Err(error) = EntryRecord::from_toml(&valid) {
        panic!("the unseeded entry must load, so the refusals below are about the path: {error}");
    }

    let in_path = format!(
        "{VALID_HEAD}\n[[expect]]\npath = \"Bolt/constraint[x]\"\nverdict = \"satisfied\"\n"
    );
    let in_same_as = format!(
        "{VALID_HEAD}{VALID_ROW}\n[[expect]]\npath = \"Bolt/summary\"\nsame_as = \"Bolt/summary[\"\n"
    );
    let in_candidates = format!(
        "{VALID_HEAD}{VALID_ROW}\n[[candidates.unknown]]\npath = \"Bolt/auto[a]\"\nvalues = [\"12mm\"]\n"
    );
    for (key, text, entry) in [
        ("[[expect]] path", "Bolt/constraint[x]", in_path),
        ("same_as", "Bolt/summary[", in_same_as),
        ("[[candidates.unknown]] path", "Bolt/auto[a]", in_candidates),
    ] {
        let message = refusal(&entry);
        assert!(
            message.contains(text),
            "a {key} the text form cannot read must be refused naming `{text}`, got: {message}"
        );
    }
}

/// An entry whose one row is a compile error, with `code_line` beside it.
fn compile_error_entry(needs: &str, code_line: &str) -> String {
    format!(
        r#"tests = "A `let` written in a sub's braces is read only there."
rule = "decision D10(f)"
provenance = "design Part 4"
needs = [{needs}]
answer_basis = "stated"

[[expect]]
compile_error = "`zz` is a `let` written in the braces of `s` and can be read only there"
{code_line}
"#
    )
}

#[test]
fn a_compile_error_row_carries_a_real_code() {
    let unknown = compile_error_entry(r#""compile-error""#, r#"code = "NoSuchCode""#);
    let message = refusal(&unknown);
    assert!(
        message.contains("NoSuchCode"),
        "an unknown code must be refused by name, got: {message}"
    );

    let uncoded = compile_error_entry(r#""compile-error", "braces""#, "");
    let message = refusal(&uncoded);
    assert!(
        message.contains("a `compile_error` row needs a `code`"),
        "a compile error of this PRD with no code must be refused, got: {message}"
    );

    let owned_elsewhere =
        compile_error_entry(r#""compile-error", "braces", "required-arguments""#, "");
    if let Err(error) = EntryRecord::from_toml(&owned_elsewhere) {
        panic!("an entry needing a word owned outside this PRD may leave out the code: {error}");
    }

    let coded = compile_error_entry(
        r#""compile-error""#,
        r#"code = "PrivateLetReadFromAnotherDefinition""#,
    );
    if let Err(error) = EntryRecord::from_toml(&coded) {
        panic!("a row naming a declared code must load: {error}");
    }
}
