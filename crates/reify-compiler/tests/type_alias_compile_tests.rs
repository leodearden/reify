//! Tests for type alias registry and resolution (task 145).
//!
//! Validates TypeAliasEntry, TypeAliasRegistry, alias compilation in the pre-pass,
//! dimensional aliases, transitive resolution, cycle detection, parameterized aliases,
//! and integration with existing type resolution paths.

use reify_compiler::{compile, CompiledModule, TypeAliasEntry, TypeAliasRegistry};
use reify_types::{ContentHash, ModulePath, Severity, SourceSpan, Type, Diagnostic};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("alias_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    compile(&parsed)
}

fn errors_only(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn warnings_only(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

// ─── step-1: TypeAliasEntry and TypeAliasRegistry data structures ────────────

#[test]
fn type_alias_entry_fields_exist() {
    let dummy_span = SourceSpan::new(0, 0);
    let hash = ContentHash::of_str("Pressure");
    let entry = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Scalar {
            dimension: reify_types::DimensionVector::LENGTH,
        }),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: dummy_span,
        content_hash: hash,
    };
    assert_eq!(entry.name, "Pressure");
    assert!(entry.resolved_type.is_some());
    assert!(entry.type_params.is_empty());
    assert!(entry.type_expr.is_none());
    assert!(entry.is_pub);
}

#[test]
fn type_alias_registry_new_and_lookup_empty() {
    let reg = TypeAliasRegistry::new();
    assert!(reg.lookup("Pressure").is_none());
    assert!(reg.lookup("Velocity").is_none());
}

#[test]
fn type_alias_registry_register_and_lookup() {
    let mut reg = TypeAliasRegistry::new();
    let entry = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Real),
        type_params: vec![],
        type_expr: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Pressure"),
    };
    assert!(reg.register(entry).is_ok());
    let looked_up = reg.lookup("Pressure");
    assert!(looked_up.is_some());
    assert_eq!(looked_up.unwrap().name, "Pressure");
}

#[test]
fn type_alias_registry_duplicate_register_returns_err() {
    let mut reg = TypeAliasRegistry::new();
    let entry1 = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Real),
        type_params: vec![],
        type_expr: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Pressure"),
    };
    let entry2 = TypeAliasEntry {
        name: "Pressure".to_string(),
        resolved_type: Some(Type::Int),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: SourceSpan::new(10, 15),
        content_hash: ContentHash::of_str("Pressure2"),
    };
    assert!(reg.register(entry1).is_ok());
    assert!(reg.register(entry2).is_err());
}

// ─── step-3: simple alias compilation ────────────────────────────────────────

#[test]
fn simple_alias_compiles_without_errors() {
    let source = r#"
        type Pressure = Force
        structure S {
            param p : Pressure = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for simple alias; got: {:?}",
        errs
    );
}

// ─── step-5: dimensional alias ───────────────────────────────────────────────

#[test]
fn dimensional_alias_force_div_area() {
    let source = r#"
        type Pressure = Force / Area
        structure S {
            param p : Pressure = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for dimensional alias; got: {:?}",
        errs
    );
    // Verify the param type is Scalar with FORCE/AREA dimension
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    let expected_dim = reify_types::dimension::FORCE.div(&reify_types::DimensionVector::AREA);
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: expected_dim,
        },
        "Pressure alias should resolve to Scalar{{FORCE/AREA}}"
    );
}

#[test]
fn dimensional_alias_force_mul_length() {
    let source = r#"
        type Energy = Force * Length
        structure S {
            param e : Energy = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for dimensional alias; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let e_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "e")
        .expect("e not found");
    let expected_dim = reify_types::dimension::FORCE.mul(&reify_types::DimensionVector::LENGTH);
    assert_eq!(
        e_cell.cell_type,
        Type::Scalar {
            dimension: expected_dim,
        },
        "Energy alias should resolve to Scalar{{FORCE*LENGTH}}"
    );
}

// ─── step-7: chained dimensional alias ──────────────────────────────────────

