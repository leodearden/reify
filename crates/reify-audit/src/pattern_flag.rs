//! The vocabulary of `reify-audit --pattern`: every detector-selector token
//! the CLI accepts, in `--help` order. Code that selects one detector names
//! its token's constant, so a misspelt token fails to compile instead of
//! selecting nothing.
//!
//! A token SELECTS a detector; a [`crate::Pattern`] CLASSIFIES a finding. The
//! two are not one-to-one: the single token `P5` selects a detector that
//! emits several Patterns, `P5PhantomDone` among them.
//!
//! Adding a detector means adding its token here, giving it a dispatch row in
//! the `reify-audit` binary, AND registering it in the `/audit` skill
//! (`.claude/skills/audit/`), the consumer that runs detectors and routes
//! their findings. A test in the binary holds its dispatch rows to [`TOKENS`];
//! the `skill_registration_parity` integration test enforces the skill half.

pub const P1: &str = "P1";
pub const P2: &str = "P2";
pub const P5: &str = "P5";
pub const PDEAD: &str = "PDEAD";
pub const PUNTESTED: &str = "PUNTESTED";
pub const PLAYER: &str = "PLAYER";
pub const PTODO: &str = "PTODO";
pub const PDSSENTINEL: &str = "PDSSENTINEL";
pub const PDIAG: &str = "PDIAG";
pub const PDOCCOVER: &str = "PDOCCOVER";
pub const PDCHECK: &str = "PDCHECK";
pub const PPRDSTATUS: &str = "PPRDSTATUS";

pub const TOKENS: &[&str] = &[
    P1,
    P2,
    P5,
    PDEAD,
    PUNTESTED,
    PLAYER,
    PTODO,
    PDSSENTINEL,
    PDIAG,
    PDOCCOVER,
    PDCHECK,
    PPRDSTATUS,
];
