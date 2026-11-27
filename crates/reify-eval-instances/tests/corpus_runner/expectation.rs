//! One `[[expect]]` row: what the host is answered at one path, in exactly
//! one of the forms of docs/prds/instances-on-the-core.md section 5.8.

use reify_core::InstancePath;
use serde::Deserialize;

use crate::expected_value::ExpectedValue;
use crate::needs::Owner;

#[derive(Debug, Deserialize)]
#[serde(try_from = "RawExpectation")]
pub struct Expectation {
    #[expect(dead_code, reason = "read once entries run")]
    pub path: Option<InstancePath>, // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
    pub after_edit: Option<u32>,
    #[expect(dead_code, reason = "read once entries run")]
    pub form: Form, // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
}

#[derive(Debug)]
#[expect(dead_code, reason = "read once entries run")] // debt: ticket tkt_0RXF3RZ3T23TSSXGWCWN0S20BF
pub enum Form {
    Value(ExpectedValue),
    Verdict(Verdict),
    Summary(SummaryCounts),
    Presence(Presence),
    Failure(FailureCode),
    UndeterminedBecause(UndeterminedClass),
    SameAs(InstancePath),
    Open {
        owner: Option<Owner>,
    },
    Roots(Vec<String>),
    Compile,
    CompileError {
        message: String,
        code: Option<String>,
    },
}

impl Form {
    /// Whether the form is about a path. The file's roots and its compile
    /// outcome are about the whole entry.
    fn takes_path(&self) -> bool {
        !matches!(
            self,
            Form::Roots(_) | Form::Compile | Form::CompileError { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Satisfied,
    Violated,
    Undetermined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    Present,
    Absent,
    Undetermined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureCode {
    Cycle,
    Blocked,
    Limit,
    AmbiguousDeclaration,
    TypeArgumentUnresolved,
    NoKindSupplied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UndeterminedClass {
    Partial,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryCounts {
    pub violated: u32,
    pub undetermined: u32,
    pub failed: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenRow {
    owner: Option<Owner>,
}

/// The one value `compile` takes: the source is accepted.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Accepted {
    Ok,
}

/// A row as written: every form key optional, so the one-form rule is
/// checked here and reported in words.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpectation {
    #[serde(default, deserialize_with = "crate::path_text::optional_path")]
    path: Option<InstancePath>,
    after_edit: Option<u32>,
    value: Option<ExpectedValue>,
    verdict: Option<Verdict>,
    summary: Option<SummaryCounts>,
    presence: Option<Presence>,
    failure: Option<FailureCode>,
    undetermined_because: Option<UndeterminedClass>,
    #[serde(default, deserialize_with = "crate::path_text::optional_path")]
    same_as: Option<InstancePath>,
    open: Option<OpenRow>,
    roots: Option<Vec<String>>,
    compile: Option<Accepted>,
    compile_error: Option<String>,
    code: Option<String>,
}

impl TryFrom<RawExpectation> for Expectation {
    type Error = String;

    fn try_from(raw: RawExpectation) -> Result<Self, String> {
        if raw.code.is_some() && raw.compile_error.is_none() {
            return Err("`code` is allowed only beside `compile_error`".to_string());
        }
        let code = raw.code;
        let candidates = [
            ("value", raw.value.map(Form::Value)),
            ("verdict", raw.verdict.map(Form::Verdict)),
            ("summary", raw.summary.map(Form::Summary)),
            ("presence", raw.presence.map(Form::Presence)),
            ("failure", raw.failure.map(Form::Failure)),
            (
                "undetermined_because",
                raw.undetermined_because.map(Form::UndeterminedBecause),
            ),
            ("same_as", raw.same_as.map(Form::SameAs)),
            (
                "open",
                raw.open.map(|open| Form::Open { owner: open.owner }),
            ),
            ("roots", raw.roots.map(Form::Roots)),
            ("compile", raw.compile.map(|Accepted::Ok| Form::Compile)),
            (
                "compile_error",
                raw.compile_error
                    .map(|message| Form::CompileError { message, code }),
            ),
        ];
        let keys: Vec<&str> = candidates.iter().map(|(key, _)| *key).collect();
        let mut present: Vec<(&str, Form)> = candidates
            .into_iter()
            .filter_map(|(key, form)| form.map(|form| (key, form)))
            .collect();
        let (key, form) = match present.len() {
            1 => present.remove(0),
            0 => {
                return Err(format!(
                    "an [[expect]] row needs exactly one form, one of `{}`; it has none",
                    keys.join("`, `")
                ));
            }
            _ => {
                let found: Vec<&str> = present.iter().map(|(key, _)| *key).collect();
                return Err(format!(
                    "an [[expect]] row needs exactly one form; it has `{}`",
                    found.join("`, `")
                ));
            }
        };
        match (form.takes_path(), &raw.path) {
            (true, None) => return Err(format!("the `{key}` form needs a `path`")),
            (false, Some(path)) => {
                return Err(format!("the `{key}` form takes no `path`, got `{path}`"));
            }
            _ => {}
        }
        Ok(Expectation {
            path: raw.path,
            after_edit: raw.after_edit,
            form,
        })
    }
}
