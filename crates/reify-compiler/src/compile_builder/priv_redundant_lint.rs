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
    use reify_core::{DiagnosticCode, ModulePath, Severity};

    use super::lint_members;

    /// Parse `source` as a module and extract the first structure's member list.
    fn parse_first_structure_members(source: &str) -> Vec<reify_ast::MemberDecl> {
        let parsed =
            reify_syntax::parse(source, ModulePath::single("test"));
        match &parsed.declarations[0] {
            reify_ast::Declaration::Structure(s) => s.members.clone(),
            other => panic!("expected Structure declaration, got {:?}", other),
        }
    }

    // --- top-level coverage: priv let / priv constraint ---

    /// `priv let x = 5` emits exactly one PrivRedundant error.
    #[test]
    fn top_level_priv_let_emits_priv_redundant() {
        let members = parse_first_structure_members("structure S { priv let x = 5 }");
        let mut diags = Vec::new();
        lint_members(&members, &mut diags, 0);
        let redundant: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect();
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for top-level `priv let`, got {}: {:?}",
            redundant.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
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
        let mut diags = Vec::new();
        lint_members(&members, &mut diags, 0);
        let redundant: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect();
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for top-level `priv constraint`, got {}: {:?}",
            redundant.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
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
        let mut diags = Vec::new();
        lint_members(&members, &mut diags, 0);
        let redundant: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect();
        assert_eq!(
            redundant.len(),
            1,
            "expected 1 PrivRedundant for `priv let` inside guarded group, got {}: {:?}",
            redundant.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // --- plain let/constraint: no emission ---

    /// Plain `let x = 5` (no `priv`) must not emit PrivRedundant.
    #[test]
    fn plain_let_does_not_emit_priv_redundant() {
        let members = parse_first_structure_members("structure S { let x = 5 }");
        let mut diags = Vec::new();
        lint_members(&members, &mut diags, 0);
        let redundant: Vec<_> = diags
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::PrivRedundant))
            .collect();
        assert_eq!(
            redundant.len(),
            0,
            "plain `let x = 5` must not emit PrivRedundant, got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
