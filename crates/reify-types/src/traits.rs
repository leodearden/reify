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
    /// Doc comment extracted from the `///` lines preceding the declaration.
    pub doc: Option<String>,
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

/// A member declaration within a trait definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraitMember {
    /// A parameter with a type and optional default value.
    Param {
        name: String,
        ty: crate::ty::Type,
        default: Option<crate::value::Value>,
    },
    /// A port with direction and type.
    Port {
        name: String,
        ty: crate::ty::Type,
        direction: PortDirection,
    },
    /// A sub-structure reference.
    Sub { name: String, trait_ref: TraitRef },
    /// A let binding with expression.
    Let {
        name: String,
        ty: crate::ty::Type,
        expr: String,
    },
    /// A constraint expression.
    Constraint { expr: String },
    /// An associated type declaration.
    AssociatedType {
        name: String,
        default: Option<crate::ty::Type>,
    },
}

/// A complete trait definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDef {
    /// The name of the trait.
    pub name: String,
    /// Doc comment from the `///` lines preceding the trait declaration.
    ///
    /// **Note:** `reify_types::TraitDef` has no production construction site in
    /// the compiler pipeline — the compiler lowers trait declarations into
    /// [`reify_compiler::CompiledTrait`], not into `TraitDef`.  This field
    /// therefore carries `None` in any real compilation run and exists solely
    /// for forward compatibility / tooling that constructs `TraitDef` directly
    /// (e.g. `reify-types` test fixtures).  The authoritative runtime
    /// observable for doc-comment propagation is `CompiledTrait::doc`.
    pub doc: Option<String>,
    /// Type parameters on the trait.
    pub type_params: Vec<TypeParam>,
    /// Names of traits this refines (supertrait names).
    pub refinements: Vec<String>,
    /// Members declared in this trait.
    pub members: Vec<TraitMember>,
}

impl EnumDef {
    /// Check if this enum contains a variant with the given name.
    pub fn contains_variant(&self, name: &str) -> bool {
        self.variants.iter().any(|v| v == name)
    }
}
