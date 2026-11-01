use std::fmt;

/// Error returned by [`ModulePath::from_dotted`] when the input is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModulePathParseError {
    /// The input string was empty.
    Empty,
    /// The input contained an empty segment (e.g. `"a..b"`, `".leading"`, or `"trailing."`).
    EmptySegment { input: String },
}

impl fmt::Display for ModulePathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModulePathParseError::Empty => {
                write!(f, "module path must not be empty")
            }
            ModulePathParseError::EmptySegment { input } => {
                write!(
                    f,
                    "module path must not contain empty segments (e.g. 'a..b'), got: '{}'",
                    input
                )
            }
        }
    }
}

impl std::error::Error for ModulePathParseError {}

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

    /// Parse a dot-separated module path string into a `ModulePath`.
    ///
    /// Each segment between dots becomes one element of the path vector:
    /// - `"std.units"` → `["std", "units"]`
    /// - `"a.b.c"` → `["a", "b", "c"]`
    /// - `"foo"` → `["foo"]`
    ///
    /// # Errors
    ///
    /// Returns [`ModulePathParseError::Empty`] if `dotted` is an empty string.
    ///
    /// Returns [`ModulePathParseError::EmptySegment`] if `dotted` contains any
    /// empty segment, i.e. it starts or ends with a dot, or contains two
    /// consecutive dots (e.g. `"a..b"`, `".leading"`, `"trailing."`).
    pub fn from_dotted(dotted: &str) -> Result<Self, ModulePathParseError> {
        if dotted.is_empty() {
            return Err(ModulePathParseError::Empty);
        }
        if dotted.split('.').any(str::is_empty) {
            return Err(ModulePathParseError::EmptySegment {
                input: dotted.to_string(),
            });
        }
        Ok(Self::new(dotted.split('.').map(String::from).collect()))
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

/// Entity prefix used for field declarations in the value cell namespace.
/// Fields are top-level declarations (not structure members), so they use
/// this prefix as the entity portion of their ValueCellId.
pub const FIELD_ENTITY_PREFIX: &str = "__field";

/// The standard LocatedPort trait name. Ports that satisfy this trait (directly or
/// transitively through refinement) carry a spatial frame and participate in
/// frame-alignment constraint generation. Used by the compiler to detect asymmetric
/// connections where one port is spatial and the other is not.
pub const LOCATED_PORT_TRAIT: &str = "LocatedPort";

/// Identifies a value cell in the topology graph.
/// A value cell corresponds to a single param, let binding, or computed property.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// Renders as `"<entity>.<member>"`.
///
/// This rendering is LOSSY whenever either half already contains a `'.'`,
/// because the joined string no longer says where the boundary was. Three live
/// families hit that case, and they disagree about which dot is the separator:
///
/// * INSTANCE PATHS carry the dots in the ENTITY half (`Rig.bolts` +
///   `line_cost`).
/// * PORT COMPOSITE MEMBERS carry a dot in the MEMBER half (`Bracket` +
///   `mount.width`).
/// * KEYED MEMBERS carry the key in the MEMBER half (`Widget` +
///   `vents["intake"]`), so a key containing a dot lands there too.
///
/// `ValueCellId::new("Rig.bolts", "line_cost")` and
/// `ValueCellId::new("Rig", "bolts.line_cost")` are distinct cells that render
/// to the identical string, so Display is not injective and NO positional split
/// inverts it. [`FromStr`](std::str::FromStr) is therefore only a PARTIAL
/// inverse: it round-trips the unambiguous single-dot case and refuses the rest
/// rather than guessing. Do not hand-roll a split — route through it.
impl fmt::Display for ValueCellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.entity, self.member)
    }
}

