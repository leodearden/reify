//! The one sparse-Cholesky driver of reify-solver-elastic (PRD §5 C2):
//! faer's symbolic analysis (AMD ordering, lower triangle) and numeric
//! `LLᵀ`, single-threaded — every faer call passes `Par::Seq` — with every
//! allocation of the numeric step and the solve fallible. `SplitCholesky`
//! and the seam's Direct tier ([`DirectTier`]) both factor through it.

use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::linalg::cholesky::llt::factor::LltError as DenseLltError;
use faer::mat::MatMut;
use faer::perm::PermRef;
use faer::sparse::linalg::cholesky::{
    CholeskySymbolicParams, LltRef, SymbolicCholesky, SymbolicCholeskyRaw,
    factorize_symbolic_cholesky,
};
use faer::sparse::{FaerError, SparseColMat, SparseColMatRef, SymbolicSparseColMatRef};
use faer::{Conj, Mat, Par, Side};

use super::control::{SolveControl, SolvePhase};
use super::fault_injection::{self, ReservationSite};
use super::outcome::{FactorReuse, NpdSite, SolveFailure, SolveStop};
use super::policy::FactorEstimate;

pub(crate) const F64_BYTES: u64 = size_of::<f64>() as u64;

/// The symbolic structure of a factor: fill-reducing permutation, variant
/// (simplicial or supernodal) and the factor's sparsity.
pub(crate) struct Symbolic {
    inner: SymbolicCholesky<usize>,
}

/// Why a numeric factorisation produced no factor.
#[derive(Debug)]
pub(crate) enum FactorizeError {
    /// One of the two reservations, of `requested` bytes, failed.
    OutOfMemory { requested: u64 },
    /// faer met a non-positive pivot; `dof` is its row of the input matrix.
    NonPositivePivot { raw: DenseLltError, dof: usize },
}

/// A numeric factor: the values of `L` laid out by their symbolic structure.
pub(crate) struct Factor {
    symbolic: Symbolic,
    values: Vec<f64>,
}

impl Symbolic {
    pub(crate) fn raw(&self) -> &SymbolicCholeskyRaw<usize> {
        self.inner.raw()
    }

    pub(crate) fn perm(&self) -> Option<PermRef<'_, usize>> {
        self.inner.perm()
    }

    pub(crate) fn len_val(&self) -> usize {
        self.inner.len_val()
    }

    pub(crate) fn nrows(&self) -> usize {
        self.inner.nrows()
    }

    /// What factoring on this structure will cost: `factor_bytes` is the
    /// values plus the numeric scratch — exactly the two reservations
    /// [`factorize`] makes.
    pub(crate) fn estimate(&self) -> FactorEstimate {
        let len_val = self.len_val() as u64;
        FactorEstimate {
            n_dofs: self.nrows(),
            len_val,
            factor_bytes: len_val
                .saturating_mul(F64_BYTES)
                .saturating_add(self.numeric_scratch().size_bytes() as u64),
            predicted_flops: self.predicted_flops(),
        }
    }

    /// faer's own flop count `Σ_j (c_j + c_j(c_j+1)/2)`, which faer computes
    /// but does not return; `c_j` is column `j`'s STORED entries, so a
    /// supernode's padded columns count at full block height. Summed exactly.
    fn predicted_flops(&self) -> f64 {
        let column = |c: u128| c + c * (c + 1) / 2;
        let (stored, flops): (u128, u128) = match self.raw() {
            SymbolicCholeskyRaw::Simplicial(s) => s
                .col_ptr()
                .windows(2)
                .map(|w| (w[1] - w[0]) as u128)
                .fold((0, 0), |(n, f), c| (n + c, f + column(c))),
            SymbolicCholeskyRaw::Supernodal(s) => {
                let pattern_ptr = s.col_ptr_for_row_idx();
                (0..s.n_supernodes())
                    .map(|sn| {
                        let ncols = (s.supernode_end()[sn] - s.supernode_begin()[sn]) as u128;
                        let height = ncols + (pattern_ptr[sn + 1] - pattern_ptr[sn]) as u128;
                        (ncols * height, ncols * column(height))
                    })
                    .fold((0, 0), |(n, f), (cn, cf)| (n + cn, f + cf))
            }
        };
        debug_assert_eq!(stored, self.len_val() as u128, "Σ c_j must equal len_val");
        flops as f64
    }

    /// The input-matrix DOF a dense non-positive pivot names: faer's index
    /// is a permuted position (1-based simplicial, 0-based supernodal) that
    /// the forward permutation array maps back.
    fn pivot_dof(&self, raw: DenseLltError) -> usize {
        let DenseLltError::NonPositivePivot { index } = raw;
        let position = match self.raw() {
            SymbolicCholeskyRaw::Simplicial(_) => index - 1,
            SymbolicCholeskyRaw::Supernodal(_) => index,
        };
        self.perm()
            .map_or(position, |perm| perm.arrays().0[position])
    }

    /// `rhs ← A⁻¹·rhs` with `values`, the numeric factor on this structure.
    /// `Err(requested)` when the solve scratch cannot be reserved.
    fn solve_in_place(&self, values: &[f64], rhs: MatMut<'_, f64>) -> Result<(), u64> {
        let scratch = self
            .inner
            .solve_in_place_scratch::<f64>(rhs.ncols(), Par::Seq);
        let mut buffer = MemBuffer::try_new(scratch).map_err(|_| scratch.size_bytes() as u64)?;
        LltRef::new(&self.inner, values).solve_in_place_with_conj(
            Conj::No,
            rhs,
            Par::Seq,
            MemStack::new(&mut buffer),
        );
        Ok(())
    }

    fn numeric_scratch(&self) -> StackReq {
        self.inner
            .factorize_numeric_llt_scratch::<f64>(Par::Seq, Default::default())
    }
}

