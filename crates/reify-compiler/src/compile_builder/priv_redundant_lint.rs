//! E_PRIV_REDUNDANT lint pass (task #3978 δ — module-and-visibility-hardening Slice C).
//!
//! Walks every member of every structure, occurrence, trait, and purpose body
//! recursively and emits a [`DiagnosticCode::PrivRedundant`] `Severity::Error`
//! when a `let` or `constraint` member carries `is_priv == true`.
//!
//! # Why E_PRIV_REDUNDANT?
//!
//! The `priv` modifier is meaningful ONLY on `param`, `sub`, and `port` members
//! — it hides them from external dot-access.  `let` and `constraint` members are
//! already inaccessible outside the defining structure body (they are not
//! exported, not accessible via dot-access on any receiver), so `priv let` and
//! `priv constraint` are always redundant and are rejected as a static error.
//!
//! # Walk coverage
//!
//! This pass does not walk members itself — it delegates to
//! [`reify_ast::walk_all_member_bodies`], which owns both the recursion set and
//! the depth bound ([`reify_ast::MAX_MEMBER_NESTING_DEPTH`]).
//!
//! One gap survives that delegation: no reify-ast walker descends into a sub's
//! keyed entries, so a `priv let` inside `sub p : Foo { "a" => { … } }` is not
//! reported.  Longstanding, and unchanged by the delegation.

use reify_ast::{Declaration, MemberDecl, ParsedModule, walk_all_member_bodies};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel};

/// Walk every declaration in `parsed` and emit a [`DiagnosticCode::PrivRedundant`]
/// `Severity::Error` for every `let` or `constraint` member carrying `is_priv == true`.
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for decl in &parsed.declarations {
        match decl {
            Declaration::Structure(s) => lint_members(&s.members, diagnostics),
            Declaration::Occurrence(o) => lint_members(&o.members, diagnostics),
            Declaration::Trait(t) => lint_members(&t.members, diagnostics),
            Declaration::Purpose(p) => {
                lint_members(&p.members, diagnostics);
                // Also walk structures nested in the purpose body.
                for s in &p.structures {
                    lint_members(&s.members, diagnostics);
                }
            }
            _ => {}
        }
    }
}

/// Lint every member reachable under `members`, at any nesting depth.
///
/// Nesting and the depth bound both belong to
/// [`reify_ast::walk_all_member_bodies`]; this function contributes only the
/// per-member predicate.
fn lint_members(members: &[MemberDecl], diagnostics: &mut Vec<Diagnostic>) {
    // `_ => {}` is this pass's PREDICATE — "not a priv let/constraint" — and
    // not a recursion decision: reify-ast's walker owns which bodies are
    // descended into, and classifies a new `MemberDecl` variant there.
    walk_all_member_bodies(members, &mut |member| match member {
        MemberDecl::Let(l) if l.is_priv => {
            diagnostics.push(
                Diagnostic::error(
                    "E_PRIV_REDUNDANT: 'priv' is not valid on let/constraint members; \
                     'let' bindings are already private to the structure body",
                )
                .with_code(DiagnosticCode::PrivRedundant)
                .with_label(DiagnosticLabel::new(l.span, "'priv' not allowed here")),
            );
        }
        MemberDecl::Constraint(c) if c.is_priv => {
            diagnostics.push(
                Diagnostic::error(
                    "E_PRIV_REDUNDANT: 'priv' is not valid on let/constraint members; \
                     'constraint' members are already private to the structure body",
                )
                .with_code(DiagnosticCode::PrivRedundant)
                .with_label(DiagnosticLabel::new(c.span, "'priv' not allowed here")),
            );
        }
        _ => {}
    });
}

