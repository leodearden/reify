//! Runtime re-elaboration of statement-form `forall` over deferred-count
//! collection subs (task 2629; PRD criterion 7 second-half).
//!
//! Pins the runtime contract that supersedes the compile-time silent-skip half
//! of PRD criterion 7 — see also `forall_constraint_over_undef_count_collection_sub_emits_no_decls_no_error`
//! in `crates/reify-compiler/tests/forall_statement_lower_tests.rs`. When a
//! `forall v in <coll_sub>` declaration is compiled over a collection sub
//! whose count cell is initially undef/non-literal, the compiler emits zero
//! per-element constraints/connections and stashes a `CompiledForallTemplate`
//! describing the per-element body. Once `Engine::edit_param` makes the count
//! known, this test module asserts that per-element constraints / connections
//! materialise in the snapshot's graph, with the correct cell-id rewriting
//! (`v → coll_sub[i]`) and removal of stale prior emissions on count decrease.
//!
//! Tests in this module follow the lifecycle Undef → known-count and the
//! reverse, exercising the `EvaluationGraph::forall_templates` carrier and
//! the `engine_edit::edit_param` collection-count re-elaboration block that
//! drives the runtime emission.
