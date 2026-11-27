//! The old-core hold list: its file format, the invariants a usable list
//! meets, and the rule that decides whether a task's path is held.
//!
//! The list's one home is [`HOLD_LIST_PATH`]. What the hold is for, and how a
//! task is held or released, is in `docs/notes/old-evaluation-core-hold.md`.

use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

/// Repo-relative path of the hold list.
pub(crate) const HOLD_LIST_PATH: &str = "docs/notes/old-evaluation-core-hold.toml";

/// Characters a held path may not contain: entries are matched literally.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']', '{', '}'];

/// The list as written on disk, before any invariant is checked.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HoldListFile {
    holder: u64,
    files: Vec<String>,
    #[serde(default)]
    registration_only: Vec<String>,
}

/// One held entry. Written with a trailing `/`, it is a directory.
#[derive(Debug)]
enum HeldPath {
    File(String),
    /// Stored without its trailing `/`.
    Dir(String),
}

impl HeldPath {
    fn from_entry(entry: &str) -> Self {
        match entry.strip_suffix('/') {
            Some(dir) => HeldPath::Dir(dir.to_string()),
            None => HeldPath::File(entry.to_string()),
        }
    }

    /// A file holds exactly itself; a directory holds itself and every path
    /// below it, never a sibling that merely shares its name as a prefix.
    fn holds(&self, path: &str) -> bool {
        match self {
            HeldPath::File(file) => path == file,
            HeldPath::Dir(dir) => path
                .strip_prefix(dir.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
        }
    }
}

/// A hold list that meets every invariant. Built only by [`HoldList::parse`].
#[derive(Debug)]
pub(crate) struct HoldList {
    holder: String,
    files: Vec<HeldPath>,
    registration_only: Vec<String>,
}

impl HoldList {
    /// Parses `text` and checks every invariant, so a `HoldList` that exists
    /// is a usable one.
    pub(crate) fn parse(text: &str) -> Result<HoldList, HoldListError> {
        let file: HoldListFile =
            toml::from_str(text).map_err(|e| HoldListError::from_toml(text, &e))?;
        if file.holder == 0 {
            return Err(HoldListError::HolderNotPositive(file.holder));
        }
        if file.files.is_empty() {
            return Err(HoldListError::NoFiles);
        }
        let mut listed = HashSet::new();
        for entry in &file.files {
            check_entry_form(entry)?;
            if !listed.insert(entry.as_str()) {
                return Err(HoldListError::Duplicate(entry.clone()));
            }
        }
        for entry in &file.registration_only {
            if entry.ends_with('/') {
                return Err(HoldListError::RegistrationOnlyDirectory(entry.clone()));
            }
            if !listed.contains(entry.as_str()) {
                return Err(HoldListError::RegistrationOnlyNotListed(entry.clone()));
            }
        }
        Ok(HoldList {
            holder: file.holder.to_string(),
            files: file.files.iter().map(|e| HeldPath::from_entry(e)).collect(),
            registration_only: file.registration_only,
        })
    }

    /// The id of the task whose dependency edge holds a task.
    pub(crate) fn holder_id(&self) -> &str {
        &self.holder
    }

    /// How many of `paths` are not test paths, and which of those are held.
    /// A path is classified before its trailing `/` is stripped, so a test
    /// directory such as `src/engine_build/tests/` stays a test path.
    pub(crate) fn held_share_of<'a>(&self, paths: &'a [String]) -> HeldShare<'a> {
        let non_test: Vec<&str> = paths
            .iter()
            .map(|p| without_leading_dot_slash(p))
            .filter(|p| !crate::is_test_path(p))
            .collect();
        let held = non_test
            .iter()
            .map(|p| normalise(p))
            .filter(|p| self.holds(p))
            .collect();
        HeldShare {
            held,
            non_test: non_test.len(),
        }
    }

    /// `path` is a held file whose only old-core change is registering a
    /// module (lib.rs).
    pub(crate) fn is_registration_only(&self, path: &str) -> bool {
        let path = normalise(path);
        self.registration_only.iter().any(|entry| entry == path)
    }

    fn holds(&self, path: &str) -> bool {
        self.files.iter().any(|entry| entry.holds(path))
    }
}

