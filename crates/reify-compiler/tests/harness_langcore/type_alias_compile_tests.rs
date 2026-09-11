//! Tests for type alias registry and resolution (task 145).
//!
//! Validates TypeAliasEntry, TypeAliasRegistry, alias compilation in the pre-pass,
//! dimensional aliases, transitive resolution, cycle detection, parameterized aliases,
//! and integration with existing type resolution paths.

use reify_compiler::CompiledTypeAlias;
use reify_core::{ContentHash, SourceSpan, Type};
use reify_test_support::{compile_source, errors_only};

// ─── step-1: CompiledTypeAlias data structures ──────────────────────────────

#[test]
fn compiled_type_alias_fields_exist() {
    let dummy_span = SourceSpan::new(0, 0);
    let hash = ContentHash::of_str("Stress");
    let alias = CompiledTypeAlias {
        name: "Stress".to_string(),
        resolved_type: Some(Type::Scalar {
            dimension: reify_core::DimensionVector::LENGTH,
        }),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: dummy_span,
        content_hash: hash,
    };
    assert_eq!(alias.name, "Stress");
    assert!(alias.resolved_type.is_some());
    assert!(alias.type_params.is_empty());
    assert!(alias.is_pub);
}

#[test]
fn compiled_alias_appears_in_module_output() {
    // A simple alias should appear in module.type_aliases after compilation.
    let source = r#"
        type Stress = Force
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias should compile cleanly; got: {:?}",
        errs
    );
    let alias = module.type_aliases.iter().find(|a| a.name == "Stress");
    assert!(
        alias.is_some(),
        "Stress alias should appear in module.type_aliases"
    );
    assert_eq!(alias.unwrap().name, "Stress");
}

#[test]
fn compiled_alias_duplicate_produces_diagnostic() {
    // Duplicate alias names should produce an error diagnostic.
    let source = r#"
        type Foo = Int
        type Foo = Real
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter()
            .any(|d| d.message.contains("duplicate") || d.message.contains("Duplicate")),
        "duplicate alias should produce an error; got: {:?}",
        errs
    );
}

// ─── step-3: simple alias compilation ────────────────────────────────────────

#[test]
fn simple_alias_compiles_without_errors() {
    let source = r#"
        type Stress = Force
        structure S {
            param p : Stress = undef
        }
    "#;
    let module = compile_source(source);
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
        type Stress = Force / Area
        structure S {
            param p : Stress = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for dimensional alias; got: {:?}",
        errs
    );
    // Verify the param type is Scalar with FORCE/AREA dimension
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    let expected_dim = reify_core::dimension::FORCE.div(&reify_core::DimensionVector::AREA);
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: expected_dim,
        },
        "Stress alias should resolve to Scalar{{FORCE/AREA}}"
    );
}

#[test]
fn dimensional_alias_force_mul_length() {
    let source = r#"
        type Energy = Force * Length
        structure S {
            param e : Energy = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for dimensional alias; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let e_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "e")
        .expect("e not found");
    let expected_dim = reify_core::dimension::FORCE.mul(&reify_core::DimensionVector::LENGTH);
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
            param a : Acceleration = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for chained dimensional alias; got: {:?}",
        errs
    );
    // Acceleration should be LENGTH / TIME^2
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let a_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "a")
        .expect("a not found");
    // LENGTH / TIME = Velocity, then Velocity / TIME = LENGTH / TIME^2
    let velocity_dim = reify_core::DimensionVector::LENGTH.div(&reify_core::DimensionVector::TIME);
    let expected_dim = velocity_dim.div(&reify_core::DimensionVector::TIME);
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
    let module = compile_source(source);
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
    let module = compile_source(source);
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
    let module = compile_source(source);
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
            param p : Measure<Force> = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for parameterized alias; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_core::dimension::FORCE,
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
            param p : Measure = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for alias with default type param; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_core::dimension::FORCE,
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
            param p : BiMeasure<Mass> = undef
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "expected no errors for multi-param alias with partial default; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_core::DimensionVector::MASS,
        },
        "BiMeasure<Mass> (A=Mass, B=Length default) should resolve to Scalar{{MASS}}"
    );
}

// ─── step-17: alias used in various contexts ───────────────────────────────

#[test]
fn alias_as_function_param_type() {
    let source = r#"
        type Stress = Force
        fn measure(p: Stress) -> Real { p }
    "#;
    let module = compile_source(source);
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
            dimension: reify_core::dimension::FORCE,
        },
        "function param typed as Stress alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_function_return_type() {
    let source = r#"
        type Stress = Force
        fn compute(x: Real) -> Stress { x }
    "#;
    let module = compile_source(source);
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
            dimension: reify_core::dimension::FORCE,
        },
        "function return type Stress alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_field_domain_codomain_type() {
    let source = r#"
        type Stress = Force
        field def f : Point3 -> Stress { source = analytical { |p: Force| p } }
    "#;
    let module = compile_source(source);
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
            dimension: reify_core::dimension::FORCE,
        },
        "field codomain typed as Stress alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_as_trait_member_type() {
    let source = r#"
        type Stress = Force
        trait HasStress {
            param p : Stress
        }
    "#;
    let module = compile_source(source);
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
        pub type Stress = Force
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "pub alias should compile cleanly; got: {:?}",
        errs
    );
    // Verify via compiled module output
    let alias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Stress")
        .expect("Stress alias not found in compiled module type_aliases");
    assert!(alias.is_pub, "pub type alias should have is_pub=true");
}

#[test]
fn non_pub_alias_has_is_pub_false() {
    let source = r#"
        type Velocity = Length
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "non-pub alias should compile cleanly; got: {:?}",
        errs
    );
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
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias with List<String> RHS should compile without errors; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
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
    let module = compile_source(source);
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
        type Stress = Force
        enum Mode { Active, Passive }
        structure Tank {
            param pressure : Stress = undef
        }
        fn measure(p: Stress) -> Real { p }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias interop with mixed declarations should compile cleanly; got: {:?}",
        errs
    );
    // Verify structure param type
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Tank")
        .expect("Tank not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "pressure")
        .expect("pressure not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_core::dimension::FORCE,
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
            dimension: reify_core::dimension::FORCE,
        },
        "function param typed as Stress should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_declared_after_use_forward_reference() {
    // Alias declared after its first use in a structure.
    // Since aliases are collected in pre-pass, declaration order shouldn't matter.
    let source = r#"
        structure S {
            param p : Stress = undef
        }
        type Stress = Force
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "forward-referenced alias should compile cleanly; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: reify_core::dimension::FORCE,
        },
        "forward-referenced Stress alias should resolve to Scalar{{FORCE}}"
    );
}

#[test]
fn alias_forward_ref_function() {
    // Function uses alias that is declared later in the source.
    // NOTE: "Velocity" was originally used here but is now a builtin named
    // dimension (task 4580). Renamed to "Foo" (not a builtin) so the test
    // continues to cover forward-referenced user-alias resolution without
    // shadowing the Velocity builtin. The asserted dimension (LENGTH) is
    // unchanged — Foo = Length still round-trips to Scalar{LENGTH}.
    let source = r#"
        fn compute(x: Foo) -> Real { x }
        type Foo = Length
    "#;
    let module = compile_source(source);
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
            dimension: reify_core::DimensionVector::LENGTH,
        },
        "forward-referenced Foo alias should resolve to Scalar{{LENGTH}}"
    );
}

