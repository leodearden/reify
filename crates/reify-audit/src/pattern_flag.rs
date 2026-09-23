//! The vocabulary of `reify-audit --pattern`: every detector-selector token
//! the CLI accepts, in `--help` order.
//!
//! A token SELECTS a detector; a [`crate::Pattern`] CLASSIFIES a finding. The
//! two are not one-to-one: the single token `P5` yields four Patterns
//! (`P5PhantomDone`, `P5MetadataFilesGitignored`, `P5TestsAssertEmpty`,
//! `P5LivePathStranded`).
//!
//! Adding a detector means adding its token here AND registering it in the
//! `/audit` skill (`.claude/skills/audit/`), the consumer that runs detectors
//! and routes their findings. The `skill_registration_parity` integration test
//! enforces the second half.

pub const TOKENS: &[&str] = &[
    "P1",
    "P2",
    "P5",
    "PDEAD",
    "PUNTESTED",
    "PLAYER",
    "PTODO",
    "PDSSENTINEL",
    "PDIAG",
    "PDOCCOVER",
    "PDCHECK",
];
