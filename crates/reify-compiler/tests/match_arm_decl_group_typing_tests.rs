//! Integration tests for `MemberDecl::MatchArmDeclGroup` typing (task 2373).
//!
//! These tests hand-construct a `ParsedModule` containing a match-arm decl
//! group and verify that:
//!   (a) `self.<group>` resolves to a `Type::Union` of the per-arm structure
//!       references (PRD `match-block-decls.md` §6.4),
//!   (b) nested `self.<group>.<member>` access typechecks against the
//!       common-field type when present in every arm,
//!   (c) external `<sub>.<group>.<member>` access typechecks the same way,
//!   (d) arm-specific fields produce a precise diagnostic listing missing arms.
//!
//! The test style mirrors `match_arm_decl_group_compile_tests.rs` (helpers
//! inline, not factored into shared common/mod.rs) to keep typing-side and
//! producer-side tests symmetric.

use reify_compiler::ValueCellDecl;
use reify_ast::{Declaration, EnumDecl, Expr, ExprKind, LetDecl, MatchArmDeclArmDecl, MatchArmDeclGroupDecl, MemberDecl, ParamDecl, ParsedModule, StructureDef, SubDecl, TypeExpr, TypeExprKind};
use reify_core::{ContentHash, ModulePath, Severity, SourceSpan, Type};

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

fn member_access(object: Expr, member: &str) -> Expr {
    Expr {
        kind: ExprKind::MemberAccess {
            object: Box::new(object),
            member: member.to_string(),
        },
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
        keyed_members: vec![],
        is_aux: false,
        pose_expr: None,
        span: zero_span(),
        content_hash: ContentHash(0),
    })
}

