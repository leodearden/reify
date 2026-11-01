//! A validated free-standing `GroupRatios` force-density spec.

/// Why a `GroupRatios` force-density spec cannot seed the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSpecDefect {
    /// `group_ids` is not parallel to `members`.
    GroupIdsNotPerMember,
    /// `group_ids[member]` names a group with no entry in `seed_ratios`.
    UnseededGroup {
        /// Position in `members` / `group_ids`.
        member: usize,
    },
    /// `reference_group` names a group with no entry in `seed_ratios`.
    UnseededReferenceGroup,
    /// `seed_ratios[group]` is zero, so it fixes no tension / compression sense.
    ZeroSeed {
        /// Position in `seed_ratios`.
        group: usize,
    },
}

/// A `GroupRatios` spec that can seed the search: members share their group's
/// density, whose sign the seed fixes and whose magnitude the search varies,
/// with `reference_group` held as the scale gauge. Only [`GroupSpec::new`]
/// builds one, so its group ids index `signs` / `seed_magnitudes` unguarded.
#[derive(Debug, Clone)]
pub(crate) struct GroupSpec {
    group_ids: Vec<usize>,
    signs: Vec<f64>,
    seed_magnitudes: Vec<f64>,
    reference_group: usize,
    seed_densities: Vec<f64>,
}

impl GroupSpec {
    /// Checks the spec for `n_members` members; the first failure wins, in the
    /// order group-id count, group ids, reference group, zero seeds.
    pub(crate) fn new(
        group_ids: &[usize],
        seed_ratios: &[f64],
        reference_group: usize,
        n_members: usize,
    ) -> Result<Self, GroupSpecDefect> {
        let n_groups = seed_ratios.len();
        if group_ids.len() != n_members {
            return Err(GroupSpecDefect::GroupIdsNotPerMember);
        }
        if let Some(member) = group_ids.iter().position(|&g| g >= n_groups) {
            return Err(GroupSpecDefect::UnseededGroup { member });
        }
        if reference_group >= n_groups {
            return Err(GroupSpecDefect::UnseededReferenceGroup);
        }
        if let Some(group) = seed_ratios.iter().position(|&r| r == 0.0) {
            return Err(GroupSpecDefect::ZeroSeed { group });
        }
        let signs: Vec<f64> = seed_ratios.iter().map(|r| r.signum()).collect();
        let seed_magnitudes: Vec<f64> = seed_ratios.iter().map(|r| r.abs()).collect();
        let seed_densities = member_densities(group_ids, &signs, &seed_magnitudes);
        Ok(Self {
            group_ids: group_ids.to_vec(),
            signs,
            seed_magnitudes,
            reference_group,
            seed_densities,
        })
    }

    /// Per-member group id, parallel to `members`.
    pub(crate) fn group_ids(&self) -> &[usize] {
        &self.group_ids
    }

    /// Per-member density at the seeds, which stands for `q` in the input
    /// contract: the search keeps each group's sign.
    pub(crate) fn seed_densities(&self) -> &[f64] {
        &self.seed_densities
    }

    /// Per-group seed magnitude `|seed_ratios[g]|`, indexed by group id.
    pub(crate) fn seed_magnitudes(&self) -> &[f64] {
        &self.seed_magnitudes
    }

    /// The groups the search varies: every group some member uses, except the
    /// reference, ascending. An unused group never enters `D`.
    pub(crate) fn free_groups(&self) -> Vec<usize> {
        let mut used = vec![false; self.signs.len()];
        for &g in &self.group_ids {
            used[g] = true;
        }
        (0..used.len())
            .filter(|&g| g != self.reference_group && used[g])
            .collect()
    }

    /// Per-member density at per-group `magnitudes` (indexed by group id):
    /// member `i`'s group's seed sign times that group's magnitude.
    pub(crate) fn densities(&self, magnitudes: &[f64]) -> Vec<f64> {
        member_densities(&self.group_ids, &self.signs, magnitudes)
    }
}

/// Per-member density: member `i`'s group's sign times that group's magnitude,
/// both indexed by group id.
fn member_densities(group_ids: &[usize], signs: &[f64], magnitudes: &[f64]) -> Vec<f64> {
    group_ids
        .iter()
        .map(|&g| signs[g] * magnitudes[g])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Members 0 and 1 in group 0, 2 in group 1, at seeds −2 and 3.
    fn two_group_spec() -> GroupSpec {
        GroupSpec::new(&[0, 1, 1], &[-2.0, 3.0], 1, 3).expect("a well-formed spec")
    }

    #[test]
    fn free_groups_are_the_used_groups_other_than_the_reference() {
        // Groups 1 and 3 are seeded but unused; group 2 is the reference.
        let groups = GroupSpec::new(&[0, 0, 2, 2], &[-1.0, 1.0, 2.0, 3.0], 2, 4)
            .expect("a well-formed spec");
        assert_eq!(groups.free_groups(), vec![0]);
    }

    #[test]
    fn densities_carry_each_groups_seed_sign_at_the_given_magnitude() {
        assert_eq!(
            two_group_spec().densities(&[5.0, 7.0]),
            vec![-5.0, 7.0, 7.0]
        );
    }

    #[test]
    fn seed_densities_are_the_densities_at_the_seed_magnitudes() {
        let groups = two_group_spec();
        assert_eq!(groups.seed_densities(), [-2.0, 3.0, 3.0]);
        assert_eq!(
            groups.densities(groups.seed_magnitudes()),
            groups.seed_densities()
        );
    }
}