/// Error returned by [`ValueCellId`]'s [`FromStr`](std::str::FromStr) impl when
/// the input does not name exactly one `(entity, member)` pair under the
/// `"<entity>.<member>"` Display grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueCellIdParseError {
    /// The input contained no `'.'` separator at all.
    MissingSeparator,
    /// The entity segment (before the first `'.'`) was empty.
    EmptyEntity,
    /// The member segment (after the first `'.'`) was empty.
    EmptyMember,
    /// The member segment still contained a `'.'`, so the input admits more
    /// than one `(entity, member)` reading and cannot say which cell is meant.
    /// See [`ValueCellId`]'s [`Display`](fmt::Display) docs for the three
    /// families that collide here.
    ///
    /// KNOWN LIMITATION: the refusal is SYNTACTIC, decided from the string
    /// alone, so it also refuses an id that a lookup against a compiled cell
    /// set could resolve to exactly one cell (e.g. `Widget` + `vents["a.b"]`).
    /// #7717 tracks making such cells addressable.
    Ambiguous,
}

impl fmt::Display for ValueCellIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueCellIdParseError::MissingSeparator => {
                write!(f, "expected '<entity>.<member>'")
            }
            ValueCellIdParseError::EmptyEntity => {
                write!(f, "value cell id has an empty entity segment")
            }
            ValueCellIdParseError::EmptyMember => {
                write!(f, "value cell id has an empty member segment")
            }
            ValueCellIdParseError::Ambiguous => write!(
                f,
                "ambiguous value cell id: the remaining '.' admits more than \
                 one (entity, member) reading, so the id cannot say which cell \
                 is meant"
            ),
        }
    }
}

impl std::error::Error for ValueCellIdParseError {}

impl std::str::FromStr for ValueCellId {
    type Err = ValueCellIdParseError;

    /// Parse the `"<entity>.<member>"` Display grammar back into a
    /// [`ValueCellId`].
    ///
    /// This is a PARTIAL inverse of [`fmt::Display`], not an exact one: that
    /// rendering is not injective (see its docs), so only an id with EXACTLY
    /// one `'.'` names a single cell. Anything with a second dot is refused as
    /// [`Ambiguous`](ValueCellIdParseError::Ambiguous) rather than split on a
    /// guessed boundary. No identifier charset is imposed otherwise — hyphens,
    /// brackets and quotes all round-trip, so the keyed-member form
    /// `Widget.vents["intake"]` parses.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, member) = s
            .split_once('.')
            .ok_or(ValueCellIdParseError::MissingSeparator)?;
        if entity.is_empty() {
            return Err(ValueCellIdParseError::EmptyEntity);
        }
        if member.is_empty() {
            return Err(ValueCellIdParseError::EmptyMember);
        }
        if member.contains('.') {
            return Err(ValueCellIdParseError::Ambiguous);
        }
        Ok(ValueCellId::new(entity, member))
    }
}

/// The four index-addressed node kinds, which share ONE rendered grammar,
/// `"<entity>#<marker>[<index>]"`, and differ only in their marker word.
///
/// Every indexed id's [`Display`](fmt::Display) renders through
/// [`marker`](Self::marker) and every indexed id's
/// [`FromStr`](std::str::FromStr) parses through the same grammar, so the two
/// directions cannot disagree about a marker. That grammar:
///
/// * anchors on the LAST `"#<marker>["`, so an entity may itself contain `'#'`
///   (and `'.'`, `':'`, `'@'`, hyphens — no identifier charset is imposed);
/// * forbids the structural delimiters `'/'`, `'['` and `']'` in the entity,
///   which keeps the mapping unambiguous (`'/'` also introduces a
///   `FeatureId`'s sub-feature suffix in reify-ir);
/// * requires the index to be all ASCII digits fitting a `u32`, followed by
///   the closing `']'` and nothing else.
///
/// Parsing is therefore the exact inverse of Display over every entity the
/// grammar admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexedNodeKind {
    Constraint,
    Realization,
    Resolution,
    Compute,
}

impl IndexedNodeKind {
    pub const ALL: [IndexedNodeKind; 4] = [
        IndexedNodeKind::Constraint,
        IndexedNodeKind::Realization,
        IndexedNodeKind::Resolution,
        IndexedNodeKind::Compute,
    ];