#[test]
fn chained_dimensional_alias_acceleration() {
    let source = r#"
        type Velocity = Length / Time
        type Acceleration = Velocity / Time
        structure S {
            param a : Acceleration = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for chained dimensional alias; got: {:?}",
        errs
    );
    // Acceleration should be LENGTH / TIME^2
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let a_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "a")
        .expect("a not found");
    // LENGTH / TIME = Velocity, then Velocity / TIME = LENGTH / TIME^2
    let velocity_dim = reify_types::DimensionVector::LENGTH.div(&reify_types::DimensionVector::TIME);
    let expected_dim = velocity_dim.div(&reify_types::DimensionVector::TIME);
    assert_eq!(
        a_cell.cell_type,
        Type::Scalar {
            dimension: expected_dim,
        },
        "Acceleration alias should resolve to Scalar{{LENGTH/TIME^2}}"
    );
}

// ─── step-9: circular alias detection ───────────────────────────────────────

#[test]
fn circular_alias_a_b_a_produces_error() {
    let source = r#"
        type A = B
        type B = A
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter().any(|d| d.message.contains("circular")),
        "expected circular alias error; got: {:?}",
        errs
    );
}

#[test]
fn self_referential_alias_produces_error() {
    let source = r#"
        type X = X
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter().any(|d| d.message.contains("circular")),
        "expected circular alias error for self-reference; got: {:?}",
        errs
    );
}

// ─── step-11: duplicate alias name ──────────────────────────────────────────

#[test]
fn duplicate_alias_name_produces_error() {
    let source = r#"
        type Foo = Int
        type Foo = Real
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter().any(|d| d.message.contains("duplicate")),
        "expected duplicate alias error; got: {:?}",
        errs
    );
    // Should have span labels pointing to both declarations
    let dup_err = errs
        .iter()
        .find(|d| d.message.contains("duplicate"))
        .unwrap();
    assert!(
        dup_err.labels.len() >= 2,
        "expected at least 2 span labels (original + duplicate); got: {:?}",
        dup_err.labels
    );
}

// ─── step-13: parameterized alias ───────────────────────────────────────────

#[test]
fn parameterized_alias_substitution() {
    // type Measure<Q> = Q
    // When instantiated as Measure<Force>, Q is substituted with Force,
    // so param p should have type Scalar{FORCE}.
    let source = r#"
        type Measure<Q> = Q
        structure S {
            param p : Measure<Force> = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for parameterized alias; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "Measure<Force> alias should resolve to Scalar{{FORCE}}"
    );
}

// ─── step-15: parameterized alias with default ──────────────────────────────

#[test]
fn parameterized_alias_with_default() {
    // type Measure<Q = Force> = Q
    // When used as bare `Measure` (zero type args), Q should default to Force.
    let source = r#"
        type Measure<Q = Force> = Q
        structure S {
            param p : Measure = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for alias with default type param; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "Measure (defaulting Q=Force) should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn multi_param_alias_with_partial_defaults() {
    // type BiMeasure<A, B = Length> = A
    // When used as `BiMeasure<Mass>`, A=Mass and B=Length (default).
    let source = r#"
        type BiMeasure<A, B = Length> = A
        structure S {
            param p : BiMeasure<Mass> = 1mm
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for multi-param alias with partial default; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_types::DimensionVector::MASS,
        },
        "BiMeasure<Mass> (A=Mass, B=Length default) should resolve to Scalar{{MASS}}"
    );
}

// ─── step-17: alias used in various contexts ───────────────────────────────

#[test]
fn alias_as_function_param_type() {
    let source = r#"
        type Pressure = Force
        fn measure(p: Pressure) -> Real { p }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias as function param type should not produce errors; got: {:?}",
        errs
    );
    // Verify function param has the correct type
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "measure")
        .expect("measure function not found");
    assert_eq!(
        func.params[0].1,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "function param typed as Pressure alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_function_return_type() {
    let source = r#"
        type Pressure = Force
        fn compute(x: Real) -> Pressure { x }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias as function return type should not produce errors; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "compute")
        .expect("compute function not found");
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "function return type Pressure alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_field_domain_codomain_type() {
    let source = r#"
        type Pressure = Force
        field def f : Point3 -> Pressure { source = analytical { |p| p } }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias as field codomain type should not produce errors; got: {:?}",
        errs
    );
    // Verify field codomain resolved to the alias target (not StructureRef)
    let field = module
        .fields
        .iter()
        .find(|f| f.name == "f")
        .expect("field f not found");
    assert_eq!(
        field.codomain_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "field codomain typed as Pressure alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_trait_member_type() {
    let source = r#"
        type Pressure = Force
        trait HasPressure {
            param p : Pressure
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias as trait member type should not produce errors; got: {:?}",
        errs
    );
}

