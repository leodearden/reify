//! TEMPORARY measurement spike 2 (task 7259 architect). Deleted before planning ends.
use faer::sparse::{SparseRowMat, Triplet};
use faer::sparse::linalg::LuError;
use faer::sparse::linalg::LltError as SparseLltError;
use faer::sparse::linalg::solvers::Lu;
use faer::linalg::solvers::SolveCore;
use faer::{Mat, Side, Conj, Par};
use faer::mat::MatMut;
use faer::dyn_stack::{MemStack, StackReq};
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, MetricOp, SparseMetricOp, StiffnessOp, lanczos_shift_invert,
    solve_eigen_dense,
};
use std::fmt::Write as _;

/// K - sigma*B over the pattern UNION, via triplet-sum (faer sums duplicates).
fn shifted(k: &SparseRowMat<usize, f64>, b: &SparseRowMat<usize, f64>, sigma: f64)
    -> SparseRowMat<usize, f64> {
    let n = k.nrows();
    let mut t: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for (m, s) in [(k, 1.0f64), (b, -sigma)] {
        let r = m.as_ref();
        let sym = r.symbolic();
        for i in 0..n {
            for (c, &v) in sym.col_idx_of_row_raw(i).iter().zip(r.val_of_row(i).iter()) {
                t.push(Triplet::new(i, *c, s * v));
            }
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &t).unwrap()
}

struct LuOp<'a> { lu: &'a Lu<usize, f64>, n: usize }
impl StiffnessOp for LuOp<'_> {
    fn n(&self) -> usize { self.n }
    fn solve_in_place(&self, out: MatMut<'_, f64>) {
        SolveCore::<f64>::solve_in_place_with_conj(self.lu, Conj::No, out);
    }
}

