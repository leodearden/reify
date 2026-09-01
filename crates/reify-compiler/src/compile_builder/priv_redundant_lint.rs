//! E_PRIV_REDUNDANT lint pass (task #3978 δ — module-and-visibility-hardening Slice C).
//!
//! Walks every member of every structure, occurrence, trait, and purpose body
//! recursively (GuardedGroup / SubDecl body / MatchArmDeclGroup / PortDecl body)
//! and emits a [`DiagnosticCode::PrivRedundant`] `Severity::Error` when a `let`
//! or `constraint` member carries `is_priv == true`.
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
//! This pass covers the same nesting as [`reify_ast::walk_specialization_scope_members`]:
//! - Top-level members of `structure`, `occurrence`, `trait`, `purpose` bodies.
//! - `MemberDecl::Sub` bodies (specialization scopes, `s.body.is_some()`).
//! - `MemberDecl::GuardedGroup` — both `members` (where) and `else_members` (else).
//! - `MemberDecl::MatchArmDeclGroup` — each arm's `member`.
//! - `MemberDecl::Port` bodies (`p.members`).
//!
//! Depth is bounded by [`MAX_DEPTH`] (mirrors [`reify_ast::MAX_MEMBER_NESTING_DEPTH`]).

use reify_ast::{Declaration, MemberDecl, ParsedModule};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel};

/// Stack-safety bound on the recursive member walk.
///
/// 32 mirrors [`reify_ast::MAX_MEMBER_NESTING_DEPTH`].
const MAX_DEPTH: usize = 32;

/// Walk every declaration in `parsed` and emit a [`DiagnosticCode::PrivRedundant`]
/// `Severity::Error` for every `let` or `constraint` member carrying `is_priv == true`.
pub(crate) fn lint_module(parsed: &ParsedModule, diagnostics: &mut Vec<Diagnostic>) {
    for decl in &parsed.declarations {
        match decl {
            Declaration::Structure(s) => lint_members(&s.members, diagnostics, 0),
            Declaration::Occurrence(o) => lint_members(&o.members, diagnostics, 0),
            Declaration::Trait(t) => lint_members(&t.members, diagnostics, 0),
            Declaration::Purpose(p) => {
                lint_members(&p.members, diagnostics, 0);
                // Also walk structures nested in the purpose body.
                for s in &p.structures {
                    lint_members(&s.members, diagnostics, 0);
                }
            }
            _ => {}
        }
    }
}

/// Recursively lint a member list at the given nesting depth.
fn lint_members(members: &[MemberDecl], diagnostics: &mut Vec<Diagnostic>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for member in members {
        match member {
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
            // Recurse into Sub bodies (specialization scopes).
            MemberDecl::Sub(s) => {
                if let Some(body) = s.body.as_ref() {
                    lint_members(body, diagnostics, depth + 1);
                }
            }
            // Recurse into both branches of a GuardedGroup.
            MemberDecl::GuardedGroup(g) => {
                lint_members(&g.members, diagnostics, depth + 1);
                lint_members(&g.else_members, diagnostics, depth + 1);
            }
            // Recurse into each arm of a MatchArmDeclGroup.
            MemberDecl::MatchArmDeclGroup(g) => {
                for arm in &g.arms {
                    lint_members(std::slice::from_ref(&*arm.member), diagnostics, depth + 1);
                }
            }
            // Recurse into Port body members.
            MemberDecl::Port(p) => {
                lint_members(&p.members, diagnostics, depth + 1);
            }
            _ => {}
        }
    }
}

// ── inline unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity};

    use super::lint_members;

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
        lint_members(members, &mut diags, 0);
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

        // NON-VACUITY: the `priv let` must really be NESTED in the sub's body.
        // If the parser hoisted it to top level this test would pass under
        // EVERY recursion set and silently lose the discriminating power it
        // exists for.
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

        // NON-VACUITY: as above — the `priv let` must really be nested inside
        // `PortDecl.members`, or this test discriminates nothing.
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

    /// A `priv let` at exactly the nesting bound is reported; one level deeper
    /// is cut off.
    ///
    /// Pins the depth-32 cutoff behaviourally, so the bound survives being
    /// re-homed from this module's own constant onto the shared walker's
    /// `reify_ast::MAX_MEMBER_NESTING_DEPTH`.
    /// Depth of the actual `where`-nesting chain in `members`.
    ///
    /// NON-VACUITY guard for the test below: without it, a fixture the parser
    /// silently truncated would report 0 diagnostics and be indistinguishable
    /// from a genuine depth cutoff.
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

    #[test]
    fn nesting_beyond_max_depth_is_cut_off() {
        let at_limit = parse_first_structure_members(&nested_where_source(32));
        let beyond_limit = parse_first_structure_members(&nested_where_source(33));
        assert_eq!(
            guarded_nesting_depth(&at_limit),
            32,
            "the 32-level fixture must really nest 32 deep"
        );
        assert_eq!(
            guarded_nesting_depth(&beyond_limit),
            33,
            "the 33-level fixture must really nest 33 deep — a truncated parse \
             would make the cutoff assertion below vacuous"
        );

        let redundant = priv_redundant_diags(&at_limit);
        assert_eq!(
            redundant.len(),
            1,
            "a `priv let` at exactly 32 levels of nesting must be reported, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
        );

        let redundant = priv_redundant_diags(&beyond_limit);
        assert_eq!(
            redundant.len(),
            0,
            "a `priv let` at 33 levels of nesting is beyond the bound and must be \
             cut off, got {}: {:?}",
            redundant.len(),
            messages(&redundant)
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
