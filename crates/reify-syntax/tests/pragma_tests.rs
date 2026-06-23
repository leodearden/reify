//! Pragma parsing tests.
//!
//! Tests for `#ident` and `#ident(args)` pragma syntax at module and block level.

use reify_ast::*;

/// Helper: parse source and return the ParsedModule.
fn parse_module(source: &str) -> ParsedModule {
    reify_syntax::parse(source, reify_core::ModulePath::single("pragma_test"))
}

// ── Step 1: bare pragma at module level ────────────────────────────

#[test]
fn parse_bare_module_pragma() {
    let source = "#optimize\nstructure S { param x: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(
        module.pragmas.len(),
        1,
        "expected 1 module-level pragma, got {:?}",
        module.pragmas
    );
    assert_eq!(module.pragmas[0].name, "optimize");
    assert!(
        module.pragmas[0].args.is_empty(),
        "expected no args, got {:?}",
        module.pragmas[0].args
    );
}

// ── Step 5/6: pragma with bare value args ────────────────────────

#[test]
fn parse_bare_value_pragma_args() {
    let source = "#feature(sse2, avx)\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "feature");
    assert_eq!(
        pragma.args.len(),
        2,
        "expected 2 args, got {:?}",
        pragma.args
    );

    match &pragma.args[0] {
        PragmaArg::Bare(PragmaValue::Ident(s)) => assert_eq!(s, "sse2"),
        other => panic!("expected Bare(Ident('sse2')), got {:?}", other),
    }

    match &pragma.args[1] {
        PragmaArg::Bare(PragmaValue::Ident(s)) => assert_eq!(s, "avx"),
        other => panic!("expected Bare(Ident('avx')), got {:?}", other),
    }
}

// ── Step 7/8: pragma with mixed args ─────────────────────────────

#[test]
fn parse_mixed_pragma_args() {
    let source = "#config(debug, level=2, name=\"prod\")\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "config");
    assert_eq!(
        pragma.args.len(),
        3,
        "expected 3 args, got {:?}",
        pragma.args
    );

    match &pragma.args[0] {
        PragmaArg::Bare(PragmaValue::Ident(s)) => assert_eq!(s, "debug"),
        other => panic!("expected Bare(Ident('debug')), got {:?}", other),
    }

    match &pragma.args[1] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "level");
            assert_eq!(*value, PragmaValue::Number(2.0));
        }
        other => panic!("expected KeyValue('level', Number(2.0)), got {:?}", other),
    }

    match &pragma.args[2] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "name");
            assert_eq!(*value, PragmaValue::String("prod".to_string()));
        }
        other => panic!("expected KeyValue('name', String('prod')), got {:?}", other),
    }
}

// ── Step 9/10: multiple module-level pragmas ─────────────────────

#[test]
fn parse_multiple_module_pragmas() {
    let source = "#optimize\n#config(level=3)\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(
        module.pragmas.len(),
        2,
        "expected 2 module-level pragmas, got {:?}",
        module.pragmas
    );
    assert_eq!(module.pragmas[0].name, "optimize");
    assert!(module.pragmas[0].args.is_empty());
    assert_eq!(module.pragmas[1].name, "config");
    assert_eq!(module.pragmas[1].args.len(), 1);
}

// ── Step 11/12: block-level pragma inside structure ───────────────

#[test]
fn parse_block_pragma_in_structure() {
    let source = "structure S { #internal\nparam x: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragmas should be empty
    assert!(
        module.pragmas.is_empty(),
        "expected no module-level pragmas, got {:?}",
        module.pragmas
    );

    // Find S and check block-level pragma
    let s = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Structure(s) = d {
            if s.name == "S" { Some(s) } else { None }
        } else {
            None
        }
    });
    let s = s.expect("structure S not found");
    assert_eq!(
        s.pragmas.len(),
        1,
        "expected 1 block-level pragma on S, got {:?}",
        s.pragmas
    );
    assert_eq!(s.pragmas[0].name, "internal");
}

// ── Step 13/14: block-level pragma inside occurrence ─────────────

#[test]
fn parse_block_pragma_in_occurrence() {
    let source = "occurrence P { #temporal\nparam t: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert!(
        module.pragmas.is_empty(),
        "expected no module-level pragmas"
    );

    let p = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Occurrence(p) = d {
            if p.name == "P" { Some(p) } else { None }
        } else {
            None
        }
    });
    let p = p.expect("occurrence P not found");
    assert_eq!(
        p.pragmas.len(),
        1,
        "expected 1 pragma on P, got {:?}",
        p.pragmas
    );
    assert_eq!(p.pragmas[0].name, "temporal");
}

