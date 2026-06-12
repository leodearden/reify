//! Topological sort for the embedded stdlib prelude.
//!
//! Provides [`compile_modules_topo`] — an in-memory, stable topological sort
//! over a set of `(module_name, source)` pairs, using `Declaration::Import`
//! edges for ordering and a gray/black DFS for cycle detection.
