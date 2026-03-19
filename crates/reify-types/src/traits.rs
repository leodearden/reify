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

/// A reference to a trait, optionally with type arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitRef {
    /// The name of the referenced trait.
    pub name: String,
    /// Type arguments applied to the trait.
    pub type_args: Vec<crate::ty::Type>,
}

/// A type parameter with optional bounds and default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParam {
    /// The name of the type parameter.
    pub name: String,
    /// Trait bounds on this type parameter.
    pub bounds: Vec<TraitBound>,
    /// Optional default type.
    pub default: Option<crate::ty::Type>,
}

/// A trait bound constraining a type parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitBound {
    /// The trait reference this bound requires.
    pub trait_ref: TraitRef,
}

impl EnumDef {
    /// Check if this enum contains a variant with the given name.
    pub fn contains_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|v| v == name)
    }
}