impl Factor {
    /// Pairs `values`, produced by [`factorize`] from `symbolic`, with it.
    pub(crate) fn new(symbolic: Symbolic, values: Vec<f64>) -> Self {
        assert_eq!(
            values.len(),
            symbolic.len_val(),
            "Factor: values do not match the structure"
        );
        Self { symbolic, values }
    }

    pub(crate) fn symbolic(&self) -> &Symbolic {
        &self.symbolic
    }

    pub(crate) fn values(&self) -> &[f64] {
        &self.values
    }
}

/// The symbolic analysis of `a`'s lower triangle.
pub(crate) fn analyse(a: SymbolicSparseColMatRef<'_, usize>) -> Result<Symbolic, FaerError> {
    fault_injection::symbolic_fault()?;
    let params = CholeskySymbolicParams {
        supernodal_flop_ratio_threshold: fault_injection::symbolic_threshold(),
        ..Default::default()
    };
    let inner = factorize_symbolic_cholesky(a, Side::Lower, Default::default(), params)?;
    fault_injection::record_symbolic(inner.raw());
    Ok(Symbolic { inner })
}

/// The numeric factor values of `a`'s lower triangle on `symbolic`'s
/// structure. Reserves the values, then the scratch, then factors.
pub(crate) fn factorize(
    symbolic: &Symbolic,
    a: SparseColMatRef<'_, usize, f64>,
) -> Result<Vec<f64>, FactorizeError> {
    let len_val = symbolic.len_val();
    let mut values = reserve(
        ReservationSite::FactorValues,
        (len_val as u64).saturating_mul(F64_BYTES),
        || {
            let mut values = Vec::new();
            values.try_reserve_exact(len_val).ok()?;
            values.resize(len_val, 0.0);
            Some(values)
        },
    )?;
    let scratch = symbolic.numeric_scratch();
    let mut buffer = reserve(
        ReservationSite::NumericScratch,
        scratch.size_bytes() as u64,
        || MemBuffer::try_new(scratch).ok(),
    )?;
    fault_injection::before_numeric();
    symbolic
        .inner
        .factorize_numeric_llt::<f64>(
            &mut values,
            a,
            Side::Lower,
            Default::default(),
            Par::Seq,
            MemStack::new(&mut buffer),
            Default::default(),
        )
        .map_err(|raw| FactorizeError::NonPositivePivot {
            raw,
            dof: symbolic.pivot_dof(raw),
        })?;
    Ok(values)
}

/// `allocate()` behind the fault hook for `site`; either failing is
/// `OutOfMemory { requested: bytes }`.
fn reserve<T>(
    site: ReservationSite,
    bytes: u64,
    allocate: impl FnOnce() -> Option<T>,
) -> Result<T, FactorizeError> {
    fault_injection::reserve(site, bytes)
        .ok()
        .and_then(|()| allocate())
        .ok_or(FactorizeError::OutOfMemory { requested: bytes })
}

/// The seam's Direct tier: the symbolic analysis `prepare` ran, factored by
/// the first solve that needs it and reused by every later one. A cancelled
/// or failed factorisation leaves it analysed, so a retry starts afresh.
pub(crate) struct DirectTier {
    symbolic: Symbolic,
    stage: Stage,
}

enum Stage {
    Analysed { csc: SparseColMat<usize, f64> },
    Factored { values: Vec<f64> },
}

impl DirectTier {
    pub(crate) fn new(symbolic: Symbolic, csc: SparseColMat<usize, f64>) -> Self {
        Self {
            symbolic,
            stage: Stage::Analysed { csc },
        }
    }

    /// `A⁻¹·rhs`, factoring first if no earlier solve did.
    pub(crate) fn solve(
        &mut self,
        rhs: &[f64],
        ctl: &mut SolveControl<'_>,
    ) -> Result<(Vec<f64>, FactorReuse), SolveStop> {
        let reuse = match &self.stage {
            Stage::Factored { .. } => FactorReuse::SessionFactor,
            Stage::Analysed { csc } => {
                ctl.enter(SolvePhase::Factor)?;
                let values = factorize(&self.symbolic, csc.as_ref()).map_err(factor_failure)?;
                self.stage = Stage::Factored { values };
                FactorReuse::None
            }
        };
        let Stage::Factored { values } = &self.stage else {
            unreachable!("the factor exists once the match above returns");
        };
        ctl.enter(SolvePhase::Substitute)?;
        let mut u = Mat::from_fn(rhs.len(), 1, |i, _| rhs[i]);
        self.symbolic
            .solve_in_place(values, u.as_mut())
            .map_err(|requested| SolveFailure::OutOfMemory {
                requested,
                phase: SolvePhase::Substitute,
            })?;
        Ok(((0..rhs.len()).map(|i| u[(i, 0)]).collect(), reuse))
    }
}

fn factor_failure(error: FactorizeError) -> SolveFailure {
    match error {
        FactorizeError::OutOfMemory { requested } => SolveFailure::OutOfMemory {
            requested,
            phase: SolvePhase::Factor,
        },
        FactorizeError::NonPositivePivot { dof, .. } => SolveFailure::NotPositiveDefinite {
            site: NpdSite::Pivot { dof },
        },
    }
}