// ─── step-19: pub alias visibility ─────────────────────────────────────────

#[test]
fn pub_alias_has_is_pub_true() {
    let source = r#"
        pub type Pressure = Force
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "pub alias should compile cleanly; got: {:?}", errs);
    // Verify via compiled module output
    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Pressure")
        .expect("Pressure alias not found in compiled module type_aliases");
    assert!(alias.is_pub, "pub type alias should have is_pub=true");
}

#[test]
fn non_pub_alias_has_is_pub_false() {
    let source = r#"
        type Velocity = Length
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "non-pub alias should compile cleanly; got: {:?}", errs);
    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Velocity")
        .expect("Velocity alias not found in compiled module type_aliases");
    assert!(!alias.is_pub, "non-pub type alias should have is_pub=false");
}

// ─── step-21: alias with non-dimensional parameterized RHS ─────────────────

#[test]
fn alias_list_of_string() {
    // type StringList = List<String>
    // When used as a param type, should resolve to Type::List(Box::new(Type::String))
    let source = r#"
        type StringList = List<String>
        structure S {
            param p : StringList = ["hello"]
        }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias with List<String> RHS should compile without errors; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::List(Box::new(Type::String)),
        "StringList alias should resolve to List<String>"
    );
}

#[test]
fn parameterized_alias_map_instantiation() {
    // type IntMap<V> = Map<Int, V>
    // When used as IntMap<String>, V=String → Map<Int, String>
    let source = r#"
        type IntMap<V> = Map<Int, V>
        fn identity(m: IntMap<String>) -> IntMap<String> { m }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "parameterized alias with Map<Int, V> should compile without errors; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "identity")
        .expect("identity function not found");
    assert_eq!(
        func.params[0].1,
        Type::Map(Box::new(Type::Int), Box::new(Type::String)),
        "IntMap<String> alias should resolve to Map<Int, String>"
    );
}

// ─── step-23: alias interop with existing declarations ─────────────────────

#[test]
fn alias_interop_mixed_declarations() {
    // Type alias coexists with structure, function, and enum declarations.
    // Alias is used as param type in structure and function params.
    let source = r#"
        type Pressure = Force
        enum Mode { Active Passive }
        structure Tank {
            param pressure : Pressure = 1mm
        }
        fn measure(p: Pressure) -> Real { p }
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias interop with mixed declarations should compile cleanly; got: {:?}",
        errs
    );
    // Verify structure param type
    let template = module.templates.iter().find(|t| t.name == "Tank").expect("Tank not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "pressure")
        .expect("pressure not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "Tank.pressure should resolve to Scalar{{FORCE}}"
    );
    // Verify function param type
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "measure")
        .expect("measure function not found");
    assert_eq!(
        func.params[0].1,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "function param typed as Pressure should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_declared_after_use_forward_reference() {
    // Alias declared after its first use in a structure.
    // Since aliases are collected in pre-pass, declaration order shouldn't matter.
    let source = r#"
        structure S {
            param p : Pressure = 1mm
        }
        type Pressure = Force
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "forward-referenced alias should compile cleanly; got: {:?}",
        errs
    );
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_types::dimension::FORCE,
        },
        "forward-referenced Pressure alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_forward_ref_function() {
    // Function uses alias that is declared later in the source.
    let source = r#"
        fn compute(x: Velocity) -> Real { x }
        type Velocity = Length
    "#;
    let module = parse_and_compile(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "forward-referenced alias in function should compile cleanly; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "compute")
        .expect("compute function not found");
    assert_eq!(
        func.params[0].1,
        Type::Scalar {
            dimension: reify_types::DimensionVector::LENGTH,
        },
        "forward-referenced Velocity alias should resolve to Scalar{{LENGTH}}"
    );
}