/// A task's non-test files, counted, and the held ones among them.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HeldShare<'a> {
    /// Normalised (a leading `./` and one trailing `/` stripped), in order.
    pub(crate) held: Vec<&'a str>,
    pub(crate) non_test: usize,
}

fn without_leading_dot_slash(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
}

fn normalise(path: &str) -> &str {
    let path = without_leading_dot_slash(path);
    path.strip_suffix('/').unwrap_or(path)
}

/// An entry is repo-relative, normalised and literal.
fn check_entry_form(entry: &str) -> Result<(), HoldListError> {
    if entry.starts_with('/') {
        return Err(HoldListError::Absolute(entry.to_string()));
    }
    if entry.contains(GLOB_METACHARACTERS) {
        return Err(HoldListError::Glob(entry.to_string()));
    }
    let body = entry.strip_suffix('/').unwrap_or(entry);
    for segment in body.split('/') {
        let fault = match segment {
            "" => HoldListError::EmptySegment,
            "." => HoldListError::DotSegment,
            ".." => HoldListError::ParentSegment,
            _ => continue,
        };
        return Err(fault(entry.to_string()));
    }
    Ok(())
}

/// Why a hold list is unusable: one variant per invariant, each carrying the
/// offending value.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HoldListError {
    /// Not TOML, or not this schema (a missing, mistyped or unknown key).
    Toml {
        line: Option<usize>,
        message: String,
    },
    HolderNotPositive(u64),
    NoFiles,
    Absolute(String),
    DotSegment(String),
    ParentSegment(String),
    EmptySegment(String),
    Glob(String),
    Duplicate(String),
    RegistrationOnlyDirectory(String),
    RegistrationOnlyNotListed(String),
}

impl HoldListError {
    fn from_toml(text: &str, error: &toml::de::Error) -> Self {
        let line = error
            .span()
            .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
        HoldListError::Toml {
            line,
            message: error.message().trim().to_string(),
        }
    }
}