// ─── step-29: user-defined parameterized alias in alias body ────────────────

#[test]
fn alias_body_references_user_parameterized_alias() {
    // Container<T> is a user-defined parameterized alias.
    // StringList uses Container with concrete type args (not type params).
    // Currently fails because resolve_type_alias_expr's name branch only
    // tries hardcoded builtins for parameterized types, missing user-defined
    // parameterized alias instantiation.
    let source = r#"
        type Container<T> = List<T>
        type StringList = Container<String>
        structure S {
            param p : StringList = ["hello"]
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "user-defined parameterized alias in alias body should compile; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::List(Box::new(Type::String)),
        "StringList (= Container<String>) should resolve to List<String>"
    );
}

#[test]
fn alias_chain_parameterized_pair_concrete_args() {
    // Pair<A, B> = Map<A, B> (user-defined parameterized alias)
    // StringIntMap uses Pair with concrete type args.
    let source = r#"
        type Pair<A, B> = Map<A, B>
        type StringIntMap = Pair<String, Int>
        fn identity(m: StringIntMap) -> StringIntMap { m }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "chained parameterized alias with concrete args should compile; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "identity")
        .expect("identity function not found");
    assert_eq!(
        func.params[0].1,
        Type::Map(Box::new(Type::String), Box::new(Type::Int)),
        "StringIntMap (= Pair<String, Int>) should resolve to Map<String, Int>"
    );
}

// ─── step-31: structured type args in parameterized alias instantiation ────

#[test]
fn parameterized_alias_with_list_type_arg() {
    // Wrapped<T> = Option<T>, instantiated as Wrapped<List<Force>>.
    // The structured type arg List<Force> must be resolved via full expression
    // resolver, not just the simple name resolver.
    let source = r#"
        type Wrapped<T> = Option<T>
        fn take_wrapped(w: Wrapped<List<Force>>) -> Real { 0.0 }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "parameterized alias with structured type arg should compile; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "take_wrapped")
        .expect("take_wrapped function not found");
    let expected = Type::Option(Box::new(Type::List(Box::new(Type::Scalar {
        dimension: reify_core::dimension::FORCE,
    }))));
    assert_eq!(
        func.params[0].1, expected,
        "Wrapped<List<Force>> should resolve to Option<List<Scalar{{FORCE}}>>"
    );
}

#[test]
fn parameterized_alias_with_map_type_arg() {
    // Boxed<T> = List<T>, instantiated as Boxed<Map<String, Int>>.
    let source = r#"
        type Boxed<T> = List<T>
        fn identity(m: Boxed<Map<String, Int>>) -> Boxed<Map<String, Int>> { m }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "parameterized alias with Map type arg should compile; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "identity")
        .expect("identity function not found");
    let expected = Type::List(Box::new(Type::Map(
        Box::new(Type::String),
        Box::new(Type::Int),
    )));
    assert_eq!(
        func.params[0].1, expected,
        "Boxed<Map<String, Int>> should resolve to List<Map<String, Int>>"
    );
}

#[test]
fn parameterized_alias_chain_with_type_param_forwarding() {
    // Wrapped<T> = Container<T> where Container<T> = List<T>.
    // Tests that when Wrapped<Int> is instantiated at a use site,
    // the type param T flows through to Container correctly.
    // This requires resolve_parameterized_alias to use the full
    // expression resolver for type args (not just simple names).
    let source = r#"
        type Container<T> = List<T>
        type Wrapped<T> = Container<T>
        structure S {
            param p : Wrapped<Int> = [1]
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "chained parameterized alias with type param forwarding should compile; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::List(Box::new(Type::Int)),
        "Wrapped<Int> should resolve to List<Int>"
    );
}

// ─── step-25: content hash determinism ───────────────────────────────────────

// NOTE: steps 25-26 already committed (hash determinism fix)

// ─── step-27: incomplete dependency collection in collect_type_expr_names ────

#[test]
fn alias_dependency_via_type_arg_reverse_order() {
    // B depends on A via type arg (not dimensional op).
    // Declared in reverse order (B before A) to test that DFS dependency
    // tracking collects type arg names — not just dimensional operator operands.
    // Currently fails because collect_type_expr_names returns ["List"] for B's
    // body, missing "A", so resolve_alias_dfs won't pre-resolve A before B.
    let source = r#"
        type B = List<A>
        type A = Int
        structure S {
            param p : B = [1]
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "alias with type arg dependency (reverse order) should compile without errors; got: {:?}",
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p not found");
    assert_eq!(
        p_cell.cell_type,
        Type::List(Box::new(Type::Int)),
        "B should resolve to List<Int>"
    );
}

#[test]
fn alias_dependency_map_via_type_args_reverse_order() {
    // Outer depends on Inner via type arg in Map<Inner, String>.
    // Inner declared after Outer to trigger the bug.
    let source = r#"
        type Outer = Map<Inner, String>
        type Inner = Real
        fn identity(m: Outer) -> Outer { m }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "Map alias with type arg dependency should compile without errors; got: {:?}",
        errs
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "identity")
        .expect("identity function not found");
    assert_eq!(
        func.params[0].1,
        Type::Map(
            Box::new(Type::dimensionless_scalar()),
            Box::new(Type::String)
        ),
        "Outer should resolve to Map<Real, String>"
    );
}

#[test]
fn alias_dependency_option_via_type_arg_reverse_order() {
    // Wrapped depends on Base via Option<Base>.
    // Base declared after Wrapped to trigger the bug.
    let source = r#"
        type Wrapped = Option<Base>
        type Base = Force
        structure S {
            param w : Wrapped = 1mm
        }
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "Option alias with type arg dependency should compile without errors; got: {:?}",
        errs
    );
}

#[test]
fn alias_content_hash_deterministic() {
    // Compile a source with 3+ aliases multiple times.
    // If alias_registry.iter() feeds hashes in non-deterministic HashMap order
    // into the order-dependent ContentHash::combine_all, the content_hash could
    // vary between compilations. We run 10 iterations to increase the chance of
    // catching non-deterministic ordering.
    let source = r#"
        type A = Int
        type B = Real
        type C = String
        type D = Bool
        type E = Length
    "#;
    let first_hash = compile_source(source).content_hash;
    for i in 1..10 {
        let hash = compile_source(source).content_hash;
        assert_eq!(
            first_hash, hash,
            "content_hash differed on iteration {} — non-deterministic alias hash ordering",
            i
        );
    }
}

// ─── recursive parameterized alias depth guard ────────────────────────────

