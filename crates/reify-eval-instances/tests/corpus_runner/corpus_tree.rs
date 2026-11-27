//! The corpus as files. The root holds the three bookkeeping files and one
//! directory per theme; a theme holds, for each entry, `<name>.toml` beside
//! its source: `<name>.ri`, or for a multi-file entry the directory
//! `<name>/`, which is its import root and holds the file its `entry_file`
//! names.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::entry::EntryRecord;

pub const BUILT: &str = "built.toml";
pub const WAITING: &str = "waiting.toml";
pub const DISPUTED: &str = "disputed.toml";

const BOOKKEEPING: [&str; 3] = [BUILT, DISPUTED, WAITING];

/// The themes of the catalogue, one directory each, sorted.
const THEMES: [&str; 18] = [
    "braces",
    "collections",
    "constraints-and-summary",
    "determinacy-tests",
    "edits",
    "evaluator-reads",
    "guards-and-presence",
    "instances-and-arguments",
    "made-instances",
    "one-name-several-guards",
    "other",
    "paths",
    "purposes",
    "roots",
    "trial-infeasibility",
    "unknowns",
    "values",
    "waiting-on-geometry",
];

fn theme_problem(theme: &str) -> Option<String> {
    (!THEMES.contains(&theme)).then(|| {
        format!(
            "`{theme}` is not a theme of the corpus, one of `{}`",
            THEMES.join("`, `")
        )
    })
}

/// `tests/corpus/` of this crate, in the tree the running test belongs to.
pub fn corpus_root() -> PathBuf {
    reify_test_support::workspace_root::workspace_root()
        .join("crates/reify-eval-instances/tests/corpus")
}

/// An entry by theme and name, written `<theme>/<name>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct EntryId {
    pub theme: String,
    pub name: String,
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.theme, self.name)
    }
}

impl TryFrom<String> for EntryId {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let Some((theme, name)) = text.split_once('/') else {
            return Err(format!("`{text}` is not `<theme>/<name>`"));
        };
        if name.is_empty() || name.contains('/') {
            return Err(format!("`{text}` is not `<theme>/<name>`"));
        }
        if let Some(problem) = theme_problem(theme) {
            return Err(problem);
        }
        Ok(EntryId {
            theme: theme.to_string(),
            name: name.to_string(),
        })
    }
}

/// What is wrong, and with what: an entry, or a file of the corpus.
#[derive(Debug)]
pub struct Problem {
    subject: String,
    what: String,
}

impl Problem {
    pub fn new(subject: impl fmt::Display, what: impl Into<String>) -> Problem {
        Problem {
            subject: subject.to_string(),
            what: what.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.subject, self.what)
    }
}

#[derive(Debug)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

#[derive(Debug)]
pub struct EntrySources {
    pub files: Vec<SourceFile>,
    /// The directory an `import` in a source resolves against.
    pub import_root: PathBuf,
}

#[derive(Debug)]
pub struct LoadedEntry {
    pub id: EntryId,
    pub record: EntryRecord,
    pub sources: EntrySources,
}

/// Every entry under `root`, in theme then name order, and every problem
/// met on the way. Nothing is skipped and nothing panics: an entry that
/// cannot be loaded is a problem naming it.
pub fn load_corpus(root: &Path) -> (Vec<LoadedEntry>, Vec<Problem>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let listing = match sorted_listing(root) {
        Ok(listing) => listing,
        Err(error) => {
            problems.push(Problem::new(root.display(), error));
            return (entries, problems);
        }
    };
    for (name, path) in listing {
        if path.is_dir() {
            problems.extend(theme_problem(&name).map(|what| Problem::new(&name, what)));
            let (theme_entries, theme_problems) = load_theme(&name, &path);
            entries.extend(theme_entries);
            problems.extend(theme_problems);
        } else if !BOOKKEEPING.contains(&name.as_str()) {
            problems.push(Problem::new(
                &name,
                "the corpus root holds only the bookkeeping files and theme directories",
            ));
        }
    }
    (entries, problems)
}