fn let_member(name: &str, value: Expr) -> MemberDecl {
    MemberDecl::Let(LetDecl {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        is_aux: false,
        type_expr: None,
        value,
        where_clause: None,
        annotations: vec![],
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

fn structure_with_members(name: &str, members: Vec<MemberDecl>) -> Declaration {
    Declaration::Structure(StructureDef {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members,
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    })
}

fn head_type_enum() -> Declaration {
    Declaration::Enum(EnumDecl {
        name: "HeadType".to_string(),
        doc: None,
        is_pub: false,
        variants: vec!["Hex".to_string(), "Socket".to_string()],
        span: zero_span(),
        content_hash: ContentHash(0),
        annotations: vec![],
    })
}

/// Find the let-cell `name` in template `template_name` and return its type.
fn find_cell_type(
    compiled: &reify_compiler::CompiledModule,
    template_name: &str,
    member_name: &str,
) -> Option<Type> {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == template_name)?;
    template
        .value_cells
        .iter()
        .find(|c: &&ValueCellDecl| c.id.member == member_name)
        .map(|c| c.cell_type.clone())
}

fn error_diagnostics(compiled: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Assert that exactly one error diagnostic was emitted (anti-cascade pin).
///
/// Call this after verifying the *specific* diagnostic is present.  A second
/// error here means `make_poison_literal` failed to suppress a cascading
/// type-mismatch in the surrounding expression.
fn assert_no_cascade(errors: &[&reify_core::Diagnostic]) {
    assert_eq!(
        errors.len(),
        1,
        "anti-cascade: expected exactly 1 error diagnostic, got {} (all errors: {:#?})",
        errors.len(),
        errors
    );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Step-7: `self.<cluster>` resolves to a `Type::Union` of the per-arm
/// structure references.
///
/// Constructs the equivalent of:
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
///     let probe = self.head
/// }
/// ```
#[test]
fn self_dot_match_cluster_resolves_to_union_of_arm_types() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let probe = let_member("probe", member_access(make_ident_expr("self"), "head"));

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_self_match_cluster_union"),
        declarations: vec![
            head_type_enum(),
            empty_structure("HexHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No Error diagnostics.
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    // (b) `probe` cell exists with cell_type Union(HexHead, SocketHead).
    let probe_type = find_cell_type(&compiled, "Bolt", "probe")
        .expect("expected `probe` value cell on Bolt template");

    let expected = Type::Union(vec![
        Type::StructureRef("HexHead".to_string()),
        Type::StructureRef("SocketHead".to_string()),
    ]);
    assert_eq!(
        probe_type, expected,
        "expected probe.cell_type == Union<HexHead | SocketHead>, got {}",
        probe_type
    );
}

/// Step-9: pipe arms produce one `GuardedDeclArm` (not one per pattern), so
/// the union has 2 members for `Hex | Button => sub head : RecessedHead;
/// Socket => sub head : SocketHead`. Pins PRD acceptance criterion 5: pipe
/// patterns do NOT fan out at the type level.
#[test]
fn self_dot_match_cluster_pipe_arm_collapses_to_one_union_member() {
    let pipe_arm = MatchArmDeclArmDecl {
        patterns: vec!["Hex".to_string(), "Button".to_string()],
        member: Box::new(sub_member("head", "RecessedHead")),
        span: zero_span(),
    };
    let socket_arm = match_arm_decl("Socket", sub_member("head", "SocketHead"));

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![pipe_arm, socket_arm],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let probe = let_member("probe", member_access(make_ident_expr("self"), "head"));

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_pipe_arm_collapses"),
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
            empty_structure("RecessedHead"),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    let probe_type = find_cell_type(&compiled, "Bolt", "probe")
        .expect("expected `probe` value cell on Bolt template");

    // Two arms in the cluster: the pipe arm produces one entry, not two.
    let expected = Type::Union(vec![
        Type::StructureRef("RecessedHead".to_string()),
        Type::StructureRef("SocketHead".to_string()),
    ]);
    assert_eq!(
        probe_type, expected,
        "expected probe.cell_type == Union<RecessedHead | SocketHead> (pipe arm \
         must NOT fan out at the type level), got {}",
        probe_type
    );
}

/// Step-11: nested `self.<cluster>.<member>` resolves to the common-field
/// type when the inner member is present in every arm with the same type.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Real }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     let probe = self.head.across_flats
/// }
/// ```
/// Asserts `probe.cell_type == Type::Real` (the common field's type, not Union).
/// Pins PRD acceptance criterion 1 (common fields type-check via the cluster).
#[test]
fn self_dot_cluster_dot_common_field_resolves_to_arm_field_type() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    // self.head.across_flats — nested MemberAccess.
    let probe = let_member(
        "probe",
        member_access(
            member_access(make_ident_expr("self"), "head"),
            "across_flats",
        ),
    );

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_cluster_common_field"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Real")]),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No Error diagnostics.
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    // (b) `probe` cell has the common field type — Real, not Union.
    let probe_type = find_cell_type(&compiled, "Bolt", "probe")
        .expect("expected `probe` value cell on Bolt template");

    assert_eq!(
        probe_type,
        Type::Real,
        "expected probe.cell_type == Real (common field across all arms), got {}",
        probe_type
    );
}

/// Step-13: arm-specific fields produce a precise diagnostic naming the
/// missing arm types.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param head_thickness : Real }
/// structure def SocketHead {}                              // no head_thickness
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     let probe = self.head.head_thickness
/// }
/// ```
/// Asserts exactly ONE error diagnostic mentions both `'head_thickness'`
/// and `SocketHead` (the arm whose type lacks the field). Pins PRD
/// acceptance criterion 2.
#[test]
fn self_dot_cluster_dot_arm_specific_field_emits_diagnostic_listing_missing_arms() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    // self.head.head_thickness — field present in HexHead, missing in SocketHead.
    let probe = let_member(
        "probe",
        member_access(
            member_access(make_ident_expr("self"), "head"),
            "head_thickness",
        ),
    );

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_cluster_arm_specific_field"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("head_thickness", "Real")]),
            empty_structure("SocketHead"),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("'head_thickness'") && d.message.contains("SocketHead"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one error diagnostic mentioning both 'head_thickness' \
         and 'SocketHead', got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
}

/// Step-17: external `<sub>.<cluster>.<field>` access typechecks against
/// the common-field type when present in every arm of the sub's cluster.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Real }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// structure def Driver {
///     sub bolt : Bolt
///     let across = bolt.head.across_flats
/// }
/// ```
/// Asserts (a) no Error diagnostics, (b) `across.cell_type == Type::Real`.
/// Pins PRD acceptance criterion 3 (external cluster access typechecks).
#[test]
fn external_sub_dot_cluster_dot_common_field_typechecks() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolt : Bolt; let across = bolt.head.across_flats }
    let across = let_member(
        "across",
        member_access(
            member_access(make_ident_expr("bolt"), "head"),
            "across_flats",
        ),
    );
    let driver = structure_with_members("Driver", vec![sub_member("bolt", "Bolt"), across]);

    let parsed = ParsedModule {
        path: ModulePath::single("test_external_cluster_common_field"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Real")]),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    let across_type = find_cell_type(&compiled, "Driver", "across")
        .expect("expected `across` value cell on Driver template");

    assert_eq!(
        across_type,
        Type::Real,
        "expected across.cell_type == Real (common field across all arms of Bolt's cluster), \
         got {}",
        across_type
    );
}

/// Amendment for review suggestion 3: when two arms expose the same field
/// name with different `Type`s, exactly one error diagnostic must mention
/// "divergent" along with both arm structure names.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Length }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
///     let probe = self.head.across_flats
/// }
/// ```
/// Pins the divergent-types branch of `resolve_cluster_inner_member`,
/// previously untested. `Real` (dimensionless) and `Length`
/// (length-dimensioned `Scalar`) hit the `all_equal == false` path inside
/// the helper.
#[test]
fn self_dot_cluster_dot_divergent_types_emits_diagnostic_listing_arms() {
    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let probe = let_member(
        "probe",
        member_access(
            member_access(make_ident_expr("self"), "head"),
            "across_flats",
        ),
    );

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), match_group, probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_cluster_divergent_types"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Length")]),
            bolt,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| {
            d.message.contains("divergent")
                && d.message.contains("HexHead")
                && d.message.contains("SocketHead")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one error diagnostic mentioning 'divergent', 'HexHead', \
         and 'SocketHead', got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
}