#[test]
fn recursive_parameterized_alias_does_not_stack_overflow() {
    // type A<T> = List<A<T>> is recursive in a way that only manifests
    // at use-site instantiation (the DFS pre-pass catches the declaration-level
    // cycle, but instantiation would previously recurse infinitely).
    let source = r#"
        type A<T> = List<A<T>>
        type UseA = A<Real>
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter().any(|d| {
            d.message.contains("circular")
                || d.message.contains("instantiation depth")
                || d.message.contains("recursive")
        }),
        "expected circular/recursive alias error; got: {:?}",
        errs
    );
}

#[test]
fn self_recursive_parameterized_alias_does_not_stack_overflow() {
    // type A<T> = A<T> — direct self-reference with type params
    let source = r#"
        type A<T> = A<T>
        type UseA = A<Int>
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.iter().any(|d| {
            d.message.contains("circular")
                || d.message.contains("instantiation depth")
                || d.message.contains("recursive")
        }),
        "expected circular/recursive alias error for self-reference; got: {:?}",
        errs
    );
}

// ─── step-33: module boundary separation — CompiledTypeAlias ───────────────

#[test]
fn compiled_type_alias_in_module_output() {
    // CompiledTypeAlias should appear in module.type_aliases with only semantic
    // fields (no type_expr from reify_syntax). Verify a pub alias compiles and
    // the CompiledTypeAlias has the correct fields.
    let source = r#"
        pub type Stress = Force
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "pub alias should compile cleanly; got: {:?}",
        errs
    );

    let alias: &CompiledTypeAlias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Stress")
        .expect("Stress alias not found in compiled module type_aliases");

    // Verify semantic fields
    assert_eq!(alias.name, "Stress");
    assert!(alias.is_pub, "pub type alias should have is_pub=true");
    assert!(
        alias.type_params.is_empty(),
        "non-parameterized alias should have empty type_params"
    );
    assert!(
        matches!(alias.resolved_type, Some(Type::Scalar { .. })),
        "Stress should resolve to a Scalar type; got: {:?}",
        alias.resolved_type
    );

    // Verify content_hash is valid (non-zero)
    let zero_hash = ContentHash::of_str("");
    assert_ne!(
        alias.content_hash, zero_hash,
        "content_hash should be meaningful"
    );
}

#[test]
fn compiled_type_alias_parameterized_in_module_output() {
    // Parameterized aliases should also appear as CompiledTypeAlias in module output,
    // with type_params populated and resolved_type=None.
    // Use `T` as the body (identity alias) so the body only references the alias's
    // own type param — no unknown names that would trip the def-site guard.
    let source = r#"
        pub type Container<T> = T
    "#;
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(
        errs.is_empty(),
        "parameterized alias should compile cleanly; got: {:?}",
        errs
    );

    let alias: &CompiledTypeAlias = module
        .type_aliases
        .iter()
        .find(|a| a.name == "Container")
        .expect("Container alias not found in compiled module type_aliases");

    assert_eq!(alias.name, "Container");
    assert!(alias.is_pub);
    assert_eq!(alias.type_params.len(), 1, "should have 1 type param");
    assert_eq!(alias.type_params[0].name, "T");
    // Parameterized aliases have None for resolved_type (need instantiation)
    assert!(
        alias.resolved_type.is_none(),
        "parameterized alias should have resolved_type=None; got: {:?}",
        alias.resolved_type
    );
}

// ─── task 6259: entity-typed alias bodies resolve at their use sites ─────────
//
// Contract established by task #6259:
//
//   A non-parametric `type AL = <Body>` used in a declared-type position must
//   lower to EXACTLY the same `Type` as writing `<Body>` directly in that same
//   position.
//
// Deliberately stated as alias/direct PARITY rather than "resolves to
// `Type::Enum`": measured on `main`, the DIRECT path yields four different
// variants depending on body kind — `Enum` for an enum, `StructureRef` for a
// structure def AND for an occurrence def, `TraitObject` for a trait — and two
// of those choices are contested by adjacent open tasks #5920 (enum names
// absent from the unified entity namespace) and #5947 (silent type-namespace
// collisions among non-enum kinds). Hard-coding the expected variant would
// either bake in a decision belonging to those tasks or break when they land;
// a parity assertion stays valid under today's and any future direct-path
// semantics.
//
// FIXTURE CONSTRAINT: never name a fixture type `Fit` or `FitCategory` —
// `crates/reify-compiler/stdlib/tolerancing.ri` already declares
// `structure def Fit` (:268) and `enum FitCategory` (:22). A fixture that
// reuses either name silently resolves against the stdlib entity (yielding
// `StructureRef("Fit")` where the test meant `Enum("Fit")`) and masks the very
// defect under test.
mod alias_to_entity_type_parity {
    use super::*;
    use reify_test_support::compile_source_with_stdlib;

