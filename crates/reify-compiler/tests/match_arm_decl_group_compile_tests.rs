//! Integration tests for `MemberDecl::MatchArmDeclGroup` compilation (task 2372, step-9/10).
//!
//! These tests hand-construct a `ParsedModule` containing a match-arm decl group
//! (no tree-sitter grammar support yet — grammar wiring is a future task) and
//! verify that:
//! (a) no `"duplicate"` diagnostic is emitted for the shared logical name; and
//! (b) a `GuardedDeclGroup` is registered in the compiled template with the
//!     correct per-arm `arm_type` metadata.
//!
//! The RED→GREEN transition for (b) requires `TopologyTemplate::match_arm_groups`
//! (added in step-10 under `#[cfg(test)]`) and the entity-compilation hook that
//! populates it.

use reify_compiler::GuardedDeclGroup;
use reify_ast::{Declaration, EnumDecl, Expr, ExprKind, LetDecl, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl, ParamDecl, ParsedModule, StructureDef, SubDecl, TypeExpr, TypeExprKind};
use reify_core::{ContentHash, ModulePath, SourceSpan, Type};

// ─── AST construction helpers ────────────────────────────────────────────────

fn zero_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

fn span_at(start: u32, end: u32) -> SourceSpan {
    SourceSpan::new(start, end)
}

fn make_ident_expr(name: &str) -> Expr {
    Expr {
        kind: ExprKind::Ident(name.to_string()),
        span: zero_span(),
    }
}

fn named_type_expr(name: &str) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_string(),
            type_args: vec![],
        },
        span: zero_span(),
    }
}

fn param_member(name: &str, type_name: &str) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        is_priv: false,
        name: name.to_string(),
        doc: None,
        type_expr: Some(named_type_expr(type_name)),
        default: None,
        where_clause: None,
        annotations: vec![],
        span: zero_span(),
        content_hash: ContentHash(0),
    })
}

fn sub_member(name: &str, structure_name: &str) -> MemberDecl {
    sub_member_with_span(name, structure_name, zero_span())
}

fn sub_member_with_span(name: &str, structure_name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        is_priv: false,
        name: name.to_string(),
        structure_name: structure_name.to_string(),
        type_args: vec![],
        args: vec![],
        is_collection: false,
        where_clause: None,
        body: None,
        param_overrides: vec![],
        keyed_members: vec![],
        is_aux: false,
        pose_expr: None,
        span,
        content_hash: ContentHash(0),
    })
}

fn param_member_with_span(name: &str, type_name: &str, span: SourceSpan) -> MemberDecl {
    MemberDecl::Param(ParamDecl {
        is_priv: false,
        name: name.to_string(),
        doc: None,
        type_expr: Some(named_type_expr(type_name)),
        default: None,
        where_clause: None,
        annotations: vec![],
        span,
        content_hash: ContentHash(0),
    })
}

fn match_arm_decl(pattern: &str, member: MemberDecl) -> MatchArmDeclArmDecl {
    MatchArmDeclArmDecl {
        patterns: vec![pattern.to_string()],
        member: Box::new(member),
        span: zero_span(),
    }
}

fn empty_structure(name: &str) -> Declaration {
    Declaration::Structure(StructureDef {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// End-to-end RED→GREEN test for `MemberDecl::MatchArmDeclGroup` compilation.
///
/// Hand-constructs the equivalent of:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead {}
/// structure def SocketHead {}
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// Asserts:
/// (a) No diagnostic message contains both `"duplicate"` and `"head"` — the
///     cluster-registration path must never route through `scope.register()`,
///     so future dup-name tightening (task 2375) cannot misfire here.
/// (b) `template.match_arm_groups` contains exactly one `GuardedDeclGroup`
///     for `"head"` with two arms whose `arm_type` fields are
///     `StructureRef("HexHead")` and `StructureRef("SocketHead")` respectively.
#[test]
fn match_arm_decl_group_registers_cluster_without_duplicate_name_diagnostics() {
    // Build the MatchArmDeclGroup AST node.
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    // Build the Bolt structure.
    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    // Assemble the module: enum + referenced structures + Bolt.
    let parsed = ParsedModule {
        path: ModulePath::single("test_match_arm_decl"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No "duplicate … head" diagnostic.
    let duplicate_head_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") && msg.contains("head")
        })
        .collect();
    assert!(
        duplicate_head_diags.is_empty(),
        "expected no 'duplicate head' diagnostics, got: {:#?}",
        duplicate_head_diags
    );

    // (b) GuardedDeclGroup for "head" with correct per-arm arm_type.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    let head_group: &GuardedDeclGroup = bolt_template
        .match_arm_groups
        .iter()
        .find(|g| g.name == "head")
        .expect("match_arm_groups should contain a group named 'head'");

    assert_eq!(
        head_group.arms.len(),
        2,
        "expected 2 arms in GuardedDeclGroup for 'head', got {}",
        head_group.arms.len()
    );

    assert!(
        matches!(&head_group.arms[0].arm_type, Type::StructureRef(s) if s == "HexHead"),
        "arm 0 should have arm_type StructureRef(\"HexHead\"), got: {:?}",
        head_group.arms[0].arm_type
    );

    assert!(
        matches!(&head_group.arms[1].arm_type, Type::StructureRef(s) if s == "SocketHead"),
        "arm 1 should have arm_type StructureRef(\"SocketHead\"), got: {:?}",
        head_group.arms[1].arm_type
    );
}

/// Variant-pipe form: a single arm covers multiple patterns.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket, Button }
/// structure def HexOrButtonHead {}
/// structure def SocketHead {}
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex | Button => sub head : HexOrButtonHead
///         Socket       => sub head : SocketHead
///     }
/// }
/// ```
///
/// The pipe arm has `patterns: ["Hex", "Button"]`; the cluster should still
/// be registered as a `GuardedDeclGroup` named `"head"` with 2 arms.
#[test]
fn match_arm_decl_group_pipe_patterns_produce_two_arm_cluster() {
    let pipe_arm = MatchArmDeclArmDecl {
        patterns: vec!["Hex".to_string(), "Button".to_string()],
        member: Box::new(sub_member("head", "HexOrButtonHead")),
        span: zero_span(),
    };
    let socket_arm = match_arm_decl("Socket", sub_member("head", "SocketHead"));

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![pipe_arm, socket_arm],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_pipe_patterns"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec![
                    "Hex".into(),
                    "Socket".into(),
                    "Button".into(),
                ],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexOrButtonHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // No "duplicate head" diagnostic.
    let dup_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") && msg.contains("head")
        })
        .collect();
    assert!(
        dup_diags.is_empty(),
        "expected no 'duplicate head' diagnostics, got: {:#?}",
        dup_diags
    );

    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    let head_group: &GuardedDeclGroup = bolt_template
        .match_arm_groups
        .iter()
        .find(|g| g.name == "head")
        .expect("match_arm_groups should contain a group named 'head'");

    assert_eq!(
        head_group.arms.len(),
        2,
        "expected 2 arms (pipe arm + socket arm), got {}",
        head_group.arms.len()
    );
}

