//! A `connect` endpoint as typed data: the root it hangs off and the port
//! member dotted below it.
//!
//! [`PortPath::parse`] is the one connect-endpoint grammar. The per-element
//! `compile_connection` path and the deferred forall-connect capture both go
//! through it, so they accept the same shapes and render the same names.
//! Inside a `forall v in coll` body the bound variable parses to
//! [`PortRoot::Element`], kept distinct from a source-written `coll[0]`, and
//! only [`PortPath::at_element`] turns it into a concrete element.

use reify_ast::{Expr, ExprKind};
use reify_ir::MemberKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortPath {
    pub root: PortRoot,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRoot {
    /// The `forall` bound variable: whichever element is being materialised.
    Element,
    /// A source-written indexed sub-component, e.g. `vents[0]` or `bays["intake"]`.
    Literal { sub: String, index: ElementIndex },
    /// A bare name: an own port or a sub-component.
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementIndex {
    Position(i64),
    Key(MemberKey),
}

impl ElementIndex {
    fn segment(&self, sub: &str) -> String {
        match self {
            ElementIndex::Position(i) => format!("{sub}[{i}]"),
            ElementIndex::Key(key) => key.path_segment(sub),
        }
    }
}

impl PortRoot {
    /// The rendered root, or `None` for [`PortRoot::Element`], which only a
    /// caller naming the element can render.
    fn fixed_segment(&self) -> Option<String> {
        match self {
            PortRoot::Element => None,
            PortRoot::Literal { sub, index } => Some(index.segment(sub)),
            PortRoot::Named(name) => Some(name.clone()),
        }
    }
}

impl PortPath {
    /// Parse a connect endpoint. `element_var` names the enclosing `forall`'s
    /// bound variable, if any; a root `Ident` equal to it becomes
    /// [`PortRoot::Element`]. Returns `None` for any shape outside the grammar,
    /// including an indexed bound variable (`v[0].x`) and nested members
    /// (`v.inner.a`).
    pub(crate) fn parse(expr: &Expr, element_var: Option<&str>) -> Option<PortPath> {
        match &expr.kind {
            ExprKind::AdHocSelector { base, .. } => Self::parse(base, element_var),
            ExprKind::MemberAccess { object, member } => Some(PortPath {
                root: parse_root(object, element_var)?,
                members: vec![member.clone()],
            }),
            _ => Some(PortPath {
                root: parse_root(expr, element_var)?,
                members: Vec::new(),
            }),
        }
    }

    /// The port name with an [`PortRoot::Element`] root rendered as
    /// `coll_sub[i]`. A literal index is rendered as written.
    pub fn at_element(&self, coll_sub: &str, i: i64) -> String {
        let root = self
            .root
            .fixed_segment()
            .unwrap_or_else(|| ElementIndex::Position(i).segment(coll_sub));
        self.joined_under(root)
    }

    /// The port name every element's endpoint shares, with an
    /// [`PortRoot::Element`] root rendered as `coll_sub[var]`, so it never names
    /// one element. The bracket form keeps the sub-member lookup's indexer
    /// stripping working. A literal index is rendered as written.
    pub(crate) fn at_bound_variable(&self, coll_sub: &str, var: &str) -> String {
        let root = self
            .root
            .fixed_segment()
            .unwrap_or_else(|| format!("{coll_sub}[{var}]"));
        self.joined_under(root)
    }

    /// The port name, or `None` when the root is the unbound element.
    pub(crate) fn port_name(&self) -> Option<String> {
        Some(self.joined_under(self.root.fixed_segment()?))
    }

    fn joined_under(&self, root: String) -> String {
        std::iter::once(root)
            .chain(self.members.iter().cloned())
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn parse_root(expr: &Expr, element_var: Option<&str>) -> Option<PortRoot> {
    match &expr.kind {
        ExprKind::Ident(name) if Some(name.as_str()) == element_var => Some(PortRoot::Element),
        ExprKind::Ident(name) => Some(PortRoot::Named(name.clone())),
        ExprKind::IndexAccess { object, index } => {
            let ExprKind::Ident(sub) = &object.kind else {
                return None;
            };
            if Some(sub.as_str()) == element_var {
                return None;
            }
            let index = match &index.kind {
                ExprKind::NumberLiteral { value, .. } => ElementIndex::Position(*value as i64),
                ExprKind::StringLiteral(key) => ElementIndex::Key(MemberKey::new(key.clone())),
                _ => return None,
            };
            Some(PortRoot::Literal {
                sub: sub.clone(),
                index,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::SourceSpan;

    fn expr(kind: ExprKind) -> Expr {
        Expr {
            kind,
            span: SourceSpan::new(0, 0),
        }
    }

    fn ident(name: &str) -> Expr {
        expr(ExprKind::Ident(name.to_string()))
    }

    fn member(object: Expr, member: &str) -> Expr {
        expr(ExprKind::MemberAccess {
            object: Box::new(object),
            member: member.to_string(),
        })
    }

    fn indexed(object: Expr, index: Expr) -> Expr {
        expr(ExprKind::IndexAccess {
            object: Box::new(object),
            index: Box::new(index),
        })
    }

    fn position(value: f64) -> Expr {
        expr(ExprKind::NumberLiteral {
            value,
            is_real: false,
        })
    }

    #[test]
    fn bare_bound_variable_is_the_element_with_no_members() {
        let path = PortPath::parse(&ident("v"), Some("v")).unwrap();

        assert_eq!(
            path,
            PortPath {
                root: PortRoot::Element,
                members: Vec::new(),
            }
        );
        assert_eq!(path.at_element("vents", 2), "vents[2]");
        assert_eq!(path.at_bound_variable("vents", "v"), "vents[v]");
        assert_eq!(path.port_name(), None);
    }

    #[test]
    fn ad_hoc_selector_parses_as_its_base() {
        let selected = expr(ExprKind::AdHocSelector {
            base: Box::new(member(ident("v"), "p")),
            selector: "face".to_string(),
            args: Vec::new(),
        });

        let path = PortPath::parse(&selected, Some("v")).unwrap();

        assert_eq!(path.root, PortRoot::Element);
        assert_eq!(path.at_element("vents", 1), "vents[1].p");
    }

    #[test]
    fn literal_index_on_the_collection_is_kept_at_every_element() {
        let endpoint = member(indexed(ident("vents"), position(0.0)), "outlet");

        let path = PortPath::parse(&endpoint, Some("v")).unwrap();

        assert_eq!(path.at_element("vents", 2), "vents[0].outlet");
        assert_eq!(path.at_bound_variable("vents", "v"), "vents[0].outlet");
        assert_eq!(path.port_name().as_deref(), Some("vents[0].outlet"));
    }

    #[test]
    fn indexed_bound_variable_is_rejected() {
        let endpoint = member(indexed(ident("v"), position(0.0)), "x");

        assert_eq!(PortPath::parse(&endpoint, Some("v")), None);
    }

    #[test]
    fn non_literal_index_is_rejected() {
        let endpoint = member(indexed(ident("vents"), ident("k")), "x");

        assert_eq!(PortPath::parse(&endpoint, Some("v")), None);
    }
}