    /// The word between `'#'` and `'['` in this kind's rendered ids.
    pub const fn marker(self) -> &'static str {
        match self {
            IndexedNodeKind::Constraint => "constraint",
            IndexedNodeKind::Realization => "realization",
            IndexedNodeKind::Resolution => "resolution",
            IndexedNodeKind::Compute => "computation",
        }
    }

    /// The kind whose marker a rendered id names: the text after the LAST
    /// `'#'`, up to the first `'['` (or to the end), must equal a marker
    /// exactly. `None` when there is no `'#'` or the segment names no marker.
    ///
    /// This only classifies; the id may still be malformed for that kind's
    /// grammar, which its `FromStr` reports.
    pub fn of_rendered_id(s: &str) -> Option<Self> {
        let (_, tail) = s.rsplit_once('#')?;
        let segment = tail.split_once('[').map_or(tail, |(head, _)| head);
        Self::ALL.into_iter().find(|kind| kind.marker() == segment)
    }

    fn write_rendered(self, f: &mut fmt::Formatter<'_>, entity: &str, index: u32) -> fmt::Result {
        write!(f, "{entity}#{}[{index}]", self.marker())
    }

    fn parse_rendered(self, s: &str) -> Result<(&str, u32), IndexedNodeIdParseError> {
        let fail = |fault| IndexedNodeIdParseError { kind: self, fault };
        let opener = format!("#{}[", self.marker());
        let (entity, rest) = s
            .rsplit_once(opener.as_str())
            .ok_or_else(|| fail(IndexedNodeIdFault::MissingMarker))?;
        if entity.is_empty() {
            return Err(fail(IndexedNodeIdFault::EmptyEntity));
        }
        if entity.contains(['/', '[', ']']) {
            return Err(fail(IndexedNodeIdFault::InvalidEntity));
        }
        let index_str = rest
            .strip_suffix(']')
            .ok_or_else(|| fail(IndexedNodeIdFault::MissingTerminator))?;
        if index_str.is_empty() || !index_str.bytes().all(|b| b.is_ascii_digit()) {
            return Err(fail(IndexedNodeIdFault::InvalidIndex));
        }
        let index: u32 = index_str
            .parse()
            .map_err(|_| fail(IndexedNodeIdFault::InvalidIndex))?;
        Ok((entity, index))
    }
}

/// Which rule of the [`IndexedNodeKind`] grammar an input broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexedNodeIdFault {
    /// The `"#<marker>["` opener was not present.
    MissingMarker,
    /// The entity segment (before `"#<marker>["`) was empty.
    EmptyEntity,
    /// The entity segment contained a structural delimiter (`'/'`, `'['`, or
    /// `']'`), which would make the Display↔FromStr mapping ambiguous.
    InvalidEntity,
    /// The input did not end with the `"]"` terminator.
    MissingTerminator,
    /// The index between the brackets was empty, contained a non-digit, or
    /// overflowed `u32`.
    InvalidIndex,
}

/// Error returned by every indexed id's [`FromStr`](std::str::FromStr) impl:
/// the `kind` whose grammar was applied and the `fault` it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNodeIdParseError {
    pub kind: IndexedNodeKind,
    pub fault: IndexedNodeIdFault,
}

impl fmt::Display for IndexedNodeIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let marker = self.kind.marker();
        match self.fault {
            IndexedNodeIdFault::MissingMarker => {
                write!(f, "expected '<entity>#{marker}[<index>]'")
            }
            IndexedNodeIdFault::EmptyEntity => {
                write!(f, "{marker} node id has an empty entity segment")
            }
            IndexedNodeIdFault::InvalidEntity => write!(
                f,
                "{marker} node id entity must not contain '/', '[', or ']'"
            ),
            IndexedNodeIdFault::MissingTerminator => {
                write!(f, "{marker} node id must end with ']'")
            }
            IndexedNodeIdFault::InvalidIndex => {
                write!(f, "{marker} node id index must be a u32")
            }
        }
    }
}

impl std::error::Error for IndexedNodeIdParseError {}

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
        IndexedNodeKind::Constraint.write_rendered(f, &self.entity, self.index)
    }
}

impl std::str::FromStr for ConstraintNodeId {
    type Err = IndexedNodeIdParseError;

