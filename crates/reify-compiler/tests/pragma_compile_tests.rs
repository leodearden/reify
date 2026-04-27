//! Pragma compilation tests.
//!
//! Tests for compiling `#name` and `#name(args)` pragmas at module and block level.

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only, warnings_only};
use reify_types::Severity;

/// Helper: filter warnings whose message contains the given substring.
fn pragma_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
    substr: &str,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| d.message.contains(substr))
        .collect()
}

// ── Step 3: CompiledModule.pragmas stores all module-level pragmas ────────────

/// Module-level pragmas are stored on CompiledModule.pragmas with correct names/args.
#[test]
fn module_pragmas_stored_on_compiled_module() {
    // `#version(0.1)` chosen because it is the supported-version happy path
    // (silent — no errors or warnings). Earlier this fixture used `#version(1)`,
    // but task 2305 made `#version` semantic: with integer-valued Number support
    // (so `0.0` parses as `(0, 0)`), `#version(1)` parses as `(1, 0)` which is
    // too-new and would raise an error.
    let module =
        compile_source("#precision(value=64)\n#version(0.1)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    // Should have stored both pragmas
    assert_eq!(
        module.pragmas.len(),
        2,
        "expected 2 pragmas, got {}: {:?}",
        module.pragmas.len(),
        module.pragmas
    );

    let precision = module.pragmas.iter().find(|p| p.name == "precision");
    assert!(
        precision.is_some(),
        "#precision pragma not found in module.pragmas"
    );
    let precision = precision.unwrap();
    assert_eq!(precision.args.len(), 1, "expected 1 arg on #precision");
    match &precision.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "value");
            assert_eq!(value, &reify_syntax::PragmaValue::Number(64.0));
        }
        other => panic!("expected KeyValue arg on #precision, got: {:?}", other),
    }

    let version = module.pragmas.iter().find(|p| p.name == "version");
    assert!(
        version.is_some(),
        "#version pragma not found in module.pragmas"
    );
    let version = version.unwrap();
    assert_eq!(version.args.len(), 1, "expected 1 arg on #version");
    match &version.args[0] {
        reify_syntax::PragmaArg::Bare(reify_syntax::PragmaValue::Number(n)) => {
            assert_eq!(n, &0.1_f64, "expected version 0.1, got {n}");
        }
        other => panic!("expected Bare(Number) arg on #version, got: {:?}", other),
    }
}

// ── Step 5: #no_prelude suppresses stdlib ─────────────────────────────────────

/// With #no_prelude and no stdlib-specific names, compilation should succeed.
#[test]
fn no_prelude_simple_structure_compiles_clean() {
    let module = compile_source_with_stdlib("#no_prelude\nstructure S { param x : Real = 1.0 }");
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for simple #no_prelude structure, got: {:?}",
        errs
    );
}

/// With #no_prelude, stdlib-only units like `km` should NOT be resolved — expect errors.
/// `km` is only in the stdlib prelude (not in the hardcoded unit_to_scalar fallback),
/// so suppressing the prelude must cause an "unknown unit" error.
#[test]
fn no_prelude_suppresses_stdlib_units() {
    let module = compile_source_with_stdlib("#no_prelude\nstructure S { param x : Length = 10km }");
    let errs = errors_only(&module);
    assert!(
        !errs.is_empty(),
        "expected errors when using stdlib-only unit `km` with #no_prelude, but got none"
    );
}

/// Without #no_prelude, stdlib-only units like `km` should resolve cleanly.
#[test]
fn without_no_prelude_stdlib_units_resolve() {
    let module = compile_source_with_stdlib("structure S { param x : Length = 10km }");
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors when using stdlib unit `km` without #no_prelude, got: {:?}",
        errs
    );
}

// ── Step 1: module-level unknown pragma warnings ─────────────────────────────

/// Unknown module-level pragma `#optimize` should emit an "unknown pragma" warning.
#[test]
fn unknown_module_pragma_emits_warning() {
    let module = compile_source("#optimize\nstructure S { param x : Real }");
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected an 'unknown pragma' warning for #optimize, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("optimize")),
        "warning should mention 'optimize', got: {:?}",
        warns
    );
}

/// Known module-level pragma `#precision` should NOT emit an unknown-pragma warning.
#[test]
fn known_module_pragma_no_warning() {
    let module = compile_source("#precision(value=64)\nstructure S { param x : Real }");
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        warns.is_empty(),
        "expected no 'unknown pragma' warning for #precision, got: {:?}",
        warns
    );
}

// ── Step 9: trait-level and purpose-level pragmas propagated ─────────────────

