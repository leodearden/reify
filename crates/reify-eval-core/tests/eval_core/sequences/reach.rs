//! What a run of the sequences reached: the shapes of design and event
//! the census counts, and the final answers checked, so a test can show a
//! run is not vacuous (PRD 12.1 rule 9).

use std::collections::BTreeMap;

/// Declares `Shape` and `Shape::ALL` from one list, so every shape the
/// enum names is one the census looks for.
macro_rules! shapes {
    ($($(#[$doc:meta])* $shape:ident,)*) => {
        /// A shape of design or event a run reached, counted to show the run
        /// is not vacuous (PRD 12.1 rule 9).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Shape {
            $($(#[$doc])* $shape,)*
        }

        impl Shape {
            /// Every shape, in the order declared.
            pub const ALL: &'static [Shape] = &[$(Shape::$shape),*];
        }
    };
}

shapes! {
    /// A cell's value.
    CellValue,
    ScopeValue,
    GuardValue,
    /// A guard answering absent because its condition reads false.
    GuardAbsentOnFalse,
    /// A path below a guard that answers absent.
    UnderAbsentGuard,
    CollectionValue,
    /// A node a collection declares at an index.
    CollectionElement,
    /// An index at or past a collection's count, answering absent.
    IndexPastCount,
    /// The value of a cell that picks an element.
    PickerValue,
    /// A failure naming the type read and the type found.
    WrongType,
    /// A cell answering its own failure: its divisor reads zero.
    DivisionByZero,
    /// A cell blocked by the failure at a ref it read.
    BlockedBy,
    /// "cycle" at a path where a node is declared: a node on a ring of
    /// waits, or one that answers what it read there.
    Cycle,
    /// "cycle" at a path with no declaration of its own: one whose
    /// declaration waited on a ring.
    CycleThroughStructure,
    /// A node deeper than the depth setting, or a path below one.
    TooDeep,
    /// A node declaring more children than the children-per-node setting,
    /// or a path below one.
    TooManyChildren,
    /// A cell blocked by a ref whose answer is a limit failure.
    BlockedByLimit,
    /// A box a shape made on the toy kernel.
    BoxValue,
    /// The volume a measure read from the toy kernel.
    MeasureValue,
    /// A call's value.
    CallValue,
    /// An answer at a path a call's run declared.
    BodyDeclaredChild,
    /// A child a call's run declared at a step whose chain holds an
    /// iteration link.
    ChildDeclaredInLoop,
    /// A child a call's run declared while its "when" reads true.
    ChildOnTakenBranch,
    /// A path at a call's site step answering absent because the call's
    /// "when" reads false.
    ChildOnUntakenBranch,
    /// An answer checked for a request left live after a Drive of no
    /// turns, told after a later event.
    LeftLiveAnswer,
    /// A Drive of no turns whose request is then ended.
    DriveZeroEnded,
    /// An Edit from a snapshot other than the latest.
    EditFromNonLatest,
    /// An Edit that sets or removes a limit setting.
    LimitEdited,
    /// A request given a budget, which the core ended when it ran out.
    EndedByBudget,
    /// A request given a budget, answered at every path it demanded.
    AnsweredWithinBudget,
    /// An answer the order check compared.
    OrderComparison,
}

/// What a run reached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    /// Final answers checked, by snapshot index.
    checked: BTreeMap<usize, u64>,
    shapes: BTreeMap<Shape, u64>,
    /// Distinct paths of a snapshot answered.
    answered: u64,
}

impl Reach {
    /// Counts `shape` reached once.
    pub fn reached(&mut self, shape: Shape) {
        self.reached_times(shape, 1);
    }

    /// Counts `shape` reached `times` times.
    pub fn reached_times(&mut self, shape: Shape, times: u64) {
        *self.shapes.entry(shape).or_default() += times;
    }

    /// Counts a final answer checked on snapshot `snapshot`.
    pub fn answer_checked(&mut self, snapshot: usize) {
        *self.checked.entry(snapshot).or_default() += 1;
    }

    /// Counts a distinct path of a snapshot answered.
    pub fn path_answered(&mut self) {
        self.answered += 1;
    }

    /// How many final answers were checked on snapshot `snapshot`.
    pub fn checked_on(&self, snapshot: usize) -> u64 {
        self.checked.get(&snapshot).copied().unwrap_or(0)
    }

    /// How many distinct paths of a snapshot were answered.
    pub fn answered(&self) -> u64 {
        self.answered
    }

    /// How many answers the order check compared.
    pub fn order_compared(&self) -> u64 {
        self.count(Shape::OrderComparison)
    }

    /// Adds what another run reached.
    pub fn add(&mut self, other: &Reach) {
        for (snapshot, checked) in &other.checked {
            *self.checked.entry(*snapshot).or_default() += checked;
        }
        for (shape, count) in &other.shapes {
            *self.shapes.entry(*shape).or_default() += count;
        }
        self.answered += other.answered;
    }

    /// The shapes never reached, in the order of `Shape::ALL`.
    pub fn missing(&self) -> Vec<Shape> {
        Shape::ALL
            .iter()
            .copied()
            .filter(|shape| self.count(*shape) == 0)
            .collect()
    }

    /// How often `shape` was reached.
    pub fn count(&self, shape: Shape) -> u64 {
        self.shapes.get(&shape).copied().unwrap_or(0)
    }
}
