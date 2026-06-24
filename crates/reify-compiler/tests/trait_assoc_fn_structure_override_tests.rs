//! Integration tests for task #4752 (trait-assoc-fn structure-body fn override).
//!
//! Confirmation tests for deliverable 2: "ts_parser lower structure-body fn →
//! MemberDecl::Fn; confirm find_structure_assoc_fn sees it."
//!
//! With the grammar gap closed (step-2), `tree-sitter-reify` now produces a
//! `function_definition` node inside `structure_definition`.  The pre-wired
//! `ts_parser::lower_member` (ts_parser.rs:2066-2067) maps it to
//! `MemberDecl::Fn`, and `find_structure_assoc_fn` (checker.rs:1100) can now
//! return `Some` → `check_phase_resolve_assoc_fns` sets `is_override = true`
//! (checker.rs:1193-1196).
//!
//! No new compiler or parser production code is needed — this test file only
//! confirms that the previously-dead path is now reachable.
//!
//! Uses `reify_test_support::{compile_source, errors_only}` and the
//! `template.assoc_fns` inspection pattern from
//! `trait_assoc_fn_overload_tests.rs`.
//!
//! Plain `Int` types are used throughout — no stdlib needed — keeping the
//! test fast and the signature-exact for the sig-lock (§5.4/§8.8).

use reify_test_support::{compile_source, errors_only};

// ── Override-beats-default resolution ────────────────────────────────────────

/// Compile a trait with a default-providing `fn f(self) -> Int { 1 }`, an
/// OVERRIDE conformer `structure def Tube : T { fn f(self) -> Int { 2 } }`,
/// and a DEFAULT-ONLY conformer `structure def Pin : T {}`.
///
/// Asserts:
/// (a) Conformance is clean — the override matches the default signature
///     exactly (no `TraitFnSignatureMismatch`).
/// (b) `Tube`'s compiled template `assoc_fns` entry for `(trait="T", fn="f")`
///     has `is_override == true`.
/// (c) `Pin`'s entry has `is_override == false`.
///
/// Directly proves the previously-dead override path: `lower_member` now
/// produces `MemberDecl::Fn` for structure-body fns, and
/// `find_structure_assoc_fn` drives `is_override = true`.
#[test]
fn override_conformer_has_is_override_true_default_conformer_false() {
    let source = r#"
trait T {
    fn f(self) -> Int { 1 }
}

structure def Tube : T {
    fn f(self) -> Int { 2 }
}

structure def Pin : T {}
"#;
    let module = compile_source(source);

    // (a) Conformance must be clean — no TraitFnSignatureMismatch and no
    //     TraitFnNotSatisfied errors.
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "override + default conformance should compile cleanly; got: {errors:?}"
    );

    // (b) Tube: the override conformer must carry is_override = true.
    let tube = module
        .templates
        .iter()
        .find(|t| t.name == "Tube")
        .expect("compiled module must contain a template for 'Tube'");

    let tube_entry = tube
        .assoc_fns
        .iter()
        .find(|e| e.trait_name == "T" && e.fn_name == "f")
        .unwrap_or_else(|| {
            panic!(
                "Tube must carry a (T, f) assoc_fns entry; assoc_fns = {:?}",
                tube.assoc_fns
            )
        });

    assert!(
        tube_entry.is_override,
        "Tube::f entry must be is_override=true (structure supplied its own body); \
         entry: {:?}",
        tube_entry
    );

    // (c) Pin: the default-only conformer must carry is_override = false.
    let pin = module
        .templates
        .iter()
        .find(|t| t.name == "Pin")
        .expect("compiled module must contain a template for 'Pin'");

    let pin_entry = pin
        .assoc_fns
        .iter()
        .find(|e| e.trait_name == "T" && e.fn_name == "f")
        .unwrap_or_else(|| {
            panic!(
                "Pin must carry a (T, f) assoc_fns entry; assoc_fns = {:?}",
                pin.assoc_fns
            )
        });

    assert!(
        !pin_entry.is_override,
        "Pin::f entry must be is_override=false (default injection — Pin has no \
         structure-body fn); entry: {:?}",
        pin_entry
    );
}
