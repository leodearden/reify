//! The corpus's three bookkeeping files (docs/prds/instances-on-the-core.md
//! section 5.8): the words built so far, the committed count of waiting
//! entries per owner, and the disputed entries; and what they make of an
//! entry's state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;

use crate::corpus_tree::{self, EntryId, LoadedEntry, Problem};
use crate::needs::{Need, Owner};

/// `built.toml`: the words built so far, listed sorted and once each.
pub struct Built(BTreeSet<Need>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuiltFile {
    built: Vec<Need>,
}

impl Built {
    pub fn parse(text: &str) -> (Built, Vec<Problem>) {
        let mut problems = Vec::new();
        let listed = match toml::from_str::<BuiltFile>(text) {
            Ok(file) => file.built,
            Err(error) => {
                problems.push(Problem::new(corpus_tree::BUILT, error.to_string()));
                Vec::new()
            }
        };
        let words: Vec<&str> = listed.iter().map(|need| need.word()).collect();
        if words.windows(2).any(|pair| pair[0] >= pair[1]) {
            problems.push(Problem::new(
                corpus_tree::BUILT,
                format!(
                    "the words must be sorted and each listed once, got `{}`",
                    words.join("`, `")
                ),
            ));
        }
        (Built(listed.into_iter().collect()), problems)
    }

    pub fn contains(&self, need: Need) -> bool {
        self.0.contains(&need)
    }
}

/// `waiting.toml`: one `<task id> = <count>` row per owner with waiting
/// entries.
pub struct Waiting(BTreeMap<Owner, i64>);

impl Waiting {
    pub fn parse(text: &str) -> (Waiting, Vec<Problem>) {
        let mut problems = Vec::new();
        let mut rows = BTreeMap::new();
        match text.parse::<toml::Table>() {
            Ok(table) => {
                for (key, value) in table {
                    match (key.parse::<u32>(), value.as_integer()) {
                        (Ok(task), Some(count)) => {
                            rows.insert(Owner(task), count);
                        }
                        (Err(_), _) => problems.push(Problem::new(
                            corpus_tree::WAITING,
                            format!("the key `{key}` is not a task id"),
                        )),
                        (_, None) => problems.push(Problem::new(
                            corpus_tree::WAITING,
                            format!("the count of `{key}` is not an integer"),
                        )),
                    }
                }
            }
            Err(error) => problems.push(Problem::new(corpus_tree::WAITING, error.to_string())),
        }
        (Waiting(rows), problems)
    }

    /// Each owner whose committed row differs from the count computed:
    /// a missing row, an extra row, a zero row or another number.
    pub fn compare(&self, computed: &BTreeMap<Owner, usize>) -> Vec<Problem> {
        let owners: BTreeSet<Owner> = self.0.keys().chain(computed.keys()).copied().collect();
        owners
            .into_iter()
            .filter_map(|owner| {
                let committed = self.0.get(&owner).copied();
                let counted = computed.get(&owner).map(|&count| count as i64);
                let what = match (committed, counted) {
                    (Some(0), _) => {
                        "a zero row; write only owners with waiting entries".to_string()
                    }
                    (Some(a), Some(b)) if a == b => return None,
                    (Some(a), Some(b)) => format!("committed {a}, computed {b}"),
                    (Some(a), None) => format!("committed {a}, computed 0: delete the row"),
                    (None, Some(b)) => format!("no row, computed {b}"),
                    (None, None) => return None,
                };
                Some(Problem::new(
                    corpus_tree::WAITING,
                    format!("owner {owner}: {what}"),
                ))
            })
            .collect()
    }
}

/// The canonical text of `waiting.toml` for these counts.
pub fn render_waiting(counts: &BTreeMap<Owner, usize>) -> String {
    counts
        .iter()
        .map(|(owner, count)| format!("{} = {count}\n", owner.0))
        .collect()
}

/// `disputed.toml`: the entries a leaf believes wrong, each with the
/// escalation that asks for a ruling. An empty file is no dispute.
pub struct Disputed(Vec<DisputedEntry>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisputedFile {
    #[serde(default)]
    disputed: Vec<DisputedEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisputedEntry {
    entry: EntryId,
    path: String,
    catalogue_answer: String,
    believed_answer: String,
    rests_on: String,
    escalation: String,
}

impl DisputedEntry {
    fn blank_fields(&self) -> Vec<&'static str> {
        [
            ("path", &self.path),
            ("catalogue_answer", &self.catalogue_answer),
            ("believed_answer", &self.believed_answer),
            ("rests_on", &self.rests_on),
            ("escalation", &self.escalation),
        ]
        .into_iter()
        .filter(|(_, text)| text.trim().is_empty())
        .map(|(key, _)| key)
        .collect()
    }
}

impl Disputed {
    pub fn parse(text: &str) -> (Disputed, Vec<Problem>) {
        match toml::from_str::<DisputedFile>(text) {
            Ok(file) => {
                let problems = file
                    .disputed
                    .iter()
                    .filter(|row| !row.blank_fields().is_empty())
                    .map(|row| {
                        Problem::new(
                            corpus_tree::DISPUTED,
                            format!(
                                "the row for `{}` has blank `{}`",
                                row.entry,
                                row.blank_fields().join("`, `")
                            ),
                        )
                    })
                    .collect();
                (Disputed(file.disputed), problems)
            }
            Err(error) => (
                Disputed(Vec::new()),
                vec![Problem::new(corpus_tree::DISPUTED, error.to_string())],
            ),
        }
    }

    pub fn lists(&self, id: &EntryId) -> bool {
        self.0.iter().any(|row| row.entry == *id)
    }

    /// A problem for each row whose entry is not one of `ids`.
    pub fn unknown_entries(&self, ids: &BTreeSet<EntryId>) -> Vec<Problem> {
        self.0
            .iter()
            .filter(|row| !ids.contains(&row.entry))
            .map(|row| {
                Problem::new(
                    corpus_tree::DISPUTED,
                    format!("`{}` names no entry of the corpus", row.entry),
                )
            })
            .collect()
    }
}

/// Whether an entry runs.
pub enum EntryState {
    Ready,
    Disputed,
    /// The words it needs that are not built, by owner.
    Waiting(BTreeMap<Owner, Vec<Need>>),
}

impl EntryState {
    pub fn of(needs: &[Need], built: &Built, disputed: bool) -> EntryState {
        if disputed {
            return EntryState::Disputed;
        }
        let unbuilt = unbuilt_by_owner(needs, built);
        if unbuilt.is_empty() {
            EntryState::Ready
        } else {
            EntryState::Waiting(unbuilt)
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            EntryState::Ready => "ready",
            EntryState::Disputed => "disputed",
            EntryState::Waiting(_) => "waiting",
        }
    }
}

impl fmt::Display for EntryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let EntryState::Waiting(unbuilt) = self else {
            return Ok(());
        };
        let owners: Vec<String> = unbuilt
            .iter()
            .map(|(owner, needs)| {
                let words: Vec<&str> = needs.iter().map(|need| need.word()).collect();
                format!("{owner} ({})", words.join(", "))
            })
            .collect();
        write!(f, "on {}", owners.join(", "))
    }
}

fn unbuilt_by_owner(needs: &[Need], built: &Built) -> BTreeMap<Owner, Vec<Need>> {
    let mut unbuilt: BTreeMap<Owner, Vec<Need>> = BTreeMap::new();
    for &need in needs.iter().filter(|&&need| !built.contains(need)) {
        unbuilt.entry(need.owner()).or_default().push(need);
    }
    unbuilt
}

/// The waiting entries per owner: an entry counts once under every owner
/// of a word it needs that is not built, whether or not it is disputed, so
/// the counts depend on needs and built words alone.
pub fn waiting_counts(entries: &[LoadedEntry], built: &Built) -> BTreeMap<Owner, usize> {
    let mut counts = BTreeMap::new();
    for entry in entries {
        for owner in unbuilt_by_owner(&entry.record.needs, built).into_keys() {
            *counts.entry(owner).or_insert(0) += 1;
        }
    }
    counts
}
