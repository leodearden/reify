//! Shared overload-resolution vocabulary.
//!
//! This module holds the single definition of the per-slot overload match
//! predicates and their supporting type-shape helpers, shared by the
//! compile-time resolver (`reify-compiler`) and the eval-time selector
//! (`reify-expr`).

use crate::ty::Type;

/// Strict constructor-head compatibility check — the middle tie-break tier
/// of the overload ladder (D-head-exact, result-fallback Layer-B task, B2).
///
/// Mirrors `unify`'s arm structure (the same constructor pairs recurse on
/// the same shape), but is a STRICT match gate rather than a permissive one:
/// `unify` treats a constructor-head mismatch as its conservative
/// `Ok(())` fallthrough (binds nothing, never errors), which is exactly why
/// it cannot discriminate `Option<T>` from `Applied{"Result", [T, E]}` — both
/// "unify" against any subject without erroring. `heads_unifiable` instead
/// returns `false` on a head mismatch, so it can serve as a genuine
/// disambiguator between two generic overloads whose type-param-carrying
/// params would otherwise both wildcard-match the same subject.
///
/// Differences from `unify`, both deliberate:
/// - A bare `Type::TypeParam` / `Type::ScalarParam` (matched against a
///   concrete `Scalar`) leaf is a wildcard slot (`true`) — the slot itself
///   carries no constructor head to disagree on.
/// - `Applied{name, ..}` vs `Enum(name)` (same name) is a head match:
///   variant construction (`Ok { .. }` / `Err { .. }`) type-erases its
///   result to `Type::Enum(name)` (`variant_construct.rs`), so a declared
///   `Applied{"Result", ..}` param must still recognise an erased `Result`
///   subject.
/// - The catch-all is `param == arg` (plain equality) rather than `unify`'s
///   permissive `Ok(())` — a head mismatch (or two leaves) must agree
///   exactly to count as "unifiable" here.
pub fn heads_unifiable(param: &Type, arg: &Type) -> bool {
    match (param, arg) {
        // Type-param / dim-param leaves: wildcard slots, always compatible.
        (Type::TypeParam(_), _) => true,
        (Type::ScalarParam(_), Type::Scalar { .. }) => true,

        // Single-inner-Type constructors: same head → recurse on the child.
        (Type::List(d), Type::List(a))
        | (Type::Set(d), Type::Set(a))
        | (Type::Keyed(d), Type::Keyed(a))
        | (Type::Option(d), Type::Option(a))
        | (Type::Complex(d), Type::Complex(a))
        | (Type::Range(d), Type::Range(a)) => heads_unifiable(d, a),

        // Two-inner-Type constructors.
        (Type::Map(dk, dv), Type::Map(ak, av)) => {
            heads_unifiable(dk, ak) && heads_unifiable(dv, av)
        }
        (
            Type::Field {
                domain: dd,
                codomain: dc,
            },
            Type::Field {
                domain: ad,
                codomain: ac,
            },
        ) => heads_unifiable(dd, ad) && heads_unifiable(dc, ac),

        // Function: equal arity → recurse on each param + the return type.
        (
            Type::Function {
                params: dp,
                return_type: dr,
            },
            Type::Function {
                params: ap,
                return_type: ar,
            },
        ) if dp.len() == ap.len() => {
            dp.iter().zip(ap.iter()).all(|(d, a)| heads_unifiable(d, a)) && heads_unifiable(dr, ar)
        }

        // Quantity-bearing aggregates: same shape → recurse on the quantity slot.
        (
            Type::Point {
                n: dn,
                quantity: dq,
            },
            Type::Point {
                n: an,
                quantity: aq,
            },
        ) if dn == an => heads_unifiable(dq, aq),
        (
            Type::Vector {
                n: dn,
                quantity: dq,
            },
            Type::Vector {
                n: an,
                quantity: aq,
            },
        ) if dn == an => heads_unifiable(dq, aq),
        (
            Type::Tensor {
                rank: drk,
                n: dn,
                quantity: dq,
            },
            Type::Tensor {
                rank: ark,
                n: an,
                quantity: aq,
            },
        ) if drk == ark && dn == an => heads_unifiable(dq, aq),
        (
            Type::Matrix {
                m: dm,
                n: dn,
                quantity: dq,
            },
            Type::Matrix {
                m: am,
                n: an,
                quantity: aq,
            },
        ) if dm == am && dn == an => heads_unifiable(dq, aq),

        // Union: equal length → recurse arm-by-arm.
        (Type::Union(da), Type::Union(aa)) if da.len() == aa.len() => {
            da.iter().zip(aa.iter()).all(|(d, a)| heads_unifiable(d, a))
        }

        // Applied: same name + same arity → recurse element-wise on args.
        (Type::Applied { name: dn, args: da }, Type::Applied { name: an, args: aa })
            if dn == an && da.len() == aa.len() =>
        {
            da.iter().zip(aa.iter()).all(|(d, a)| heads_unifiable(d, a))
        }

        // Erased-subject rule: a declared `Applied{name}` param head-matches
        // an erased `Enum(name)` arg (same name) — see the doc comment above.
        (Type::Applied { name: dn, .. }, Type::Enum(en)) if dn == en => true,

        // Projection: same member → recurse on the bases.
        (
            Type::Projection {
                base: db,
                member: dm,
            },
            Type::Projection {
                base: ab,
                member: am,
            },
        ) if dm == am => heads_unifiable(db, ab),

        // Catch-all: leaves and mismatched/differently-shaped constructors
        // must agree by plain equality — unlike `unify`'s permissive
        // `Ok(())` fallthrough, a head mismatch here is `false`.
        _ => param == arg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::Type;

    /// The corpus behind [`heads_unifiable_matches_pinned_corpus_verdicts`]
    /// below: `(param, arg, expected, label)`, with at least one case per match
    /// arm of [`heads_unifiable`].
    ///
    /// Each arm's MATCH case is deliberately paired with a NEAR-MISS that must
    /// fall to the `_ => param == arg` catch-all, so an implementation that
    /// accidentally widened an arm (dropped an `n == n` / arity / name guard) is
    /// caught as readily as one that dropped the arm entirely.
    ///
    /// Keep both verdicts represented: a corpus edited down to all-`true` (or
    /// all-`false`) rows would agree under implementations that had drifted on
    /// the arms it no longer covers.
    fn heads_unifiable_corpus() -> Vec<(Type, Type, bool, &'static str)> {
        let t = || Type::TypeParam("T".to_string());
        let q = || Type::ScalarParam("Q".to_string());
        let result_of = |args: Vec<Type>| Type::Applied {
            name: "Result".to_string(),
            args,
        };
        let proj = |base: Type, member: &str| Type::Projection {
            base: Box::new(base),
            member: member.to_string(),
        };
        let func = |params: Vec<Type>, ret: Type| Type::Function {
            params,
            return_type: Box::new(ret),
        };
        let field = |domain: Type, codomain: Type| Type::Field {
            domain: Box::new(domain),
            codomain: Box::new(codomain),
        };

        // (param, arg, expected, label) — label names the arm the pair exercises.
        vec![
            // Type-param / dim-param wildcard leaves.
            (t(), Type::Int, true, "TypeParam vs anything"),
            (
                t(),
                result_of(vec![Type::Int]),
                true,
                "TypeParam vs constructor",
            ),
            (q(), Type::length(), true, "ScalarParam vs Scalar"),
            (
                q(),
                Type::Int,
                false,
                "ScalarParam vs non-Scalar (catch-all)",
            ),
            // Single-inner-Type constructors, match + near-miss.
            (
                Type::List(Box::new(t())),
                Type::List(Box::new(Type::Int)),
                true,
                "List recurse",
            ),
            (
                Type::List(Box::new(Type::Int)),
                Type::List(Box::new(Type::String)),
                false,
                "List inner mismatch",
            ),
            (
                Type::List(Box::new(Type::Int)),
                Type::Set(Box::new(Type::Int)),
                false,
                "List vs Set head",
            ),
            (
                Type::Set(Box::new(t())),
                Type::Set(Box::new(Type::Int)),
                true,
                "Set recurse",
            ),
            (
                Type::Keyed(Box::new(t())),
                Type::Keyed(Box::new(Type::Int)),
                true,
                "Keyed recurse",
            ),
            (
                Type::Option(Box::new(t())),
                Type::Option(Box::new(Type::length())),
                true,
                "Option recurse",
            ),
            (
                Type::Option(Box::new(t())),
                Type::Enum("Option".to_string()),
                false,
                "Option vs erased Enum (NOT the Applied rule)",
            ),
            (
                Type::Complex(Box::new(t())),
                Type::Complex(Box::new(Type::dimensionless_scalar())),
                true,
                "Complex recurse",
            ),
            (
                Type::Range(Box::new(t())),
                Type::Range(Box::new(Type::Int)),
                true,
                "Range recurse",
            ),
            // Two-inner-Type constructors.
            (
                Type::Map(Box::new(Type::String), Box::new(t())),
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                true,
                "Map recurse",
            ),
            (
                Type::Map(Box::new(Type::String), Box::new(Type::Int)),
                Type::Map(Box::new(Type::Int), Box::new(Type::Int)),
                false,
                "Map key mismatch",
            ),
            (
                field(t(), t()),
                field(Type::length(), Type::Int),
                true,
                "Field recurse",
            ),
            (
                field(Type::Int, Type::Int),
                field(Type::String, Type::Int),
                false,
                "Field domain mismatch",
            ),
            // Function: arity guard + recursion on params and return type.
            (
                func(vec![t()], t()),
                func(vec![Type::Int], Type::String),
                true,
                "Function recurse",
            ),
            (
                func(vec![t()], t()),
                func(vec![Type::Int, Type::Int], Type::Int),
                false,
                "Function arity",
            ),
            (
                func(vec![Type::Int], Type::Int),
                func(vec![Type::String], Type::Int),
                false,
                "Function param mismatch",
            ),
            // Quantity-bearing aggregates: shape guards + quantity recursion.
            (
                Type::Point {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Point {
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Point recurse",
            ),
            (
                Type::Point {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Point {
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Point n guard",
            ),
            (
                Type::Vector {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Vector {
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Vector recurse",
            ),
            (
                Type::Vector {
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Vector {
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Vector n guard",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Tensor recurse",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 3,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Tensor rank guard",
            ),
            (
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Tensor {
                    rank: 2,
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Tensor n guard",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                true,
                "Matrix recurse",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 2,
                    n: 3,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Matrix m guard",
            ),
            (
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity: Box::new(t()),
                },
                Type::Matrix {
                    m: 3,
                    n: 2,
                    quantity: Box::new(Type::length()),
                },
                false,
                "Matrix n guard",
            ),
            // Union: length guard + arm-by-arm recursion.
            (
                Type::Union(vec![t(), Type::Int]),
                Type::Union(vec![Type::String, Type::Int]),
                true,
                "Union recurse",
            ),
            (
                Type::Union(vec![t()]),
                Type::Union(vec![Type::Int, Type::Int]),
                false,
                "Union length guard",
            ),
            (
                Type::Union(vec![Type::Int, Type::Int]),
                Type::Union(vec![Type::Int, Type::String]),
                false,
                "Union arm mismatch",
            ),
            // Applied: name + arity guards, element-wise recursion.
            (
                result_of(vec![t(), t()]),
                result_of(vec![Type::length(), Type::String]),
                true,
                "Applied recurse",
            ),
            (
                result_of(vec![t(), t()]),
                Type::Applied {
                    name: "Either".to_string(),
                    args: vec![Type::Int, Type::Int],
                },
                false,
                "Applied name guard",
            ),
            (
                result_of(vec![t(), t()]),
                result_of(vec![Type::Int]),
                false,
                "Applied arity guard",
            ),
            // Erased-subject rule — the arm the whole #5685 fix turns on.
            (
                result_of(vec![t(), t()]),
                Type::Enum("Result".to_string()),
                true,
                "Applied vs erased Enum, same name",
            ),
            (
                result_of(vec![t(), t()]),
                Type::Enum("Option".to_string()),
                false,
                "Applied vs erased Enum, different name",
            ),
            (
                Type::Option(Box::new(t())),
                result_of(vec![Type::length(), Type::String]),
                false,
                "Option param vs Applied Result arg (the mis-resolution)",
            ),
            // Projection: member guard + base recursion.
            (
                proj(t(), "Out"),
                proj(Type::Int, "Out"),
                true,
                "Projection recurse",
            ),
            (
                proj(t(), "Out"),
                proj(Type::Int, "In"),
                false,
                "Projection member guard",
            ),
            // Catch-all leaves.
            (Type::Int, Type::Int, true, "catch-all equal"),
            (Type::Int, Type::String, false, "catch-all unequal"),
            (
                Type::length(),
                Type::length(),
                true,
                "catch-all equal Scalar",
            ),
            (
                Type::TraitObject("Load".to_string()),
                Type::StructureRef("PointLoad".to_string()),
                false,
                "catch-all TraitObject vs StructureRef",
            ),
        ]
    }

    #[test]
    fn heads_unifiable_matches_pinned_corpus_verdicts() {
        for (param, arg, expected, label) in &heads_unifiable_corpus() {
            assert_eq!(
                super::heads_unifiable(param, arg),
                *expected,
                "heads_unifiable SEMANTICS changed on `{label}`: it no longer \
                 returns {expected} for param={param:?}, arg={arg:?}. If the \
                 rule genuinely changed, update this corpus row — but note that \
                 relaxing a guard here (e.g. the `dn == en` name guard on the \
                 erased-subject arm) makes unrelated overloads head-match, which \
                 is the #5685 Option/Result mis-selection."
            );
        }
    }
}
