//! Meaning-free cross-tier primitive tags.
//!
//! This module is the home for small enums and constants that are needed by
//! both AST-tier and IR-tier code inside `reify-types`, and which carry no
//! dependencies on other `reify-types` modules. At task γ this module will
//! move wholesale into `reify-core`.

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
/// dispatcher consumer is tracked separately and the annotation has no runtime
/// effect until that lands.
pub const SOLID_ANNOTATION: &str = "solid";
