//! What a source unit's compile returns beside its tree: the table of the
//! references the tree reads, and the instances it asks to have made.
//!
//! Plain data. A reference's steps are given as written in the source; the
//! second output turns them into typed paths.

use reify_core::ValueCellId;
use reify_ir::CompiledExpr;

/// The entity of every reference a source unit mints. `$` cannot start an
/// identifier, so it never names a definition.
#[doc(hidden)]
pub const LOCAL_REFERENCE_ENTITY: &str = "$unit";

/// Where a local reference's steps start.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceRoot {
    /// The instance whose definition the compile scope describes.
    Own,
    /// The scope that wrote a sub's braces: `self` and the names that fall
    /// through to it there.
    Writer,
    /// A `let` of the same braces; its name is the first step.
    BracesLet,
    /// A binder of the unit's own expression.
    Binder { id: ValueCellId },
}

/// One step of a reference, as written in the source.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStep {
    Member(String),
    Index(i64),
    Key(String),
}

/// The structural queries an instance answers about its own members.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralQuery {
    Children,
    Members,
    Descendants,
}

impl StructuralQuery {
    const ALL: [StructuralQuery; 3] = [
        StructuralQuery::Children,
        StructuralQuery::Members,
        StructuralQuery::Descendants,
    ];

    pub const NAMES: [&'static str; 3] = [
        StructuralQuery::Children.name(),
        StructuralQuery::Members.name(),
        StructuralQuery::Descendants.name(),
    ];

    pub const fn name(self) -> &'static str {
        match self {
            StructuralQuery::Children => "children",
            StructuralQuery::Members => "members",
            StructuralQuery::Descendants => "descendants",
        }
    }

    pub fn from_member(member: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|query| query.name() == member)
    }
}

/// What a reference reads at the end of its steps.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceRead {
    /// The value there.
    Value,
    /// Whether the thing there is present (`present(...)`).
    Presence,
    /// A structural query of the instance there.
    Query(StructuralQuery),
    /// The unknown an `auto` argument for `parameter` stands for.
    ArgumentUnknown { parameter: String },
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalReference {
    pub id: ValueCellId,
    pub root: ReferenceRoot,
    pub steps: Vec<SourceStep>,
    pub read: ReferenceRead,
}

/// One argument of an instance request. `unknown` is the reference an `auto`
/// argument compiled to.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestArgument {
    pub parameter: String,
    pub unknown: Option<ValueCellId>,
}

/// A request to make an instance of `definition`, keyed by the place of its
/// constructor node: the child indices from the tree's root.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceRequest {
    pub place: Vec<usize>,
    pub definition: String,
    pub arguments: Vec<RequestArgument>,
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct CompiledSourceUnit {
    pub expr: CompiledExpr,
    pub references: Vec<LocalReference>,
    pub instance_requests: Vec<InstanceRequest>,
}
