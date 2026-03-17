//! Demand registry: tracks which nodes are "demanded" (their results are needed).
//!
//! A node is demanded if it is either always-demanded (e.g., an active constraint)
//! or feeds into an always-demanded node transitively. The demand cone is the set
//! of all such nodes, computed via backward BFS from always-demanded roots.