    /// The exact inverse of [`fmt::Display`] over every entity the
    /// [`IndexedNodeKind`] grammar admits.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, index) = IndexedNodeKind::Constraint.parse_rendered(s)?;
        Ok(Self::new(entity, index))
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

impl fmt::Display for RealizationNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        IndexedNodeKind::Realization.write_rendered(f, &self.entity, self.index)
    }
}

impl std::str::FromStr for RealizationNodeId {
    type Err = IndexedNodeIdParseError;

    /// The exact inverse of [`fmt::Display`] over every entity the
    /// [`IndexedNodeKind`] grammar admits.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, index) = IndexedNodeKind::Realization.parse_rendered(s)?;
        Ok(Self::new(entity, index))
    }
}

/// Identifies a resolution node (constraint solver group) in the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolutionNodeId {
    pub entity: String,
    pub index: u32,
}

impl ResolutionNodeId {
    pub fn new(entity: impl Into<String>, index: u32) -> Self {
        Self {
            entity: entity.into(),
            index,
        }
    }
}

impl fmt::Display for ResolutionNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        IndexedNodeKind::Resolution.write_rendered(f, &self.entity, self.index)
    }
}

impl std::str::FromStr for ResolutionNodeId {
    type Err = IndexedNodeIdParseError;

    /// The exact inverse of [`fmt::Display`] over every entity the
    /// [`IndexedNodeKind`] grammar admits.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, index) = IndexedNodeKind::Resolution.parse_rendered(s)?;
        Ok(Self::new(entity, index))
    }
}

/// Identifies a compute node (e.g. an @optimized FEA/solver computation) in the topology graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComputeNodeId {
    pub entity: String,
    pub index: u32,
}

impl ComputeNodeId {
    pub fn new(entity: impl Into<String>, index: u32) -> Self {
        Self {
            entity: entity.into(),
            index,
        }
    }
}

impl fmt::Display for ComputeNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        IndexedNodeKind::Compute.write_rendered(f, &self.entity, self.index)
    }
}

impl std::str::FromStr for ComputeNodeId {
    type Err = IndexedNodeIdParseError;

