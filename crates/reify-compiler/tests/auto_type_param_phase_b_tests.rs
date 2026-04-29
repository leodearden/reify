//! Phase B tests for `auto` type-parameter resolution per-candidate feasibility filter.
//!
//! Targets `crates/reify-compiler/src/auto_type_param.rs`'s public
//! `filter_feasible_candidates` function and its two-arm result enum
//! [`FeasibilityResult`], plus the [`RejectedCandidate`] record type.
//! The PRD that drives this work is
//! `docs/prds/auto-type-param-resolution.md` and language spec §3.9 (lines 500-512).
//!
//! Phase B takes the candidate names produced by Phase A's [`enumerate_candidates`]
//! (a `&[String]` slice) and runs the value-auto solver's constraint feasibility
//! primitives on the parameterized definition's constraints, returning the subset
//! that does not provably falsify any constraint.
//!
//! # Feasibility predicate
//!
//! Architecture §2.5 monotonic-feasible rule: `feasible(c) ≡ satisfaction != Violated`.
//! Both `Satisfied` and `Indeterminate` count as feasible; only `Violated` causes
//! rejection. This is the "treat undef as feasible" rule from PRD §"Phase B".
//!
//! # Scope
//!
//! Phase B checks only the template's top-level (unguarded) constraints.
//! Guarded-group constraints are NOT collected here (that lives in `reify-eval`).
//! No type-substitution mechanics: with an empty `ValueMap`, the candidate name
//! does not yet vary constraint outcomes. Phase C (selection), D (topology trigger)
//! are out of scope here and live in follow-up tasks.
//!
//! # Test approach
//!
//! Tests use `MockConstraintChecker` (from `reify_test_support`) to drive
//! per-`ConstraintNodeId` satisfaction outcomes without spinning up the full
//! `SimpleConstraintChecker`. Templates are built via `TopologyTemplateBuilder`
//! with literal constraint expressions (the mock ignores expr content).

#![allow(unused_imports)]

use reify_compiler::auto_type_param::*;
use reify_test_support::{MockConstraintChecker, TopologyTemplateBuilder};
use reify_types::{
    CompiledExpr, CompiledFunction, ConstraintNodeId, Satisfaction, Type, Value,
};