/// Step-19: external `<sub>.<cluster>.<inner>` referencing an arm-specific
/// field emits exactly one error diagnostic naming the missing arm.
///
/// Constructs Driver { sub bolt : Bolt; let probe = bolt.head.head_thickness }
/// where HexHead has `head_thickness : Real` and SocketHead does not.
/// Pins PRD acceptance criterion 2 from outside the cluster.
#[test]
fn external_sub_dot_cluster_dot_arm_specific_field_emits_diagnostic() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolt : Bolt; let probe = bolt.head.head_thickness }
    let probe = let_member(
        "probe",
        member_access(
            member_access(make_ident_expr("bolt"), "head"),
            "head_thickness",
        ),
    );
    let driver = structure_with_members("Driver", vec![sub_member("bolt", "Bolt"), probe]);

    let parsed = ParsedModule {
        path: ModulePath::single("test_external_arm_specific_field"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("head_thickness", "Real")]),
            empty_structure("SocketHead"),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("'head_thickness'") && d.message.contains("SocketHead"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one error diagnostic mentioning both 'head_thickness' \
         and 'SocketHead', got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
}

// ─── Collection-sub indexed-access helpers (task 2871) ───────────────────────

/// Mirror of `sub_member` with `is_collection: true` — represents
/// `sub <name> : List<<structure_name>>` in the AST.
fn collection_sub_member(name: &str, structure_name: &str) -> MemberDecl {
    MemberDecl::Sub(SubDecl {
        name: name.to_string(),
        structure_name: structure_name.to_string(),
        type_args: vec![],
        args: vec![],
        is_collection: true,
        where_clause: None,
        body: None,
        keyed_members: vec![],
        is_aux: false,
        pose_expr: None,
        span: zero_span(),
        content_hash: ContentHash(0),
    })
}