// ── Step 15/16: block-level pragma inside trait ───────────────────

#[test]
fn parse_block_pragma_in_trait() {
    let source = "trait R { #required\nparam mass: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert!(
        module.pragmas.is_empty(),
        "expected no module-level pragmas"
    );

    let r = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Trait(r) = d {
            if r.name == "R" { Some(r) } else { None }
        } else {
            None
        }
    });
    let r = r.expect("trait R not found");
    assert_eq!(
        r.pragmas.len(),
        1,
        "expected 1 pragma on R, got {:?}",
        r.pragmas
    );
    assert_eq!(r.pragmas[0].name, "required");
}

// ── Step 17/18: pragma scoping isolation ─────────────────────────

#[test]
fn parse_pragma_scoping_isolation() {
    let source = "#module_level\nstructure S { #block_level\nparam x: Real }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragma: only "module_level"
    assert_eq!(
        module.pragmas.len(),
        1,
        "expected 1 module-level pragma, got {:?}",
        module.pragmas
    );
    assert_eq!(module.pragmas[0].name, "module_level");

    // Block-level pragma on S: only "block_level"
    let s = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Structure(s) = d {
            if s.name == "S" { Some(s) } else { None }
        } else {
            None
        }
    });
    let s = s.expect("structure S not found");
    assert_eq!(
        s.pragmas.len(),
        1,
        "expected 1 block-level pragma on S, got {:?}",
        s.pragmas
    );
    assert_eq!(s.pragmas[0].name, "block_level");
}

// ── Step 19/20: boolean and number value types ────────────────────

#[test]
fn parse_pragma_bool_and_number_values() {
    let source = "#feature(enabled=true, count=42)\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "feature");
    assert_eq!(
        pragma.args.len(),
        2,
        "expected 2 args, got {:?}",
        pragma.args
    );

    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "enabled");
            assert_eq!(*value, PragmaValue::Bool(true));
        }
        other => panic!("expected KeyValue('enabled', Bool(true)), got {:?}", other),
    }

    match &pragma.args[1] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "count");
            assert_eq!(*value, PragmaValue::Number(42.0));
        }
        other => panic!("expected KeyValue('count', Number(42.0)), got {:?}", other),
    }
}

// ── Step 23: block-level pragma inside purpose ────────────────────

#[test]
fn parse_block_pragma_in_purpose() {
    let source = "purpose Optimize(s : Structure) { #solver\nconstraint s.x > 0 }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragmas should be empty
    assert!(
        module.pragmas.is_empty(),
        "expected no module-level pragmas, got {:?}",
        module.pragmas
    );

    let p = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Purpose(p) = d {
            if p.name == "Optimize" { Some(p) } else { None }
        } else {
            None
        }
    });
    let p = p.expect("purpose Optimize not found");
    assert_eq!(
        p.pragmas.len(),
        1,
        "expected 1 pragma on Optimize, got {:?}",
        p.pragmas
    );
    assert_eq!(p.pragmas[0].name, "solver");
    assert!(
        p.pragmas[0].args.is_empty(),
        "expected no args, got {:?}",
        p.pragmas[0].args
    );
}

// ── Step 25: block-level pragma inside constraint def ─────────────

#[test]
fn parse_block_pragma_in_constraint() {
    let source = "constraint def Positive { #validate\nparam x: Real\nx > 0 }";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );

    // Module-level pragmas should be empty
    assert!(
        module.pragmas.is_empty(),
        "expected no module-level pragmas, got {:?}",
        module.pragmas
    );

    let c = module.declarations.iter().find_map(|d| {
        if let reify_ast::Declaration::Constraint(c) = d {
            if c.name == "Positive" { Some(c) } else { None }
        } else {
            None
        }
    });
    let c = c.expect("constraint def Positive not found");
    assert_eq!(
        c.pragmas.len(),
        1,
        "expected 1 pragma on Positive, got {:?}",
        c.pragmas
    );
    assert_eq!(c.pragmas[0].name, "validate");
    assert!(
        c.pragmas[0].args.is_empty(),
        "expected no args, got {:?}",
        c.pragmas[0].args
    );
}

// ── Quantity pragma values (task 2296: #precision support) ─────

#[test]
fn parse_pragma_with_quantity_metres_value() {
    let source = "#precision(0.001m)\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "precision");
    assert_eq!(
        pragma.args.len(),
        1,
        "expected 1 arg, got {:?}",
        pragma.args
    );

    match &pragma.args[0] {
        PragmaArg::Bare(PragmaValue::Quantity { value, unit }) => {
            assert_eq!(*value, 0.001);
            assert_eq!(unit, "m");
        }
        other => panic!("expected Bare(Quantity{{0.001, 'm'}}), got {:?}", other),
    }
}