// ── inline unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use reify_ast::MAX_MEMBER_NESTING_DEPTH;
    use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity};

    use super::{lint_members, lint_module};

    /// Parse `source` as a module and extract the first structure's member list.
    ///
    /// Refuses a fixture the parser could not read. Without that check a
    /// mis-typed fixture would lower to an empty/truncated member list, and a
    /// "must emit 0 diagnostics" assertion over it would pass VACUOUSLY.
    fn parse_first_structure_members(source: &str) -> Vec<reify_ast::MemberDecl> {
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse cleanly, got {:?} for source:\n{source}",
            parsed.errors
        );
        match &parsed.declarations[0] {
            reify_ast::Declaration::Structure(s) => s.members.clone(),
            other => panic!("expected Structure declaration, got {:?}", other),
        }
    }

    /// Run the lint over `members` and keep only its `PrivRedundant` diagnostics.
    ///
    /// Every test in this module goes through this one helper, so `lint_members`'
    /// signature appears exactly once here. The filter is a no-op in practice —
    /// `lint_members` emits nothing else — but it keeps each test asserting about
    /// the code it names rather than about "whatever the pass happened to emit".
    fn priv_redundant_diags(members: &[reify_ast::MemberDecl]) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        lint_members(members, &mut diags);
        diags
            .into_iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect()
    }

    /// Diagnostic messages, for assertion failure output.
    fn messages(diags: &[Diagnostic]) -> Vec<&String> {
        diags.iter().map(|d| &d.message).collect()
    }

    // --- top-level coverage: priv let / priv constraint ---

    /// `priv let x = 5` emits exactly one PrivRedundant error.
    #[test]
    fn top_level_priv_let_emits_priv_redundant() {
        let members = parse_first_structure_members("structure S { priv let x = 5 }");
        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for top-level `priv let`, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
        assert_eq!(redundant[0].severity, Severity::Error);
        assert!(redundant[0].message.contains("E_PRIV_REDUNDANT"));
        assert!(!redundant[0].labels.is_empty());
    }

    /// `priv constraint t > 0` emits exactly one PrivRedundant error.
    #[test]
    fn top_level_priv_constraint_emits_priv_redundant() {
        let members = parse_first_structure_members(
            "structure S { param t : Real = 1  priv constraint t > 0 }",
        );
        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for top-level `priv constraint`, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
        assert_eq!(redundant[0].severity, Severity::Error);
        assert!(redundant[0].message.contains("E_PRIV_REDUNDANT"));
    }

    // --- GuardedGroup coverage ---

    /// `priv let` inside a `where { … } else { … }` guarded group is found.
    #[test]
    fn priv_let_inside_guarded_group_is_detected() {
        let source = r#"
structure S {
    param flag : Real = 1
    where flag > 0 {
        priv let y = 2
    } else {
        let z = 3
    }
}
"#;
        let members = parse_first_structure_members(source);
        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` inside guarded group, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    /// `priv let` in the ELSE branch is found too — `else_members` is a
    /// recursion site distinct from `members`, and the test above only pins
    /// the WHERE branch.
    #[test]
    fn priv_let_inside_guarded_else_branch_is_detected() {
        let source = r#"
structure S {
    param flag : Real = 1
    where flag > 0 {
        let y = 2
    } else {
        priv let z = 3
    }
}
"#;
        let members = parse_first_structure_members(source);
        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` in the guarded ELSE branch, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    // --- recursion-set discriminators: sub body and port body ---
    //
    // These two are the ONLY compiler-side tests with the power to catch a walk
    // wired to the wrong member-recursion set. `GuardedGroup` and
    // `MatchArmDeclGroup` are recursed unconditionally, so no set value can
    // switch them off; `SubDecl.body` and `PortDecl.members` are the two cells
    // that actually differ between sets.

    /// `priv let` inside a `sub`'s specialization-override body is found.
    ///
    /// Fails if the walk is wired to a recursion set that skips `SubDecl.body`.
    #[test]
    fn priv_let_inside_sub_body_is_detected() {
        let source = r#"
structure S {
    sub motor : ElectricMotor {
        priv let m = 1mm
    }
}
"#;
        let members = parse_first_structure_members(source);

        // NON-VACUITY: a `priv let` the parser hoisted to top level would be
        // found under EVERY recursion set, discriminating nothing.
        let sub = match members.as_slice() {
            [reify_ast::MemberDecl::Sub(s)] => s,
            other => panic!("fixture must lower to exactly one Sub member, got {other:#?}"),
        };
        let body = sub
            .body
            .as_ref()
            .expect("the sub must open a specialization scope (body: Some)");
        assert!(
            body.iter()
                .any(|m| matches!(m, reify_ast::MemberDecl::Let(l) if l.is_priv)),
            "the `priv let` must live INSIDE SubDecl.body, got {body:#?}"
        );

        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` inside a sub body, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    /// `priv let` inside a `port` body is found.
    ///
    /// Fails if the walk is wired to a recursion set that skips
    /// `PortDecl.members` — notably `SPECIALIZATION_SCOPE`, which deliberately
    /// does not descend into port bodies.
    #[test]
    fn priv_let_inside_port_body_is_detected() {
        let source = r#"
structure S {
    port mount : MyPort {
        priv let v = 1mm
    }
}
"#;
        let members = parse_first_structure_members(source);

        // NON-VACUITY: as above, for `PortDecl.members`.
        let port = match members.as_slice() {
            [reify_ast::MemberDecl::Port(p)] => p,
            other => panic!("fixture must lower to exactly one Port member, got {other:#?}"),
        };
        assert!(
            port.members
                .iter()
                .any(|m| matches!(m, reify_ast::MemberDecl::Let(l) if l.is_priv)),
            "the `priv let` must live INSIDE PortDecl.members, got {:#?}",
            port.members
        );

        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` inside a port body, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    // --- recursion-set discriminator: keyed sub entries ---

    /// The overrides of the single keyed entry of the single `sub` member in
    /// `members`, asserting the keyed lowering on the way through.
    ///
    /// NON-VACUITY guard shared by the two keyed tests below, in the same spirit
    /// as the hand-rolled guards in the sub-body and port-body tests above: a
    /// `priv` member the parser hoisted into `SubDecl.body` — or up to top level
    /// — would already be found by a pre-existing recursion site, so it would
    /// discriminate nothing.
    fn single_keyed_entry_overrides(members: &[reify_ast::MemberDecl]) -> &[reify_ast::MemberDecl] {
        let subs: Vec<_> = members
            .iter()
            .filter_map(|m| match m {
                reify_ast::MemberDecl::Sub(s) => Some(s),
                _ => None,
            })
            .collect();
        let sub = match subs.as_slice() {
            [s] => *s,
            other => panic!("fixture must contain exactly one Sub member, got {other:#?}"),
        };
        assert!(
            sub.body.is_none(),
            "the keyed form must lower with `body: None`, got {:#?}",
            sub.body
        );
        match sub.keyed_members.as_slice() {
            [entry] => &entry.overrides,
            other => panic!("fixture must lower to exactly one keyed entry, got {other:#?}"),
        }
    }

    /// `priv let` inside a keyed sub entry's overrides is found.
    ///
    /// `sub p : Foo { "a" => { … } }` lowers its overrides into
    /// `SubDecl.keyed_members[].overrides` with `body: None`, so this is a
    /// recursion site distinct from the `SubDecl.body` test above — and the one
    /// the pass was blind to before task 6958.
    #[test]
    fn priv_let_inside_keyed_sub_overrides_is_detected() {
        let members = parse_first_structure_members(
            r#"structure S { sub p : Foo { "a" => { priv let x = 1 } } }"#,
        );
        let overrides = single_keyed_entry_overrides(&members);
        assert!(
            overrides
                .iter()
                .any(|m| matches!(m, reify_ast::MemberDecl::Let(l) if l.is_priv)),
            "the `priv let` must live INSIDE keyed_members[0].overrides, got {overrides:#?}"
        );

        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` inside a keyed sub entry, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
        assert_eq!(redundant[0].severity, Severity::Error);
        assert!(redundant[0].message.contains("E_PRIV_REDUNDANT"));
    }

    /// `priv constraint` inside a keyed sub entry's overrides is found — the
    /// sibling predicate arm, over the same recursion site.
    #[test]
    fn priv_constraint_inside_keyed_sub_overrides_is_detected() {
        let members = parse_first_structure_members(
            r#"structure S { param t : Real = 1  sub p : Foo { "a" => { priv constraint t > 0 } } }"#,
        );
        let overrides = single_keyed_entry_overrides(&members);
        assert!(
            overrides
                .iter()
                .any(|m| matches!(m, reify_ast::MemberDecl::Constraint(c) if c.is_priv)),
            "the `priv constraint` must live INSIDE keyed_members[0].overrides, got {overrides:#?}"
        );

        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv constraint` inside a keyed sub entry, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    // --- depth bound ---

    /// `structure S { param flag …  where … { where … { … priv let deep = 1 } } }`
    /// with `depth` levels of guarded nesting around the `priv let`.
    fn nested_where_source(depth: usize) -> String {
        let mut inner = "priv let deep = 1".to_string();
        for _ in 0..depth {
            inner = format!("where flag > 0 {{ {inner} }}");
        }
        format!("structure S {{ param flag : Real = 1  {inner} }}")
    }

    /// Depth of the actual `where`-nesting chain in `members`.
    ///
    /// NON-VACUITY guard for the test below: a fixture the parser silently
    /// truncated would report 0 diagnostics and be indistinguishable from a
    /// genuine depth cutoff.
    fn guarded_nesting_depth(members: &[reify_ast::MemberDecl]) -> usize {
        members
            .iter()
            .filter_map(|m| match m {
                reify_ast::MemberDecl::GuardedGroup(g) => {
                    Some(1 + guarded_nesting_depth(&g.members))
                }
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// A `priv let` at exactly [`MAX_MEMBER_NESTING_DEPTH`] levels of nesting
    /// is reported; one level deeper is cut off.
    ///
    /// The bound belongs to reify-ast's walker; what this pins is that the lint
    /// inherits it end-to-end, from `.ri` source through the parser to the
    /// emitted diagnostic.
    #[test]
    fn nesting_beyond_max_depth_is_cut_off() {
        let at_limit =
            parse_first_structure_members(&nested_where_source(MAX_MEMBER_NESTING_DEPTH));
        let beyond_limit =
            parse_first_structure_members(&nested_where_source(MAX_MEMBER_NESTING_DEPTH + 1));
        assert_eq!(
            guarded_nesting_depth(&at_limit),
            MAX_MEMBER_NESTING_DEPTH,
            "the at-limit fixture must really nest {MAX_MEMBER_NESTING_DEPTH} deep"
        );
        assert_eq!(
            guarded_nesting_depth(&beyond_limit),
            MAX_MEMBER_NESTING_DEPTH + 1,
            "the beyond-limit fixture must really nest one deeper than \
             {MAX_MEMBER_NESTING_DEPTH} — a truncated parse would make the \
             cutoff assertion below vacuous"
        );

        let redundant = priv_redundant_diags(&at_limit);
        assert_eq!(
            redundant.len(),
            1,
            "a `priv let` at exactly {MAX_MEMBER_NESTING_DEPTH} levels of nesting must be \
             reported, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );

        let redundant = priv_redundant_diags(&beyond_limit);
        assert_eq!(
            redundant.len(),
            0,
            "a `priv let` one level beyond {MAX_MEMBER_NESTING_DEPTH} must be cut off, \
             got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );
    }

    // --- lint_module's declaration-level fan-out ---

    /// `lint_module` reaches each declaration kind it dispatches on: a trait, an
    /// occurrence, a purpose body, and a structure nested inside that purpose.
    ///
    /// Every other test here calls `lint_members` directly, so without this one
    /// a dropped fan-out arm — forgetting `p.structures`, say — reaches no
    /// assertion at all. One `priv let` per site, so a missing arm is a missing
    /// diagnostic rather than a changed count nobody can attribute.
    #[test]
    fn lint_module_reaches_every_dispatched_declaration_kind() {
        let source = r#"
trait T {
    priv let in_trait = 1
}
occurrence def O {
    priv let in_occurrence = 2
}
purpose P(subject : Structure) {
    priv let in_purpose = 3
    structure NestedInPurpose {
        priv let in_purpose_structure = 4
    }
}
"#;
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "fixture must parse cleanly, got {:?}",
            parsed.errors
        );

        // NON-VACUITY: the fixture must really exercise all four arms. A parse
        // that dropped a declaration, or hoisted the nested structure to top
        // level, would make the count below agree for the wrong reason.
        let kinds: Vec<&str> = parsed
            .declarations
            .iter()
            .map(|d| match d {
                reify_ast::Declaration::Trait(_) => "trait",
                reify_ast::Declaration::Occurrence(_) => "occurrence",
                reify_ast::Declaration::Purpose(_) => "purpose",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            ["trait", "occurrence", "purpose"],
            "fixture must lower to exactly one trait, one occurrence and one purpose"
        );
        let purpose = match &parsed.declarations[2] {
            reify_ast::Declaration::Purpose(p) => p,
            other => panic!("expected Purpose, got {other:?}"),
        };
        assert_eq!(
            purpose.structures.len(),
            1,
            "the nested structure must stay INSIDE the purpose, not be hoisted"
        );

        let mut diags = Vec::new();
        lint_module(&parsed, &mut diags);
        let redundant: Vec<&Diagnostic> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect();
        assert_eq!(
            redundant.len(),
            4,
            "expected one PrivRedundant per dispatched declaration kind (trait, \
             occurrence, purpose body, purpose-nested structure), got {}: {:?}",
            redundant.len(),
            redundant.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // --- plain let/constraint: no emission ---

    /// Plain `let x = 5` (no `priv`) must not emit PrivRedundant.
    #[test]
    fn plain_let_does_not_emit_priv_redundant() {
        let members = parse_first_structure_members("structure S { let x = 5 }");
        let redundant = priv_redundant_diags(&members);
        assert_eq!(
            redundant.len(),
            0,
            "plain `let x = 5` must not emit PrivRedundant, got: {:?}",
            messages(&redundant)
        );
    }
}
