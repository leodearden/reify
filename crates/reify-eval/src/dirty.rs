//! Dirty cone computation and evaluation set construction.
//!
//! When a parameter changes, the dirty cone is the set of all nodes that
//! transitively depend on the changed cells. The evaluation set is the
//! intersection of the dirty cone and the demand cone, topologically sorted
//! so that dependencies are evaluated before their dependents.
