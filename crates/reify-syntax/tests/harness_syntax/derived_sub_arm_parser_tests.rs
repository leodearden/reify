//! AST-level (CST→AST lowering) tests for the **derived sub arm** —
//! `sub b = mirror of a across <plane> { … }` / `sub b = image of a under
//! <transform> { … }`.
//!
//! Leaf A-alpha of `docs/prds/v0_6/assembly-derivation-toolbox.md` (task #6615).
//! The CST half of the contract lives in
//! `tree-sitter-reify/tests/derived_sub_grammar_tests.rs`; this file pins that
//! the new `derivation` CST field actually reaches `SubDecl::derivation`.
//!
//! Registered in `crates/reify-syntax/tests/harness_syntax.rs` — that harness
//! root is a single integration-test compile unit (task #5275), so an
//! unregistered file here is never compiled and its tests silently do not run.
//!
//! # TDD status
//!
//! Every test below is **RED before the AST/lowering steps** — and RED as a
//! COMPILE error, not an assertion failure, because `SubDecl` has no
//! `derivation` field yet (the same shape the sibling
//! `keyed_sub_member_block_parser_tests` used for `keyed_members`). They go
//! GREEN once the types land and `lower_sub` populates them.

use reify_ast::ast::ExprKind;
use reify_ast::decl::{Declaration, MemberDecl, SubDecl, SubDerivationKind, SubDispositionKind};
use reify_core::ModulePath;

/// Parse `source` and return the first structure's first `sub` member,
/// asserting the parse produced no errors.
///
/// Same shape as `keyed_sub_member_block_parser_tests::parse_first_sub` /
/// `indexed_sub_instantiation_parser_tests::first_sub`.
fn parse_first_sub(source: &str) -> SubDecl {
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "unexpected parse errors: {:?}",
        module.errors,
    );
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration");
    };
    let MemberDecl::Sub(sub) = s
        .members
        .iter()
        .find(|m| matches!(m, MemberDecl::Sub(_)))
        .expect("expected a Sub member")
    else {
        unreachable!()
    };
    sub.clone()
}

/// The `Ident` name behind an expression, for asserting which identifier
/// reached a plane/transform slot without pinning the whole `Expr`.
fn ident_name(expr: &reify_ast::ast::Expr) -> &str {
    match &expr.kind {
        ExprKind::Ident(name) => name,
        other => panic!("expected an Ident expression, got {other:?}"),
    }
}

/// PRIMARY AST SIGNAL — the mirror arm lowers into `SubDecl::derivation`.
///
/// The prototype span is asserted to cover the prototype token ALONE: A-beta
/// (#6616) underlines exactly that span for its unknown / non-sibling /
/// cyclic-prototype diagnostics, so a derivation-wide span would silently
/// degrade every one of those messages.
#[test]
fn mirror_arm_lowers_into_sub_derivation() {
    const SRC: &str = "structure S {
    sub unit_b = mirror of unit_a across plane_yz {
        z = 55mm
        keep capstan
    }
}";
    let sub = parse_first_sub(SRC);
    let derivation = sub
        .derivation
        .as_ref()
        .expect("the mirror arm must lower into SubDecl::derivation");

    match &derivation.kind {
        SubDerivationKind::Mirror { plane } => assert_eq!(ident_name(plane), "plane_yz"),
        other => panic!("expected SubDerivationKind::Mirror, got {other:?}"),
    }

    assert_eq!(derivation.prototype.name, "unit_a");
    let proto_span = derivation.prototype.span;
    assert_eq!(
        &SRC[proto_span.start as usize..proto_span.end as usize],
        "unit_a",
        "the prototype span must cover the prototype token ALONE, so A-beta's \
         unknown-prototype diagnostic underlines exactly it"
    );

    assert_eq!(derivation.param_overrides.len(), 1);
    assert_eq!(derivation.param_overrides[0].0, "z");
    assert!(
        matches!(
            derivation.param_overrides[0].1.kind,
            ExprKind::QuantityLiteral { value, .. } if value == 55.0
        ),
        "got {:?}",
        derivation.param_overrides[0].1.kind
    );

    assert_eq!(derivation.dispositions.len(), 1);
    assert!(matches!(
        derivation.dispositions[0].kind,
        SubDispositionKind::Keep
    ));
    assert_eq!(derivation.dispositions[0].path, vec!["capstan"]);
    assert!(
        derivation.dispositions[0].using_plane.is_none(),
        "a bare `keep` must leave the reserved `using_plane` slot empty"
    );
}

