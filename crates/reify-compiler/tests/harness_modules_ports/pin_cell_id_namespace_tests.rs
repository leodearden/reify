//! Pins the two GUI cell-id namespaces derived from a `CompiledModule` apart,
//! from a real compile of `prj/printer_v01/printer.ri`, so a future rename in
//! that file or a lowering change in the compiler cannot make the namespaces
//! silently drift back into each other (task #7450, a follow-up to #5098).
//!
//! # The two namespaces
//!
//! - **constraint namespace** — `gui/src-tauri/src/engine.rs::build_constraints`
//!   (L4997) fills each `ConstraintData.parameter_ids` entry from
//!   `CompiledExpr::collect_value_refs` (`crates/reify-ir/src/expr.rs:1008`),
//!   rendered through `ValueCellId`'s `Display` impl
//!   (`crates/reify-core/src/identity.rs:134`, `"{entity}.{member}"`). For a
//!   cross-sub reference (`self.<sub>.<member>`) the compiler stamps the
//!   entity half as `format!("{}.{}", scope.entity_name, sub_name)` —
//!   `crates/reify-compiler/src/expr.rs:4215` and `:5909`, the one lowering
//!   line reached from two different `MemberAccess` arms — so a constraint's
//!   ref renders as `<parent>.<sub_name>.<member>`, an INSTANCE path.
//! - **values namespace** — `build_values` (L4897) fills each
//!   `ValueData.cell_id` with `cell.id.to_string()` walked straight off
//!   `template.value_cells`, so its entity half is the DECLARING TEMPLATE's
//!   own name — a TYPE path, `<TemplateName>.<member>`.
//!
//! These two namespaces are not the same string for a cross-sub cell, and
//! esc-5098-6 is the review that found task #5098's GUI gate selecting pins
//! with values-namespace ids against constraints that only ever carry
//! instance-namespace ids, so every selection came back empty and the gate's
//! constraint half was silently inert at every phase. That gate,
//! `gui/test/visual/railLengtheningGate.mjs`, is not read by this file: it is
//! not on `main` (it lives on the unmerged `task/5098` branch). The four
//! scoped ids it needs are instead stated here as documented `const`s, cited
//! to their producers below, and proven by a real compile.
//!
//! # COMPILE ONLY
//!
//! Every test in this file compiles `prj/printer_v01/printer.ri` via
//! `reify_test_support::compile_source_with_stdlib` and inspects the
//! resulting `CompiledModule` only — never `check_source*`, `eval_source`, or
//! anything that reaches `reify_eval`. printer.ri SIGSEGVs the engine on the
//! eval/CSG path (#7383); a crashed test process reds the whole merge gate,
//! not just this file. This file and its helpers must never import
//! `reify_eval` or call a `check_*`/`eval_*` helper.
