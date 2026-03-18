//! Priority promotion for in-flight tasks.
//!
//! When a higher-priority task depends on a lower-priority in-flight task,
//! the lower-priority task is promoted. Per §8.2: 'if a P1-slow task depends
//! on a P3 task already in-flight, the P3 task is promoted to P1-slow.'
