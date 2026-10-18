//! The name-and-arity derivative facts the dual traversal knows about stdlib
//! builtins: which are smooth, their partials, and which are kinks.  The
//! traversal that applies them is `crate::dual_eval`.

/// The non-smooth builtins.  `min`/`max` appear at BOTH arities: binary numeric
/// selection, and the 1-argument field reduction.
pub(crate) fn is_kink_builtin(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("min" | "max", 2)
            | ("abs" | "sign" | "floor" | "ceil" | "round", 1)
            | ("clamp", 3)
            | ("mod", 2)
            | ("max" | "min" | "argmax" | "argmin", 1)
    )
}

/// Statically — with no operand values in hand — could this call be a BOUNDED
/// field reduction that [`is_kink_builtin`] does not already claim?
///
/// `is_kink_builtin` claims `("min" | "max", 2)` for the BINARY NUMERIC kink,
/// which incidentally keeps `max(field, bounds)` off the fast path.  The
/// arity-2 `argmax`/`argmin` form has no binary-numeric reading to claim it, so
/// without this a seed-independent `argmax(field, bounds)` would be swallowed
/// whole by the fast path and record NOTHING — while genuinely selecting an
/// extremum grid node.  An empty [`BranchRecord`] has to mean "no kink here",
/// never "we did not look".
///
/// Being a static walk, this answers CONSERVATIVELY: it only suppresses the
/// fast path.  [`field_reduction_kind`] makes the real decision once the
/// operand values exist, and a call that turns out not to be a field reduction
/// falls through to the ordinary refusal path exactly as before.
///
/// [`BranchRecord`]: crate::BranchRecord
/// [`field_reduction_kind`]: crate::dual_eval::field_reduction_kind
pub(crate) fn is_bounded_reduction_shaped(name: &str, arity: usize) -> bool {
    matches!((name, arity), ("argmax" | "argmin", 2))
}

/// True when `name` at this arity has an entry in [`builtin_partials`].
pub(crate) fn is_differentiable_builtin(name: &str, arity: usize) -> bool {
    matches!(
        (name, arity),
        ("sqrt" | "exp" | "log" | "log10", 1)
            | ("sin" | "cos" | "tan" | "asin" | "acos" | "atan", 1)
            | ("sinh" | "cosh" | "tanh", 1)
            | ("atan2" | "pow", 2)
            | ("lerp", 3)
            | ("remap", 5)
    )
}

