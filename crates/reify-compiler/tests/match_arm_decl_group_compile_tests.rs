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
use reify_syntax::{
    Declaration, EnumDecl, Expr, ExprKind, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl,
    ParsedModule, ParamDecl, StructureDef, SubDecl, TypeExpr, TypeExprKind,
};
use reify_types::{ContentHash, ModulePath, SourceSpan, Type};

// ─── AST construction helpers ────────────────────────────────────────────────

fn zero_span() -> SourceSpan {
    SourceSpan::new(0, 0)
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
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: structure_name.to_string(),
        type_args: vec![],
        args: vec![],
        is_collection: false,
        where_clause: None,
        body: None,
        span: zero_span(),
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
                variants: vec!["Hex".to_string(), "Socket".to_string()],
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
                variants: vec![
                    "Hex".to_string(),
                    "Socket".to_string(),
                    "Button".to_string(),
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
                variants: vec!["Hex".to_string(), "Socket".to_string()],
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
                    variants: vec!["Z1".to_string(), "Z2".to_string()],
                    span: zero_span(),
                    content_hash: ContentHash(0),
                    annotations: vec![],
                }),
                Declaration::Enum(EnumDecl {
                    name: "AKind".to_string(),
                    doc: None,
                    is_pub: false,
                    variants: vec!["A1".to_string(), "A2".to_string()],
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
                variants: vec!["Hex".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
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
    let has_second_diag = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("could not resolve type for match-arm param"));
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
                variants: vec!["Hex".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
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
                variants: vec!["Hex".to_string(), "Socket".to_string()],
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
                variants: vec!["A1".to_string(), "A2".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            Declaration::Enum(EnumDecl {
                name: "KindB".to_string(),
                doc: None,
                is_pub: false,
                variants: vec!["B1".to_string(), "B2".to_string()],
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

/// Task 2612 step-3: characterization test pinning the current
/// non-exhaustive-allowed behavior.
///
/// `HeadType` has THREE variants (`Hex`, `Socket`, `Button`) but only TWO arms
/// are declared (`Hex => sub head : HexHead`, `Socket => sub head : SocketHead`).
/// No exhaustiveness gate exists yet (task 2375 adds it), so:
///   (a) no diagnostic mentions "exhaustive" or "missing variant", and
///   (b) a `GuardedDeclGroup` for `"head"` with exactly 2 arms is registered.
///
/// **Intentional deviation from strict RED→GREEN:** this test passes on first run
/// because it pins *current* semantics. Task 2375 must flip assertion (a) and
/// update (b) when the exhaustiveness gate lands. The change of contract will be
/// visible in the diff and recorded explicitly.
#[test]
fn match_arm_decl_group_non_exhaustive_arms_register_partial_cluster() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
            // "Button" arm intentionally omitted to test non-exhaustive behavior.
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
        path: ModulePath::single("test_non_exhaustive_partial_cluster"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "HeadType".to_string(),
                doc: None,
                is_pub: false,
                // THREE variants; only two arms declared above.
                variants: vec![
                    "Hex".to_string(),
                    "Socket".to_string(),
                    "Button".to_string(),
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
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No exhaustiveness diagnostic yet — task 2375 will flip this.
    let has_exhaustive_diag = compiled.diagnostics.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("exhaustive") || msg.contains("missing variant")
    });
    assert!(
        !has_exhaustive_diag,
        "expected no exhaustiveness diagnostic (task 2375 adds the gate), got: {:#?}",
        compiled.diagnostics
    );

    // (b) The partial cluster should still be registered with 2 arms.
    let bolt_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("Bolt template should be compiled");

    assert_eq!(
        bolt_template.match_arm_groups.len(),
        1,
        "expected 1 match_arm_groups entry for partial cluster, got: {:#?}",
        bolt_template.match_arm_groups
    );

    assert_eq!(
        bolt_template.match_arm_groups[0].arms.len(),
        2,
        "expected 2 arms in the partial cluster, got: {:#?}",
        bolt_template.match_arm_groups[0]
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
                variants: vec!["Hex".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
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
