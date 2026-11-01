//! The per-auto clause a one-sided [`DiagnosticCode::ConstraintNonUnique`]
//! names its cause with: no constraint bounds `X` above.
//!
//! Two producers write it, the compile-time static arm in reify-compiler and
//! the runtime default-bound evidence path in reify-constraints, and tests in
//! several crates match it. Both render through [`unbounded_auto_clause`] so
//! the clause is spelled in one place.
//!
//! [`DiagnosticCode::ConstraintNonUnique`]: super::DiagnosticCode::ConstraintNonUnique

use std::fmt;

use crate::ValueCellId;

/// A side of a scalar `auto` that a constraint can bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundSide {
    Below,
    Above,
}

impl BoundSide {
    pub fn opposite(self) -> Self {
        match self {
            BoundSide::Below => BoundSide::Above,
            BoundSide::Above => BoundSide::Below,
        }
    }
}

impl fmt::Display for BoundSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BoundSide::Below => "below",
            BoundSide::Above => "above",
        })
    }
}

/// One side of an `auto` that no constraint bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct UnboundedSide {
    pub side: BoundSide,
    /// The solver-internal bound a solve fell back to on this side, rendered
    /// with its unit. `None` where no solve ran.
    pub solver_default: Option<String>,
}

/// `no constraint bounds `X` <sides>`, the sides joined with `or`.
///
/// `unbounded` must be non-empty: an auto bounded on both sides has no clause.
pub fn unbounded_auto_clause(auto: &ValueCellId, unbounded: &[UnboundedSide]) -> String {
    debug_assert!(
        !unbounded.is_empty(),
        "an auto bounded on both sides has no unbounded-side clause"
    );
    let sides: Vec<String> = unbounded
        .iter()
        .map(|u| match &u.solver_default {
            Some(default) => format!("{} (solver default {default})", u.side),
            None => u.side.to_string(),
        })
        .collect();
    format!("no constraint bounds `{auto}` {}", sides.join(" or "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_without_a_solve_names_only_the_side() {
        let clause = unbounded_auto_clause(
            &ValueCellId::new("T", "x"),
            &[UnboundedSide {
                side: BoundSide::Above,
                solver_default: None,
            }],
        );
        assert_eq!(clause, "no constraint bounds `T.x` above");
    }

    #[test]
    fn both_sides_carry_their_solver_defaults_in_order() {
        let clause = unbounded_auto_clause(
            &ValueCellId::new("Part", "t"),
            &[
                UnboundedSide {
                    side: BoundSide::Below,
                    solver_default: Some("0.000001 m".to_owned()),
                },
                UnboundedSide {
                    side: BoundSide::Above,
                    solver_default: Some("10 m".to_owned()),
                },
            ],
        );
        assert_eq!(
            clause,
            "no constraint bounds `Part.t` below (solver default 0.000001 m) \
             or above (solver default 10 m)"
        );
    }

    #[test]
    fn opposite_swaps_the_side() {
        assert_eq!(BoundSide::Below.opposite(), BoundSide::Above);
        assert_eq!(BoundSide::Above.opposite(), BoundSide::Below);
    }
}
