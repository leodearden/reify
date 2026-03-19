//! M5 trait-related type definitions.
//!
//! Contains EnumDef, TraitDef, TraitMember, TraitRef, TraitBound, TypeParam, PortDirection.

/// Definition of an enum type with named variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    /// The name of the enum type.
    pub name: String,
    /// The variant names.
    pub variants: Vec<String>,
}

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

impl EnumDef {
    /// Check if this enum contains a variant with the given name.
    pub fn contains_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|v| v == name)
    }
}