/// Constructs `object[idx_val]` — an `ExprKind::IndexAccess` with a
/// `NumberLiteral` index. Used in tests for `bolts[0].head.X` patterns.
fn index_access(object: Expr, idx_val: f64) -> Expr {
    Expr {
        kind: ExprKind::IndexAccess {
            object: Box::new(object),
            index: Box::new(Expr {
                kind: ExprKind::NumberLiteral {
                    value: idx_val,
                    is_real: false,
                },
                span: zero_span(),
            }),
        },
        span: zero_span(),
    }
}

// ─── Task 2871 regression tests ──────────────────────────────────────────────

/// Task 2871 step-1 (RED before step-2): `bolts[0].head.across_flats`
/// typechecks to `Type::Real` when both arm structures declare
/// `across_flats : Real`.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Real }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// structure def Driver {
///     sub bolts : List<Bolt>   // collection sub
///     let probe = bolts[0].head.across_flats
/// }
/// ```
/// Asserts (a) no Error diagnostics and (b) `probe.cell_type == Type::Real`.
/// Pins PRD acceptance criterion 1 for the indexed-access / collection-sub
/// entry point (task 2871 option (a)).
#[test]
fn external_collection_sub_indexed_dot_cluster_dot_common_field_typechecks() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolts : List<Bolt>; let probe = bolts[0].head.across_flats }
    let probe_expr = member_access(
        member_access(index_access(make_ident_expr("bolts"), 0.0), "head"),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_indexed_cluster_common_field"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Real")]),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    // (a) No Error diagnostics.
    let errors = error_diagnostics(&compiled);
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:#?}",
        errors
    );

    // (b) `probe` cell has the common field type — Real.
    let probe_type = find_cell_type(&compiled, "Driver", "probe")
        .expect("expected `probe` value cell on Driver template");

    assert_eq!(
        probe_type,
        Type::Real,
        "expected probe.cell_type == Real (common field across all arms of \
         collection sub Bolt's cluster), got {}",
        probe_type
    );
}

/// Task 2871 step-3: `bolts[0].head.head_thickness` emits exactly one error
/// diagnostic naming the arm structure that lacks the field (`SocketHead`).
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param head_thickness : Real }
/// structure def SocketHead {}                              // no head_thickness
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[0].head.head_thickness
/// }
/// ```
/// Pins the missing-arm branch of `resolve_cluster_inner_member` (expr.rs:240)
/// for the indexed-access entry point. Mirrors the assertion shape of
/// `external_sub_dot_cluster_dot_arm_specific_field_emits_diagnostic`.
#[test]
fn external_collection_sub_indexed_dot_cluster_dot_arm_specific_field_emits_diagnostic() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolts : List<Bolt>; let probe = bolts[0].head.head_thickness }
    let probe_expr = member_access(
        member_access(index_access(make_ident_expr("bolts"), 0.0), "head"),
        "head_thickness",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_indexed_cluster_arm_specific"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("head_thickness", "Real")]),
            empty_structure("SocketHead"),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("'head_thickness'") && d.message.contains("SocketHead"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one error diagnostic mentioning both 'head_thickness' \
         and 'SocketHead', got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    // Anti-cascade pin (task 3046): `make_poison_literal` returns Type::Error
    // from the missing-arm branch, which must suppress further type-mismatch
    // errors in the surrounding expr.
    assert_no_cascade(&errors);
}

/// Task 2871 step-4: `bolts[0].head.across_flats` emits exactly one error
/// diagnostic mentioning "divergent", "HexHead", and "SocketHead" when the
/// two arm structures expose `across_flats` with different types.
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Length }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[0].head.across_flats
/// }
/// ```
/// Pins the divergent-types branch of `resolve_cluster_inner_member`
/// (expr.rs:214) for the indexed-access entry point. Mirrors the assertion
/// shape of `self_dot_cluster_dot_divergent_types_emits_diagnostic_listing_arms`.
#[test]
fn external_collection_sub_indexed_dot_cluster_dot_divergent_types_emits_diagnostic() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolts : List<Bolt>; let probe = bolts[0].head.across_flats }
    // HexHead.across_flats : Real, SocketHead.across_flats : Length → divergent
    let probe_expr = member_access(
        member_access(index_access(make_ident_expr("bolts"), 0.0), "head"),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_indexed_cluster_divergent_types"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Length")]),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);

    let errors = error_diagnostics(&compiled);
    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| {
            d.message.contains("divergent")
                && d.message.contains("HexHead")
                && d.message.contains("SocketHead")
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one error diagnostic mentioning 'divergent', 'HexHead', \
         and 'SocketHead', got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    // Anti-cascade pin (task 3046): `make_poison_literal` returns Type::Error
    // from the divergent-types branch, which must suppress further type-mismatch
    // errors in the surrounding expr.
    assert_no_cascade(&errors);
}

