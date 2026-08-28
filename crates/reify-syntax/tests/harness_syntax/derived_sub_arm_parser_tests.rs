//! AST-level (CST→AST lowering) tests for the **derived sub arm** —
//! `sub b = mirror of a across <plane> { … }` / `sub b = image of a under
//! <transform> { … }`.
//!
//! Leaf A-alpha of `docs/prds/v0_6/assembly-derivation-toolbox.md` (task #6615).
//! The CST half of the contract lives in
//! `tree-sitter-reify/tests/derived_sub_grammar_tests.rs`; this file pins that
//! the new `derivation` CST field actually reaches `SubDecl::derivation`.
//!
//! Registered in `crates/reify-syntax/tests/harness_syntax.rs` — that harness
//! root is a single integration-test compile unit (task #5275), so an
//! unregistered file here is never compiled and its tests silently do not run.