/// Block-level pragma on a trait body is propagated to CompiledTrait.pragmas.
#[test]
fn trait_pragma_propagated_to_compiled_trait() {
    let module = compile_source("trait T { #precision(bits=32) param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait");
    let trait_def = &module.trait_defs[0];
    assert_eq!(
        trait_def.pragmas.len(),
        1,
        "expected 1 pragma on trait, got {}: {:?}",
        trait_def.pragmas.len(),
        trait_def.pragmas
    );
    let precision = &trait_def.pragmas[0];
    assert_eq!(
        precision.name, "precision",
        "expected pragma name 'precision'"
    );
    assert_eq!(precision.args.len(), 1, "expected 1 arg on #precision");
    match &precision.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "bits");
            assert_eq!(value, &reify_syntax::PragmaValue::Number(32.0));
        }
        other => panic!("expected KeyValue arg on #precision, got: {:?}", other),
    }
}

/// Block-level pragma on a purpose body is propagated to CompiledPurpose.pragmas.
#[test]
fn purpose_pragma_propagated_to_compiled_purpose() {
    let source = r#"
        structure S { param x : Real = 0.0 }
        purpose p(s : Structure) {
            #solver(method="gradient")
            constraint 1 > 0
        }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.compiled_purposes.len(), 1, "expected 1 purpose");
    let purpose = &module.compiled_purposes[0];
    assert_eq!(
        purpose.pragmas.len(),
        1,
        "expected 1 pragma on purpose, got {}: {:?}",
        purpose.pragmas.len(),
        purpose.pragmas
    );
    let solver = &purpose.pragmas[0];
    assert_eq!(solver.name, "solver", "expected pragma name 'solver'");
    assert_eq!(solver.args.len(), 1, "expected 1 arg on #solver");
    match &solver.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "method");
            assert_eq!(
                value,
                &reify_syntax::PragmaValue::String("gradient".to_string())
            );
        }
        other => panic!("expected KeyValue arg on #solver, got: {:?}", other),
    }
}

// ── Step 7: entity-level pragmas propagated to TopologyTemplate ───────────────

/// Block-level pragma on a structure body is propagated to TopologyTemplate.pragmas.
#[test]
fn structure_pragma_propagated_to_topology_template() {
    let module = compile_source(r#"structure S { #solver(backend="ipopt") param x : Real }"#);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(module.templates.len(), 1, "expected 1 template");
    let template = &module.templates[0];
    assert_eq!(
        template.pragmas.len(),
        1,
        "expected 1 pragma on template, got {}: {:?}",
        template.pragmas.len(),
        template.pragmas
    );
    let solver = &template.pragmas[0];
    assert_eq!(solver.name, "solver", "expected pragma name 'solver'");
    assert_eq!(solver.args.len(), 1, "expected 1 arg on #solver");
    match &solver.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "backend");
            assert_eq!(
                value,
                &reify_syntax::PragmaValue::String("ipopt".to_string())
            );
        }
        other => panic!("expected KeyValue arg on #solver, got: {:?}", other),
    }
}

// ── Step 11: block-level unknown pragma warnings ──────────────────────────────

/// Unknown pragma `#turbo` inside a structure body should emit a warning.
#[test]
fn unknown_structure_pragma_emits_warning() {
    let module = compile_source(r#"structure S { #turbo param x : Real }"#);
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected an 'unknown pragma' warning for #turbo on structure, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("turbo")),
        "warning should mention 'turbo', got: {:?}",
        warns
    );
}

/// Unknown pragma `#fast` inside a trait body should emit a warning.
#[test]
fn unknown_trait_pragma_emits_warning() {
    let module = compile_source(r#"trait T { #fast param x : Real }"#);
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected an 'unknown pragma' warning for #fast on trait, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("fast")),
        "warning should mention 'fast', got: {:?}",
        warns
    );
}

/// Unknown pragma `#accel` inside a purpose body should emit a warning.
#[test]
fn unknown_purpose_pragma_emits_warning() {
    let module = compile_source(
        r#"
        structure S { param x : Real = 0.0 }
        purpose p(s : Structure) {
            #accel
            constraint 1 > 0
        }
        "#,
    );
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        !warns.is_empty(),
        "expected an 'unknown pragma' warning for #accel on purpose, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("accel")),
        "warning should mention 'accel', got: {:?}",
        warns
    );
}

/// Known block pragma `#precision` on a structure should NOT emit an unknown-pragma warning.
#[test]
fn known_block_pragma_precision_no_warning_on_structure() {
    let module = compile_source(r#"structure S { #precision(bits=64) param x : Real }"#);
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        warns.is_empty(),
        "expected no 'unknown pragma' warning for #precision on structure, got: {:?}",
        warns
    );
}

