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
/// owner. The enum, `Need::ALL`, `word` and `owner` are all made from it.
macro_rules! vocabulary {
    ($($need:ident = $word:literal, owner $owner:literal;)*) => {
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
        }
    };
}

vocabulary! {
    TwoFiles = "two-files", owner 10634;
    Paths = "paths", owner 10635;
    GrammarPurpose = "grammar-purpose", owner 10637;
    CompileError = "compile-error", owner 10643;
    Instance = "instance", owner 10645;
    EvalReads = "eval-reads", owner 10645;
    Constraint = "constraint", owner 10646;
    Summary = "summary", owner 10646;
    Guard = "guard", owner 10647;
    OneName = "one-name", owner 10648;
    Collection = "collection", owner 10649;
    Value = "value", owner 10650;
    Made = "made", owner 10651;
    Open = "open", owner 10652;
    Braces = "braces", owner 10653;
    Unknown = "unknown", owner 10655;
    SolveForced = "solve-forced", owner 10655;
    SolveInfeasible = "solve-infeasible", owner 10655;
    NestedSolve = "nested-solve", owner 10656;
    Purpose = "purpose", owner 10657;
    Roots = "roots", owner 10658;
    Connect = "connect", owner 10659;
    Generic = "generic", owner 10660;
    StateTests = "state-tests", owner 10661;
    Edits = "edits", owner 10662;
    SolveReal = "solve-real", owner 10610;
    Geometry = "geometry", owner 10611;
    Session = "session", owner 10551;
    IndexedSubs = "indexed-subs", owner 5482;
    RequiredArguments = "required-arguments", owner 7874;
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