    /// The exact inverse of [`fmt::Display`] over every entity the
    /// [`IndexedNodeKind`] grammar admits.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, index) = IndexedNodeKind::Compute.parse_rendered(s)?;
        Ok(Self::new(entity, index))
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

/// Monotonically increasing version identifier for tracking snapshot lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(pub u64);

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// Unique identifier for a snapshot in the evaluation history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub u64);

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snap-{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn version_id_construction_and_equality() {
        let v1 = VersionId(0);
        let v2 = VersionId(0);
        let v3 = VersionId(1);
        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
    }

    #[test]
    fn version_id_copy_semantics() {
        let v1 = VersionId(42);
        let v2 = v1; // Copy
        assert_eq!(v1, v2); // v1 still usable
    }

    #[test]
    fn version_id_debug_format() {
        let v = VersionId(7);
        let debug = format!("{:?}", v);
        assert!(debug.contains("VersionId"));
        assert!(debug.contains("7"));
    }

    #[test]
    fn version_id_display() {
        assert_eq!(format!("{}", VersionId(0)), "v0");
        assert_eq!(format!("{}", VersionId(42)), "v42");
    }

    #[test]
    fn version_id_ordering() {
        let v0 = VersionId(0);
        let v1 = VersionId(1);
        let v2 = VersionId(2);
        assert!(v0 < v1);
        assert!(v1 < v2);
        assert!(v0 < v2);

        let mut versions = vec![v2, v0, v1];
        versions.sort();
        assert_eq!(versions, vec![v0, v1, v2]);
    }

    #[test]
    fn version_id_as_hashmap_key() {
        let mut map = HashMap::new();
        map.insert(VersionId(0), "initial");
        map.insert(VersionId(1), "edit");
        assert_eq!(map.get(&VersionId(0)), Some(&"initial"));
        assert_eq!(map.get(&VersionId(1)), Some(&"edit"));
        assert_eq!(map.get(&VersionId(2)), None);
    }

    #[test]
    fn snapshot_id_construction_and_equality() {
        let s1 = SnapshotId(0);
        let s2 = SnapshotId(0);
        let s3 = SnapshotId(1);
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn snapshot_id_copy_semantics() {
        let s1 = SnapshotId(99);
        let s2 = s1; // Copy
        assert_eq!(s1, s2); // s1 still usable
    }

    #[test]
    fn snapshot_id_debug_format() {
        let s = SnapshotId(3);
        let debug = format!("{:?}", s);
        assert!(debug.contains("SnapshotId"));
        assert!(debug.contains("3"));
    }

    #[test]
    fn snapshot_id_display() {
        assert_eq!(format!("{}", SnapshotId(0)), "snap-0");
        assert_eq!(format!("{}", SnapshotId(5)), "snap-5");
    }

    #[test]
    fn snapshot_id_as_hashmap_key() {
        let mut map = HashMap::new();
        map.insert(SnapshotId(0), "first");
        map.insert(SnapshotId(1), "second");
        assert_eq!(map.get(&SnapshotId(0)), Some(&"first"));
        assert_eq!(map.get(&SnapshotId(2)), None);
    }

    #[test]
    fn resolution_node_id_construction() {
        let id = ResolutionNodeId::new("Bracket", 0);
        assert_eq!(id.entity, "Bracket");
        assert_eq!(id.index, 0);
    }

    #[test]
    fn resolution_node_id_display() {
        let id = ResolutionNodeId::new("Bracket", 0);
        assert_eq!(format!("{}", id), "Bracket#resolution[0]");

        let id2 = ResolutionNodeId::new("Flange", 3);
        assert_eq!(format!("{}", id2), "Flange#resolution[3]");
    }

    #[test]
    fn resolution_node_id_equality() {
        let a = ResolutionNodeId::new("Bracket", 0);
        let b = ResolutionNodeId::new("Bracket", 0);
        let c = ResolutionNodeId::new("Bracket", 1);
        let d = ResolutionNodeId::new("Flange", 0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    // ── ModulePath::from_dotted ───────────────────────────────────────

    #[test]
    fn from_dotted_two_segments() {
        let path = ModulePath::from_dotted("std.units").unwrap();
        assert_eq!(path.0, vec!["std".to_string(), "units".to_string()]);
    }

    #[test]
    fn from_dotted_three_segments() {
        let path = ModulePath::from_dotted("a.b.c").unwrap();
        assert_eq!(
            path.0,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn from_dotted_single_segment() {
        let path = ModulePath::from_dotted("foo").unwrap();
        assert_eq!(path.0, vec!["foo".to_string()]);
    }

    #[test]
    fn from_dotted_empty_string_returns_err() {
        let result = ModulePath::from_dotted("");
        assert_eq!(result, Err(ModulePathParseError::Empty));
    }

    #[test]
    fn from_dotted_double_dot_returns_empty_segment_err() {
        let result = ModulePath::from_dotted("a..b");
        assert_eq!(
            result,
            Err(ModulePathParseError::EmptySegment {
                input: "a..b".into()
            })
        );
    }

    #[test]
    fn from_dotted_leading_dot_returns_empty_segment_err() {
        let result = ModulePath::from_dotted(".leading");
        assert_eq!(
            result,
            Err(ModulePathParseError::EmptySegment {
                input: ".leading".into()
            })
        );
    }

    #[test]
    fn from_dotted_trailing_dot_returns_empty_segment_err() {
        let result = ModulePath::from_dotted("trailing.");
        assert_eq!(
            result,
            Err(ModulePathParseError::EmptySegment {
                input: "trailing.".into()
            })
        );
    }

    #[test]
    fn resolution_node_id_as_hashmap_key() {
        let mut map = HashMap::new();
        let id1 = ResolutionNodeId::new("Bracket", 0);
        let id2 = ResolutionNodeId::new("Bracket", 1);
        map.insert(id1.clone(), "first");
        map.insert(id2.clone(), "second");
        assert_eq!(map.get(&id1), Some(&"first"));
        assert_eq!(map.get(&id2), Some(&"second"));
        assert_eq!(map.get(&ResolutionNodeId::new("Missing", 0)), None);
    }

    #[test]
    fn compute_node_id_construction() {
        let id = ComputeNodeId::new("Bracket", 0);
        assert_eq!(id.entity, "Bracket");
        assert_eq!(id.index, 0);
    }

    #[test]
    fn compute_node_id_display() {
        let id = ComputeNodeId::new("Bracket", 0);
        assert_eq!(format!("{}", id), "Bracket#computation[0]");

        let id2 = ComputeNodeId::new("Bracket", 3);
        assert_eq!(format!("{}", id2), "Bracket#computation[3]");
    }

    #[test]
    fn compute_node_id_equality() {
        let a = ComputeNodeId::new("Bracket", 0);
        let b = ComputeNodeId::new("Bracket", 0);
        let c = ComputeNodeId::new("Bracket", 1);
        let d = ComputeNodeId::new("Flange", 0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn compute_node_id_as_hashmap_key() {
        let mut map = HashMap::new();
        let id1 = ComputeNodeId::new("Bracket", 0);
        let id2 = ComputeNodeId::new("Bracket", 1);
        map.insert(id1.clone(), "first");
        map.insert(id2.clone(), "second");
        assert_eq!(map.get(&id1), Some(&"first"));
        assert_eq!(map.get(&id2), Some(&"second"));
        assert_eq!(map.get(&ComputeNodeId::new("Missing", 0)), None);
    }

    // ── Shared indexed-id grammar (`<entity>#<marker>[<index>]`) ─────

    fn check_indexed_grammar<T>(kind: IndexedNodeKind, make: fn(&str, u32) -> T)
    where
        T: std::str::FromStr<Err = IndexedNodeIdParseError> + fmt::Display + PartialEq + fmt::Debug,
    {
        let entities = [
            "Foo",
            // Hyphens, as in reify-ir's `FeatureId::realization` fixtures: no
            // identifier charset is imposed.
            "box-2658-smoke",
            "Rig.bolts",
            "a#b",
            // The constraint-entity shape minted by reify-eval's engine_edit.rs
            // for `forall connect` bodies.
            "forall_connect:v@Rig.bolts#3",
        ];
        for entity in entities {
            for index in [0, 7, u32::MAX] {
                let id = make(entity, index);
                assert_eq!(
                    id.to_string().parse::<T>(),
                    Ok(make(entity, index)),
                    "{kind:?} id {id} must round-trip"
                );
            }
        }

        let m = kind.marker();
        let other = IndexedNodeKind::ALL
            .into_iter()
            .find(|k| *k != kind)
            .unwrap()
            .marker();
        let rejections = [
            (String::new(), IndexedNodeIdFault::MissingMarker),
            ("Foo".to_string(), IndexedNodeIdFault::MissingMarker),
            (format!("Foo#{other}[0]"), IndexedNodeIdFault::MissingMarker),
            (format!("Foo#{m}"), IndexedNodeIdFault::MissingMarker),
            (format!("#{m}[0]"), IndexedNodeIdFault::EmptyEntity),
            (format!("a/b#{m}[0]"), IndexedNodeIdFault::InvalidEntity),
            (format!("a[b#{m}[0]"), IndexedNodeIdFault::InvalidEntity),
            (format!("a]b#{m}[0]"), IndexedNodeIdFault::InvalidEntity),
            (format!("Foo#{m}[0"), IndexedNodeIdFault::MissingTerminator),
            (
                format!("Foo#{m}[0]junk"),
                IndexedNodeIdFault::MissingTerminator,
            ),
            (
                format!("Foo#{m}[0] "),
                IndexedNodeIdFault::MissingTerminator,
            ),
            // A trailing "/mid_surface" belongs to reify-ir's FeatureId
            // grammar, not to a bare indexed id.
            (
                format!("Foo#{m}[0]/mid_surface"),
                IndexedNodeIdFault::MissingTerminator,
            ),
            (format!("Foo#{m}[0]]"), IndexedNodeIdFault::InvalidIndex),
            (format!("Foo#{m}[]"), IndexedNodeIdFault::InvalidIndex),
            (format!("Foo#{m}[x]"), IndexedNodeIdFault::InvalidIndex),
            (format!("Foo#{m}[-1]"), IndexedNodeIdFault::InvalidIndex),
            // All digits, so only the u32 parse itself rejects it.
            (
                format!("Foo#{m}[99999999999]"),
                IndexedNodeIdFault::InvalidIndex,
            ),
        ];
        for (input, fault) in rejections {
            assert_eq!(
                input.parse::<T>(),
                Err(IndexedNodeIdParseError { kind, fault }),
                "{kind:?} must reject {input:?} as {fault:?}"
            );
        }
    }

    #[test]
    fn indexed_node_ids_share_one_grammar_that_inverts_display() {
        check_indexed_grammar(IndexedNodeKind::Constraint, |e, i| {
            ConstraintNodeId::new(e, i)
        });
        check_indexed_grammar(IndexedNodeKind::Realization, |e, i| {
            RealizationNodeId::new(e, i)
        });
        check_indexed_grammar(IndexedNodeKind::Resolution, |e, i| {
            ResolutionNodeId::new(e, i)
        });
        check_indexed_grammar(IndexedNodeKind::Compute, |e, i| ComputeNodeId::new(e, i));
    }

    #[test]
    fn indexed_node_kind_of_rendered_id_reads_the_last_marker() {
        for k in IndexedNodeKind::ALL {
            let m = k.marker();
            assert_eq!(
                IndexedNodeKind::of_rendered_id(&format!("Foo#{m}[0]")),
                Some(k)
            );
            assert_eq!(
                IndexedNodeKind::of_rendered_id(&format!("Foo#{m}")),
                Some(k)
            );
        }
        assert_eq!(
            IndexedNodeKind::of_rendered_id("a#b#realization[1]"),
            Some(IndexedNodeKind::Realization),
            "the segment after the LAST '#' names the kind"
        );
        for s in [
            "Bracket.width",
            "Foo",
            "Foo#bogus[0]",
            "Foo#constraintx[0]",
            r#"Widget.vents["x#y"]"#,
        ] {
            assert_eq!(
                IndexedNodeKind::of_rendered_id(s),
                None,
                "{s:?} names no marker"
            );
        }
    }

    #[test]
    fn indexed_node_kind_markers_are_pairwise_distinct() {
        let all = IndexedNodeKind::ALL;
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.marker(), b.marker(), "{a:?} and {b:?} share a marker");
            }
        }
    }

    // ── ValueCellId::from_str (FromStr) ──────────────────────────────
    //
    // The PARTIAL inverse of the `"<entity>.<member>"` Display grammar. Only the
    // unambiguous single-dot case is addressable as a string; see the
    // non-injectivity test below for why no positional split can do better.

    #[test]
    fn value_cell_id_from_str_roundtrips_unambiguous_ids() {
        // Every shape a real mint site produces with exactly one dot in the
        // rendered string must survive Display → parse unchanged.
        let cases = [
            ValueCellId::new("Bracket", "width"),
            // FIELD_ENTITY_PREFIX form: fields are top-level, not members.
            ValueCellId::new(FIELD_ENTITY_PREFIX, "gravity"),
            // Compiler-synthesized member, minted by `compile_block_guard` in
            // reify-compiler/src/guards.rs.
            ValueCellId::new("Part", "__guard_0"),
            // No strict identifier charset, mirroring the hyphenated entities
            // of `indexed_node_ids_share_one_grammar_that_inverts_display`.
            ValueCellId::new("my-part_2", "hole_diameter"),
            // Keyed member, minted by `keyed_member_cell` in reify-ir/src/value.rs
            // and asserted by its `keyed_member_cell_carries_key_in_nodeid_path`
            // test. Brackets and quotes must round-trip: the rendered id still
            // has exactly one dot.
            ValueCellId::new("Widget", r#"vents["intake"]"#),
        ];
        for id in cases {
            let parsed: ValueCellId = id
                .to_string()
                .parse()
                .unwrap_or_else(|e| panic!("{id} must round-trip, got {e}"));
            assert_eq!(parsed, id, "{id} must round-trip unchanged");
        }
    }

    #[test]
    fn value_cell_id_from_str_rejects_missing_separator() {
        for s in ["", "width"] {
            assert_eq!(
                s.parse::<ValueCellId>(),
                Err(ValueCellIdParseError::MissingSeparator),
                "{s:?} has no '.' separator, so it names no (entity, member) pair"
            );
        }
    }

    #[test]
    fn value_cell_id_display_is_not_injective_so_dotted_members_are_refused() {
        // THE load-bearing property. Three live id families all render into the
        // same `<entity>.<member>` string shape, and they disagree about where
        // the split belongs:
        //
        //   1. INSTANCE PATHS put the dots in the ENTITY half
        //      (`Rig.bolts` + `line_cost`). Splitting these needs the LAST dot.
        //   2. PORT COMPOSITE MEMBERS put a dot in the MEMBER half
        //      (`Bracket` + `mount.width`). Splitting these needs the FIRST dot.
        //   3. KEYED MEMBERS put the key in the MEMBER half
        //      (`Widget` + `vents["a.b"]`). A dotted key needs the FIRST dot
        //      too, and a last-dot split would cut INSIDE the quoted key.
        //
        // So `rsplit_once('.')` is not "the correct inverse" of Display: it
        // would fix family 1 and break families 2 and 3. No positional split
        // can be right, because Display is NOT INJECTIVE — two distinct cells
        // render to one identical string, and the information needed to choose
        // between them is simply not present in that string:
        let instance_path = ValueCellId::new("Rig.bolts", "line_cost");
        let port_composite = ValueCellId::new("Rig", "bolts.line_cost");
        assert_ne!(
            instance_path, port_composite,
            "these are two DIFFERENT cells"
        );
        assert_eq!(
            instance_path.to_string(),
            port_composite.to_string(),
            "… yet they render to ONE string, so no parser can tell them apart"
        );

        // The only correct answer for that one string is therefore to refuse,
        // naming ambiguity as the reason rather than silently picking either
        // family's split.
        assert_eq!(
            instance_path.to_string().parse::<ValueCellId>(),
            Err(ValueCellIdParseError::Ambiguous)
        );
    }

    #[test]
    fn value_cell_id_from_str_rejects_keyed_member_with_dotted_key() {
        // A key carrying a dot pushes a second dot into the member half, so the
        // keyed-member form stops being addressable as a string …
        //
        // This is a CAPABILITY LOSS pinned deliberately, not a latent bug being
        // ratified: the entity half here is undotted, so this id does name one
        // cell and a cell-set lookup could recover it. See the KNOWN LIMITATION
        // on `ValueCellIdParseError::Ambiguous` and #7717 — if that refusal is
        // ever lifted, this assertion is the one that must change with it.
        let dotted_key = ValueCellId::new("Widget", r#"vents["a.b"]"#);
        assert_eq!(
            dotted_key.to_string().parse::<ValueCellId>(),
            Err(ValueCellIdParseError::Ambiguous),
            "a dot inside the key makes the id ambiguous"
        );
        // … while its undotted sibling still round-trips. Without this contrast
        // the test above would also pass a parser that refused every keyed
        // member outright.
        let plain_key = ValueCellId::new("Widget", r#"vents["intake"]"#);
        assert_eq!(
            plain_key.to_string().parse::<ValueCellId>(),
            Ok(plain_key.clone()),
            "an undotted key must still round-trip"
        );
    }

    #[test]
    fn value_cell_id_from_str_rejects_empty_entity() {
        assert_eq!(
            ".width".parse::<ValueCellId>(),
            Err(ValueCellIdParseError::EmptyEntity)
        );
    }

    #[test]
    fn value_cell_id_from_str_rejects_empty_member() {
        assert_eq!(
            "Bracket.".parse::<ValueCellId>(),
            Err(ValueCellIdParseError::EmptyMember)
        );
    }
}