/// Known block pragma `#solver` on a trait should NOT emit an unknown-pragma warning.
#[test]
fn known_block_pragma_solver_no_warning_on_trait() {
    let module = compile_source(r#"trait T { #solver(backend="ipopt") param x : Real }"#);
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        warns.is_empty(),
        "expected no 'unknown pragma' warning for #solver on trait, got: {:?}",
        warns
    );
}

// ── Step A: context-aware module-only pragma warning ─────────────────────────

/// Module-only pragma `#no_prelude` on a structure block should emit a
/// "only valid at module level" warning, not the generic "unknown pragma" one.
///
/// The new assertion `pragma_warnings(&module, "unknown pragma").is_empty()` pins
/// the absence of the legacy generic warning so a regression that emits both
/// (module-only + unknown-pragma), or reverts the classify_pragma split in
/// annotations.rs back to a single generic-unknown branch, would fail here.
/// Contract: `#no_prelude` on a block emits *exactly* the module-only warning —
/// neither less nor more.
#[test]
fn no_prelude_on_structure_emits_module_only_warning() {
    let module = compile_source(r#"structure S { #no_prelude param x : Real }"#);
    let warns = pragma_warnings(&module, "no_prelude");
    assert!(
        !warns.is_empty(),
        "expected a warning for #no_prelude on structure, got none; all warnings: {:?}",
        warnings_only(&module)
    );
    assert!(
        warns.iter().any(|d| d.message.contains("only valid at module level")),
        "expected warning to mention 'only valid at module level', got: {:?}",
        warns
    );
    assert!(
        pragma_warnings(&module, "unknown pragma").is_empty(),
        "expected no legacy 'unknown pragma' warning for #no_prelude on a block, got: {:?}",
        pragma_warnings(&module, "unknown pragma")
    );
}

/// Known block pragma `#kernel` on a purpose should NOT emit an unknown-pragma warning.
#[test]
fn known_block_pragma_kernel_no_warning_on_purpose() {
    let module = compile_source(
        r#"
        structure S { param x : Real = 0.0 }
        purpose p(s : Structure) {
            #kernel(name="my_kernel")
            constraint 1 > 0
        }
        "#,
    );
    let warns = pragma_warnings(&module, "unknown pragma");
    assert!(
        warns.is_empty(),
        "expected no 'unknown pragma' warning for #kernel on purpose, got: {:?}",
        warns
    );
}

// ── Step B: module pragma contributes to content_hash ────────────────────────

/// Changing a module-level pragma value must produce a different content_hash,
/// and compiling the same source twice must produce an identical hash (determinism).
///
/// Fails on current main: `compute_module_hash` does not include `parsed.pragmas`,
/// so two sources differing only in `#precision(value=...)` produce identical hashes.
#[test]
fn module_pragma_change_changes_module_content_hash() {
    let path = reify_types::ModulePath::single("m");

    let source_a = "#precision(value=32)\nstructure S { param x : Real }";
    let parsed_a = reify_syntax::parse(source_a, path.clone());
    assert!(parsed_a.errors.is_empty(), "parse errors in a: {:?}", parsed_a.errors);
    let compiled_a = reify_compiler::compile(&parsed_a);

    let source_b = "#precision(value=64)\nstructure S { param x : Real }";
    let parsed_b = reify_syntax::parse(source_b, path.clone());
    assert!(parsed_b.errors.is_empty(), "parse errors in b: {:?}", parsed_b.errors);
    let compiled_b = reify_compiler::compile(&parsed_b);

    assert_ne!(
        compiled_a.content_hash, compiled_b.content_hash,
        "sources differing only in module-level pragma should produce different content_hashes"
    );

    // Determinism: compiling the same source twice yields the same hash.
    let parsed_a2 = reify_syntax::parse(source_a, path.clone());
    let compiled_a2 = reify_compiler::compile(&parsed_a2);
    assert_eq!(
        compiled_a.content_hash, compiled_a2.content_hash,
        "same source compiled twice should produce identical content_hashes"
    );
}

/// Changing a block-level pragma value must produce a different content_hash,
/// and compiling the same source twice must produce an identical hash (determinism).
///
/// This exercises a different path than `module_pragma_change_changes_module_content_hash`:
/// block-level pragmas are stored on `TopologyTemplate.pragmas` (via entity.rs:1772
/// `pragmas: structure.pragmas.to_vec()`), and the template's `content_hash` is
/// computed in the `let content_hash = { ... }` block in entity.rs. The module hash
/// incorporates templates via `ctx.templates.iter().map(|t| t.content_hash)` (hash.rs:27),
/// so block-level pragma changes must propagate: template.content_hash → module.content_hash.
///
/// Pins the block-level pragma → template.content_hash → module.content_hash propagation
/// chain now implemented in entity.rs (pragma folding in the `let content_hash = { ... }` block,
/// entity.rs:1530-1543). This is a passing regression guard — not a known-failing test.
#[test]
fn block_pragma_change_changes_module_content_hash() {
    let path = reify_types::ModulePath::single("m");

    let source_a = "structure S { #precision(bits=32) param x : Real }";
    let parsed_a = reify_syntax::parse(source_a, path.clone());
    assert!(parsed_a.errors.is_empty(), "parse errors in a: {:?}", parsed_a.errors);
    let compiled_a = reify_compiler::compile(&parsed_a);

    let source_b = "structure S { #precision(bits=64) param x : Real }";
    let parsed_b = reify_syntax::parse(source_b, path.clone());
    assert!(parsed_b.errors.is_empty(), "parse errors in b: {:?}", parsed_b.errors);
    let compiled_b = reify_compiler::compile(&parsed_b);

    assert_ne!(
        compiled_a.content_hash, compiled_b.content_hash,
        "sources differing only in block-level pragma should produce different content_hashes"
    );

    // Determinism: compiling the same source twice yields the same hash.
    let parsed_a2 = reify_syntax::parse(source_a, path.clone());
    let compiled_a2 = reify_compiler::compile(&parsed_a2);
    assert_eq!(
        compiled_a.content_hash, compiled_a2.content_hash,
        "same source compiled twice should produce identical content_hashes"
    );
}

/// Parameterized test covering the remaining (arg variant × value variant) combinations
/// for module-level pragma hashing that are not covered by
/// `module_pragma_change_changes_module_content_hash` (which exercises KeyValue + Number).
///
/// Combined with that test, these cases give full coverage of all four `PragmaValue`
/// variants (Ident, Number, String, Bool) and both `PragmaArg` variants (Bare, KeyValue)
/// as encoded by `hash_pragma_arg` + `hash_pragma_value` in compile_builder/hash.rs.
///
/// Each case is a characterization/regression test: `hash_pragma_value` already encodes
/// every variant with a distinct kind-tag prefix, so these pass on current main.
/// They guard against a regression that drops or merges kind-tag prefixes and thereby
/// produces silent hash collisions between pragma variants.
///
/// Module-level coverage is sufficient here because both module-level and block-level pragma
/// hashing call the same `hash_pragma` helper (compile_builder/hash.rs). A variant-encoding
/// bug in `hash_pragma_value` would surface identically on either path; the block-level
/// path is independently verified by `block_pragma_change_changes_module_content_hash`.
#[test]
fn pragma_value_variants_produce_distinct_content_hashes() {
    let path = reify_types::ModulePath::single("m");

    let cases: &[(&str, &str, &str)] = &[
        // Bare + Ident: #precision(bare_ident_a) vs #precision(bare_ident_b)
        (
            "Bare+Ident",
            "#precision(bare_ident_a)\nstructure S { param x : Real }",
            "#precision(bare_ident_b)\nstructure S { param x : Real }",
        ),
        // KeyValue + Bool: #flag(enabled=true) vs #flag(enabled=false)
        (
            "KeyValue+Bool",
            "#flag(enabled=true)\nstructure S { param x : Real }",
            "#flag(enabled=false)\nstructure S { param x : Real }",
        ),
        // KeyValue + String: #tag(name="a") vs #tag(name="b")
        (
            "KeyValue+String",
            r#"#tag(name="a")
structure S { param x : Real }"#,
            r#"#tag(name="b")
structure S { param x : Real }"#,
        ),
    ];

    for &(label, source_a, source_b) in cases {
        let parsed_a = reify_syntax::parse(source_a, path.clone());
        assert!(
            parsed_a.errors.is_empty(),
            "[{label}] parse errors in a: {:?}",
            parsed_a.errors
        );
        let compiled_a = reify_compiler::compile(&parsed_a);

        let parsed_b = reify_syntax::parse(source_b, path.clone());
        assert!(
            parsed_b.errors.is_empty(),
            "[{label}] parse errors in b: {:?}",
            parsed_b.errors
        );
        let compiled_b = reify_compiler::compile(&parsed_b);

        assert_ne!(
            compiled_a.content_hash, compiled_b.content_hash,
            "[{label}] sources differing only in pragma value should produce different content_hashes"
        );
    }
}

// ── Step C: characterization tests ───────────────────────────────────────────

/// Characterization: block-level pragma on an occurrence def is propagated to
/// TopologyTemplate.pragmas via EntityDefRef::from(&OccurrenceDef) → compile_entity
/// → pragmas: structure.pragmas.to_vec().
///
/// Mirrors `structure_pragma_propagated_to_topology_template`, but for occurrences.
/// No implementation change needed — this guards existing behavior.
#[test]
fn occurrence_pragma_propagated_to_topology_template() {
    let module =
        compile_source(r#"occurrence def O { #solver(backend="ipopt") param x : Real }"#);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "O")
        .expect("template named 'O' not found");

    assert_eq!(
        template.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "expected entity_kind Occurrence"
    );
    assert_eq!(
        template.pragmas.len(),
        1,
        "expected 1 pragma on occurrence template, got {}: {:?}",
        template.pragmas.len(),
        template.pragmas
    );
    let solver = &template.pragmas[0];
    assert_eq!(solver.name, "solver", "expected pragma name 'solver'");
    assert_eq!(solver.args.len(), 1, "expected 1 arg on #solver");
    match &solver.args[0] {
        reify_syntax::PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "backend");
            assert_eq!(
                value,
                &reify_syntax::PragmaValue::String("ipopt".to_string())
            );
        }
        other => panic!("expected KeyValue arg on #solver, got: {:?}", other),
    }
}

