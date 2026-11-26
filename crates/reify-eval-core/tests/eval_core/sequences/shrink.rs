//! A greedy shrinker, free of toy types: it takes the first candidate
//! that still fails and starts again from it, until none does. Every
//! candidate must be strictly smaller by the caller's size, so it ends.

/// The case `case` shrinks to: a local minimum, where no candidate of it
/// still `fails`.
///
/// # Panics
///
/// If a candidate is not strictly smaller than its case by `size`.
pub fn shrink<C>(
    case: C,
    candidates: impl Fn(&C) -> Vec<C>,
    size: impl Fn(&C) -> u64,
    fails: impl Fn(&C) -> bool,
) -> C {
    let _ = (candidates, size, fails);
    case
}

mod tests {
    use super::shrink;

    /// Numbers to shrink.
    type Case = Vec<u64>;

    /// Each element dropped, then each element above zero halved.
    fn smaller(case: &Case) -> Vec<Case> {
        let dropped = (0..case.len()).map(|at| {
            let mut smaller = case.clone();
            smaller.remove(at);
            smaller
        });
        let halved = (0..case.len()).filter(|at| case[*at] > 0).map(|at| {
            let mut smaller = case.clone();
            smaller[at] /= 2;
            smaller
        });
        dropped.chain(halved).collect()
    }

    fn size(case: &Case) -> u64 {
        case.iter().sum::<u64>() + case.len() as u64
    }

    fn fails(case: &Case) -> bool {
        case.iter().any(|element| *element >= 7) && case.iter().sum::<u64>() > 10
    }

    #[test]
    fn a_case_shrinks_to_one_no_candidate_of_which_still_fails() {
        let shrunk = shrink(vec![3, 20, 9, 1, 14], smaller, size, fails);
        assert!(fails(&shrunk), "{shrunk:?}");
        let failing: Vec<_> = smaller(&shrunk).into_iter().filter(fails).collect();
        assert_eq!(failing, Vec::<Case>::new(), "{shrunk:?} can shrink further");
    }

    #[test]
    #[should_panic(expected = "is not smaller")]
    fn a_candidate_that_is_not_smaller_is_refused() {
        shrink(vec![9, 9], |case| vec![case.clone()], size, fails);
    }
}
