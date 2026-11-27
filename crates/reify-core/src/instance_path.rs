//! The typed path that names an instance's node: a root definition, then a
//! flat sequence of steps (docs/prds/instances-on-the-core.md section 5.3).
//! Its one text form, both ways, is `instance_path_text`'s.
//!
//! Why the steps and their invariants are these: rule F of
//! docs/instance-kinds-design.md.

use std::fmt;

/// A member, argument, definition or module segment name: a Reify
/// identifier, `[A-Za-z_][A-Za-z0-9_]*`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathName(String);

impl PathName {
    pub fn new(text: impl Into<String>) -> Result<Self, InstancePathError> {
        let text = text.into();
        let mut chars = text.chars();
        let starts_a_name = chars.next().is_some_and(is_name_start);
        if starts_a_name && chars.all(is_name_char) {
            Ok(Self(text))
        } else {
            Err(InstancePathError::NotAnIdentifier { text })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// Whether `c` may appear in a [`PathName`] after its first character.
pub(crate) fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A module other than the entry file's: its dotted path, at least one
/// segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleName(Vec<PathName>);

impl ModuleName {
    pub fn new(segments: Vec<PathName>) -> Result<Self, InstancePathError> {
        if segments.is_empty() {
            return Err(InstancePathError::EmptyModule);
        }
        Ok(Self(segments))
    }

    pub fn segments(&self) -> &[PathName] {
        &self.0
    }
}

/// A definition, in a named module or (`None`) in the entry file's module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefinitionName {
    module: Option<ModuleName>,
    name: PathName,
}

impl DefinitionName {
    pub fn new(module: Option<ModuleName>, name: PathName) -> Self {
        Self { module, name }
    }

    pub fn module(&self) -> Option<&ModuleName> {
        self.module.as_ref()
    }

    pub fn name(&self) -> &PathName {
        &self.name
    }
}

/// The key of a keyed collection's entry: any text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey(String);

impl PathKey {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An element of a collection, or of a `forall` block's constraint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathElement {
    Index(u64),
    Key(PathKey),
}

/// The call sites and iteration indices from a member's expression to a
/// call: at least one index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallChain(Vec<u64>);

impl CallChain {
    pub fn new(indices: Vec<u64>) -> Result<Self, InstancePathError> {
        if indices.is_empty() {
            return Err(InstancePathError::EmptyChain);
        }
        Ok(Self(indices))
    }

    pub fn indices(&self) -> &[u64] {
        &self.0
    }
}

/// Where an unknown with no name of its own is written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AutoAnchor {
    /// A member of the owner, with its key for a keyed collection's entry.
    Member {
        name: PathName,
        key: Option<PathKey>,
    },
    /// The owner's `k`th `connect`.
    Connect(u64),
}

/// One segment of an auto site after its anchor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AutoSiteSegment {
    /// The argument the unknown fills.
    Argument(PathName),
    /// The chain to a call inside the expression; the argument it fills
    /// follows.
    Chain(CallChain),
    /// The placement of the anchor's sub.
    Placement,
}

/// The site of an unknown with no name of its own: an anchor and at least
/// one segment, every chain followed by the argument it fills.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AutoSite {
    anchor: AutoAnchor,
    segments: Vec<AutoSiteSegment>,
}

impl AutoSite {
    pub fn new(
        anchor: AutoAnchor,
        segments: Vec<AutoSiteSegment>,
    ) -> Result<Self, InstancePathError> {
        if segments.is_empty() {
            return Err(InstancePathError::EmptyAutoSite);
        }
        for (position, segment) in segments.iter().enumerate() {
            let AutoSiteSegment::Chain(chain) = segment else {
                continue;
            };
            let followed_by = segments.get(position + 1);
            if !matches!(followed_by, Some(AutoSiteSegment::Argument(_))) {
                return Err(InstancePathError::ChainWithoutArgument {
                    chain: chain.clone(),
                    followed_by: followed_by.cloned(),
                });
            }
        }
        Ok(Self { anchor, segments })
    }

    pub fn anchor(&self) -> &AutoAnchor {
        &self.anchor
    }

    pub fn segments(&self) -> &[AutoSiteSegment] {
        &self.segments
    }
}

