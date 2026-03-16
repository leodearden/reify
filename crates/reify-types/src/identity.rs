use std::fmt;

/// Path to a module in the project (e.g., "bracket" or "lib/fasteners/bolt").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn new(segments: Vec<String>) -> Self {
        Self(segments)
    }

    pub fn single(name: impl Into<String>) -> Self {
        Self(vec![name.into()])
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.join("/"))
    }
}

/// Path to a named entity within a module (e.g., "Bracket").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityPath {
    pub module: ModulePath,
    pub entity: String,
}

impl fmt::Display for EntityPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.entity)
    }
}

/// Name of a member within an entity (param, let, constraint, sub).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberName(pub String);

impl MemberName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for MemberName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies a value cell in the topology graph.
/// A value cell corresponds to a single param, let binding, or computed property.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueCellId {
    pub entity: String,
    pub member: String,
}

impl ValueCellId {
    pub fn new(entity: impl Into<String>, member: impl Into<String>) -> Self {
        Self {
            entity: entity.into(),
            member: member.into(),
        }
    }
}

impl fmt::Display for ValueCellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.entity, self.member)
    }
}

/// Identifies a constraint node in the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConstraintNodeId {
    pub entity: String,
    pub index: u32,
}

impl ConstraintNodeId {
    pub fn new(entity: impl Into<String>, index: u32) -> Self {
        Self {
            entity: entity.into(),
            index,
        }
    }
}

impl fmt::Display for ConstraintNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#constraint[{}]", self.entity, self.index)
    }
}

/// Identifies a realization node (geometry output) in the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RealizationNodeId {
    pub entity: String,
    pub index: u32,
}

impl RealizationNodeId {
    pub fn new(entity: impl Into<String>, index: u32) -> Self {
        Self {
            entity: entity.into(),
            index,
        }
    }
}

/// Identifies a source node (input from the parser/file).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceNodeId {
    pub module: ModulePath,
    pub declaration_index: u32,
}

/// Identifies a lexical scope for name resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);
