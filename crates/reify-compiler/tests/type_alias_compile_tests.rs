//! Tests for type alias registry and resolution (task 145).
//!
//! Validates TypeAliasEntry, TypeAliasRegistry, alias compilation in the pre-pass,
//! dimensional aliases, transitive resolution, cycle detection, parameterized aliases,
//! and integration with existing type resolution paths.

use reify_compiler::CompiledTypeAlias;
use reify_test_support::{compile_source, errors_only};
use reify_core::{ContentHash, SourceSpan, Type};

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
    let velocity_dim =
        reify_core::DimensionVector::LENGTH.div(&reify_core::DimensionVector::TIME);
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
        enum Mode { Active Passive }
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
        Type::Map(Box::new(Type::dimensionless_scalar()), Box::new(Type::String)),
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
fn compiled_type_alias_has_no_type_expr_field() {
    // CompiledTypeAlias must NOT have a type_expr field — this is the key module
    // boundary invariant. The struct should only contain semantic data.
    // This test verifies indirectly: we construct a CompiledTypeAlias directly
    // and confirm it compiles without a type_expr field.
    let alias = CompiledTypeAlias {
        name: "TestAlias".to_string(),
        resolved_type: Some(Type::dimensionless_scalar()),
        type_params: vec![],
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("TestAlias"),
    };
    assert_eq!(alias.name, "TestAlias");
    assert!(!alias.is_pub);
}

#[test]
fn compiled_type_alias_parameterized_in_module_output() {
    // Parameterized aliases should also appear as CompiledTypeAlias in module output,
    // with type_params populated and resolved_type=None.
    let source = r#"
        pub type Container<T> = List<T>
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
