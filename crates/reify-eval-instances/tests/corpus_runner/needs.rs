//! The fixed vocabulary of what an entry needs, and the one task that builds
//! each word (docs/prds/instances-on-the-core.md section 5.8).

use std::fmt;

use serde::Deserialize;

/// A task id: the task that builds a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(transparent)]
pub struct Owner(pub u32);

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// One row per word: the variant, the word as an entry spells it, and its
/// owner, a `leaf` of docs/prds/instances-on-the-core.md or a `task` outside
/// it. The enum, `Need::ALL`, `word`, `owner` and `owned_by_this_prd` are all
/// made from it.
macro_rules! vocabulary {
    ($($need:ident = $word:literal, $kind:ident $owner:literal;)*) => {
        /// A word of the vocabulary. An entry spelling any other word fails
        /// to load, naming the word.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[serde(try_from = "String")]
        pub enum Need {
            $($need,)*
        }

        impl Need {
            pub const ALL: &'static [Need] = &[$(Need::$need,)*];

            pub fn word(self) -> &'static str {
                match self {
                    $(Need::$need => $word,)*
                }
            }

            pub fn owner(self) -> Owner {
                match self {
                    $(Need::$need => Owner($owner),)*
                }
            }

            /// Whether a leaf of this PRD builds the word.
            pub fn owned_by_this_prd(self) -> bool {
                match self {
                    $(Need::$need => owned_by_this_prd!($kind),)*
                }
            }
        }
    };
}

/// The ownership column of one `vocabulary!` row.
macro_rules! owned_by_this_prd {
    (leaf) => {
        true
    };
    (task) => {
        false
    };
}

vocabulary! {
    TwoFiles = "two-files", leaf 10634;
    Paths = "paths", leaf 10635;
    GrammarPurpose = "grammar-purpose", leaf 10637;
    CompileError = "compile-error", leaf 10643;
    Instance = "instance", leaf 10645;
    EvalReads = "eval-reads", leaf 10645;
    Constraint = "constraint", leaf 10646;
    Summary = "summary", leaf 10646;
    Guard = "guard", leaf 10647;
    OneName = "one-name", leaf 10648;
    Collection = "collection", leaf 10649;
    Value = "value", leaf 10650;
    Made = "made", leaf 10651;
    Open = "open", leaf 10652;
    Braces = "braces", leaf 10653;
    Unknown = "unknown", leaf 10655;
    SolveForced = "solve-forced", leaf 10655;
    SolveInfeasible = "solve-infeasible", leaf 10655;
    NestedSolve = "nested-solve", leaf 10656;
    Purpose = "purpose", leaf 10657;
    Roots = "roots", leaf 10658;
    Connect = "connect", leaf 10659;
    Generic = "generic", leaf 10660;
    StateTests = "state-tests", leaf 10661;
    Edits = "edits", leaf 10662;
    SolveReal = "solve-real", task 10610;
    Geometry = "geometry", task 10611;
    Session = "session", task 10551;
    IndexedSubs = "indexed-subs", task 5482;
    RequiredArguments = "required-arguments", task 7874;
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

impl TryFrom<String> for Need {
    type Error = String;

    fn try_from(word: String) -> Result<Self, String> {
        Need::ALL
            .iter()
            .copied()
            .find(|need| need.word() == word)
            .ok_or_else(|| {
                let words: Vec<&str> = Need::ALL.iter().map(|need| need.word()).collect();
                format!(
                    "unknown need `{word}`, expected one of `{}`",
                    words.join("`, `")
                )
            })
    }
}