/// Characterization: `#no_prelude` at module level is stored on CompiledModule.pragmas
/// via `pragmas: parsed.pragmas.clone()` in ctx.rs.
///
/// Complements the behavioral tests (no_prelude_simple_structure_compiles_clean,
/// no_prelude_suppresses_stdlib_units) with a storage assertion. Guards ctx.rs:161.
#[test]
fn no_prelude_is_stored_on_compiled_module_pragmas() {
    let module =
        compile_source_with_stdlib("#no_prelude\nstructure S { param x : Real = 1.0 }");
    assert!(
        module.pragmas.iter().any(|p| p.name == "no_prelude"),
        "expected #no_prelude in module.pragmas, got: {:?}",
        module.pragmas
    );
}

// ── Task 2296: #precision pragma — default_tolerance plumbing ────────────────

/// Without any `#precision` pragma, `module.default_tolerance` is `None`.
#[test]
fn default_tolerance_none_when_no_precision_pragma() {
    let module = compile_source("structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None when no #precision pragma, got {:?}",
        module.default_tolerance
    );
}

/// `#precision(0.001m)` at module level sets `default_tolerance = Some(0.001)`.
#[test]
fn precision_pragma_with_metres_quantity_sets_default_tolerance() {
    let module = compile_source("#precision(0.001m)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(0.001),
        "expected default_tolerance Some(0.001) for #precision(0.001m)"
    );
}