fn load_theme(theme: &str, dir: &Path) -> (Vec<LoadedEntry>, Vec<Problem>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let listing = match sorted_listing(dir) {
        Ok(listing) => listing,
        Err(error) => {
            problems.push(Problem::new(theme, error));
            return (entries, problems);
        }
    };
    let has_toml = |stem: &str| {
        listing
            .iter()
            .any(|(file, _)| *file == format!("{stem}.toml"))
    };
    for (file, path) in &listing {
        let source_stem = if path.is_dir() {
            Some(file.as_str())
        } else {
            file.strip_suffix(".ri")
        };
        if let Some(stem) = source_stem {
            if !has_toml(stem) {
                problems.push(Problem::new(
                    format!("{theme}/{file}"),
                    format!("belongs to no entry: there is no {stem}.toml"),
                ));
            }
        } else if let Some(name) = file.strip_suffix(".toml") {
            let id = EntryId {
                theme: theme.to_string(),
                name: name.to_string(),
            };
            match load_entry(&id, dir) {
                Ok(entry) => entries.push(entry),
                Err(what) => problems.push(Problem::new(&id, what)),
            }
        } else {
            problems.push(Problem::new(
                format!("{theme}/{file}"),
                "a theme holds only entries' .toml and .ri files and directories",
            ));
        }
    }
    (entries, problems)
}

fn load_entry(id: &EntryId, theme_dir: &Path) -> Result<LoadedEntry, String> {
    let toml_path = theme_dir.join(format!("{}.toml", id.name));
    let text = fs::read_to_string(&toml_path).map_err(|error| format!("unreadable: {error}"))?;
    let record = EntryRecord::from_toml(&text).map_err(|error| error.to_string())?;
    let single = theme_dir.join(format!("{}.ri", id.name));
    let directory = theme_dir.join(&id.name);
    let sources = match (&record.entry_file, single.is_file(), directory.is_dir()) {
        (None, true, false) => EntrySources {
            files: vec![read_source(&single)?],
            import_root: theme_dir.to_path_buf(),
        },
        (Some(entry_file), false, true) => multi_file_sources(&directory, entry_file)?,
        (None, _, true) => {
            return Err(format!("has a directory {}/ but no `entry_file`", id.name));
        }
        (Some(_), _, false) => {
            return Err(format!(
                "names an `entry_file` but has no directory {}/",
                id.name
            ));
        }
        (_, true, true) => return Err(format!("has both {0}.ri and {0}/", id.name)),
        (None, false, false) => return Err(format!("has no source {}.ri", id.name)),
    };
    Ok(LoadedEntry {
        id: id.clone(),
        record,
        sources,
    })
}

fn multi_file_sources(directory: &Path, entry_file: &str) -> Result<EntrySources, String> {
    let mut files = Vec::new();
    for (name, path) in sorted_listing(directory)? {
        if path.is_dir() || !name.ends_with(".ri") {
            return Err(format!(
                "its directory holds `{name}`; it may hold only .ri files"
            ));
        }
        files.push(read_source(&path)?);
    }
    if !files.iter().any(|file| file.name == entry_file) {
        return Err(format!(
            "its `entry_file` {entry_file} is not in its directory"
        ));
    }
    Ok(EntrySources {
        files,
        import_root: directory.to_path_buf(),
    })
}

fn read_source(path: &Path) -> Result<SourceFile, String> {
    let name = file_name(path);
    let text =
        fs::read_to_string(path).map_err(|error| format!("{name} is unreadable: {error}"))?;
    Ok(SourceFile { name, text })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A directory's members as (file name, path), sorted by name.
fn sorted_listing(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let read = fs::read_dir(dir).map_err(|error| format!("unreadable directory: {error}"))?;
    let mut listing = Vec::new();
    for member in read {
        let path = member
            .map_err(|error| format!("unreadable directory: {error}"))?
            .path();
        listing.push((file_name(&path), path));
    }
    listing.sort();
    Ok(listing)
}

#[test]
fn an_entry_id_is_a_theme_of_the_corpus_and_a_name() {
    let parsed = EntryId::try_from("roots/constructor-call-counts-as-use".to_string());
    assert_eq!(
        parsed,
        Ok(EntryId {
            theme: "roots".to_string(),
            name: "constructor-call-counts-as-use".to_string(),
        })
    );
    for (text, what) in [
        ("roots", "is not `<theme>/<name>`"),
        ("roots/", "is not `<theme>/<name>`"),
        ("roots/a/b", "is not `<theme>/<name>`"),
        ("root/a", "`root` is not a theme of the corpus"),
    ] {
        match EntryId::try_from(text.to_string()) {
            Ok(id) => panic!("`{text}` must be refused, but parsed as {id}"),
            Err(error) => assert!(
                error.contains(what),
                "the refusal of `{text}` must say {what:?}, got: {error}"
            ),
        }
    }
}
