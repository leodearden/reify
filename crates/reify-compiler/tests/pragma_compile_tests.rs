//! Pragma compilation tests.
//!
//! Tests for compiling `#name` and `#name(args)` pragmas at module and block level.

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only, warnings_only};

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
    let module =
        compile_source("#precision(value=64)\n#version(1)\nstructure S { param x : Real }");
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
            assert_eq!(n, &1.0_f64, "expected version 1, got {n}");
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
