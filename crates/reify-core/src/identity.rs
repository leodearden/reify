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

impl fmt::Display for RealizationNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#realization[{}]", self.entity, self.index)
    }
}

/// Error returned by [`RealizationNodeId`]'s [`FromStr`](std::str::FromStr) impl
/// when the input does not match the `"<entity>#realization[<index>]"` Display
/// grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealizationNodeIdParseError {
    /// The `"#realization["` marker was not present.
    MissingMarker,
    /// The entity segment (before `"#realization["`) was empty.
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

impl fmt::Display for RealizationNodeIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RealizationNodeIdParseError::MissingMarker => {
                write!(f, "expected '<entity>#realization[<index>]'")
            }
            RealizationNodeIdParseError::EmptyEntity => {
                write!(f, "realization node id has an empty entity segment")
            }
            RealizationNodeIdParseError::InvalidEntity => write!(
                f,
                "realization node id entity must not contain '/', '[', or ']'"
            ),
            RealizationNodeIdParseError::MissingTerminator => {
                write!(f, "realization node id must end with ']'")
            }
            RealizationNodeIdParseError::InvalidIndex => {
                write!(f, "realization node id index must be a u32")
            }
        }
    }
}

impl std::error::Error for RealizationNodeIdParseError {}

impl std::str::FromStr for RealizationNodeId {
    type Err = RealizationNodeIdParseError;

    /// Parse the `"<entity>#realization[<index>]"` Display grammar back into a
    /// [`RealizationNodeId`]. This is the exact inverse of [`fmt::Display`] and
    /// round-trips any Display output, including hyphenated entities — no strict
    /// identifier charset is imposed (only the structural delimiters `'/'`,
    /// `'['`, `']'` are forbidden in the entity). Anchors on the *last*
    /// `"#realization["` marker so the mapping stays unambiguous.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (entity, rest) = s
            .rsplit_once("#realization[")
            .ok_or(RealizationNodeIdParseError::MissingMarker)?;
        if entity.is_empty() {
            return Err(RealizationNodeIdParseError::EmptyEntity);
        }
        if entity.contains(['/', '[', ']']) {
            return Err(RealizationNodeIdParseError::InvalidEntity);
        }
        let index_str = rest
            .strip_suffix(']')
            .ok_or(RealizationNodeIdParseError::MissingTerminator)?;
        if index_str.is_empty() || !index_str.bytes().all(|b| b.is_ascii_digit()) {
            return Err(RealizationNodeIdParseError::InvalidIndex);
        }
        let index: u32 = index_str
            .parse()
            .map_err(|_| RealizationNodeIdParseError::InvalidIndex)?;
        Ok(RealizationNodeId::new(entity, index))
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
        write!(f, "{}#resolution[{}]", self.entity, self.index)
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
        write!(f, "{}#computation[{}]", self.entity, self.index)
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

    // ── RealizationNodeId::from_str (FromStr) ─────────────────────────
    //
    // The inverse of the `"<entity>#realization[<index>]"` Display grammar.
    // Round-trip (I2/B3): any `RealizationNodeId` Display output must parse
    // back to an equal value, including hyphenated entities produced by
    // `FeatureId::realization(...)` test fixtures. Strictly reject malformed
    // or structurally-ambiguous inputs.

    #[test]
    fn realization_node_id_from_str_roundtrip() {
        let id = RealizationNodeId::new("Foo", 3);
        let parsed: RealizationNodeId = id.to_string().parse().unwrap();
        assert_eq!(parsed, RealizationNodeId::new("Foo", 3));
        assert_eq!(parsed.entity, "Foo");
        assert_eq!(parsed.index, 3);
    }

    #[test]
    fn realization_node_id_from_str_index_zero_roundtrip() {
        let id = RealizationNodeId::new("Bracket", 0);
        let parsed: RealizationNodeId = id.to_string().parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn realization_node_id_from_str_hyphenated_entity_roundtrips() {
        // CRUCIAL (I2/B3): realization()-constructed fixtures use hyphens.
        // from_str MUST NOT impose a strict [a-zA-Z_] identifier charset —
        // hyphens (and any non-delimiter char) must round-trip.
        for entity in ["box-2658-smoke", "not-a-real-feature", "F99-never-existed"] {
            let id = RealizationNodeId::new(entity, 0);
            let parsed: RealizationNodeId = id.to_string().parse().unwrap();
            assert_eq!(parsed, id, "hyphenated entity {entity:?} must round-trip");
        }
    }

    #[test]
    fn realization_node_id_from_str_rejects_empty() {
        assert!("".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_missing_marker() {
        assert!("Foo".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_missing_brackets() {
        assert!("Foo#realization".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_empty_index() {
        assert!("Foo#realization[]".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_non_digit_index() {
        assert!("Foo#realization[x]".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_empty_entity() {
        assert!("#realization[0]".parse::<RealizationNodeId>().is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_trailing_junk() {
        assert!("Foo#realization[0]junk".parse::<RealizationNodeId>().is_err());
        assert!("Foo#realization[0] ".parse::<RealizationNodeId>().is_err());
        assert!("Foo#realization[0]]".parse::<RealizationNodeId>().is_err());
        // A trailing "/mid_surface" belongs to the FeatureId grammar, not the
        // bare RealizationNodeId grammar — reject it here.
        assert!("Foo#realization[0]/mid_surface"
            .parse::<RealizationNodeId>()
            .is_err());
    }

    #[test]
    fn realization_node_id_from_str_rejects_structural_delimiter_in_entity() {
        // An entity containing a structural delimiter ('/', '[', ']') would make
        // the Display↔FromStr mapping ambiguous, so reject such inputs.
        assert!("a[b#realization[0]".parse::<RealizationNodeId>().is_err());
        assert!("a/b#realization[0]".parse::<RealizationNodeId>().is_err());
        assert!("a]b#realization[0]".parse::<RealizationNodeId>().is_err());
    }
}