fn mv(m: &SparseRowMat<usize,f64>, x: &[f64]) -> Vec<f64> {
    let r = m.as_ref(); let sym = r.symbolic();
    (0..m.nrows()).map(|i| sym.col_idx_of_row_raw(i).iter().zip(r.val_of_row(i).iter())
        .map(|(c,&v)| v * x[*c]).sum()).collect()
}
fn nrm(v: &[f64]) -> f64 { v.iter().map(|x| x*x).sum::<f64>().sqrt() }
fn laplacian(n: usize) -> SparseRowMat<usize, f64> {
    let mut t = Vec::new();
    for i in 0..n {
        t.push(Triplet::new(i, i, 2.0));
        if i > 0 { t.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { t.push(Triplet::new(i, i + 1, -1.0)); }
    }
    SparseRowMat::try_new_from_triplets(n, n, &t).unwrap()
}
fn ident(n: usize) -> SparseRowMat<usize, f64> {
    let t: Vec<_> = (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    SparseRowMat::try_new_from_triplets(n, n, &t).unwrap()
}

/// Run the FULL beta design: assemble, dispatch, Lanczos, back-shift.
const MAXIT: usize = 40;
fn flush(out: &str) { std::fs::write("/tmp/spike7259b.txt", out).unwrap(); }
fn beta(out: &mut String, label: &str, k: &SparseRowMat<usize,f64>, b: &SparseRowMat<usize,f64>,
        sigma: f64, n_modes: usize) {
    let t0 = std::time::Instant::now();
    let n = k.nrows();
    let a = shifted(k, b, sigma);
    let m_op = SparseMetricOp { m: b.as_ref() };
    let opts = EigenSolverOptions { n_modes, tol: 1e-10, max_iters: MAXIT, sigma: 0.0 };
    let (path, res) = match a.sp_cholesky(Side::Lower) {
        Ok(llt) => {
            let k_op = reify_solver_elastic::eigensolve::SparseStiffnessOp { llt: &llt, n };
            ("CHOL", lanczos_shift_invert(&k_op, &m_op, opts))
        }
        Err(SparseLltError::Numeric(_)) => match a.sp_lu() {
            Ok(lu) => { let k_op = LuOp { lu: &lu, n }; ("LU", lanczos_shift_invert(&k_op, &m_op, opts)) }
            Err(LuError::SymbolicSingular{index}) => {
                writeln!(out, "{label}: LU SymbolicSingular index={index} elapsed={:?}", t0.elapsed()).unwrap(); flush(out); return; }
            Err(e) => { writeln!(out, "{label}: LU Generic {e:?}").unwrap(); return; }
        },
        Err(e) => { writeln!(out, "{label}: chol Generic {e:?}").unwrap(); return; }
    };
    // lanczos returns 1/mu; lambda = sigma + 1/mu
    let mut lam: Vec<f64> = res.eigenvalues.iter().map(|&x| sigma + x).collect();
    let raw = lam.clone();
    lam.sort_by(|p,q| p.abs().total_cmp(&q.abs()));
    // dense reference at same sigma
    let d = solve_eigen_dense(k, b, EigenSolverOptions { n_modes, tol: 1e-10, max_iters: 1000, sigma });
    // max abs diff of sorted multisets
    let mut ls = lam.clone(); ls.sort_by(f64::total_cmp);
    let mut ds = d.eigenvalues.clone(); ds.sort_by(f64::total_cmp);
    let maxdiff = if ls.len()==ds.len() {
        ls.iter().zip(ds.iter()).map(|(x,y)| (x-y).abs()).fold(0.0f64, f64::max)
    } else { f64::NAN };
    writeln!(out, "{label}\n   path={path} n_conv={} converged={} len={}\n   lanczos(pre-C3 order)={raw:?}\n   lanczos(C3)={lam:?}\n   dense       ={:?}\n   MAXDIFF={maxdiff:.6e} nonfinite={}",
        res.n_converged, res.converged, lam.len(), d.eigenvalues,
        lam.iter().any(|x| !x.is_finite())).unwrap();
    // Normwise relative eigen-residual on the ORIGINAL pencil, per mode.
    let mut maxres = 0.0f64;
    for (c, &l) in raw.iter().enumerate() {
        let phi: Vec<f64> = (0..n).map(|r| res.eigenvectors[(r, c)]).collect();
        let kp = mv(k, &phi); let bp = mv(b, &phi);
        let r2: f64 = kp.iter().zip(bp.iter()).map(|(x,y)| (x - l*y).powi(2)).sum::<f64>().sqrt();
        let den = nrm(&kp) + l.abs()*nrm(&bp);
        maxres = maxres.max(r2 / den.max(f64::MIN_POSITIVE));
    }
    writeln!(out, "   MAX_REL_EIGEN_RESIDUAL={maxres:.6e}").unwrap();
    writeln!(out, "   elapsed={:?}", t0.elapsed()).unwrap();
    flush(out);
}

#[test]
fn spike2() {
    let mut out = String::new();
    let n = 80usize;
    let (k, b) = (laplacian(n), ident(n));
    let lam = |j: usize| 2.0 * (1.0 - f64::cos(j as f64 * std::f64::consts::PI / 81.0));
    writeln!(out, "[ref] lam1..8 = {:?}", (1..=8).map(lam).collect::<Vec<_>>()).unwrap();

    // BT2-shaped: modest sigma above lambda1
    beta(&mut out, "== sigma=0.0 (C1 baseline via beta code)", &k, &b, 0.0, 5);
    beta(&mut out, "== sigma=0.02 (above lam3)", &k, &b, 0.02, 5);
    beta(&mut out, "== sigma=0.5 (LARGE, Q1 measurement)", &k, &b, 0.5, 5);
    beta(&mut out, "== sigma=2.0 (VERY large, mid-spectrum)", &k, &b, 2.0, 5);
    beta(&mut out, "== sigma=-0.05 (negative)", &k, &b, -0.05, 5);
    beta(&mut out, "== sigma=lam3 EXACT (near-singular)", &k, &b, lam(3), 3);

    // EXACTLY singular construction: K=diag(integers), B=I, sigma = an exact integer.
    let diagvals: Vec<f64> = (1..=n).map(|i| i as f64).collect();
    let t: Vec<_> = diagvals.iter().enumerate().map(|(i,&v)| Triplet::new(i,i,v)).collect();
    let kd = SparseRowMat::try_new_from_triplets(n, n, &t).unwrap();
    beta(&mut out, "== EXACT-SINGULAR diag pencil, sigma=7.0 (== lambda_7)", &kd, &b, 7.0, 3);
    beta(&mut out, "== diag pencil, sigma=7.5 (between eigenvalues, healthy)", &kd, &b, 7.5, 3);

    std::fs::write("/tmp/spike7259b.txt", &out).unwrap();
    eprintln!("{out}");
}