// ─── Task 3045 regression tests — non-finite / out-of-range literal index ────
//
// These tests pin the guard added at expr.rs:1342 that rejects NaN, ±∞, and
// large-magnitude finite floats (e.g. 1e20) as collection-sub indices.
// Previously those values passed the `n.fract() != 0.0 || *n < 0.0` check and
// `*n as i64` saturated silently to i64::MAX, producing a bogus ValueRef
// pointing to a non-existent cell.

/// Task 3045: `bolts[NaN].across_flats` must emit exactly one "out of range or
/// non-finite" error diagnostic and return a poison literal, not a silent ValueRef.
///
/// Constructs:
/// ```text
/// structure def Bolt   { param across_flats : Real }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[NaN].across_flats
/// }
/// ```
#[test]
fn collection_sub_indexed_nan_emits_out_of_range_diagnostic() {
    let bolt = structure_with_members("Bolt", vec![param_member("across_flats", "Real")]);

    let probe_expr = member_access(
        index_access(make_ident_expr("bolts"), f64::NAN),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_nan_index"),
        declarations: vec![bolt, driver],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);
    let errors = error_diagnostics(&compiled);

    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("out of range or non-finite"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one 'out of range or non-finite' error for NaN index, \
         got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    assert_no_cascade(&errors);
}

/// Task 3045: `bolts[+∞].across_flats` must emit exactly one "out of range or
/// non-finite" error diagnostic and return a poison literal.
///
/// Constructs:
/// ```text
/// structure def Bolt   { param across_flats : Real }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[+inf].across_flats
/// }
/// ```
#[test]
fn collection_sub_indexed_infinity_emits_out_of_range_diagnostic() {
    let bolt = structure_with_members("Bolt", vec![param_member("across_flats", "Real")]);

    let probe_expr = member_access(
        index_access(make_ident_expr("bolts"), f64::INFINITY),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_infinity_index"),
        declarations: vec![bolt, driver],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);
    let errors = error_diagnostics(&compiled);

    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("out of range or non-finite"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one 'out of range or non-finite' error for +∞ index, \
         got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    assert_no_cascade(&errors);
}

/// Task 3045: `bolts[1e20].across_flats` must emit exactly one "out of range or
/// non-finite" error diagnostic and return a poison literal, not a silent ValueRef
/// pointing to a bogus `Driver.bolts[9223372036854775807]` cell.
///
/// 1e20 is a finite, integer-valued float that passes `n.fract() != 0.0 || *n < 0.0`
/// but is larger than i64::MAX — the silent-saturation case closed by task 3045.
///
/// Constructs:
/// ```text
/// structure def Bolt   { param across_flats : Real }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[1e20].across_flats
/// }
/// ```
#[test]
fn collection_sub_indexed_1e20_emits_out_of_range_diagnostic() {
    let bolt = structure_with_members("Bolt", vec![param_member("across_flats", "Real")]);

    let probe_expr = member_access(
        index_access(make_ident_expr("bolts"), 1e20_f64),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_1e20_index"),
        declarations: vec![bolt, driver],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);
    let errors = error_diagnostics(&compiled);

    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("out of range or non-finite"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one 'out of range or non-finite' error for 1e20 index, \
         got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    assert_no_cascade(&errors);
}

/// Task 3045 amendment: pin the cluster-aware branch's `>` → `>=` boundary fix.
///
/// Without the fix (`> i64::MAX as f64`), `n = 2^63` satisfies
/// `n > i64::MAX as f64 == false` (because `i64::MAX = 2^63 − 1` is not
/// representable in f64 — it rounds UP to `2^63`), so the cluster branch enters
/// the `else` arm and calls `resolve_cluster_inner_member` with `*n as i64`
/// saturating silently to `i64::MAX`.  With the `>=` fix, the guard fires and
/// control falls through to the regular indexed-access branch, where the
/// `!n.is_finite() || *n >= i64::MAX as f64` guard added in step-2 emits the
/// explicit diagnostic.  This test exercises the cluster-routing path
/// (`sub_match_arm_groups` populated for `bolts`) and therefore covers the first
/// guard change, which the three step-1 tests do not reach (they use a plain
/// `collection_sub_member` with no match-arm group).
///
/// Constructs:
/// ```text
/// enum HeadType { Hex, Socket }
/// structure def HexHead    { param across_flats : Real }
/// structure def SocketHead { param across_flats : Real }
/// structure def Bolt {
///     param head_type : HeadType
///     match head_type {
///         Hex    => sub head : HexHead
///         Socket => sub head : SocketHead
///     }
/// }
/// structure def Driver {
///     sub bolts : List<Bolt>
///     let probe = bolts[2^63].head.across_flats   // 2^63 == i64::MAX as f64
/// }
/// ```
/// Without the `>` → `>=` fix this would silently produce a ValueRef to
/// `Driver.bolts[9223372036854775807]` (saturated i64::MAX).  With the fix it
/// emits exactly one "out of range or non-finite" diagnostic and a poison literal.
#[test]
fn cluster_routing_path_i64_max_boundary_emits_out_of_range_diagnostic() {
    let bolt_match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: make_ident_expr("head_type"),
        arms: vec![
            match_arm_decl("Hex", sub_member("head", "HexHead")),
            match_arm_decl("Socket", sub_member("head", "SocketHead")),
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let bolt = structure_with_members(
        "Bolt",
        vec![param_member("head_type", "HeadType"), bolt_match_group],
    );

    // Driver { sub bolts : List<Bolt>; let probe = bolts[2^63].head.across_flats }
    // 2.0_f64.powi(63) == i64::MAX as f64 (the f64 value that i64::MAX rounds UP to)
    let probe_expr = member_access(
        member_access(
            index_access(make_ident_expr("bolts"), 2.0_f64.powi(63)),
            "head",
        ),
        "across_flats",
    );
    let probe = let_member("probe", probe_expr);
    let driver = structure_with_members(
        "Driver",
        vec![collection_sub_member("bolts", "Bolt"), probe],
    );

    let parsed = ParsedModule {
        path: ModulePath::single("test_collection_sub_cluster_i64max_boundary"),
        declarations: vec![
            head_type_enum(),
            structure_with_members("HexHead", vec![param_member("across_flats", "Real")]),
            structure_with_members("SocketHead", vec![param_member("across_flats", "Real")]),
            bolt,
            driver,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
    };

    let compiled = reify_compiler::compile(&parsed);
    let errors = error_diagnostics(&compiled);

    let matching: Vec<&&reify_core::Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains("out of range or non-finite"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one 'out of range or non-finite' error for 2^63 cluster index, \
         got {} (all errors: {:#?})",
        matching.len(),
        errors
    );
    assert_no_cascade(&errors);
}
