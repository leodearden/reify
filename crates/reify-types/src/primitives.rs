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
