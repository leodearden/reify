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
        let seed_densities = group_ids
            .iter()
            .map(|&g| signs[g] * seed_magnitudes[g])
            .collect();
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
}