/// One step from a node to a node below it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathStep {
    /// A member, by name.
    Member(PathName),
    /// An element, by position or key.
    Element(PathElement),
    /// The instance's summary.
    Summary,
    /// The instance read as a value.
    AsValue,
    /// A definition's member table.
    Defs(DefinitionName),
    /// The `k`th constraint that gives a verdict.
    Constraint(u64),
    /// What is written in a sub's braces.
    Braces,
    /// The `k`th applied purpose.
    Purpose(u64),
    /// The built-in verdict on an instance's arguments.
    Arguments,
    /// The `k`th guard's condition.
    Guard(u64),
    /// The count of the named collection.
    Count(PathName),
    /// The placement of the named sub.
    At(PathName),
    /// An unknown with no name of its own.
    Auto(AutoSite),
    /// The list of what a cell made.
    MadeList,
    /// A made instance, by its chain.
    Made(CallChain),
    /// A cell's value opened as an instance; in mid-expression, by its
    /// chain.
    Open(Option<CallChain>),
    /// What an instance tells a solve.
    Problem,
    /// The solve design's nodes.
    Solves,
}

/// A root definition and the steps from it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstancePath {
    root: DefinitionName,
    steps: Vec<PathStep>,
}

impl InstancePath {
    pub fn new(root: DefinitionName) -> Self {
        Self {
            root,
            steps: Vec::new(),
        }
    }

    pub fn from_steps(
        root: DefinitionName,
        steps: Vec<PathStep>,
    ) -> Result<Self, InstancePathError> {
        let mut previous = None;
        for step in &steps {
            check_may_follow(previous, step)?;
            previous = Some(step);
        }
        Ok(Self { root, steps })
    }

    /// This path with `step` appended; `self` is unchanged.
    pub fn then(&self, step: PathStep) -> Result<Self, InstancePathError> {
        check_may_follow(self.steps.last(), &step)?;
        let mut steps = self.steps.clone();
        steps.push(step);
        Ok(Self {
            root: self.root.clone(),
            steps,
        })
    }

    pub fn root(&self) -> &DefinitionName {
        &self.root
    }

    pub fn steps(&self) -> &[PathStep] {
        &self.steps
    }
}

fn check_may_follow(previous: Option<&PathStep>, next: &PathStep) -> Result<(), InstancePathError> {
    match (previous, next) {
        (Some(bare @ (PathStep::MadeList | PathStep::Open(None))), PathStep::Element(element)) => {
            Err(InstancePathError::ElementAfterBareStep {
                previous: bare.clone(),
                element: element.clone(),
            })
        }
        _ => Ok(()),
    }
}

/// An invariant of a path's parts that a construction would break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstancePathError {
    NotAnIdentifier {
        text: String,
    },
    EmptyModule,
    EmptyChain,
    EmptyAutoSite,
    ChainWithoutArgument {
        chain: CallChain,
        followed_by: Option<AutoSiteSegment>,
    },
    ElementAfterBareStep {
        previous: PathStep,
        element: PathElement,
    },
}

impl fmt::Display for InstancePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnIdentifier { text } => write!(
                f,
                "`{text}` is not a path name: a path name is a Reify identifier, \
                 [A-Za-z_][A-Za-z0-9_]*"
            ),
            Self::EmptyModule => write!(f, "a module name has at least one segment"),
            Self::EmptyChain => write!(f, "a call chain has at least one index"),
            Self::EmptyAutoSite => {
                write!(f, "an auto site has at least one segment after its anchor")
            }
            Self::ChainWithoutArgument { chain, followed_by } => {
                write!(
                    f,
                    "in an auto site a call chain is followed by the argument it fills: \
                     the chain {:?} is followed by ",
                    chain.indices()
                )?;
                match followed_by {
                    Some(segment) => write!(f, "{segment:?}"),
                    None => write!(f, "nothing"),
                }
            }
            Self::ElementAfterBareStep { previous, element } => write!(
                f,
                "an element step cannot follow a bare `made` or `open`: their instances are \
                 named by a chain (the element {element:?} follows {previous:?})"
            ),
        }
    }
}

impl std::error::Error for InstancePathError {}