/// `#precision(1mm)` converts to 0.001 metres on `default_tolerance`.
#[test]
fn precision_pragma_with_mm_unit_converts_to_metres() {
    let module = compile_source("#precision(1mm)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(0.001),
        "expected default_tolerance Some(0.001) for #precision(1mm)"
    );
}

/// `#precision(2cm)` converts to 0.02 metres on `default_tolerance`.
#[test]
fn precision_pragma_with_cm_unit_converts_to_metres() {
    let module = compile_source("#precision(2cm)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(0.02),
        "expected default_tolerance Some(0.02) for #precision(2cm)"
    );
}

/// Multiple module-level `#precision` pragmas: first wins, the second emits
/// exactly one warning indicating it is ignored.
#[test]
fn multiple_module_level_precision_pragmas_first_wins() {
    let module = compile_source(
        "#precision(0.001m)\n#precision(0.002m)\nstructure S { param x : Real }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(0.001),
        "expected default_tolerance Some(0.001) (first wins) for the duplicate pragma case"
    );

    // Filter warnings whose message mentions one of the "ignored / first wins /
    // subsequent" keywords. The PRD requires *exactly one* such warning — for the
    // second `#precision`, not the first.
    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| {
            let m = d.message.to_lowercase();
            m.contains("ignored") || m.contains("first wins") || m.contains("subsequent")
        })
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 warning for subsequent #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(0.001s)` (a Time quantity, not a Length) emits a warning that
/// mentions "Length" and does not set `default_tolerance`. Crucially the
/// warning must NOT be the generic "unknown pragma" warning (which is reserved
/// for unrecognised pragma NAMES, not unrecognised arg shapes).
#[test]
fn precision_pragma_with_non_length_unit_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(0.001s)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for non-Length unit, got {:?}",
        module.default_tolerance
    );

    // Exactly one warning whose message mentions "Length" (case-insensitive)
    // and is NOT the generic "unknown pragma" warning.
    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| {
            let m = d.message.to_lowercase();
            m.contains("length") && !m.contains("unknown pragma")
        })
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 warning mentioning 'Length' for #precision(0.001s), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(float64)` is the legacy ident form: emit a single Info-severity
/// diagnostic explaining it is recognised but ignored, and tell the user to
/// use a Length literal instead. Does not set `default_tolerance`.
#[test]
fn precision_pragma_with_legacy_float64_ident_emits_info() {
    let module = compile_source("#precision(float64)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for #precision(float64), got {:?}",
        module.default_tolerance
    );

    let infos: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Info
                && d.message.contains("recognised but ignored")
                && d.message.contains("Length literal")
        })
        .collect();
    assert_eq!(
        infos.len(),
        1,
        "expected exactly 1 info diagnostic for #precision(float64), got {}: {:?}",
        infos.len(),
        infos
    );
}

