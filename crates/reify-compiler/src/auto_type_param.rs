//! Phase A of the `auto` type-parameter resolution algorithm.
//!
//! Implements the **candidate enumeration** step described in
//! `docs/prds/auto-type-param-resolution.md` and `docs/reify-language-spec.md`
//! §3.9 (lines 474–516): walk the in-scope name table at the use site and
//! collect every concrete structure whose declared trait bounds satisfy
//! a required trait bound. The pool is capped at
//! [`MAX_AUTO_TYPE_PARAM_CANDIDATES`]; if the pool would exceed the cap, an
//! [`reify_types::DiagnosticCode::AutoTypeParamPoolOverflow`] error is emitted
//! and the (alphabetically-first) capped list is returned to the caller.
//!
//! # Scope
//!
//! Phase A is delivered as a **pure utility module**: the parser does not yet
//! accept `auto: TraitName` syntax inside `type_arg_list`
//! (`tree-sitter-reify/grammar.js:601-605` only permits `$.type_expr`), so
//! end-to-end source-level resolution is impossible until a follow-up parser
//! task lands the new syntax. This module's [`enumerate_candidates`] function
//! and [`CandidateEnumeration`] result enum are unit-tested against
//! compiler-built `template_registry`/`trait_registry` registries; a future
//! task will wire them into the compile pipeline once the parser/AST learn
//! `auto:` in type-arg position.
//!
//! Phases B (per-candidate feasibility filter), C (selection logic /
//! strict-vs-free), and D (topology trigger) are explicitly deferred to
//! follow-up tasks.
