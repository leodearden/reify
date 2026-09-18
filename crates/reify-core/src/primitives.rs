//! Meaning-free cross-tier primitive tags.
//!
//! This module is the home for small enums and constants that are needed by
//! both AST-tier and IR-tier code inside `reify-types`, and which carry no
//! dependencies on other `reify-types` modules. Per PRD
//! `docs/prds/core-ast-ir-layering.md` §5 (the `reify-core` split), this
//! module is scheduled to move wholesale into `reify-core`.

/// Direction of a port in a trait definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortDirection {
    /// Input port.
    In,
    /// Output port.
    Out,
    /// Bidirectional port.
    Bidi,
}

impl PortDirection {
    /// The direction's spelling in `.ri` source — what the parser accepts and
    /// what a diagnostic must quote back for the designer to be able to write
    /// it. Lives here, beside the enum, for the same reason the annotation-name
    /// constants above do: a second copy elsewhere can drift.
    ///
    /// This is the LANGUAGE's vocabulary, not the doc model's, which spells
    /// `Bidi` as `inout` (see `reify_doc::model`).
    pub fn as_str(&self) -> &'static str {
        match self {
            PortDirection::In => "in",
            PortDirection::Out => "out",
            PortDirection::Bidi => "bidi",
        }
    }
}

impl std::fmt::Display for PortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical lowercase spelling of the `@test` annotation name.
///
/// Use this constant instead of hard-coding `"test"` to keep the annotation
/// name as a single source of truth across crates.
pub const TEST_ANNOTATION: &str = "test";

/// Canonical lowercase spelling of the `@deprecated` annotation name.
///
/// Use this constant instead of hard-coding `"deprecated"` to keep the
/// annotation name as a single source of truth across crates.
pub const DEPRECATED_ANNOTATION: &str = "deprecated";

/// Canonical lowercase spelling of the `@optimized` annotation name.
///
/// Use this constant instead of hard-coding `"optimized"` to keep the
/// annotation name as a single source of truth across crates.
pub const OPTIMIZED_ANNOTATION: &str = "optimized";

/// Canonical lowercase spelling of the `@solver_hint` annotation name.
///
/// Use this constant instead of hard-coding `"solver_hint"` to keep the
/// annotation name as a single source of truth across crates.
pub const SOLVER_HINT_ANNOTATION: &str = "solver_hint";

/// Canonical lowercase spelling of the `@shell` annotation name.
///
/// Marks an entity declaration as a thin-walled shell. The optional first
/// argument is a numeric thickness (Length-typed in a future pass); when
/// omitted, downstream consumers (T18 auto-classification dispatcher) are
/// expected to derive thickness from medial-axis analysis.
pub const SHELL_ANNOTATION: &str = "shell";

/// Canonical lowercase spelling of the `@solid` annotation name.
///
/// Marks an entity declaration as a solid body to bypass medial-axis extraction
/// and force tet meshing in the T18 auto-classification dispatcher. The annotation
/// is a bare marker — no arguments are accepted.
///
/// Note: as of this commit only the parse/validate path is wired; the T18
/// dispatcher consumer is wired separately (no runtime effect until the T18 dispatcher wiring lands).
pub const SOLID_ANNOTATION: &str = "solid";

/// Canonical lowercase spelling of the `@display` annotation name.
///
/// Marks a `param`/`let` binding with a preferred display unit label (e.g.
/// `@display("L")`). The single required argument is a string-literal unit
/// label; the compiler validates its shape (arg_check) and, in a separate
/// pass, that the label is a rung in the binding dimension's unit ladder.
///
/// Note: as of this commit only the parse/validate path is wired; the runtime
/// rendering effect is wired separately (no runtime effect yet).
pub const DISPLAY_ANNOTATION: &str = "display";