/// `#precision(0.001)` — bare number (no unit) — emits exactly one warning that
/// mentions "Length literal" and the example "0.001m"; default_tolerance stays
/// None.
#[test]
fn precision_pragma_with_bare_number_warns_to_use_length_literal() {
    let module = compile_source("#precision(0.001)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for bare-number #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("Length literal") && d.message.contains("0.001m"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 warning mentioning 'Length literal' and '0.001m' for #precision(0.001), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(value=64)` — key=value form — emits exactly one warning that
/// mentions "expected a Length literal"; default_tolerance stays None. Coexists
/// with the existing `module_pragmas_stored_on_compiled_module` test which only
/// checks `errors_only` (so adding a warning is safe).
#[test]
fn precision_pragma_with_keyvalue_arg_warns_unrecognised_form() {
    let module = compile_source("#precision(value=64)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for key=value #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 warning mentioning 'expected a Length literal' for #precision(value=64), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// Helper: filter warnings that match the block-level "deferred to v0.2"
/// shape — message contains "ignored in v0.1" AND ("v0.2" OR "per-block").
fn deferred_v02_warnings<'a>(
    module: &'a reify_compiler::CompiledModule,
) -> Vec<&'a reify_types::Diagnostic> {
    warnings_only(module)
        .into_iter()
        .filter(|d| {
            d.message.contains("ignored in v0.1")
                && (d.message.contains("v0.2") || d.message.contains("per-block"))
        })
        .collect()
}

/// Block-level `#precision` on a structure emits exactly one "ignored in v0.1;
/// per-block tolerance deferred to v0.2" warning, leaves default_tolerance
/// unset, and produces no errors.
#[test]
fn block_level_precision_pragma_emits_deferred_warning() {
    let module = compile_source("structure S { #precision(0.001m) param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "block-level #precision must NOT set the module default, got {:?}",
        module.default_tolerance
    );

    let warns = deferred_v02_warnings(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 deferred-to-v0.2 warning for block-level #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

/// Same deferred-warning behaviour for trait-level `#precision`.
#[test]
fn trait_level_precision_pragma_emits_deferred_warning() {
    let module = compile_source("trait T { #precision(0.001m) param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "trait-level #precision must NOT set the module default, got {:?}",
        module.default_tolerance
    );

    let warns = deferred_v02_warnings(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 deferred-to-v0.2 warning for trait-level #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

/// Same deferred-warning behaviour for purpose-level `#precision`.
#[test]
fn purpose_level_precision_pragma_emits_deferred_warning() {
    let source = r#"
        structure S { param x : Real = 0.0 }
        purpose p(s : Structure) {
            #precision(0.001m)
            constraint 1 > 0
        }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "purpose-level #precision must NOT set the module default, got {:?}",
        module.default_tolerance
    );

    let warns = deferred_v02_warnings(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 deferred-to-v0.2 warning for purpose-level #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

/// Same deferred-warning behaviour for `constraint def`-level `#precision`.
///
/// `warn_block_level_precision` walks `module.constraint_defs` alongside
/// templates / trait_defs / compiled_purposes. Without this test, a refactor
/// that dropped the constraint-def branch would slip through unnoticed.
#[test]
fn constraint_def_level_precision_pragma_emits_deferred_warning() {
    let source = r#"
        constraint def C { #precision(0.001m) param x : Real }
    "#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "constraint-def-level #precision must NOT set the module default, got {:?}",
        module.default_tolerance
    );

    let warns = deferred_v02_warnings(&module);
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 deferred-to-v0.2 warning for constraint-def-level \
         #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision()` (zero args) hits the catch-all "expected a Length literal"
/// branch and emits exactly one warning; `default_tolerance` stays None.
///
/// Distinct match arm from the bare-Number / KeyValue / float64-Ident cases.
#[test]
fn precision_pragma_with_zero_args_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision()\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for zero-arg #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'expected a Length literal' warning for #precision(), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(0.001m, 0.002m)` (multi-arg) hits the catch-all branch and emits
/// exactly one warning; `default_tolerance` stays None even though the first
/// arg in isolation would have been a valid Length quantity. Multi-arg is its
/// own match arm — the catch-all `_` — separate from the well-formed
/// single-Quantity arm above it.
#[test]
fn precision_pragma_with_multiple_args_warns_and_does_not_set_tolerance() {
    let module =
        compile_source("#precision(0.001m, 0.002m)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for multi-arg #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'expected a Length literal' warning for multi-arg \
         #precision, got {}: {:?}",
        warns.len(),
        warns
    );
}

// ── Amendment round 2: extra unit + bare-value coverage ──────────────────────

/// `#precision(1in)` converts the inch literal to its SI metres value.
///
/// The `in` arm in `unit_to_scalar` is the only imperial Length unit we
/// promise to support in v0.1. Without this test, a regression that dropped
/// the `in` match arm would leave the only failing assertion in much further
/// downstream tests.
#[test]
fn precision_pragma_with_in_unit_converts_to_metres() {
    let module = compile_source("#precision(1in)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(0.0254),
        "expected default_tolerance Some(0.0254) for #precision(1in)"
    );
}

/// `#precision("0.001m")` — a bare String argument — hits the catch-all `_`
/// match arm and emits exactly one "expected a Length literal" warning;
/// default_tolerance stays None.
#[test]
fn precision_pragma_with_bare_string_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(\"0.001m\")\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for bare-string #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'expected a Length literal' warning for #precision(\"0.001m\"), \
         got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(true)` — a bare Bool argument — hits the catch-all `_` match
/// arm and emits exactly one "expected a Length literal" warning;
/// default_tolerance stays None.
#[test]
fn precision_pragma_with_bare_bool_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(true)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for bare-bool #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'expected a Length literal' warning for #precision(true), \
         got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(somename)` — a bare Ident other than the legacy `float64` —
/// hits the catch-all `_` match arm and emits exactly one "expected a Length
/// literal" warning; default_tolerance stays None.
///
/// Distinguishes the catch-all branch from the float64-Ident special case
/// (which emits an Info diagnostic with different text).
#[test]
fn precision_pragma_with_non_float64_ident_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(somename)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for non-float64-ident #precision, got {:?}",
        module.default_tolerance
    );

    let warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("expected a Length literal"))
        .collect();
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'expected a Length literal' warning for #precision(somename), \
         got {}: {:?}",
        warns.len(),
        warns
    );
}

// ── Amendment round 2: tolerance range bounds ────────────────────────────────

/// `#precision(0m)` — explicit zero tolerance — is rejected with a "must be
/// positive" warning; default_tolerance stays None so the engine falls back to
/// its built-in default.
#[test]
fn precision_pragma_with_zero_tolerance_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(0m)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for #precision(0m), got {:?}",
        module.default_tolerance
    );

    let warns = pragma_warnings(&module, "must be positive");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'must be positive' warning for #precision(0m), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(1000m)` — far above the v0.1 cap of 1.0m — is rejected with a
/// "exceeds the v0.1 cap" warning; default_tolerance stays None so the engine
/// falls back to its built-in default.
#[test]
fn precision_pragma_above_cap_warns_and_does_not_set_tolerance() {
    let module = compile_source("#precision(1000m)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.default_tolerance.is_none(),
        "expected default_tolerance None for #precision(1000m), got {:?}",
        module.default_tolerance
    );

    let warns = pragma_warnings(&module, "exceeds the v0.1 cap");
    assert_eq!(
        warns.len(),
        1,
        "expected exactly 1 'exceeds the v0.1 cap' warning for #precision(1000m), got {}: {:?}",
        warns.len(),
        warns
    );
}

/// `#precision(1m)` — exactly at the v0.1 cap — is accepted (the bound is
/// inclusive on the upper end).
#[test]
fn precision_pragma_at_upper_cap_is_accepted() {
    let module = compile_source("#precision(1m)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.default_tolerance,
        Some(1.0),
        "expected default_tolerance Some(1.0) for #precision(1m) (cap is inclusive)"
    );
}

// ── Task 2305: #version pragma — declared_version plumbing ────────────────────

/// Without any `#version` pragma, `module.declared_version` is `None`.
#[test]
fn version_pragma_absent_keeps_declared_version_none() {
    let module = compile_source("structure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert!(
        module.declared_version.is_none(),
        "expected declared_version None when no #version pragma, got {:?}",
        module.declared_version
    );
}

/// `#version(0.1)` (Number form, supported version) sets
/// `declared_version = Some((0, 1))` and emits no errors or warnings about version.
#[test]
fn version_pragma_with_number_form_zero_one_sets_declared_version() {
    let module = compile_source("#version(0.1)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.declared_version,
        Some((0, 1)),
        "expected declared_version Some((0, 1)) for #version(0.1)"
    );
    let version_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("version"))
        .collect();
    assert_eq!(
        version_warns.len(),
        0,
        "expected zero 'version'-mentioning warnings for in-range #version(0.1), got {}: {:?}",
        version_warns.len(),
        version_warns
    );
}

/// `#version("0.1")` (String form, supported version) sets
/// `declared_version = Some((0, 1))` and emits no errors or warnings about version.
#[test]
fn version_pragma_with_string_form_zero_one_sets_declared_version() {
    let module = compile_source("#version(\"0.1\")\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.declared_version,
        Some((0, 1)),
        "expected declared_version Some((0, 1)) for #version(\"0.1\")"
    );
    let version_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| d.message.contains("version"))
        .collect();
    assert_eq!(
        version_warns.len(),
        0,
        "expected zero 'version'-mentioning warnings for in-range #version(\"0.1\"), got {}: {:?}",
        version_warns.len(),
        version_warns
    );
}

/// `#version(0.2)` declares a too-new version: emits exactly one error
/// mentioning both "module declares version 0.2" and "this compiler supports
/// up to 0.1", and stores `Some((0, 2))` (storage = declared, per design).
#[test]
fn version_pragma_too_new_number_form_emits_error_with_supported_wording() {
    let module = compile_source("#version(0.2)\nstructure S { param x : Real }");
    let errors: Vec<_> = errors_only(&module);
    let version_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains("module declares version 0.2")
                && d.message.contains("this compiler supports up to 0.1")
        })
        .collect();
    assert_eq!(
        version_errors.len(),
        1,
        "expected exactly 1 too-new version error for #version(0.2), got {}: {:?}",
        version_errors.len(),
        errors
    );
    assert_eq!(
        module.declared_version,
        Some((0, 2)),
        "expected declared_version Some((0, 2)) for #version(0.2) (storage reflects declared)"
    );
}

/// `#version("1.0")` (String form, too new): same error wording, declared_version
/// reflects the user-declared (1, 0).
#[test]
fn version_pragma_too_new_string_form_emits_error_with_supported_wording() {
    let module = compile_source("#version(\"1.0\")\nstructure S { param x : Real }");
    let errors: Vec<_> = errors_only(&module);
    let version_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains("module declares version 1.0")
                && d.message.contains("this compiler supports up to 0.1")
        })
        .collect();
    assert_eq!(
        version_errors.len(),
        1,
        "expected exactly 1 too-new version error for #version(\"1.0\"), got {}: {:?}",
        version_errors.len(),
        errors
    );
    assert_eq!(
        module.declared_version,
        Some((1, 0)),
        "expected declared_version Some((1, 0)) for #version(\"1.0\") (storage reflects declared)"
    );
}

/// `#version(0.0)` declares a too-old version: zero errors, exactly one warning
/// containing "declared version 0.0", "predates the first stable language",
/// and "treating as 0.1". `declared_version` reflects the user-declared tuple.
#[test]
fn version_pragma_too_old_emits_warning_predates_stable() {
    let module = compile_source("#version(0.0)\nstructure S { param x : Real }");
    assert!(
        errors_only(&module).is_empty(),
        "unexpected errors for #version(0.0): {:?}",
        errors_only(&module)
    );
    let predates_warns: Vec<_> = warnings_only(&module)
        .into_iter()
        .filter(|d| {
            d.message.contains("declared version 0.0")
                && d.message.contains("predates the first stable language")
                && d.message.contains("treating as 0.1")
        })
        .collect();
    assert_eq!(
        predates_warns.len(),
        1,
        "expected exactly 1 too-old version warning for #version(0.0), got {}: {:?}",
        predates_warns.len(),
        warnings_only(&module)
    );
    assert_eq!(
        module.declared_version,
        Some((0, 0)),
        "expected declared_version Some((0, 0)) for #version(0.0) (storage reflects declared)"
    );
}