/// The image arm lowers with an `Image` constructor, and `<param> = default`
/// lands in `param_resets` rather than `param_overrides`.
///
/// Two constructors of ONE element type (PRD §6 D1), not two flattened
/// plane/transform fields: Layer-3 group elements become further constructors,
/// so consumers switch on the constructor.
#[test]
fn image_arm_lowers_with_param_resets_separated_from_overrides() {
    let sub = parse_first_sub(
        "structure S {
    sub rail_l = image of rail_r under c2_z {
        span_au = 430mm
        span_bu = default
    }
}",
    );
    let derivation = sub.derivation.as_ref().expect("expected a derivation");

    match &derivation.kind {
        SubDerivationKind::Image { transform } => assert_eq!(ident_name(transform), "c2_z"),
        other => panic!("expected SubDerivationKind::Image, got {other:?}"),
    }

    assert_eq!(
        derivation
            .param_overrides
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["span_au"],
        "`span_bu = default` is a RESET and must not appear as an override"
    );
    assert_eq!(
        derivation
            .param_resets
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        vec!["span_bu"],
    );
}

/// Dispositions preserve SOURCE ORDER, dotted paths split into segments, and
/// the reserved `using <plane>` tail is stored.
///
/// Source order is load-bearing: A-beta resolves dispositions against the
/// prototype's feature tree, and a reordered list would make its diagnostics
/// point at the wrong item.
#[test]
fn dispositions_preserve_source_order_and_split_dotted_paths() {
    let sub = parse_first_sub(
        "structure S {
    sub b = mirror of a across P {
        keep a
        exclude b.c
        keep d
        keep drum using plane_xz
    }
}",
    );
    let dispositions = &sub.derivation.as_ref().expect("expected a derivation").dispositions;

    let summary: Vec<(bool, Vec<String>, bool)> = dispositions
        .iter()
        .map(|d| {
            (
                matches!(d.kind, SubDispositionKind::Keep),
                d.path.clone(),
                d.using_plane.is_some(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (true, vec!["a".to_string()], false),
            (false, vec!["b".to_string(), "c".to_string()], false),
            (true, vec!["d".to_string()], false),
            (true, vec!["drum".to_string()], true),
        ],
        "dispositions must lower in source order, with dotted paths split into \
         segments and the reserved `using <plane>` slot populated only where \
         the source carries it"
    );

    let using = dispositions[3]
        .using_plane
        .as_ref()
        .expect("the `using` tail must populate using_plane");
    assert_eq!(ident_name(using), "plane_xz");
}

/// `auto` / `auto(free)` overrides reach `ExprKind::Auto`, exactly as on the
/// specialization arm — so a derived override needs no separate lowering path.
#[test]
fn derived_overrides_admit_auto_binding_values() {
    let sub = parse_first_sub(
        "structure S {
    sub b = mirror of a across P {
        z = auto
        w = auto(free)
    }
}",
    );
    let overrides = &sub.derivation.as_ref().expect("expected a derivation").param_overrides;
    assert_eq!(overrides.len(), 2);
    assert!(
        matches!(overrides[0].1.kind, ExprKind::Auto { free: false, .. }),
        "got {:?}",
        overrides[0].1.kind
    );
    assert!(
        matches!(overrides[1].1.kind, ExprKind::Auto { free: true, .. }),
        "got {:?}",
        overrides[1].1.kind
    );
}