#[test]
fn parse_pragma_with_quantity_mm_value() {
    let source = "#precision(1mm)\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "precision");
    assert_eq!(
        pragma.args.len(),
        1,
        "expected 1 arg, got {:?}",
        pragma.args
    );

    match &pragma.args[0] {
        PragmaArg::Bare(PragmaValue::Quantity { value, unit }) => {
            assert_eq!(*value, 1.0);
            assert_eq!(unit, "mm");
        }
        other => panic!("expected Bare(Quantity{{1.0, 'mm'}}), got {:?}", other),
    }
}

// ── Step 3/4: pragma with key=value args ─────────────────────────

#[test]
fn parse_key_value_pragma_args() {
    let source = "#config(level=3, name=\"test\")\nstructure S {}";
    let module = parse_module(source);
    assert!(
        module.errors.is_empty(),
        "parse errors: {:?}",
        module.errors
    );
    assert_eq!(module.pragmas.len(), 1, "expected 1 pragma");

    let pragma = &module.pragmas[0];
    assert_eq!(pragma.name, "config");
    assert_eq!(
        pragma.args.len(),
        2,
        "expected 2 args, got {:?}",
        pragma.args
    );

    // First arg: level=3
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "level");
            assert_eq!(*value, PragmaValue::Number(3.0));
        }
        other => panic!("expected KeyValue, got {:?}", other),
    }

    // Second arg: name="test"
    match &pragma.args[1] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "name");
            assert_eq!(*value, PragmaValue::String("test".to_string()));
        }
        other => panic!("expected KeyValue, got {:?}", other),
    }
}

// ── Pragma-value range rejection (task #4681) ─────────────────────────────────

/// `#config(tol=1e400)` — overflow in a pragma number arm: must produce a
/// parse error rather than silently propagating +Inf.
#[test]
fn pragma_number_overflow_1e400_is_rejected() {
    let module = parse_module("#config(tol=1e400)\nstructure S {}");
    assert!(
        !module.errors.is_empty(),
        "pragma with 1e400 should produce a parse error (overflow → Inf rejected); got empty error list"
    );
    let msg = module.errors[0].message.to_lowercase();
    assert!(
        msg.contains("overflow") || msg.contains("out of range") || msg.contains("1e400"),
        "error message should mention overflow or the literal; got: {:?}",
        module.errors[0].message
    );
}

/// `#config(tol=1e-400)` — underflow in a pragma number arm: must produce a
/// parse error rather than silently propagating 0.0.
#[test]
fn pragma_number_underflow_1e_minus_400_is_rejected() {
    let module = parse_module("#config(tol=1e-400)\nstructure S {}");
    assert!(
        !module.errors.is_empty(),
        "pragma with 1e-400 should produce a parse error (underflow → 0.0 rejected); got empty error list"
    );
    let msg = module.errors[0].message.to_lowercase();
    assert!(
        msg.contains("underflow") || msg.contains("out of range") || msg.contains("1e-400"),
        "error message should mention underflow or the literal; got: {:?}",
        module.errors[0].message
    );
}

/// `#config(level=2)` regression — valid pragma number must still parse without
/// error and lower to `PragmaValue::Number(2.0)`.
#[test]
fn pragma_number_valid_level_2_still_accepted() {
    let module = parse_module("#config(level=2)\nstructure S {}");
    assert!(
        module.errors.is_empty(),
        "pragma with level=2 should have no errors; got: {:?}",
        module.errors
    );
    let pragma = &module.pragmas[0];
    match &pragma.args[0] {
        PragmaArg::KeyValue { key, value } => {
            assert_eq!(key, "level");
            assert_eq!(*value, PragmaValue::Number(2.0));
        }
        other => panic!("expected KeyValue(level, Number(2.0)); got {:?}", other),
    }
}

/// `#config(tol=1e400mm)` — overflow in a pragma quantity arm: must produce a
/// parse error.
#[test]
fn pragma_quantity_overflow_1e400mm_is_rejected() {
    let module = parse_module("#config(tol=1e400mm)\nstructure S {}");
    assert!(
        !module.errors.is_empty(),
        "pragma with 1e400mm should produce a parse error (overflow → Inf rejected); got empty error list"
    );
    let msg = module.errors[0].message.to_lowercase();
    assert!(
        msg.contains("overflow") || msg.contains("out of range") || msg.contains("1e400"),
        "error message should mention overflow or the literal; got: {:?}",
        module.errors[0].message
    );
}
