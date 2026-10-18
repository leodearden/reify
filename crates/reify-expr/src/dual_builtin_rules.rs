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