    /// Compile `source` with stdlib and return the declared `Type` of
    /// `entity`'s `member` param together with every Error-severity message.
    ///
    /// NOTE: params live in `TopologyTemplate.value_cells` — there is no
    /// `.params` field on a template.
    pub(super) fn param_type_and_errors(
        source: &str,
        entity: &str,
        member: &str,
    ) -> (Type, Vec<String>) {
        let module = compile_source_with_stdlib(source);
        let errors: Vec<String> = errors_only(&module)
            .iter()
            .map(|d| d.message.clone())
            .collect();
        let template = module
            .templates
            .iter()
            .find(|t| t.name == entity)
            .unwrap_or_else(|| panic!("template `{entity}` not found; errors: {errors:?}"));
        let cell = template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| {
                panic!("value cell `{entity}.{member}` not found; errors: {errors:?}")
            });
        (cell.cell_type.clone(), errors)
    }

    /// One parity row: a preamble of entity declarations plus the body spelling
    /// that is written BOTH directly (`param p : <body>`) and behind an alias
    /// (`type AL = <body>` + `param p : AL`).
    struct ParityCase {
        label: &'static str,
        decls: &'static str,
        body: &'static str,
    }

    /// The five body kinds measured as broken on `main` (each produced
    /// `Type::Error` + one Error `unresolved type: AL` via the alias).
    const PARITY_CASES: &[ParityCase] = &[
        ParityCase {
            // direct lowers to Type::Enum("Zq")
            label: "enum",
            decls: "enum Zq { Close, Medium }",
            body: "Zq",
        },
        ParityCase {
            // direct lowers to Type::StructureRef("Bq")
            label: "structure def",
            decls: "structure def Bq {\n    param w : Length = 1.0mm\n}",
            body: "Bq",
        },
        ParityCase {
            // direct lowers to Type::StructureRef("Oq") — same variant as a
            // structure def, which is exactly why this asserts parity, not a
            // hard-coded variant (see #5947).
            label: "occurrence def",
            decls: "occurrence def Oq {\n    param w : Length = 1.0mm\n}",
            body: "Oq",
        },
        ParityCase {
            // direct lowers to Type::TraitObject("Hq")
            label: "trait",
            decls: "trait Hq {\n    param w : Length\n}",
            body: "Hq",
        },
        ParityCase {
            // nested-in-builtin: direct lowers to Option(Enum("Zq"))
            label: "Option<enum>",
            decls: "enum Zq { Close, Medium }",
            body: "Option<Zq>",
        },
    ];

    #[test]
    fn alias_to_entity_body_lowers_identically_to_the_direct_spelling() {
        let mut failures: Vec<String> = Vec::new();

        for case in PARITY_CASES {
            let direct_src = format!(
                "{decls}\nstructure def D {{\n    param p : {body}\n}}\n",
                decls = case.decls,
                body = case.body
            );
            let alias_src = format!(
                "{decls}\ntype AL = {body}\nstructure def D {{\n    param p : AL\n}}\n",
                decls = case.decls,
                body = case.body
            );

            // The DIRECT spelling is the oracle — if it is not clean the row
            // says nothing about the alias path, so fail loudly on the fixture.
            let (direct_ty, direct_errs) = param_type_and_errors(&direct_src, "D", "p");
            assert!(
                direct_errs.is_empty(),
                "[{}] DIRECT baseline must compile cleanly for the parity oracle \
                 to mean anything; got: {:?}\n--- source ---\n{}",
                case.label,
                direct_errs,
                direct_src
            );

            let (alias_ty, alias_errs) = param_type_and_errors(&alias_src, "D", "p");
            if !alias_errs.is_empty() {
                failures.push(format!(
                    "[{}] alias spelling produced Error diagnostics: {:?}",
                    case.label, alias_errs
                ));
            }
            if alias_ty != direct_ty {
                failures.push(format!(
                    "[{}] alias `type AL = {}` lowered `D.p` to {:?}, but the direct \
                     spelling lowers it to {:?}",
                    case.label, case.body, alias_ty, direct_ty
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "alias/direct parity violated:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn transitive_alias_to_enum_lowers_identically_to_the_direct_spelling() {
        let direct_src = "enum Zq { Close, Medium }\n\
                          structure def D {\n    param p : Zq\n}\n";
        let alias_src = "enum Zq { Close, Medium }\n\
                         type A1 = Zq\n\
                         type A2 = A1\n\
                         structure def D {\n    param p : A2\n}\n";

        let (direct_ty, direct_errs) = param_type_and_errors(direct_src, "D", "p");
        assert!(
            direct_errs.is_empty(),
            "DIRECT baseline must compile cleanly; got: {direct_errs:?}"
        );

        let (alias_ty, alias_errs) = param_type_and_errors(alias_src, "D", "p");
        assert!(
            alias_errs.is_empty(),
            "transitive alias chain `A2 -> A1 -> Zq` should compile cleanly; got: {alias_errs:?}"
        );
        assert_eq!(
            alias_ty, direct_ty,
            "`type A2 = A1` / `type A1 = Zq` should lower `D.p` exactly as the direct `Zq` does"
        );
    }

    // ── Regression locks ────────────────────────────────────────────────────
    //
    // Both PASS on `main` today. They are the safety net for the deferred
    // use-site resolution added by task #6259: the first pins that the deferral
    // terminates on a cycle instead of recursing forever, the second pins that
    // re-resolving an alias body at each use site does not re-emit the
    // definition-site diagnostic for that body.

    #[test]
    fn circular_alias_still_terminates_and_reports_a_cycle() {
        // BOTH entries end up with `resolved_type: None` AND `type_expr:
        // Some(..)`, which is precisely the shape the deferred use-site arm
        // fires on — without a cycle guard this recurses until the compiler
        // stack overflows.
        let source = "type C1 = C2\n\
                      type C2 = C1\n\
                      structure def D {\n    param p : C1\n}\n";
        let module = compile_source_with_stdlib(source);
        let errs = errors_only(&module);
        assert!(
            errs.iter().any(|d| d.message.contains("circular type alias")),
            "a circular alias must still report the cycle; got: {:?}",
            errs.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unresolvable_alias_body_reports_its_inner_error_exactly_once() {
        // The definition-site DFS already reports `NotADim`. Re-resolving the
        // body at the use site must NOT emit it a second time, so the deferred
        // arm has to resolve into a scratch diagnostics vector and discard it
        // on failure.
        let source = "type Bad = Scalar<NotADim>\n\
                      structure def D {\n    param p : Bad\n}\n";
        let module = compile_source_with_stdlib(source);
        let errs = errors_only(&module);
        let not_a_dim_count = errs
            .iter()
            .filter(|d| d.message.contains("NotADim"))
            .count();
        assert_eq!(
            not_a_dim_count,
            1,
            "the inner `NotADim` error belongs to the alias DEFINITION site and must be \
             reported exactly once, not once per use site; got: {:?}",
            errs.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // ── Hygiene locks: no USE-SITE parameter capture ────────────────────────
    //
    // Both PASS on `main` and are regression locks on task #6259's diff, not
    // new-feature tests. The deferred use-site arm re-resolves the alias body
    // in an EMPTY type/dim parameter scope — NEVER the caller's. A type alias is
    // always a top-level declaration
    // (`reify-ast/src/decl.rs:36` — `Declaration` is "A top-level declaration
    // in a module", and there is no nested-scope alias form), so the body of a
    // NON-parametric alias can never legitimately name a type or dimension
    // parameter: the empty set IS the alias's own declaration-site scope.
    // Threading the use site's parameter scope into the body is therefore
    // capture, never a feature — these two locks are what catch a regression
    // back to it.
    //
    // Note the asymmetry in what each row asserts, and why: `dim_param_names`
    // is non-empty ONLY at fn-signature callers (`src/functions.rs:64,600` are
    // the only two places it is ever built from a non-empty source), so row (b)
    // MUST be spelled as a `fn` — a struct param cannot reproduce it.

    #[test]
    fn deferred_alias_body_does_not_capture_a_use_site_type_param() {
        // `T` is `D`'s type parameter, NOT anything the module-scope alias
        // `AL` can see. Capturing it turns a hard error into a SILENTLY WRONG
        // type: with the use site's scope threaded in, `D.p` lowers to
        // `Type::TypeParam("T")` with ZERO diagnostics.
        //
        // Assert BOTH halves. A diagnostic-only assertion would not catch the
        // silent form at all — the same trap documented for the
        // enum-variant-payload row in
        // `alias_to_entity_type_private_enum_namespaces`.
        let source = "type AL = T\n\
                      structure def D<T> {\n    param p : AL = 1.0\n}\n";
        let (ty, errs) = param_type_and_errors(source, "D", "p");

        assert!(
            errs.iter().any(|m| m.contains("unresolved type: AL")),
            "a module-scope alias whose body names `D`'s type parameter must stay \
             unresolved; got errors: {errs:?}"
        );
        assert_ne!(
            ty,
            Type::TypeParam("T".to_string()),
            "`type AL = T` must NOT capture the use site's type parameter `T`"
        );
        assert_eq!(
            ty,
            Type::Error,
            "an unresolvable alias body must leave the poison sentinel; got {ty:?} \
             with errors {errs:?}"
        );
    }

    #[test]
    fn deferred_alias_body_does_not_capture_a_use_site_dim_param() {
        // `Q` is `k`'s DIMENSION parameter. Capturing it makes the deferred
        // arm's recursion re-enter the bare-dim-param intercept in
        // `type_resolution.rs`, which tests the BODY name `Q` against the
        // CALLER's `dim_param_names` — so the compiler reports a name the
        // alias's declaration scope never had.
        //
        // Assert on the ERROR SET only: this site surfaces no `value_cell`.
        // And assert the PROPERTY (which name is mentioned), not the wording —
        // the fn-signature site has its own message, so a lock written against
        // the plain-param arm's `unresolved type: AD` would be wrong here and a
        // substring pin would break on a benign rewording.
        let source = "type AD = Q\n\
                      fn k<Q: Dimension>(x: Scalar<Q>, y: AD) -> Real { 1.0 }\n";
        let module = compile_source_with_stdlib(source);
        let errs: Vec<String> = errors_only(&module)
            .iter()
            .map(|d| d.message.clone())
            .collect();

        assert!(
            !errs.iter().any(|m| m.contains("'Q'")),
            "no diagnostic may name `Q` — it is `k`'s dimension parameter, invisible \
             to the module-scope alias `AD`; got: {errs:?}"
        );
        assert!(
            errs.iter().any(|m| m.contains("AD")),
            "the unresolvable alias `AD` must still be reported at its use site; \
             got: {errs:?}"
        );
    }
}

// ─── task 6259: the two OTHER `unresolved type` emit sites ──────────────────
//
// Both consume the SAME resolver chain as a plain struct param
// (`resolve_type_expr_with_aliases` → `resolve_type_expr_with_aliases_kinded`),
// so the deferred use-site arm covers them with no per-call-site change. They
// get their own locks because each fails DIFFERENTLY, and worse, than the plain
// param arm:
//
//   * guarded-group param (`src/guards.rs`, `register_guarded_names`): its
//     unresolved fallback is `Type::dimensionless_scalar()`, NOT the
//     `Type::Error` poison — so a regression here leaves a WRONG DIMENSIONED
//     type behind rather than a poisoned one.
//
//   * port param (`src/entity.rs`): emits the differently-worded
//     `unresolved type name '{}' in port parameter`, so a lock written against
//     the plain arm's message would not cover it.
//
// These two are DIAGNOSTIC-ONLY locks, deliberately: neither member reaches
// `TopologyTemplate.value_cells`, so there is no compiled cell to assert parity
// on. Measured on this fixture, `D`'s cells are exactly `[("active", Bool)]`
// for the guarded case and `[]` for the port case — a guarded-group param and a
// port param live only in the pre-pass scope (`scope.names`, the port one under
// the composite name `pt.q`). Do NOT "strengthen" these by asserting a
// `value_cells` entry: `find(...)` would yield `None` on both sides and the
// comparison would pass vacuously.
//
// The diagnostic assertion is not weak here — it is the same signal the
// production code emits alongside the bad fallback, and it was MEASURED red:
// with the deferred arm disabled these fail with `unresolved type: AL` and
// `unresolved type name 'AL' in port parameter` respectively.
//
// If either goes RED while the plain-param parity tests above stay GREEN, the
// deferred arm has been moved onto a path these two sites do not reach — fix
// the insertion point, do not special-case the call sites.
mod alias_to_entity_type_other_emit_sites {
    use super::*;
    use reify_test_support::compile_source_with_stdlib;

    fn error_messages(source: &str) -> Vec<String> {
        let module = compile_source_with_stdlib(source);
        errors_only(&module)
            .iter()
            .map(|d| d.message.clone())
            .collect()
    }

    /// Assert the alias spelling of a body compiles as cleanly as the direct
    /// spelling of the same body at this use site.
    fn assert_alias_resolves_like_direct(direct_src: &str, alias_src: &str, what: &str) {
        let direct_errs = error_messages(direct_src);
        assert!(
            direct_errs.is_empty(),
            "[{what}] DIRECT baseline must compile cleanly for this lock to mean \
             anything; got: {direct_errs:?}\n--- source ---\n{direct_src}"
        );
        let alias_errs = error_messages(alias_src);
        assert!(
            alias_errs.is_empty(),
            "[{what}] `type AL = Zq` used here must resolve exactly as the direct \
             `Zq` spelling does; got: {alias_errs:?}\n--- source ---\n{alias_src}"
        );
    }

    #[test]
    fn alias_to_enum_resolves_in_a_guarded_group_param() {
        let direct_src = "enum Zq { Close, Medium }\n\
                          structure def D {\n\
                          \x20   param active : Bool = true\n\
                          \x20   where active {\n\
                          \x20       param p : Zq\n\
                          \x20   }\n\
                          }\n";
        let alias_src = "enum Zq { Close, Medium }\n\
                         type AL = Zq\n\
                         structure def D {\n\
                         \x20   param active : Bool = true\n\
                         \x20   where active {\n\
                         \x20       param p : AL\n\
                         \x20   }\n\
                         }\n";
        assert_alias_resolves_like_direct(direct_src, alias_src, "guarded-group param");
    }

    #[test]
    fn alias_to_enum_resolves_in_a_port_param() {
        let direct_src = "enum Zq { Close, Medium }\n\
                          trait Tq { param d : Length }\n\
                          structure def D {\n\
                          \x20   port pt : out Tq { param q : Zq }\n\
                          }\n";
        let alias_src = "enum Zq { Close, Medium }\n\
                         type AL = Zq\n\
                         trait Tq { param d : Length }\n\
                         structure def D {\n\
                         \x20   port pt : out Tq { param q : AL }\n\
                         }\n";
        assert_alias_resolves_like_direct(direct_src, alias_src, "port param");
    }
}

// ─── task 6259: the three positions that own a PRIVATE enum namespace ───────
//
// The deferred use-site arm (`resolve_type_expr_with_aliases_kinded`) resolves
// an unresolved non-parametric alias's body by RECURSING into itself, so the
// body's ENUM-ness is only visible where the ambient enum set
// (`RESOLUTION_ENUM_NAMES`, installed by `EnumNameScope`) is live. That scope is
// installed at exactly two places — struct-param resolution (`entity.rs`) and
// fn param/return resolution (`functions.rs`).
//
// These THREE positions install no such scope. Each instead owns a PRIVATE enum
// namespace, consulted by a post-hoc `.or_else(..)` fallback keyed on the OUTER
// name — which is the ALIAS name `AL`, never the body name `Zq` — so the body's
// enum-ness is structurally unreachable there:
//
//   * enum variant payload  — `compile_builder/enums_phase.rs`, the
//                             `enum_names.contains(name)` arm.
//   * trait member param    — `traits.rs`, the `resolve_enum_type_with_args`
//                             fallback in `resolve_trait_member_type_annotation`.
//   * constraint def param  — `compile_builder/defs_phase.rs`, the
//                             `resolve_enum_type(name, enum_defs)` clause of the
//                             unknown-type guard.
//
// MEASURED RED on this branch, `enum Zq` body via `type AL = Zq`:
//
//   position              | direct        | via alias
//   ----------------------+---------------+---------------------------------
//   enum variant payload  | Enum("Zq")    | Error, and NO diagnostic at all
//   trait member param    | Param(Enum)   | Param(Error) + `unresolved type in
//                         |               | trait 'Tq': AL`
//   constraint def param  | ty: None,     | ty: None + spurious `unknown type
//                         | clean         | 'AL' in param 'g' ...`
//
// The variant-payload row is the reason every row asserts the RESOLVED TYPE and
// not merely the diagnostics: there, the alias spelling is SILENTLY wrong — a
// diagnostic-only lock would stay green while the payload field carries
// `Type::Error`.
//
// Each row also asserts its DIRECT baseline clean FIRST — the same non-vacuity
// discipline as `alias_to_entity_type_parity` above — so a parity assertion can
// never pass with both sides equally broken.
//
// STRUCTURE-bodied companion rows pin the already-working half: `structure_names`
// is threaded into the resolver as a real argument, so `resolve_type_with_aliases`
// resolves a structure-def body directly and all three positions ALREADY agree
// (MEASURED `StructureRef("Bq")` on both sides). They are what localises the
// defect to the enum namespace specifically, and they will catch a fix that
// regresses the working half.
//
// CONSTRAINT-DEF ROW, READ BEFORE "STRENGTHENING": for the ENUM body, `ty` is
// `None` on BOTH sides. The direct path's `resolve_enum_type` guard only
// SUPPRESSES the diagnostic — it never populates `ty`. So the parity target
// there is `ty == None` AND zero errors; do NOT assert `Some(Enum("Zq"))`, which
// would over-specify beyond parity and demand instantiation-time work this task
// deliberately leaves out of scope.
mod alias_to_entity_type_private_enum_namespaces {
    use super::*;
    use reify_compiler::RequirementKind;
    use reify_ir::VariantPayload;
    use reify_test_support::compile_source_with_stdlib;

    /// One of the three declared-type positions that consults a private enum
    /// namespace instead of installing `EnumNameScope`.
    #[derive(Clone, Copy)]
    pub(super) enum Position {
        EnumVariantPayload,
        TraitMemberParam,
        ConstraintDefParam,
    }

    impl Position {
        pub(super) fn label(self) -> &'static str {
            match self {
                Position::EnumVariantPayload => "enum variant payload",
                Position::TraitMemberParam => "trait member param",
                Position::ConstraintDefParam => "constraint def param",
            }
        }

        /// Build a source in which `type_name` occupies this position.
        pub(super) fn source(self, decls: &str, type_name: &str) -> String {
            match self {
                Position::EnumVariantPayload => format!(
                    "{decls}\nenum Wrap {{\n    W {{ v : {type_name} }},\n    N\n}}\n"
                ),
                Position::TraitMemberParam => {
                    format!("{decls}\ntrait Tq {{\n    param g : {type_name}\n}}\n")
                }
                Position::ConstraintDefParam => format!(
                    "{decls}\nconstraint def K {{\n    param g : {type_name}\n    \
                     param w : Length\n    w > 0.0mm\n}}\n"
                ),
            }
        }

        /// Compile `source` and read back the declared `Type` this position
        /// stored for the member named `g` / field named `v`, plus every
        /// Error-severity message.
        ///
        /// `None` means "the position stored no type" — which is the CORRECT
        /// direct-path outcome for a bare enum at the constraint-def position,
        /// and is therefore a legitimate parity target rather than a failure.
        pub(super) fn resolved_type_and_errors(self, source: &str) -> (Option<Type>, Vec<String>) {
            let module = compile_source_with_stdlib(source);
            let errors: Vec<String> = errors_only(&module)
                .iter()
                .map(|d| d.message.clone())
                .collect();
            let ty = match self {
                Position::EnumVariantPayload => {
                    let wrap = module
                        .enum_defs
                        .iter()
                        .find(|e| e.name == "Wrap")
                        .unwrap_or_else(|| panic!("enum `Wrap` not found; errors: {errors:?}"));
                    let variant = wrap
                        .variants
                        .iter()
                        .find(|v| v.name == "W")
                        .unwrap_or_else(|| panic!("variant `Wrap.W` not found; errors: {errors:?}"));
                    match &variant.payload {
                        VariantPayload::Named(fields) => fields
                            .iter()
                            .find(|(n, _)| n == "v")
                            .map(|(_, t)| t.clone()),
                        VariantPayload::Unit => panic!(
                            "`Wrap.W` lost its named payload entirely; errors: {errors:?}"
                        ),
                    }
                }
                Position::TraitMemberParam => {
                    let tq = module
                        .trait_defs
                        .iter()
                        .find(|t| t.name == "Tq")
                        .unwrap_or_else(|| panic!("trait `Tq` not found; errors: {errors:?}"));
                    let member = tq
                        .required_members
                        .iter()
                        .find(|m| m.name == "g")
                        .unwrap_or_else(|| {
                            panic!("trait member `Tq.g` not found; errors: {errors:?}")
                        });
                    match &member.kind {
                        RequirementKind::Param(t) => Some(t.clone()),
                        other => panic!(
                            "`Tq.g` should be a Param requirement, got {other:?}; \
                             errors: {errors:?}"
                        ),
                    }
                }
                Position::ConstraintDefParam => {
                    let k = module
                        .constraint_defs
                        .iter()
                        .find(|c| c.name == "K")
                        .unwrap_or_else(|| {
                            panic!("constraint def `K` not found; errors: {errors:?}")
                        });
                    k.params
                        .iter()
                        .find(|p| p.name == "g")
                        .unwrap_or_else(|| {
                            panic!("constraint param `K.g` not found; errors: {errors:?}")
                        })
                        .ty
                        .clone()
                }
            };
            (ty, errors)
        }
    }

    pub(super) const POSITIONS: &[Position] = &[
        Position::EnumVariantPayload,
        Position::TraitMemberParam,
        Position::ConstraintDefParam,
    ];

    /// Run the direct-vs-alias comparison for one body at every position,
    /// returning a human-readable failure per violated row.
    fn parity_failures(body_label: &str, decls: &str, body: &str) -> Vec<String> {
        let mut failures = Vec::new();
        for &pos in POSITIONS {
            let direct_src = pos.source(decls, body);
            let alias_src = pos.source(&format!("{decls}\ntype AL = {body}"), "AL");

            // The DIRECT spelling is the oracle. If it is not clean the row says
            // nothing about the alias path, so fail loudly on the fixture rather
            // than letting the parity assertion pass vacuously.
            let (direct_ty, direct_errs) = pos.resolved_type_and_errors(&direct_src);
            assert!(
                direct_errs.is_empty(),
                "[{}/{}] DIRECT baseline must compile cleanly for the parity oracle to \
                 mean anything; got: {:?}\n--- source ---\n{}",
                pos.label(),
                body_label,
                direct_errs,
                direct_src
            );

            let (alias_ty, alias_errs) = pos.resolved_type_and_errors(&alias_src);
            if !alias_errs.is_empty() {
                failures.push(format!(
                    "[{}/{}] alias spelling produced Error diagnostics: {:?}",
                    pos.label(),
                    body_label,
                    alias_errs
                ));
            }
            if alias_ty != direct_ty {
                failures.push(format!(
                    "[{}/{}] alias `type AL = {}` resolved to {:?}, but the direct \
                     spelling resolves to {:?}",
                    pos.label(),
                    body_label,
                    body,
                    alias_ty,
                    direct_ty
                ));
            }
        }
        failures
    }

    #[test]
    fn alias_to_enum_resolves_identically_to_the_direct_spelling_in_all_three() {
        let failures = parity_failures("enum", "enum Zq { Close, Medium }", "Zq");
        assert!(
            failures.is_empty(),
            "alias/direct parity violated in a private-enum-namespace position:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn alias_to_structure_def_resolves_identically_to_the_direct_spelling_in_all_three() {
        // Companion row: `structure_names` is a real resolver argument, so this
        // half ALREADY works in all three positions. It is here to localise the
        // defect to the enum namespace and to catch a fix that regresses the
        // working half.
        let failures = parity_failures(
            "structure def",
            "structure def Bq {\n    param w : Length = 1.0mm\n}",
            "Bq",
        );
        assert!(
            failures.is_empty(),
            "the already-working structure-def half regressed:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn transitive_alias_chain_to_enum_resolves_identically_in_all_three() {
        // `A2 -> A1 -> Zq`: the fix must WALK the unresolved-alias chain, not
        // just hop one link.
        let decls = "enum Zq { Close, Medium }";
        let mut failures = Vec::new();
        for &pos in POSITIONS {
            let direct_src = pos.source(decls, "Zq");
            let alias_src = pos.source(&format!("{decls}\ntype A1 = Zq\ntype A2 = A1"), "A2");

            let (direct_ty, direct_errs) = pos.resolved_type_and_errors(&direct_src);
            assert!(
                direct_errs.is_empty(),
                "[{}] DIRECT baseline must compile cleanly; got: {:?}",
                pos.label(),
                direct_errs
            );

            let (alias_ty, alias_errs) = pos.resolved_type_and_errors(&alias_src);
            if !alias_errs.is_empty() {
                failures.push(format!(
                    "[{}] chain `A2 -> A1 -> Zq` produced Error diagnostics: {:?}",
                    pos.label(),
                    alias_errs
                ));
            }
            if alias_ty != direct_ty {
                failures.push(format!(
                    "[{}] chain `A2 -> A1 -> Zq` resolved to {:?}, direct `Zq` resolves \
                     to {:?}",
                    pos.label(),
                    alias_ty,
                    direct_ty
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "transitive alias chain parity violated:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn circular_alias_in_a_private_enum_namespace_position_still_terminates() {
        // The step-7 walker inherits the SAME cycle hazard the deferred arm
        // documented: `type C1 = C2` / `type C2 = C1` leaves BOTH entries
        // unresolved with a bare-`Named` body, so a naive chain walk ping-pongs
        // until the stack overflows. Compiling at all is the assertion; the
        // cycle diagnostic must survive.
        for &pos in POSITIONS {
            let source = pos.source("type C1 = C2\ntype C2 = C1", "C1");
            let module = compile_source_with_stdlib(&source);
            let errs = errors_only(&module);
            assert!(
                errs.iter().any(|d| d.message.contains("circular type alias")),
                "[{}] a circular alias must still report the cycle; got: {:?}",
                pos.label(),
                errs.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
        }
    }
}

// ─── task 6259: shadow semantics for alias bodies, and the literal reproducer ─
//
// DECISION RECORDED HERE (the `enum-shadow-coherence` RATCHET NOTE's explicit
// ask that this task decide alias-body shadow semantics deliberately rather
// than inherit them by accident):
//
//   Alias bodies get NO separate name-resolution rule. A `type AL = <Body>`
//   used in a declared-type position resolves its body through the IDENTICAL
//   `resolve_type_expr_with_aliases_kinded` path the direct spelling goes
//   through, at the same use site — including under shadowing.
//
// Consequence for `docs/prds/v0_6/enum-shadow-coherence.md` §3 D1 / §5 C1:
// whatever leaf α makes the DIRECT spelling do under `LocalEnumShadowScope`,
// the alias body does too, automatically. There is no second rule to keep in
// sync, and α needs no alias-specific clause.
//
// WHY THESE ARE PARITY ASSERTIONS AND NOT `assert_eq!(ty, StructureRef(..))`:
// today's precedence is structure-wins in every shadowed configuration
// (MEASURED below), but that is exactly the precedence α is chartered to
// revisit. Freezing the literal variant here would hand α a test to fight;
// asserting `alias_ty == direct_ty` records the DECISION — the coupling — and
// tracks α automatically.
//
// α HAS SINCE LANDED, AND THE DECISION SURVIVED IT — measured, not predicted.
// `LocalEnumShadowScope` does not exist on this branch's base (`1f9731ad7d`;
// grepped: zero hits workspace-wide), but it DOES exist on `main`
// @ `fee75336ca`, which advanced 20 commits while this task was in flight.
//
// Two MEASUREMENTS, both first-hand:
//
//   * On this branch (α absent): both shadowed configurations, all four
//     positions (struct param + the three private-enum-namespace ones), direct
//     AND alias — `StructureRef` with zero errors. Parity holds everywhere.
//
//   * On a scratch merge of this branch with `main` @ `fee75336ca` (α present,
//     two textual conflicts in `type_resolution.rs` resolved by keeping both
//     sides — each is a pure addition against an EMPTY merge base): the direct
//     spelling of the prelude-vs-local case flips to `Enum("Fit")`, because a
//     module-local `enum Fit` now shadows the prelude `structure def Fit`.
//     Every parity test in this module passed UNCHANGED in that run
//     (harness_langcore 319 passed / 1 failed, the single failure being an
//     earlier draft of the non-vacuity test below that had frozen the literal
//     `StructureRef`).
//
// That is the decision doing exactly what it was written to do: the alias
// spelling tracked the direct spelling's flip automatically, with no
// alias-specific rule to update. Nothing in this module needs to change when α
// merges — which is the property being recorded.
//
// `Fit` IS USED DELIBERATELY HERE, unlike everywhere else in this file — the
// `alias_to_entity_type_parity` fixture constraint bans it precisely because
// `stdlib/tolerancing.ri` declares `structure def Fit` (:268) and that
// collision masks an ordinary parity defect. Here the collision IS the case
// under test: it is the only prelude-vs-local shadow available without a
// multi-module fixture.
mod alias_body_shadow_semantics {
    use super::alias_to_entity_type_parity::param_type_and_errors;
    use super::alias_to_entity_type_private_enum_namespaces::POSITIONS;
    use super::*;
    use reify_test_support::compile_source_with_stdlib;

    /// The literal reproducer from task 6259's description, which no earlier
    /// test covers because step-1's fixture constraint deliberately avoids the
    /// name `Fit`. This is the task's acceptance criterion.
    ///
    /// A LOCK, not a RED test: measured clean on this branch. It went from
    /// `unresolved type: F` to zero errors with the deferred use-site arm.
    #[test]
    fn task_description_reproducer_compiles_with_zero_errors() {
        let source = "enum Fit { Close, Medium }\n\
                      type F = Fit\n\
                      structure def C {\n    param g : F = Fit.Close\n}\n";
        let module = compile_source_with_stdlib(source);
        let errs: Vec<String> = errors_only(&module)
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            !errs.iter().any(|m| m.contains("unresolved type: F")),
            "the reported defect `unresolved type: F` must be gone; got: {errs:?}"
        );
        assert!(
            errs.is_empty(),
            "the task-description reproducer must compile with ZERO errors; got: {errs:?}"
        );
    }

    /// One shadowed configuration: a name bound BOTH as an enum and as a
    /// structure def, either across the prelude boundary or inside one module.
    struct ShadowCase {
        label: &'static str,
        decls: &'static str,
        body: &'static str,
    }

    const SHADOW_CASES: &[ShadowCase] = &[
        ShadowCase {
            // prelude-vs-local: local `enum Fit` collides with
            // `stdlib/tolerancing.ri`'s `structure def Fit` (:268).
            label: "prelude-vs-local (local `enum Fit` vs stdlib `structure def Fit`)",
            decls: "enum Fit { Close, Medium }",
            body: "Fit",
        },
        ShadowCase {
            // local-vs-local: both bindings declared in the module under test.
            label: "local-vs-local (`enum Zq` + `structure def Zq`)",
            decls: "enum Zq { Close, Medium }\n\
                    structure def Zq {\n    param w : Length = 1.0mm\n}",
            body: "Zq",
        },
    ];

    #[test]
    fn shadowed_alias_body_resolves_identically_to_the_direct_spelling_in_a_struct_param() {
        let mut failures: Vec<String> = Vec::new();

        for case in SHADOW_CASES {
            let direct_src = format!(
                "{decls}\nstructure def D {{\n    param p : {body}\n}}\n",
                decls = case.decls,
                body = case.body
            );
            let alias_src = format!(
                "{decls}\ntype F = {body}\nstructure def D {{\n    param p : F\n}}\n",
                decls = case.decls,
                body = case.body
            );

            let (direct_ty, direct_errs) = param_type_and_errors(&direct_src, "D", "p");
            assert!(
                direct_errs.is_empty(),
                "[{}] DIRECT baseline must compile cleanly for the parity oracle to \
                 mean anything; got: {:?}\n--- source ---\n{}",
                case.label,
                direct_errs,
                direct_src
            );

            let (alias_ty, alias_errs) = param_type_and_errors(&alias_src, "D", "p");
            if !alias_errs.is_empty() {
                failures.push(format!(
                    "[{}] alias spelling produced Error diagnostics: {:?}",
                    case.label, alias_errs
                ));
            }
            if alias_ty != direct_ty {
                failures.push(format!(
                    "[{}] `type F = {}` lowered `D.p` to {:?}, but the direct spelling \
                     lowers it to {:?} — an alias body must not get its own shadowing \
                     rule",
                    case.label, case.body, alias_ty, direct_ty
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "alias/direct parity violated under shadowing:\n  {}",
            failures.join("\n  ")
        );
    }

    #[test]
    fn shadowed_alias_body_resolves_identically_in_the_three_hopped_positions() {
        // The step-7 hop (`unresolved_alias_body_name`) must NOT fire when the
        // body already resolved — here it resolves as a STRUCTURE, so the
        // private-enum-namespace fallback is never reached and the alias must
        // land on `StructureRef`, not on the shadowed enum. This test is what
        // catches a hop that started firing too eagerly.
        let mut failures: Vec<String> = Vec::new();

        for case in SHADOW_CASES {
            for &pos in POSITIONS {
                let direct_src = pos.source(case.decls, case.body);
                let alias_src = pos.source(
                    &format!("{}\ntype AL = {}", case.decls, case.body),
                    "AL",
                );

                let (direct_ty, direct_errs) = pos.resolved_type_and_errors(&direct_src);
                assert!(
                    direct_errs.is_empty(),
                    "[{}/{}] DIRECT baseline must compile cleanly; got: {:?}\n\
                     --- source ---\n{}",
                    pos.label(),
                    case.label,
                    direct_errs,
                    direct_src
                );

                let (alias_ty, alias_errs) = pos.resolved_type_and_errors(&alias_src);
                if !alias_errs.is_empty() {
                    failures.push(format!(
                        "[{}/{}] alias spelling produced Error diagnostics: {:?}",
                        pos.label(),
                        case.label,
                        alias_errs
                    ));
                }
                if alias_ty != direct_ty {
                    failures.push(format!(
                        "[{}/{}] `type AL = {}` resolved to {:?}, but the direct \
                         spelling resolves to {:?} — the step-7 hop must not fire when \
                         the body already resolved as a structure",
                        pos.label(),
                        case.label,
                        case.body,
                        alias_ty,
                        direct_ty
                    ));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "alias/direct parity violated under shadowing in a hopped position:\n  {}",
            failures.join("\n  ")
        );
    }

    /// Non-vacuity guard for the parity tests above: it must not be possible for
    /// them to pass because BOTH spellings are equally broken.
    ///
    /// PRECEDENCE-AGNOSTIC ON PURPOSE. An earlier draft asserted the literal
    /// `Type::StructureRef("Fit")` — today's answer on this branch's base. That
    /// is precisely the shape this module's doc warns against, and it was
    /// measured to break: against `main` @ `fee75336ca` (where
    /// `enum-shadow-coherence` leaf α HAS landed) the same fixture yields
    /// `Type::Enum("Fit")`, because a module-local `enum Fit` now shadows the
    /// prelude `structure def Fit`. Every parity test in this module passed
    /// unchanged in that same run — which is the decision working as designed.
    ///
    /// So this asserts only what non-vacuity actually needs: the shadowed name
    /// resolves to SOMETHING concrete and unpoisoned, with no errors. Which of
    /// `Enum` / `StructureRef` wins is α's call, not this task's.
    #[test]
    fn shadowed_name_resolves_to_a_real_type_so_the_parity_tests_are_not_vacuous() {
        for case in SHADOW_CASES {
            let source = format!(
                "{decls}\nstructure def D {{\n    param p : {body}\n}}\n",
                decls = case.decls,
                body = case.body
            );
            let (ty, errs) = param_type_and_errors(&source, "D", "p");
            assert!(
                errs.is_empty(),
                "[{}] baseline must be clean; got: {:?}",
                case.label,
                errs
            );
            assert!(
                !ty.is_error(),
                "[{}] a shadowed name must resolve to a real type, not the `Type::Error` \
                 poison — otherwise the parity assertions above could hold with BOTH \
                 spellings broken. Got: {:?}",
                case.label,
                ty
            );
            assert!(
                matches!(ty, Type::Enum(_) | Type::StructureRef(_)),
                "[{}] expected the shadowed name to land on one of the two competing \
                 bindings (enum or structure); which one wins is `enum-shadow-coherence` \
                 leaf α's call, not this task's. Got: {:?}",
                case.label,
                ty
            );
        }
    }
}
