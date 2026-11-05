//! A validated free-standing `GroupRatios` force-density spec.

/// The smallest group magnitude the search admits, in units of the reference
/// group's seed magnitude.
pub(crate) const MIN_UNIT_MAGNITUDE: f64 = 1e-10;

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
/// with `reference_group` held as the scale gauge. The search runs in units of
/// the reference group's seed magnitude, so a whole-gauge rescale of the seeds
/// is a change of unit it never sees. Only [`GroupSpec::new`] builds one, so
/// its group ids index `signs` / `seed_magnitudes` unguarded.
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

    /// Per-member density at the seeds, which stands for `q` in the input
    /// contract: the search keeps each group's sign.
    pub(crate) fn seed_densities(&self) -> &[f64] {
        &self.seed_densities
    }

    /// The reference group's seed magnitude: the unit the search runs in.
    pub(crate) fn reference_magnitude(&self) -> f64 {
        self.seed_magnitudes[self.reference_group]
    }

    /// Per-group seed magnitude `|seed_ratios[g]|` in reference units (the
    /// reference group's is 1), indexed by group id.
    pub(crate) fn unit_seed_magnitudes(&self) -> Vec<f64> {
        let unit = self.reference_magnitude();
        self.seed_magnitudes.iter().map(|m| m / unit).collect()
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

    /// Per-member density in reference units at per-group reference-unit
    /// magnitudes (indexed by group id): member `i`'s group's seed sign times
    /// that group's magnitude.
    pub(crate) fn unit_densities(&self, unit_magnitudes: &[f64]) -> Vec<f64> {
        member_densities(&self.group_ids, &self.signs, unit_magnitudes)
    }

    /// Per-member physical density at per-group reference-unit magnitudes:
    /// [`GroupSpec::unit_densities`] times the reference magnitude.
    pub(crate) fn densities(&self, unit_magnitudes: &[f64]) -> Vec<f64> {
        let unit = self.reference_magnitude();
        self.unit_densities(unit_magnitudes)
            .iter()
            .map(|q| q * unit)
            .collect()
    }

    /// Sets each group's reference-unit magnitude in `unit_magnitudes` to
    /// `|qᵢ|` of its members `i`, in member order, in reference units and
    /// floored at [`MIN_UNIT_MAGNITUDE`].
    pub(crate) fn refresh_unit_magnitudes(&self, unit_magnitudes: &mut [f64], q: &[f64]) {
        let unit = self.reference_magnitude();
        for (&group, qi) in self.group_ids.iter().zip(q) {
            unit_magnitudes[group] = (qi.abs() / unit).max(MIN_UNIT_MAGNITUDE);
        }
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

    /// Member 0 in group 0, members 1 and 2 in the reference group 1, at seeds
    /// −2 and 4: powers of two, so converting to reference units is exact.
    fn two_group_spec() -> GroupSpec {
        GroupSpec::new(&[0, 1, 1], &[-2.0, 4.0], 1, 3).expect("a well-formed spec")
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
        let groups = two_group_spec();
        assert_eq!(groups.unit_densities(&[5.0, 7.0]), vec![-5.0, 7.0, 7.0]);
        assert_eq!(groups.densities(&[5.0, 7.0]), vec![-20.0, 28.0, 28.0]);
    }

    #[test]
    fn seed_densities_are_the_densities_at_the_seed_magnitudes() {
        let groups = two_group_spec();
        assert_eq!(groups.unit_seed_magnitudes(), vec![0.5, 1.0]);
        assert_eq!(groups.seed_densities(), [-2.0, 4.0, 4.0]);
        assert_eq!(
            groups.densities(&groups.unit_seed_magnitudes()),
            groups.seed_densities()
        );
    }

    #[test]
    fn refreshed_magnitudes_are_floored_relative_to_the_reference_seed() {
        let reference_seed = 2.0_f64.powi(-20);
        let groups =
            GroupSpec::new(&[0, 1], &[-1.0, reference_seed], 1, 2).expect("a well-formed spec");
        let mut unit_magnitudes = groups.unit_seed_magnitudes();
        groups.refresh_unit_magnitudes(
            &mut unit_magnitudes,
            &[-reference_seed * 2.0_f64.powi(-40), 2.0_f64.powi(-21)],
        );
        assert_eq!(unit_magnitudes, vec![MIN_UNIT_MAGNITUDE, 0.5]);
    }
}