/// The local partial derivatives `∂f/∂arg_i` of a smooth builtin at `xs`.
///
/// `None` when the name/arity has no entry.  A derivative that does not exist
/// at this point is returned as `NaN` or `±inf` — which the caller's finiteness
/// guard turns into `Tangent::None` *only for an argument that actually carries
/// a tangent*.  A non-finite partial on a constant argument is discarded by
/// `combine` and therefore never refuses anything, so an entry here is free to
/// state the general form of a partial that is undefined exactly where the
/// chain rule does not read it.
pub(crate) fn builtin_partials(name: &str, xs: &[f64]) -> Option<Vec<f64>> {
    Some(match (name, xs.len()) {
        ("sqrt", 1) => vec![0.5 / xs[0].sqrt()],
        ("exp", 1) => vec![xs[0].exp()],
        // `log` IS the natural logarithm — there is no `ln` binding.
        ("log", 1) => vec![1.0 / xs[0]],
        ("log10", 1) => vec![1.0 / (xs[0] * std::f64::consts::LN_10)],
        ("sin", 1) => vec![xs[0].cos()],
        ("cos", 1) => vec![-xs[0].sin()],
        ("tan", 1) => {
            let c = xs[0].cos();
            vec![1.0 / (c * c)]
        }
        ("asin", 1) => vec![1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("acos", 1) => vec![-1.0 / (1.0 - xs[0] * xs[0]).sqrt()],
        ("atan", 1) => vec![1.0 / (1.0 + xs[0] * xs[0])],
        ("sinh", 1) => vec![xs[0].cosh()],
        ("cosh", 1) => vec![xs[0].sinh()],
        ("tanh", 1) => {
            let t = xs[0].tanh();
            vec![1.0 - t * t]
        }
        // NOTE the argument order: atan2(y, x).
        ("atan2", 2) => {
            let (y, x) = (xs[0], xs[1]);
            let d = x * x + y * y;
            vec![x / d, -y / d]
        }
        ("pow", 2) => {
            let (x, y) = (xs[0], xs[1]);
            // The BASE partial at a zero exponent is EXACTLY zero — `x^0` is
            // the constant `1` — whereas the general form evaluates
            // `0 · 0^(−1)` = `0 · inf` = NaN at `x == 0`.  This is the same
            // identity `pow_tangent` states for the BinOp spelling of a power
            // (see its `b == 0.0` arm); stated once more here so both
            // spellings of `x^k` compute the same function rather than
            // disagreeing on which one the author happened to type.
            let dx = if y == 0.0 { 0.0 } else { y * x.powf(y - 1.0) };
            // The EXPONENT partial is left as written: `ln x` genuinely does
            // not exist for `x <= 0`, and the caller's guard is masked by
            // argument contribution, so this NaN now refuses only when the
            // exponent actually moves — which is exactly when it must.
            vec![dx, x.powf(y) * x.ln()]
        }
        // lerp(a, b, t) = a + t(b − a)
        ("lerp", 3) => {
            let (a, b, t) = (xs[0], xs[1], xs[2]);
            vec![1.0 - t, t, b - a]
        }
        // remap(x, flo, fhi, tlo, thi) = tlo + (x − flo)·(thi − tlo)/(fhi − flo)
        // With s = fhi − flo, d = thi − tlo, u = (x − flo)/s and k = d/s:
        //   ∂/∂x = k, ∂/∂flo = −k + u·k, ∂/∂fhi = −u·k,
        //   ∂/∂tlo = 1 − u, ∂/∂thi = u
        ("remap", 5) => {
            let (x, flo, fhi, tlo, thi) = (xs[0], xs[1], xs[2], xs[3], xs[4]);
            let s = fhi - flo;
            let k = (thi - tlo) / s;
            let u = (x - flo) / s;
            vec![k, -k + u * k, -u * k, 1.0 - u, u]
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name the four tables mention, plus names they must not claim.
    const PROBE: &[&str] = &[
        "sqrt", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh",
        "tanh", "atan2", "pow", "lerp", "remap", "min", "max", "abs", "sign", "floor", "ceil",
        "round", "clamp", "mod", "argmax", "argmin", "hyp", "norm", "sample", "cross", "ln",
    ];

    fn grid() -> impl Iterator<Item = (&'static str, usize)> {
        PROBE.iter().flat_map(|&name| (0..=6).map(move |arity| (name, arity)))
    }

    /// The dual traversal trusts both halves: a smooth name with no partials
    /// goes silently opaque, and the chain rule indexes one partial per
    /// argument, so a short vector panics.
    #[test]
    fn every_smooth_builtin_has_partials_of_its_own_arity_and_nothing_else_does() {
        for (name, arity) in grid() {
            let partials = builtin_partials(name, &vec![0.5; arity]);
            assert_eq!(
                is_differentiable_builtin(name, arity),
                partials.is_some(),
                "{name}/{arity}: smoothness and the partials table disagree"
            );
            if let Some(partials) = partials {
                assert_eq!(partials.len(), arity, "{name}/{arity}: one partial per argument");
            }
        }
    }

    /// A name in both tables would make the smooth rule unreachable: the
    /// traversal takes the kink arm first, with no signal that it did.
    #[test]
    fn no_builtin_is_both_smooth_and_a_kink() {
        for (name, arity) in grid() {
            assert!(
                !(is_kink_builtin(name, arity) && is_differentiable_builtin(name, arity)),
                "{name}/{arity} is claimed as both smooth and a kink"
            );
        }
    }

    #[test]
    fn the_bounded_reduction_shape_is_one_is_kink_builtin_does_not_already_claim() {
        for (name, arity) in grid() {
            if is_bounded_reduction_shaped(name, arity) {
                assert!(!is_kink_builtin(name, arity), "{name}/{arity} is claimed twice");
            }
        }
        for name in ["argmax", "argmin"] {
            assert!(is_bounded_reduction_shaped(name, 2), "{name}/2 is the bounded shape");
        }
        for name in ["max", "min"] {
            assert!(!is_bounded_reduction_shaped(name, 2), "{name}/2 is not the bounded shape");
            assert!(is_kink_builtin(name, 2), "the binary-numeric kink claims {name}/2");
        }
    }
}
