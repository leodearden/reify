//! The typed reason a residual has no usable gradient row: the refusal a
//! dual traversal returns in place of a derivative it cannot honestly state.

use crate::branch_signature::KinkSite;

/// Why a residual has no usable gradient row.
///
/// Every variant names the construct responsible, because η (#6675) turns this
/// into a tier-2 refusal the user reads WITHOUT the surrounding code.
///
/// This type exists so that "the derivative is zero" and "we could not compute
/// the derivative" can never be confused.  Returning a zero row for the second
/// case is a *claim* — that the residual does not move when the variable moves
/// — and it is false in the most damaging possible way: the solver believes it
/// is already stationary in that direction and stops pushing.  PRD §7.7
/// ("non-numeric operands stop contributing phantom gradients") and INV-SF-7
/// ("a well-typed wrong value is the worst shape") are the same rule seen from
/// two sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonDifferentiable {
    /// The residual evaluated to `Undef` — the value semantics refused to
    /// produce a number, so there is no function to differentiate.
    UndefPrimal { site: KinkSite },
    /// A seed-dependent subtree reached a construct with no derivative rule.
    UnsupportedKind { kind: &'static str, site: KinkSite },
    /// The residual is not a scalar, so it has no gradient ROW at all.
    NonScalarResult { got: &'static str },
    /// A tangent component came out NaN or ±Inf.  The primal may look perfectly
    /// ordinary, which is exactly why this is checked rather than assumed.
    NonFiniteTangent { column: usize },
}

impl NonDifferentiable {
    /// This refusal with its site re-rooted under `prefix`.
    ///
    /// The sibling of [`BranchRecord::prefixed`], and there for the same
    /// reason: a refusal raised while evaluating some SUBSIDIARY expression — a
    /// solver dependent cell — is root-relative to THAT expression, so
    /// reporting it against a residual without moving it would name a site in
    /// the residual's own tree.  The two variants that carry no site are
    /// returned unchanged.
    ///
    /// [`BranchRecord::prefixed`]: crate::BranchRecord::prefixed
    pub fn prefixed(&self, prefix: &[u16]) -> NonDifferentiable {
        let moved = |site: &KinkSite| KinkSite::new([prefix, site.path()].concat());
        match self {
            NonDifferentiable::UndefPrimal { site } => {
                NonDifferentiable::UndefPrimal { site: moved(site) }
            }
            NonDifferentiable::UnsupportedKind { kind, site } => {
                NonDifferentiable::UnsupportedKind { kind, site: moved(site) }
            }
            other => other.clone(),
        }
    }
}

impl std::fmt::Display for NonDifferentiable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NonDifferentiable::UndefPrimal { site } => write!(
                f,
                "residual evaluates to undef at expression path {:?}, so it has no derivative",
                site.path()
            ),
            NonDifferentiable::UnsupportedKind { kind, site } => write!(
                f,
                "no derivative rule for {kind} at expression path {:?}; \
                 the residual depends on a solved variable through it",
                site.path()
            ),
            NonDifferentiable::NonScalarResult { got } => write!(
                f,
                "residual must be a scalar to have a gradient row, but it evaluated to {got}"
            ),
            NonDifferentiable::NonFiniteTangent { column } => write!(
                f,
                "derivative with respect to variable {column} is not finite \
                 (NaN or infinite), so it cannot enter a linear solve"
            ),
        }
    }
}

impl std::error::Error for NonDifferentiable {}