/// Pattern-validation diagnostic (review feedback for task 2372).
///
/// A pattern that does not name a variant of the discriminant's enum must
/// produce a diagnostic, not silently compile to an always-false guard.
#[test]
fn match_arm_decl_group_unknown_variant_pattern_emits_diagnostic() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            // 'Hexx' is a typo — not a variant of HeadType.
            match_arm_decl("Hexx", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_unknown_variant"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let unknown_variant_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("not a variant") && msg.contains("hexx")
        })
        .collect();
    assert!(
        !unknown_variant_diags.is_empty(),
        "expected a diagnostic naming the unknown variant 'Hexx', got: {:#?}",
        compiled.diagnostics
    );
}

/// Determinism regression test (review feedback for task 2372).
///
/// A structure with multiple match-arm clusters must expose
/// `TopologyTemplate::match_arm_groups` in deterministic (lexicographic) order,
/// regardless of the source-order in which the clusters were declared. Backed by
/// `CompilationScope::match_arm_groups: BTreeMap` so that `.values()` iteration
/// is key-sorted.
///
/// The construction order here is deliberately reversed (`zebra` declared before
/// `alpha`) so a `HashMap`-backed scope would (with high probability) produce a
/// non-sorted vec, while a `BTreeMap`-backed scope is guaranteed to produce
/// `["alpha", "zebra"]`. Compiling twice and asserting the same vec also pins
/// down run-to-run stability.
#[test]
fn match_arm_groups_iteration_order_is_deterministic() {
    fn build_module() -> ParsedModule {
        // Two enums, each driving its own match-arm cluster.
        let zebra_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
            discriminant: make_ident_expr("z_kind"),
            arms: vec![
                match_arm_decl("Z1", sub_member("zebra", "ZebraOne")),
                match_arm_decl("Z2", sub_member("zebra", "ZebraTwo")),
            ],
            span: zero_span(),
            content_hash: ContentHash(0),
        });
        let alpha_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
            discriminant: make_ident_expr("a_kind"),
            arms: vec![
                match_arm_decl("A1", sub_member("alpha", "AlphaOne")),
                match_arm_decl("A2", sub_member("alpha", "AlphaTwo")),
            ],
            span: zero_span(),
            content_hash: ContentHash(0),
        });

        let bolt = Declaration::Structure(StructureDef {
            name: "Bolt".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            // Note: zebra cluster declared first, alpha second — declaration
            // order intentionally reverse-lex.
            members: vec![
                param_member("z_kind", "ZKind"),
                param_member("a_kind", "AKind"),
                zebra_group,
                alpha_group,
            ],
            span: zero_span(),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        });

        ParsedModule {
            path: ModulePath::single("test_match_arm_decl_determinism"),
            declarations: vec![
                Declaration::Enum(EnumDecl {
                    name: "ZKind".to_string(),
                    doc: None,
                    is_pub: false,
                    type_params: vec![],
                    variants: vec!["Z1".into(), "Z2".into()],
                    span: zero_span(),
                    content_hash: ContentHash(0),
                    annotations: vec![],
                }),
                Declaration::Enum(EnumDecl {
                    name: "AKind".to_string(),
                    doc: None,
                    is_pub: false,
                    type_params: vec![],
                    variants: vec!["A1".into(), "A2".into()],
                    span: zero_span(),
                    content_hash: ContentHash(0),
                    annotations: vec![],
                }),
                empty_structure("ZebraOne"),
                empty_structure("ZebraTwo"),
                empty_structure("AlphaOne"),
                empty_structure("AlphaTwo"),
                bolt,
            ],
            errors: vec![],
            content_hash: ContentHash(0),
            pragmas: vec![],
            declared_module_path: None,
        }
    }

    let compile_once = || {
        let parsed = build_module();
        let compiled = reify_compiler::compile(&parsed);
        let bolt = compiled
            .templates
            .iter()
            .find(|t| t.name == "Bolt")
            .expect("Bolt template should be compiled")
            .clone();
        bolt.match_arm_groups
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>()
    };

    let order_a = compile_once();
    let order_b = compile_once();

    assert_eq!(
        order_a,
        vec!["alpha".to_string(), "zebra".to_string()],
        "match_arm_groups must be exposed in lexicographic key order, got {:?}",
        order_a
    );
    assert_eq!(
        order_a, order_b,
        "match_arm_groups order must be stable across compiles — got {:?} then {:?}",
        order_a, order_b
    );
}

// ─── Diagnostic-coverage tests (reviewer suggestions 9, 11) ─────────────────

