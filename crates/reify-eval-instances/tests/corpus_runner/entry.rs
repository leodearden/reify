//! One entry's expectations file, `<name>.toml`, loaded with a closed set of
//! keys: a key no entry may carry fails the load, naming the key.

use std::fmt;

use reify_core::InstancePath;
use serde::Deserialize;

use crate::expectation::Expectation;
use crate::expected_value::ExpectedValue;
use crate::needs::Need;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryRecord {
    pub tests: String,
    pub rule: String,
    pub provenance: String,
    pub needs: Vec<Need>,
    pub answer_basis: String,
    pub today: Option<String>,
    pub note: Option<String>,
    pub not_pinned: Option<String>,
    /// The entry file of a multi-file entry, whose sources sit in a
    /// directory named after the entry.
    pub entry_file: Option<String>,
    #[expect(dead_code, reason = "read once entries run")]
    pub settings: Option<Settings>, // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
    pub candidates: Option<Candidates>,
    #[serde(default)]
    pub edits: Vec<Edit>,
    pub expect: Vec<Expectation>,
}

/// A limit that changes answers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "read once entries run")] // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
pub struct Settings {
    pub path_depth: u32,
}

/// The values the test solver tries for each unknown, the k-th of each
/// together.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidates {
    pub note: Option<String>,
    #[expect(dead_code, reason = "read once entries run")]
    pub unknown: Vec<CandidateList>, // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "read once entries run")] // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
pub struct CandidateList {
    #[serde(deserialize_with = "crate::path_text::path")]
    pub path: InstancePath,
    pub values: Vec<ExpectedValue>,
}

/// One edit, applied to the text the edit before it left.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "read once entries run")] // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
pub struct Edit {
    pub definition: String,
    pub replace: String,
    pub by: String,
}

#[derive(Debug)]
pub struct LoadError(String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl EntryRecord {
    pub fn from_toml(text: &str) -> Result<EntryRecord, LoadError> {
        let record: EntryRecord =
            toml::from_str(text).map_err(|error| LoadError(error.to_string()))?;
        record.check().map_err(LoadError)?;
        Ok(record)
    }

    /// Every prose key the entry carries, by name. Prose is for the reader;
    /// the runner only checks that each says something.
    fn prose(&self) -> Vec<(&'static str, &str)> {
        let candidates_note = self.candidates.as_ref().and_then(|c| c.note.as_deref());
        [
            ("tests", Some(self.tests.as_str())),
            ("rule", Some(self.rule.as_str())),
            ("provenance", Some(self.provenance.as_str())),
            ("answer_basis", Some(self.answer_basis.as_str())),
            ("today", self.today.as_deref()),
            ("note", self.note.as_deref()),
            ("not_pinned", self.not_pinned.as_deref()),
            ("candidates.note", candidates_note),
        ]
        .into_iter()
        .filter_map(|(key, text)| text.map(|text| (key, text)))
        .collect()
    }

    /// What the key set alone cannot say.
    fn check(&self) -> Result<(), String> {
        if let Some((key, _)) = self
            .prose()
            .into_iter()
            .find(|(_, text)| text.trim().is_empty())
        {
            return Err(format!("`{key}` is blank"));
        }
        if let Some(need) = self
            .needs
            .iter()
            .enumerate()
            .find_map(|(at, need)| self.needs[..at].contains(need).then_some(need))
        {
            return Err(format!("`needs` lists `{need}` more than once"));
        }
        if self.expect.is_empty() {
            return Err("an entry needs at least one [[expect]] row".to_string());
        }
        for row in &self.expect {
            if let Some(edit) = row.after_edit
                && !(1..=self.edits.len()).contains(&(edit as usize))
            {
                return Err(format!(
                    "`after_edit = {edit}` names no edit; the entry has {} edits",
                    self.edits.len()
                ));
            }
        }
        Ok(())
    }
}

#[test]
fn a_needs_word_listed_twice_is_refused_by_name() {
    let text = r#"tests = "t"
rule = "r"
provenance = "p"
needs = ["roots", "instance", "roots"]
answer_basis = "stated"

[[expect]]
roots = ["Asm"]
"#;
    match EntryRecord::from_toml(text) {
        Ok(_) => panic!("an entry listing `roots` twice in `needs` must be refused"),
        Err(error) => assert_eq!(error.to_string(), "`needs` lists `roots` more than once"),
    }
}
