//! The one sparse-Cholesky driver of reify-solver-elastic (PRD §5 C2):
//! faer's symbolic analysis (AMD ordering, lower triangle) and numeric
//! `LLᵀ`, single-threaded — every faer call passes `Par::Seq` — with both
//! numeric-step allocations fallible. `SplitCholesky` factors through it.

use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::cholesky::llt::factor::LltError as DenseLltError;
use faer::perm::PermRef;
use faer::sparse::linalg::cholesky::{
    CholeskySymbolicParams, SymbolicCholesky, SymbolicCholeskyRaw, factorize_symbolic_cholesky,
};
use faer::sparse::{FaerError, SparseColMatRef, SymbolicSparseColMatRef};
use faer::{Par, Side};

use super::fault_injection::{self, ReservationSite};

const F64_BYTES: u64 = size_of::<f64>() as u64;

/// The symbolic structure of a factor: fill-reducing permutation, variant
/// (simplicial or supernodal) and the factor's sparsity.
pub(crate) struct Symbolic {
    inner: SymbolicCholesky<usize>,
}

/// Why a numeric factorisation produced no factor.
#[derive(Debug)]
pub(crate) enum FactorizeError {
    /// One of the two reservations failed.
    OutOfMemory,
    /// faer met a non-positive pivot.
    NonPositivePivot { raw: DenseLltError },
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
    let scratch = symbolic
        .inner
        .factorize_numeric_llt_scratch::<f64>(Par::Seq, Default::default());
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
        .map_err(|raw| FactorizeError::NonPositivePivot { raw })?;
    Ok(values)
}

/// `allocate()` behind the fault hook for `site`, which records `bytes`;
/// either failing is `OutOfMemory`.
fn reserve<T>(
    site: ReservationSite,
    bytes: u64,
    allocate: impl FnOnce() -> Option<T>,
) -> Result<T, FactorizeError> {
    fault_injection::reserve(site, bytes)
        .ok()
        .and_then(|()| allocate())
        .ok_or(FactorizeError::OutOfMemory)
}