/// The derived body's `let` and `constraint` children land in
/// `SubDerivation::members` as ordinary `MemberDecl`s.
#[test]
fn derived_body_members_lower_as_let_and_constraint() {
    let sub = parse_first_sub(
        "structure S {
    sub b = mirror of a across P {
        let g = 3mm
        constraint g > 1mm
    }
}",
    );
    let members = &sub.derivation.as_ref().expect("expected a derivation").members;
    assert_eq!(members.len(), 2, "got {members:?}");
    assert!(matches!(members[0], MemberDecl::Let(_)), "got {:?}", members[0]);
    assert!(
        matches!(members[1], MemberDecl::Constraint(_)),
        "got {:?}",
        members[1]
    );
}

/// `at <pose>` on a derived sub still lowers into `SubDecl::pose_expr` and
/// emits NO parse-layer error.
///
/// This is the AST-layer half of the D3-adversary ownership ruling: rejecting
/// an explicit `at` on a derived sub is `E_DERIVED_SUB_EXPLICIT_AT` (T8),
/// A-beta's (#6616) COMPILE-scope diagnostic. Emitting anything here would
/// pre-empt it with a worse message. `parse_first_sub` asserts
/// `module.errors.is_empty()`, so the no-error half is enforced too.
#[test]
fn explicit_at_on_a_derived_sub_lowers_without_a_parse_error() {
    let sub = parse_first_sub("structure S { sub b = mirror of a across P { } at origin }");
    assert!(sub.derivation.is_some());
    let pose = sub
        .pose_expr
        .as_ref()
        .expect("`at origin` must still reach SubDecl::pose_expr");
    assert_eq!(ident_name(pose), "origin");
}

/// THE DISCRIMINATOR INVARIANT.
///
/// When `derivation.is_some()`, every specialization-scope field is empty. The
/// derived arm shares no surface with the other three, so a consumer that sees
/// a derivation can rely on those fields carrying no meaning — and, critically,
/// existing compiler consumers that read `structure_name` / `args` /
/// `is_collection` as specialization-scope signals cannot mistake a derived sub
/// for one of their own.
///
/// Asserted here, at the AST layer, because `lower_sub` is the single producer
/// and this is the contract A-beta (#6616) builds on.
#[test]
fn derived_sub_leaves_every_specialization_scope_field_empty() {
    for source in [
        "structure S { sub b = mirror of a across P { z = 1mm  keep k } }",
        "structure S { sub b = image of a under T { } at origin }",
        "structure S { priv aux sub b = mirror of a across P { let g = 1mm } }",
    ] {
        let sub = parse_first_sub(source);
        assert!(sub.derivation.is_some(), "{source}");
        assert!(sub.structure_name.is_empty(), "{source}: structure_name");
        assert!(sub.body.is_none(), "{source}: body");
        assert!(
            sub.spec_param_overrides.is_empty(),
            "{source}: spec_param_overrides"
        );
        assert!(sub.keyed_members.is_empty(), "{source}: keyed_members");
        assert!(sub.args.is_empty(), "{source}: args");
        assert!(sub.type_args.is_empty(), "{source}: type_args");
        assert!(!sub.is_collection, "{source}: is_collection");
    }
}

/// `priv` and `aux` still reach the derived arm's `SubDecl`.
#[test]
fn derived_sub_preserves_priv_and_aux_modifiers() {
    let sub = parse_first_sub("structure S { priv aux sub b = mirror of a across P { } }");
    assert!(sub.derivation.is_some());
    assert!(sub.is_priv, "`priv` must reach the derived arm's SubDecl");
    assert!(sub.is_aux, "`aux` must reach the derived arm's SubDecl");
}

/// Regression floor — the three PRE-EXISTING `sub` arms all lower with
/// `derivation == None`, so nothing that already worked starts looking derived.
#[test]
fn existing_sub_arms_lower_with_no_derivation() {
    for source in [
        "structure S { sub a = Unit(z: 1mm) at origin }",
        "structure S { sub xs : List<Vent> }",
        "structure S { sub m : Motor { bore = auto } at f }",
    ] {
        let sub = parse_first_sub(source);
        assert!(
            sub.derivation.is_none(),
            "{source}: a pre-existing arm must not lower a derivation"
        );
    }
}