/// suggestion 9a: discriminant param has a non-enum type → diagnostic.
///
/// The discriminant `head_type` is declared as `param head_type : HexHead`
/// where `HexHead` is a structure, not an enum.  The compiler must emit a
/// "expected an enum" diagnostic and must NOT produce a cluster.
#[test]
fn match_arm_decl_group_discriminant_not_enum_emits_diagnostic() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![match_arm_decl("Hex", sub_member("head", "HexHead"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // head_type is typed as a structure (not an enum)
        members: vec![param_member("head_type", "HexHead"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_discriminant_not_enum"),
        declarations: vec![empty_structure("HexHead"), bolt],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_enum_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("expected an enum"));
    assert!(
        has_enum_diag,
        "expected 'expected an enum' diagnostic, got: {:#?}",
        compiled.diagnostics
    );

    // No cluster should be registered when discriminant resolution fails.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");
    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "no cluster should be registered when discriminant is not an enum, got: {:?}",
        bolt_template.match_arm_groups
    );
}

/// suggestion 9b: discriminant name not in scope → "not found in scope" diagnostic.
///
/// The match block references `nonexistent` which is never declared.
#[test]
fn match_arm_decl_group_discriminant_unresolved_emits_diagnostic() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("nonexistent"),
        arms: vec![match_arm_decl("Hex", sub_member("head", "HexHead"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_discriminant_unresolved"),
        declarations: vec![empty_structure("HexHead"), bolt],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_unresolved_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("not found in scope"));
    assert!(
        has_unresolved_diag,
        "expected 'not found in scope' diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// suggestion 11: non-Ident discriminant (MemberAccess) → "simple identifier" diagnostic.
///
/// The user wrote `match self.head_type { ... }` — a member-access expression.
/// This pins the contract: complex discriminants are rejected until task 2373
/// extends support.
#[test]
fn match_arm_decl_group_member_access_discriminant_emits_simple_identifier_diagnostic() {
    // Construct `self.head_type` as a MemberAccess expr.
    let member_discriminant = Expr {
        kind: ExprKind::MemberAccess {
            object: Box::new(make_ident_expr("self")),
            member: "head_type".to_string(),
        },
        span: zero_span(),
    };

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: member_discriminant,
        arms: vec![match_arm_decl("Hex", sub_member("head", "HexHead"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_member_access_discriminant"),
        declarations: vec![empty_structure("HexHead"), bolt],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_ident_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("simple identifier"));
    assert!(
        has_ident_diag,
        "expected 'simple identifier' diagnostic for MemberAccess discriminant, got: {:#?}",
        compiled.diagnostics
    );
}

/// suggestion 9d / suggestion 6: a Param arm inside a match block is rejected
/// with an explicit 'only sub declarations are supported' diagnostic.
///
/// This pins the pre-pass rejection invariant: Param arms must never be inserted
/// into scope.names (which would corrupt task 2375's dup-name tightening).
#[test]
fn match_arm_decl_group_param_arm_emits_unsupported_diagnostic() {
    let param_arm = MatchArmDeclArmDecl {
        patterns: vec!["Hex".to_string()],
        member: Box::new(param_member("head_width", "Real")),
        span: zero_span(),
    };

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![param_arm],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_param_arm_rejected"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_unsupported_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("only 'sub' declarations are supported"));
    assert!(
        has_unsupported_diag,
        "expected 'only sub declarations are supported' diagnostic for Param arm, got: {:#?}",
        compiled.diagnostics
    );

    // Suggestion 2: the pre-pass emits the first diagnostic and compile_match_arm_decl_group
    // now skips non-Sub arms, so the second "could not resolve type for match-arm param"
    // diagnostic should NOT be emitted.
    let has_second_diag = compiled.diagnostics.iter().any(|d| {
        d.message
            .contains("could not resolve type for match-arm param")
    });
    assert!(
        !has_second_diag,
        "expected no second 'could not resolve type' diagnostic for Param arm, got: {:#?}",
        compiled.diagnostics
    );
}

/// suggestion 5: an empty match block emits 'must contain at least one arm'.
#[test]
fn match_arm_decl_group_empty_arms_emits_diagnostic() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![], // intentionally empty
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_empty_arms"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_empty_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("at least one arm"));
    assert!(
        has_empty_diag,
        "expected 'at least one arm' diagnostic for empty match block, got: {:#?}",
        compiled.diagnostics
    );
}

/// Regression test for task 2872: mismatched arm names must not leave orphan entries
/// in `match_arm_group_arm_member_types` when `match_arm_groups` is empty.
///
/// Hand-constructs the equivalent of:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead {}
/// structure def SocketHead {}
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head  : HexHead
///         Socket => sub spike : SocketHead   -- name mismatch!
///     }
/// }
/// ```
///
/// Pre-fix: the pass-1 pre-pass inserts `match_arm_group_arm_member_types["head"]`
/// and `["spike"]` while `match_arm_groups` remains empty (pass-2 returns early on
/// the mismatch). The `assert!` in `compile_entity` then fires, causing this
/// test to panic. RED.
///
/// Post-fix: the per-arm maps are written only inside `compile_match_arm_decl_group`
/// after `register_match_arm_group` succeeds, so the key sets stay in sync. GREEN.
///
/// Asserts:
/// (a) The logical-name-mismatch diagnostic is still emitted.
/// (b) `bolt_template.match_arm_groups.is_empty()` — no cluster is registered on mismatch.
/// (c) Implicitly: the `assert!` in `compile_entity` passes (no panic), verifying
///     that no orphan per-arm entry persists.
#[test]
fn match_arm_decl_group_mismatched_arm_names_does_not_orphan_per_arm_member_types() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("spike", "SocketHead")), // name mismatch!
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_mismatched_names_no_orphan"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) The logical-name-mismatch diagnostic must be present.
    let has_mismatch_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("expected 'head'") && d.message.contains("found 'spike'"));
    assert!(
        has_mismatch_diag,
        "expected mismatch diagnostic ('expected head, found spike'), got: {:#?}",
        compiled.diagnostics
    );

    // (b) No cluster registered — the mismatch path skips register_match_arm_group.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");
    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected match_arm_groups to be empty on arm-name mismatch, got: {:#?}",
        bolt_template.match_arm_groups
    );

    // (c) The assert! in compile_entity verifies key-set parity unconditionally across all
    // build profiles — if it fires, this test panics before reaching here. No explicit
    // assertion needed; reaching this line proves the invariant holds.
}

/// suggestion 4: arms with mismatched logical names emit a diagnostic.
///
/// Arm 0 declares `sub head : HexHead` but arm 1 declares `sub foot : SocketHead`.
/// The compiler must reject this with an 'expected head, found foot' diagnostic.
#[test]
fn match_arm_decl_group_mismatched_arm_names_emits_diagnostic() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("foot", "SocketHead")), // name mismatch!
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_mismatched_names"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_mismatch_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("expected 'head'") && d.message.contains("found 'foot'"));
    assert!(
        has_mismatch_diag,
        "expected mismatch diagnostic ('expected head, found foot'), got: {:#?}",
        compiled.diagnostics
    );
}

/// suggestion 8: two match blocks in the same structure declaring the same
/// logical name → 'duplicate match-arm cluster name' diagnostic.
#[test]
fn match_arm_decl_group_duplicate_cluster_name_emits_diagnostic() {
    // Two separate match blocks, both producing cluster "head".
    let match_group_1 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("kind_a"),
        arms: vec![
            match_arm_decl("A1", sub_member("head", "HeadA1")),
            match_arm_decl("A2", sub_member("head", "HeadA2")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });
    let match_group_2 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("kind_b"),
        arms: vec![
            match_arm_decl("B1", sub_member("head", "HeadB1")),
            match_arm_decl("B2", sub_member("head", "HeadB2")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            param_member("kind_a", "KindA"),
            param_member("kind_b", "KindB"),
            match_group_1,
            match_group_2,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_duplicate_cluster"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "KindA".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["A1".into(), "A2".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            Declaration::Enum(EnumDecl {
                name: "KindB".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["B1".into(), "B2".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HeadA1"),
            empty_structure("HeadA2"),
            empty_structure("HeadB1"),
            empty_structure("HeadB2"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let has_dup_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate match-arm cluster name"));
    assert!(
        has_dup_diag,
        "expected 'duplicate match-arm cluster name' diagnostic, got: {:#?}",
        compiled.diagnostics
    );
}

/// Task 2375 step-1: a non-exhaustive match-arm decl group must emit a
/// "non-exhaustive match" diagnostic naming the missing variant, and the
/// cluster must NOT be registered on `match_arm_groups`.
///
/// `HeadType` has THREE variants (`Hex`, `Socket`, `Button`) but only TWO arms
/// are declared (`Hex => sub head : HexHead`, `Socket => sub head : SocketHead`).
/// The exhaustiveness gate (step-2) must:
///   (a) emit a diagnostic whose message contains "non-exhaustive match" and "Button", and
///   (b) leave `bolt_template.match_arm_groups` EMPTY (cluster did NOT form).
///
/// RED before the exhaustiveness gate (step-2); GREEN after.
#[test]
fn match_arm_decl_group_non_exhaustive_arms_emits_diagnostic_and_skips_cluster() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
            // "Button" arm intentionally omitted — must trigger exhaustiveness diagnostic.
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_non_exhaustive_emits_diagnostic"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                // THREE variants; only two arms declared above — Button is missing.
                variants: vec![
                    "Hex".into(),
                    "Socket".into(),
                    "Button".into(),
                ],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) A "non-exhaustive match" diagnostic naming "Button" must be emitted.
    let has_exhaustive_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("non-exhaustive match") && d.message.contains("Button"));
    assert!(
        has_exhaustive_diag,
        "expected a 'non-exhaustive match' diagnostic naming 'Button', got: {:#?}",
        compiled.diagnostics
    );

    // (b) The cluster must NOT be registered — match_arm_groups must be empty.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected no match_arm_groups entry when non-exhaustive (cluster must not form), got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2375 step-1 (sibling): a pipe arm `Hex | Socket` covers two variants but
/// not `Button` — the exhaustiveness gate must still fire and must NOT register the
/// cluster.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket, Button }
/// structure RecessedHead {}
/// structure Bolt {
///     param head_type : HeadType
///     match head_type { Hex | Socket => sub head : RecessedHead }
/// }
/// ```
///
/// The pipe arm flattens to covered = {"Hex", "Socket"}; "Button" is missing.
///
/// RED before the exhaustiveness gate (step-2); GREEN after.
#[test]
fn match_arm_decl_group_non_exhaustive_pipe_arm_emits_diagnostic() {
    // Single arm covering Hex and Socket via pipe-pattern.
    let pipe_arm = MatchArmDeclArmDecl {
        patterns: vec!["Hex".to_string(), "Socket".to_string()],
        member: Box::new(sub_member("head", "RecessedHead")),
        span: zero_span(),
    };

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![pipe_arm],
        // "Button" is not covered — exhaustiveness check must catch this.
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_non_exhaustive_pipe_arm"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec![
                    "Hex".into(),
                    "Socket".into(),
                    "Button".into(),
                ],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("RecessedHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Diagnostic must name both "non-exhaustive match" and "Button".
    let has_exhaustive_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("non-exhaustive match") && d.message.contains("Button"));
    assert!(
        has_exhaustive_diag,
        "expected 'non-exhaustive match' diagnostic naming 'Button' (pipe arm Hex|Socket \
         does not cover Button), got: {:#?}",
        compiled.diagnostics
    );

    // (b) No cluster must form.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected no match_arm_groups when non-exhaustive (pipe arm), got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2612 step-1: a match block whose only arm is `param` (non-Sub) must NOT
/// register an empty cluster on `TopologyTemplate::match_arm_groups`.
///
/// The pre-pass already emits "only 'sub' declarations are supported" for the
/// Param arm; the per-arm loop then `continue`s, leaving `group_arms` empty.
/// Before the gate in entity.rs the unconditional `scope.register_match_arm_group`
/// call would register an empty `GuardedDeclGroup` — this test pins that the gate
/// makes `match_arm_groups` remain empty.
///
/// RED before `if !group_arms.is_empty()` gate; GREEN after.
#[test]
fn match_arm_decl_group_param_only_arms_leave_cluster_unregistered() {
    // Build a match block with a single param arm (not Sub).
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![match_arm_decl("Hex", param_member("head_width", "Real"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("head_type", "HeadType"), match_group],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_param_only_cluster_unregistered"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Sanity-check: the pre-pass diagnostic must still be present.
    let has_unsupported_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("only 'sub' declarations are supported"));
    assert!(
        has_unsupported_diag,
        "precondition: expected 'only sub declarations are supported' diagnostic, got: {:#?}",
        compiled.diagnostics
    );

    // (b) No empty cluster should be registered — match_arm_groups must be empty.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected no match_arm_groups entry when all arms are Param/non-Sub, got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2613: duplicate match clusters must not pollute the first cluster's
/// `scope.sub_member_types` entries.
///
/// Hand-constructs the equivalent of:
/// ```text
/// enum KindA { A1 }
/// enum KindB { B1 }
/// structure HeadA1 { param first_only : Real }   // first cluster's child type
/// structure HeadB1 { param second_only : Real }  // second cluster's child type (different member)
///
/// structure Bolt {
///     param kind_a : KindA
///     param kind_b : KindB
///     match kind_a { A1 => sub head : HeadA1 }   // cluster #1
///     match kind_b { B1 => sub head : HeadB1 }   // cluster #2: duplicate logical name "head"
///     let probe = self.head.first_only            // exercises sub_member_types["head"]
/// }
/// ```
///
/// Asserts:
/// (a) `compiled.diagnostics` contains a "duplicate match-arm cluster name" message
///     (precondition — already true pre-fix).
/// (b) NO diagnostic message contains "unknown member 'first_only' on sub 'head'".
///     Pre-fix: pass-1 pre-pass overwrites sub_member_types["head"] with HeadB1's members
///     (which lack `first_only`), so the probe fails. Post-fix: the pre-pass skips the
///     second cluster, sub_member_types["head"] retains HeadA1's members, and the probe
///     resolves cleanly.
#[test]
fn duplicate_match_cluster_does_not_pollute_first_cluster_sub_member_types() {
    // match cluster #1: match kind_a { A1 => sub head : HeadA1 }
    let match_group_1 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("kind_a"),
        arms: vec![match_arm_decl("A1", sub_member("head", "HeadA1"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });
    // match cluster #2 (duplicate logical name "head"): match kind_b { B1 => sub head : HeadB1 }
    let match_group_2 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("kind_b"),
        arms: vec![match_arm_decl("B1", sub_member("head", "HeadB1"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    // let probe = self.head.first_only
    // Nested MemberAccess: outer member = "first_only", inner object = self.head
    let probe_value = Expr {
        kind: ExprKind::MemberAccess {
            object: Box::new(Expr {
                kind: ExprKind::MemberAccess {
                    object: Box::new(make_ident_expr("self")),
                    member: "head".to_string(),
                },
                span: zero_span(),
            }),
            member: "first_only".to_string(),
        },
        span: zero_span(),
    };
    let probe_let = MemberDecl::Let(LetDecl {
        name: "probe".to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value: probe_value,
        where_clause: None,
        annotations: vec![],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            param_member("kind_a", "KindA"),
            param_member("kind_b", "KindB"),
            match_group_1,
            match_group_2,
            probe_let,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    // HeadA1 has `param first_only : Real`  — member exists only in the FIRST cluster's child.
    // HeadB1 has `param second_only : Real` — different member name; the asymmetry is
    //   the bug-detector: if sub_member_types["head"] is overwritten with HeadB1's members,
    //   `self.head.first_only` fails with "unknown member 'first_only' on sub 'head'".
    let head_a1 = Declaration::Structure(StructureDef {
        name: "HeadA1".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("first_only", "Real")],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });
    let head_b1 = Declaration::Structure(StructureDef {
        name: "HeadB1".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![param_member("second_only", "Real")],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_dup_cluster_no_pollution"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "KindA".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["A1".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            Declaration::Enum(EnumDecl {
                name: "KindB".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["B1".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            head_a1,
            head_b1,
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Precondition: duplicate cluster diagnostic must be present (unchanged behavior).
    let has_dup_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate match-arm cluster name"));
    assert!(
        has_dup_diag,
        "precondition: expected 'duplicate match-arm cluster name' diagnostic, got: {:#?}",
        compiled.diagnostics
    );

    // (b) Regression: sub_member_types["head"] must NOT be overwritten by the rejected cluster.
    // Pre-fix this fires because sub_member_types["head"] = HeadB1's members = {second_only},
    //   so self.head.first_only → "unknown member 'first_only' on sub 'head'".
    // Post-fix sub_member_types["head"] = HeadA1's members = {first_only}, lookup succeeds.
    let has_unknown_member_diag = compiled.diagnostics.iter().any(|d| {
        d.message
            .contains("unknown member 'first_only' on sub 'head'")
    });
    assert!(
        !has_unknown_member_diag,
        "regression: unexpected 'unknown member first_only on sub head' diagnostic — \
         sub_member_types[\"head\"] was overwritten by the rejected second cluster; \
         diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// Task 2375 step-3: a regular `sub head` declared BEFORE the match block must
/// trigger a collision diagnostic when the match block also declares `head`.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultHead {}
/// structure HexHead {}
/// structure SocketHead {}
/// structure Bolt {
///     param head_type : HeadType
///     sub head : DefaultHead          // outside Sub — comes BEFORE the match
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// The regular `sub head` is registered in scope BEFORE the match block is
/// processed in the pre-pass. The forward-direction check (step-4) must detect
/// the collision and emit:
///   `"match-arm cluster 'head' collides with declaration of 'head' outside the match block"`
///
/// RED before the forward-collision detection (step-4); GREEN after.
#[test]
fn match_arm_decl_group_outside_sub_before_match_emits_collision_diagnostic() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let outside_span = span_at(1, 5);
    let cluster_span = span_at(10, 20);

    let outside_sub = sub_member_with_span("head", "DefaultHead", outside_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Sub precedes the match block in source order.
        members: vec![
            param_member("head_type", "HeadType"),
            outside_sub,
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_outside_sub_before_match_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a collision diagnostic for 'head' (outside Sub before match), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );
}

/// Task 2375 step-5: a regular `sub head` declared AFTER the match block must
/// also trigger a collision diagnostic (reverse direction).
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultHead {}
/// structure HexHead {}
/// structure SocketHead {}
/// structure Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     sub head : DefaultHead          // outside Sub — comes AFTER the match
/// }
/// ```
///
/// The match block is processed first in the pre-pass. When `sub head` is
/// encountered later, the reverse-direction check (step-6) must detect the
/// collision and emit:
///   `"match-arm cluster 'head' collides with declaration of 'head' outside the match block"`
///
/// RED before the reverse-collision detection (step-6); GREEN after.
#[test]
fn match_arm_decl_group_outside_sub_after_match_emits_collision_diagnostic() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let cluster_span = span_at(1, 10);
    let outside_span = span_at(20, 30);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let outside_sub = sub_member_with_span("head", "DefaultHead", outside_span);

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Sub follows the match block in source order.
        members: vec![
            param_member("head_type", "HeadType"),
            match_group,
            outside_sub,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_outside_sub_after_match_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a collision diagnostic for 'head' (outside Sub after match), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );
}

/// Task 2375 step-7(a): a Param `head` declared before the match block must
/// trigger a collision diagnostic when the match block also declares `head`.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure Bolt {
///     param head : Real               // outside Param — collides with cluster
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// RED before step-8 confirms it's already GREEN from step-4's `scope.names` check.
#[test]
fn match_arm_decl_group_outside_param_collision_emits_diagnostic() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let outside_span = span_at(1, 5);
    let cluster_span = span_at(10, 20);

    let outside_param = param_member_with_span("head", "Real", outside_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Param precedes the match block.
        members: vec![
            outside_param,
            param_member("head_type", "HeadType"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_outside_param_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected collision diagnostic for 'head' (outside Param before match), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );
}

/// Task 2375 step-7(b): a Let `head` declared before the match block must
/// trigger a collision diagnostic when the match block also declares `head`.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure Bolt {
///     let head = 1.0                  // outside Let — collides with cluster
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// RED before step-8 confirms it's already GREEN from step-4's `scope.names` check.
#[test]
fn match_arm_decl_group_outside_let_collision_emits_diagnostic() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let outside_span = span_at(1, 5);
    let cluster_span = span_at(10, 20);

    let outside_let = MemberDecl::Let(LetDecl {
        name: "head".to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value: Expr {
            kind: ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: zero_span(),
        },
        where_clause: None,
        annotations: vec![],
        span: outside_span,
        content_hash: ContentHash(0),
    });

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Let precedes the match block.
        members: vec![
            outside_let,
            param_member("head_type", "HeadType"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_outside_let_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected collision diagnostic for 'head' (outside Let before match), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );
}

/// Task 2375 step-9: forward-direction collision must ALSO suppress cluster
/// registration — match_arm_groups must be empty when a collision is detected.
///
/// Same scenario as step-3 (outside Sub BEFORE match), but now asserting BOTH:
///   (a) collision diagnostic IS emitted, AND
///   (b) bolt_template.match_arm_groups is EMPTY (cluster did NOT form).
///
/// RED before pass-2 short-circuit (step-10); GREEN after.
#[test]
fn match_arm_decl_group_outside_collision_suppresses_cluster_registration() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let outside_span = span_at(1, 5);
    let cluster_span = span_at(10, 20);

    let outside_sub = sub_member_with_span("head", "DefaultHead", outside_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Sub precedes the match block in source order (forward direction).
        members: vec![
            param_member("head_type", "HeadType"),
            outside_sub,
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_forward_collision_suppresses_cluster"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Collision diagnostic must be emitted with correct two-label structure.
    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected collision diagnostic for 'head' (forward direction), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );

    // (b) Cluster must NOT be registered — match_arm_groups must be empty.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected no match_arm_groups entry when forward collision detected, got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2375 step-9: reverse-direction collision must ALSO suppress cluster
/// registration — same as above but with outside Sub declared AFTER the match.
///
/// RED before pass-2 short-circuit (step-10); GREEN after.
#[test]
fn match_arm_decl_group_reverse_collision_suppresses_cluster_registration() {
    // Distinct spans so the test can verify the two-label structure of the diagnostic.
    let cluster_span = span_at(1, 10);
    let outside_span = span_at(20, 30);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let outside_sub = sub_member_with_span("head", "DefaultHead", outside_span);

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        // outside Sub follows the match block in source order (reverse direction).
        members: vec![
            param_member("head_type", "HeadType"),
            match_group,
            outside_sub,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_reverse_collision_suppresses_cluster"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Collision diagnostic must be emitted with correct two-label structure.
    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected collision diagnostic for 'head' (reverse direction), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, outside_span,
        "second label must point to the outside-of-match declaration"
    );

    // (b) Cluster must NOT be registered — match_arm_groups must be empty.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert!(
        bolt_template.match_arm_groups.is_empty(),
        "expected no match_arm_groups entry when reverse collision detected, got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2376 step-1: when an outside Sub AND two match clusters all share the
/// logical name `head`, the compiler must emit BOTH a collision diagnostic and a
/// duplicate-cluster diagnostic, in that order (collision before duplicate).
///
/// Precedence rule (entity.rs:572): duplicate-check fires before collision-check
/// for the *first* cluster, but when the first cluster is suppressed by collision,
/// the second cluster's duplicate status must still surface.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultHead {}
/// structure HexHead {}
/// structure SocketHead {}
/// structure HexHead2 {}
/// structure SocketHead2 {}
/// structure Bolt {
///     param head_type : HeadType
///     sub head : DefaultHead              // outside the match block (span 1..5)
///     match head_type {                   // cluster-1 (span 10..20)
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     match head_type {                   // cluster-2 (span 30..40) — duplicate
///         Hex    => sub head : HexHead2
///         Socket => sub head : SocketHead2
///     }
/// }
/// ```
///
/// Expected: exactly one collision diagnostic AND exactly one duplicate-cluster
/// diagnostic; collision appears before duplicate in the diagnostics vec.
#[test]
fn match_arm_decl_group_duplicate_and_outside_collision_emit_in_order() {
    let outside_span = span_at(1, 5);
    let cluster1_span = span_at(10, 20);
    let cluster2_span = span_at(30, 40);

    let outside_sub = sub_member_with_span("head", "DefaultHead", outside_span);

    let match_group_1 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster1_span,
        content_hash: ContentHash(0),
    });

    let match_group_2 = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead2")),
            match_arm_decl("Socket", sub_member("head", "SocketHead2")),
        ],
        span: cluster2_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            param_member("head_type", "HeadType"),
            outside_sub,
            match_group_1,
            match_group_2,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_duplicate_and_outside_collision_order"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            empty_structure("HexHead2"),
            empty_structure("SocketHead2"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) Exactly one collision diagnostic.
    let collision_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .collect();
    assert_eq!(
        collision_diags.len(),
        1,
        "expected exactly one collision diagnostic, got: {:#?}",
        compiled.diagnostics
    );

    // (b) Exactly one duplicate-cluster diagnostic.
    let duplicate_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate match-arm cluster name"))
        .collect();
    assert_eq!(
        duplicate_diags.len(),
        1,
        "expected exactly one duplicate-cluster diagnostic, got: {:#?}",
        compiled.diagnostics
    );

    // (c) Collision appears before duplicate in the diagnostics vec.
    // This asserts insertion order, which IS the contract today: the
    // forward-collision check fires in the pre-pass when the cluster is
    // first encountered; the duplicate-cluster check fires later when the
    // second cluster is processed. Both follow source order. If a future
    // change introduces a diagnostics sort pass, update this assertion to
    // compare diagnostic spans instead of vec indices.
    let collision_pos = compiled
        .diagnostics
        .iter()
        .position(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .expect("collision diagnostic not found");
    let duplicate_pos = compiled
        .diagnostics
        .iter()
        .position(|d| d.message.contains("duplicate match-arm cluster name"))
        .expect("duplicate-cluster diagnostic not found");
    assert!(
        collision_pos < duplicate_pos,
        "expected collision diagnostic (index {}) to appear before \
         duplicate-cluster diagnostic (index {})",
        collision_pos,
        duplicate_pos
    );
}

/// Task 2376 step-3: an outside Sub declared BEFORE the match AND another outside
/// Sub declared AFTER the match, both named `head`, must trigger exactly ONE
/// collision diagnostic — forward + reverse do not double-fire.
///
/// The forward collision (cluster vs. the before-Sub) marks the cluster in
/// `clusters_with_outside_collision` and skips populating
/// `match_arm_cluster_logical_names`.  The after-Sub's reverse check finds no
/// entry in `match_arm_cluster_logical_names` and therefore emits nothing.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultHeadBefore {}
/// structure HexHead {}
/// structure SocketHead {}
/// structure DefaultHeadAfter {}
/// structure Bolt {
///     param head_type : HeadType
///     sub head : DefaultHeadBefore        // before the match (span 1..5)
///     match head_type {                   // cluster (span 10..20)
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     sub head : DefaultHeadAfter         // after the match (span 30..35)
/// }
/// ```
///
/// Expected: exactly one collision diagnostic; its second label points at the
/// before-Sub (forward direction, span 1..5).
#[test]
fn match_arm_decl_group_outside_sub_before_and_after_match_emits_single_collision() {
    let before_span = span_at(1, 5);
    let cluster_span = span_at(10, 20);
    let after_span = span_at(30, 35);

    let outside_sub_before = sub_member_with_span("head", "DefaultHeadBefore", before_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let outside_sub_after = sub_member_with_span("head", "DefaultHeadAfter", after_span);

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            param_member("head_type", "HeadType"),
            outside_sub_before,
            match_group,
            outside_sub_after,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_before_and_after_sub_single_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHeadBefore"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            empty_structure("DefaultHeadAfter"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // Exactly one collision diagnostic (forward + reverse must not double-fire).
    let collision_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .collect();
    assert_eq!(
        collision_diags.len(),
        1,
        "expected exactly one collision diagnostic (no double-fire), got: {:#?}",
        compiled.diagnostics
    );

    // The single diagnostic is the forward-direction one: labels[1] points at the
    // before-Sub (span 1..5), not the after-Sub (span 30..35).
    let diag = &collision_diags[0];
    assert_eq!(
        diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        diag.labels[0].span, cluster_span,
        "first label must point to the cluster"
    );
    assert_eq!(
        diag.labels[1].span, before_span,
        "second label must point to the before-Sub (forward collision direction)"
    );
}

/// Task 2376 step-5: when the outside Sub has a *different* name (`foot`) from
/// the match cluster (`head`), no collision diagnostic must be emitted, and the
/// cluster registers successfully in `match_arm_groups`.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultFoot {}
/// structure HexHead {}
/// structure SocketHead {}
/// structure Bolt {
///     param head_type : HeadType
///     sub foot : DefaultFoot      // different name — must NOT collide with `head`
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// ```
///
/// Expected: zero collision diagnostics; `bolt_template.match_arm_groups` contains
/// the `head` cluster.
#[test]
fn match_arm_decl_group_outside_sub_with_different_name_emits_no_collision() {
    let outside_sub = sub_member("foot", "DefaultFoot");

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            param_member("head_type", "HeadType"),
            outside_sub,
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_different_name_no_collision"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultFoot"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // No collision diagnostic — different names cannot collide.
    let collision_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("match-arm cluster") && d.message.contains("outside the match block")
        })
        .collect();
    assert!(
        collision_diags.is_empty(),
        "expected no collision diagnostic when outside Sub 'foot' and cluster 'head' differ, \
         got: {:#?}",
        compiled.diagnostics
    );

    // Positive: the `head` cluster must be registered successfully.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");
    assert!(
        bolt_template
            .match_arm_groups
            .iter()
            .any(|g| g.name == "head"),
        "expected 'head' cluster in match_arm_groups (no collision suppression), got: {:#?}",
        bolt_template.match_arm_groups
    );
}

/// Task 2376 step-7: when the discriminant param has an unresolved enum type
/// (`MissingEnum` is not declared in the module), the compiler must emit the
/// upstream "unresolved type" diagnostic but must NOT emit a spurious
/// "non-exhaustive match" diagnostic.
///
/// Control flow: `param head_type : MissingEnum` resolves to `Type::Real` (fallback)
/// and emits `"unresolved type: MissingEnum"`.  When `compile_match_arm_decl_group`
/// calls `scope.resolve("head_type")`, it gets `(cell_id, Type::Real)` → hits the
/// `"expected an enum"` branch at entity.rs:2152 → returns early at line 2163 —
/// never reaching the exhaustiveness gate at line 2289.
///
/// This test is a regression guard: if a future refactor moves exhaustiveness ahead
/// of discriminant-resolution, the spurious diagnostic would reappear.
///
/// Constructs:
/// ```text
/// // NO EnumDecl for MissingEnum
/// structure HexHead {}
/// structure Bolt {
///     param head_type : MissingEnum       // unresolved type — falls back to Real
///     match head_type {                   // single arm — deliberately under-covering
///         Hex => sub head : HexHead
///     }
/// }
/// ```
///
/// Expected:
///   (a) at least one diagnostic contains "unresolved type" or "MissingEnum"
///   (b) zero diagnostics contain "non-exhaustive match"
#[test]
fn match_arm_decl_group_unknown_enum_discriminant_emits_no_spurious_non_exhaustive() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        // Single arm — intentionally under-covering whatever MissingEnum *would* have.
        arms: vec![match_arm_decl("Hex", sub_member("head", "HexHead"))],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            // MissingEnum is intentionally absent from the module declarations.
            param_member("head_type", "MissingEnum"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_unknown_enum_no_spurious_non_exhaustive"),
        declarations: vec![
            // No EnumDecl for MissingEnum — that is the point.
            empty_structure("HexHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) The unresolved-type diagnostic must be present.
    let has_unresolved = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved type") || d.message.contains("MissingEnum"));
    assert!(
        has_unresolved,
        "expected a diagnostic containing 'unresolved type' or 'MissingEnum', got: {:#?}",
        compiled.diagnostics
    );

    // (b) No spurious "non-exhaustive match" diagnostic must be emitted.
    let non_exhaustive_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("non-exhaustive match"))
        .collect();
    assert!(
        non_exhaustive_diags.is_empty(),
        "expected NO 'non-exhaustive match' diagnostic when discriminant type is unresolved, \
         got: {:#?}",
        compiled.diagnostics
    );
}

/// Task 2877 step-1: when two `param head` declarations share the same name,
/// the collision diagnostic's second label must anchor to the FIRST declaration's
/// span — not the last.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure Bolt {
///     param head : Real          // first  — span_at(1, 5)
///     param head : Real          // second — span_at(7, 11), duplicate name
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }                          // cluster — span_at(20, 30)
/// }
/// ```
///
/// Pre-fix: `outside_decl_spans.insert` overwrites silently, so the second param's
/// span wins and `labels[1].span == second_param_span` → test fails RED.
/// Post-fix: `entry().or_insert()` keeps the first span → test passes GREEN.
#[test]
fn match_arm_decl_group_duplicate_outside_param_anchors_to_first_decl() {
    let first_param_span = span_at(1, 5);
    let second_param_span = span_at(7, 11);
    let cluster_span = span_at(20, 30);

    let first_param = param_member_with_span("head", "Real", first_param_span);
    let second_param = param_member_with_span("head", "Real", second_param_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            first_param,
            second_param,
            param_member("head_type", "HeadType"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_duplicate_outside_param_anchors_to_first"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a collision diagnostic for 'head' (duplicate outside Param), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, first_param_span,
        "second label must point to the FIRST outside-param declaration (not the second)"
    );
}

/// Task 2877 step-3: when two `let head` declarations share the same name,
/// the collision diagnostic's second label must anchor to the FIRST declaration's
/// span — not the last.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure Bolt {
///     let head = 1.0             // first  — span_at(1, 5)
///     let head = 2.0             // second — span_at(7, 11), duplicate name
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }                          // cluster — span_at(20, 30)
/// }
/// ```
///
/// Pre-fix: `outside_decl_spans.insert` overwrites, so the second let's span wins
/// → test fails RED.  Post-fix: `entry().or_insert()` at the Let site → GREEN.
#[test]
fn match_arm_decl_group_duplicate_outside_let_anchors_to_first_decl() {
    let first_let_span = span_at(1, 5);
    let second_let_span = span_at(7, 11);
    let cluster_span = span_at(20, 30);

    let first_let = MemberDecl::Let(LetDecl {
        name: "head".to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value: Expr {
            kind: ExprKind::NumberLiteral {
                value: 1.0,
                is_real: false,
            },
            span: zero_span(),
        },
        where_clause: None,
        annotations: vec![],
        span: first_let_span,
        content_hash: ContentHash(0),
    });

    let second_let = MemberDecl::Let(LetDecl {
        name: "head".to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value: Expr {
            kind: ExprKind::NumberLiteral {
                value: 2.0,
                is_real: false,
            },
            span: zero_span(),
        },
        where_clause: None,
        annotations: vec![],
        span: second_let_span,
        content_hash: ContentHash(0),
    });

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            first_let,
            second_let,
            param_member("head_type", "HeadType"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_duplicate_outside_let_anchors_to_first"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a collision diagnostic for 'head' (duplicate outside Let), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, first_let_span,
        "second label must point to the FIRST outside-let declaration (not the second)"
    );
}

/// Task 2877 step-5: when two `sub head` declarations share the same name,
/// the collision diagnostic's second label must anchor to the FIRST declaration's
/// span — not the last.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure DefaultHead {}
/// structure Bolt {
///     sub head : DefaultHead     // first  — span_at(1, 5)
///     sub head : DefaultHead     // second — span_at(7, 11), duplicate name
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }                          // cluster — span_at(20, 30)
/// }
/// ```
///
/// Pre-fix: `outside_decl_spans.insert` overwrites, so the second sub's span wins
/// → test fails RED.  Post-fix: `entry().or_insert()` at the Sub site → GREEN.
#[test]
fn match_arm_decl_group_duplicate_outside_sub_anchors_to_first_decl() {
    let first_sub_span = span_at(1, 5);
    let second_sub_span = span_at(7, 11);
    let cluster_span = span_at(20, 30);

    let first_sub = sub_member_with_span("head", "DefaultHead", first_sub_span);
    let second_sub = sub_member_with_span("head", "DefaultHead", second_sub_span);

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: cluster_span,
        content_hash: ContentHash(0),
    });

    let bolt = Declaration::Structure(StructureDef {
        name: "Bolt".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            first_sub,
            second_sub,
            param_member("head_type", "HeadType"),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_duplicate_outside_sub_anchors_to_first"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                variants: vec!["Hex".into(), "Socket".into()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_structure("DefaultHead"),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let collision_diag = compiled
        .diagnostics
        .iter()
        .find(|d| {
            d.message.contains("match-arm cluster 'head'")
                && d.message.contains("outside the match block")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a collision diagnostic for 'head' (duplicate outside Sub), got: {:#?}",
                compiled.diagnostics
            )
        });
    assert_eq!(
        collision_diag.labels.len(),
        2,
        "collision diagnostic must have exactly two labels"
    );
    assert_eq!(
        collision_diag.labels[0].span, cluster_span,
        "first label must point to the cluster declaration"
    );
    assert_eq!(
        collision_diag.labels[1].span, first_sub_span,
        "second label must point to the FIRST outside-sub declaration (not the second)"
    );
}