impl fmt::Display for HoldListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use HoldListError::*;
        match self {
            Toml {
                line: Some(line),
                message,
            } => write!(
                f,
                "not valid TOML for the hold-list schema at line {line}: {message}"
            ),
            Toml {
                line: None,
                message,
            } => {
                write!(f, "not valid TOML for the hold-list schema: {message}")
            }
            HolderNotPositive(holder) => {
                write!(f, "holder must be a positive task id, got {holder}")
            }
            NoFiles => write!(
                f,
                "files is empty; the list must name at least one held path"
            ),
            Absolute(p) => write!(f, "held path `{p}` is absolute; entries are repo-relative"),
            DotSegment(p) => write!(
                f,
                "held path `{p}` has a `.` segment; entries are normalised"
            ),
            ParentSegment(p) => {
                write!(
                    f,
                    "held path `{p}` has a `..` segment; entries are normalised"
                )
            }
            EmptySegment(p) => {
                write!(
                    f,
                    "held path `{p}` has an empty segment; entries are normalised"
                )
            }
            Glob(p) => write!(
                f,
                "held path `{p}` contains a glob metacharacter; entries are matched literally"
            ),
            Duplicate(p) => write!(f, "held path `{p}` is listed twice; entries are unique"),
            RegistrationOnlyDirectory(p) => write!(
                f,
                "registration_only entry `{p}` is a directory; only a file entry can be registration-only"
            ),
            RegistrationOnlyNotListed(p) => {
                write!(f, "registration_only entry `{p}` is not in files")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
holder = 10510
files = [
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_edit.rs",
    "crates/reify-eval/src/engine_edit/",
    "crates/reify-eval/src/engine_build.rs",
    "crates/reify-eval/src/engine_build/",
    "crates/reify-eval/src/lib.rs",
]
registration_only = ["crates/reify-eval/src/lib.rs"]
"#;

    fn valid() -> HoldList {
        HoldList::parse(VALID).expect("the valid fixture parses")
    }

    fn holds(list: &HoldList, path: &str) -> bool {
        !list.held_share_of(&[path.to_string()]).held.is_empty()
    }

    #[test]
    fn a_valid_list_parses_and_reads_back_its_holder() {
        assert_eq!(valid().holder_id(), "10510");
    }

    #[test]
    fn a_file_entry_holds_by_equality_and_a_dir_entry_holds_itself_and_below() {
        let list = valid();
        for held in [
            "crates/reify-eval/src/engine_eval.rs",
            "crates/reify-eval/src/engine_build/stage.rs",
            "crates/reify-eval/src/engine_build/",
            "./crates/reify-eval/src/engine_eval.rs",
        ] {
            assert!(holds(&list, held), "{held} is held");
        }
    }

    #[test]
    fn no_prefix_of_a_crate_module_or_directory_name_is_held() {
        let list = valid();
        for not_held in [
            "crates/reify-evalcore/src/engine_eval.rs",
            "crates/reify-eval/src/engine_editor.rs",
            "crates/reify-eval/src/engine_build_util.rs",
            "crates/reify-eval/src",
        ] {
            assert!(!holds(&list, not_held), "{not_held} is not held");
        }
    }

    #[test]
    fn held_share_of_counts_the_non_test_paths_and_returns_the_held_ones_normalised_in_order() {
        let paths = [
            "./crates/reify-eval/src/engine_eval.rs",
            "crates/reify-gui/src/x.rs",
            "crates/reify-eval/src/engine_build/tests/stage.rs",
            "crates/reify-eval/src/engine_build/tests/",
            "crates/reify-eval/src/engine_build/",
        ]
        .map(String::from);
        assert_eq!(
            valid().held_share_of(&paths),
            HeldShare {
                held: vec![
                    "crates/reify-eval/src/engine_eval.rs",
                    "crates/reify-eval/src/engine_build",
                ],
                non_test: 3,
            },
        );
    }

    #[test]
    fn only_a_registration_only_entry_reads_as_registration_only() {
        let list = valid();
        assert!(list.is_registration_only("crates/reify-eval/src/lib.rs"));
        assert!(!list.is_registration_only("crates/reify-eval/src/engine_eval.rs"));
    }

    /// A list over `files` with no registration-only entries.
    fn with_files(files: &str) -> String {
        format!("holder = 10510\nfiles = [{files}]\n")
    }

    #[test]
    fn every_invariant_violation_names_the_invariant_and_the_offending_value() {
        let cases = [
            (
                "malformed TOML",
                "holder = \n".to_string(),
                "TOML",
                "line 1",
            ),
            (
                "an unknown key",
                format!("{}extra = 1\n", with_files(r#""a.rs""#)),
                "unknown field",
                "extra",
            ),
            (
                "holder 0",
                "holder = 0\nfiles = [\"a.rs\"]\n".to_string(),
                "positive",
                "0",
            ),
            ("empty files", with_files(""), "empty", "files"),
            (
                "an absolute entry",
                with_files(r#""/crates/x.rs""#),
                "absolute",
                "/crates/x.rs",
            ),
            (
                "a ./ prefix",
                with_files(r#""./crates/x.rs""#),
                "`.` segment",
                "./crates/x.rs",
            ),
            (
                "a .. segment",
                with_files(r#""crates/../x.rs""#),
                "`..` segment",
                "crates/../x.rs",
            ),
            (
                "an empty segment",
                with_files(r#""crates//x.rs""#),
                "empty segment",
                "crates//x.rs",
            ),
            (
                "a glob",
                with_files(r#""crates/reify-eval/src/*.rs""#),
                "glob",
                "crates/reify-eval/src/*.rs",
            ),
            (
                "a duplicate",
                with_files(r#""crates/x.rs", "crates/x.rs""#),
                "listed twice",
                "crates/x.rs",
            ),
            (
                "a registration_only entry absent from files",
                format!(
                    "{}registration_only = [\"crates/y.rs\"]\n",
                    with_files(r#""crates/x.rs""#)
                ),
                "not in files",
                "crates/y.rs",
            ),
            (
                "a registration_only directory",
                format!(
                    "{}registration_only = [\"crates/d/\"]\n",
                    with_files(r#""crates/d/""#)
                ),
                "directory",
                "crates/d/",
            ),
        ];
        for (label, text, invariant, offending) in cases {
            let message = match HoldList::parse(&text) {
                Ok(_) => panic!("{label}: parsed, but must be refused:\n{text}"),
                Err(e) => e.to_string(),
            };
            assert!(
                message.contains(invariant) && message.contains(offending),
                "{label}: `{message}` must name the invariant ({invariant}) and the offending value ({offending})",
            );
        }
    }
}
