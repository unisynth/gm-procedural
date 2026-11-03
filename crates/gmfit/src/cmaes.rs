//! Seeded CMA-ES (full covariance, rank-one + rank-μ update, cumulative step-size adaptation; Hansen's tutorial
//! formulation) minimising over the unit box [0,1]^n (samples clamped, updates use the clamped steps).
//! Deterministic for a given seed. Covariance eigendecomposition by cyclic Jacobi (n is small: ≤ ~60).

pub struct Cma {
    n: usize,
    pub lambda: usize,
    mu: usize,
    w: Vec<f64>,
    mueff: f64,
    cc: f64,
    cs: f64,
    c1: f64,
    cmu: f64,
    damps: f64,
    chi_n: f64,
    pub mean: Vec<f64>,
    pub sigma: f64,
    c: Vec<Vec<f64>>,
    b: Vec<Vec<f64>>,
    d: Vec<f64>,
    pc: Vec<f64>,
    ps: Vec<f64>,
    rng: u64,
    generation: i32,
    /// Best point and value seen.
    pub best: (Vec<f64>, f64),
}

/// Eigen-decomposition of a symmetric matrix: returns (eigenvectors as columns, eigenvalues).
fn jacobi(a: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<f64>) {
    let n = a.len();
    let mut m = a.to_vec();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for _ in 0..50 {
        let off: f64 = (0..n).flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j))).map(|(i, j)| m[i][j] * m[i][j]).sum();
        if off < 1e-22 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if m[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
                for k in 0..n {
                    let (mkp, mkq) = (m[k][p], m[k][q]);
                    m[k][p] = c * mkp - s * mkq;
                    m[k][q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ((0..n).map(|i| v[i].clone()).collect(), (0..n).map(|i| m[i][i]).collect())
}

impl Cma {
    pub fn new(x0: &[f64], sigma: f64, seed: u64) -> Cma {
        let n = x0.len().max(1);
        let nf = n as f64;
        let lambda = 4 + (3.0 * nf.ln()).floor() as usize;
        let mu = lambda / 2;
        let raw: Vec<f64> = (0..mu).map(|i| ((lambda as f64 + 1.0) / 2.0).ln() - ((i + 1) as f64).ln()).collect();
        let sum: f64 = raw.iter().sum();
        let w: Vec<f64> = raw.iter().map(|x| x / sum).collect();
        let mueff = 1.0 / w.iter().map(|x| x * x).sum::<f64>();
        let cc = (4.0 + mueff / nf) / (nf + 4.0 + 2.0 * mueff / nf);
        let cs = (mueff + 2.0) / (nf + mueff + 5.0);
        let c1 = 2.0 / ((nf + 1.3).powi(2) + mueff);
        let cmu = (2.0 * (mueff - 2.0 + 1.0 / mueff) / ((nf + 2.0).powi(2) + mueff)).min(1.0 - c1);
        let damps = 1.0 + 2.0 * (((mueff - 1.0) / (nf + 1.0)).sqrt() - 1.0).max(0.0) + cs;
        let chi_n = nf.sqrt() * (1.0 - 1.0 / (4.0 * nf) + 1.0 / (21.0 * nf * nf));
        let eye: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
        Cma {
            n,
            lambda,
            mu,
            w,
            mueff,
            cc,
            cs,
            c1,
            cmu,
            damps,
            chi_n,
            mean: x0.to_vec(),
            sigma,
            c: eye.clone(),
            b: eye,
            d: vec![1.0; n],
            pc: vec![0.0; n],
            ps: vec![0.0; n],
            rng: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            generation: 0,
            best: (x0.to_vec(), f64::INFINITY),
        }
    }

    fn uniform(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((self.rng >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    /// λ candidate points in [0,1]^n.
    pub fn ask(&mut self) -> Vec<Vec<f64>> {
        (0..self.lambda)
            .map(|_| {
                let z: Vec<f64> = (0..self.n).map(|_| self.gauss()).collect();
                (0..self.n)
                    .map(|i| {
                        let y: f64 = (0..self.n).map(|j| self.b[i][j] * self.d[j] * z[j]).sum();
                        (self.mean[i] + self.sigma * y).clamp(0.0, 1.0)
                    })
                    .collect()
            })
            .collect()
    }

    /// Updates from the values (lower is better) of the points of the last `ask`.
    pub fn tell(&mut self, xs: &[Vec<f64>], f: &[f64]) {
        let n = self.n;
        let mut idx: Vec<usize> = (0..xs.len()).collect();
        idx.sort_by(|a, b| f[*a].total_cmp(&f[*b]));
        if f[idx[0]] < self.best.1 {
            self.best = (xs[idx[0]].clone(), f[idx[0]]);
        }
        let old = self.mean.clone();
        let ys: Vec<Vec<f64>> = idx[..self.mu].iter().map(|&k| (0..n).map(|i| (xs[k][i] - old[i]) / self.sigma).collect()).collect();
        let yw: Vec<f64> = (0..n).map(|i| (0..self.mu).map(|j| self.w[j] * ys[j][i]).sum()).collect();
        for i in 0..n {
            self.mean[i] = old[i] + self.sigma * yw[i];
        }
        // C^{-1/2} yw = B D^{-1} Bᵀ yw.
        let bt: Vec<f64> = (0..n).map(|j| (0..n).map(|i| self.b[i][j] * yw[i]).sum::<f64>() / self.d[j]).collect();
        let inv: Vec<f64> = (0..n).map(|i| (0..n).map(|j| self.b[i][j] * bt[j]).sum()).collect();
        let k = (self.cs * (2.0 - self.cs) * self.mueff).sqrt();
        for i in 0..n {
            self.ps[i] = (1.0 - self.cs) * self.ps[i] + k * inv[i];
        }
        self.generation += 1;
        let psn = self.ps.iter().map(|x| x * x).sum::<f64>().sqrt();
        let hsig = psn / (1.0 - (1.0 - self.cs).powi(2 * self.generation)).sqrt() / self.chi_n < 1.4 + 2.0 / (n as f64 + 1.0);
        let kc = (self.cc * (2.0 - self.cc) * self.mueff).sqrt();
        for i in 0..n {
            self.pc[i] = (1.0 - self.cc) * self.pc[i] + if hsig { kc * yw[i] } else { 0.0 };
        }
        let dh = if hsig { 0.0 } else { self.cc * (2.0 - self.cc) };
        for i in 0..n {
            for j in 0..=i {
                let rank_mu: f64 = (0..self.mu).map(|q| self.w[q] * ys[q][i] * ys[q][j]).sum();
                let v = (1.0 - self.c1 - self.cmu) * self.c[i][j] + self.c1 * (self.pc[i] * self.pc[j] + dh * self.c[i][j]) + self.cmu * rank_mu;
                self.c[i][j] = v;
                self.c[j][i] = v;
            }
        }
        self.sigma *= ((self.cs / self.damps) * (psn / self.chi_n - 1.0)).exp();
        self.sigma = self.sigma.min(1.0);
        let (b, ev) = jacobi(&self.c);
        self.b = b;
        self.d = ev.iter().map(|e| e.max(1e-20).sqrt()).collect();
    }
}

/// Runs `iters` generations; `f` evaluates a whole population (so callers can parallelise). Returns the best.
pub fn minimize(x0: &[f64], sigma: f64, seed: u64, iters: usize, mut f: impl FnMut(&[Vec<f64>]) -> Vec<f64>) -> (Vec<f64>, f64) {
    let mut c = Cma::new(x0, sigma, seed);
    let f0 = f(&[x0.to_vec()])[0];
    c.best = (x0.to_vec(), f0);
    for _ in 0..iters {
        let xs = c.ask();
        let v = f(&xs);
        c.tell(&xs, &v);
        if c.sigma * c.d.iter().copied().fold(0.0, f64::max) < 1e-9 {
            break;
        }
    }
    c.best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shifted(x: &[f64]) -> Vec<f64> {
        x.iter().map(|v| v * 4.0 - 2.0).collect()
    }

    #[test]
    fn sphere_converges() {
        let (x, v) = minimize(&[0.9; 6], 0.3, 1, 300, |xs| {
            xs.iter().map(|x| shifted(x).iter().map(|v| (v - 0.5) * (v - 0.5)).sum()).collect()
        });
        assert!(v < 1e-8, "{v} at {x:?}");
    }

    #[test]
    fn rosenbrock_converges() {
        let (x, v) = minimize(&[0.2; 4], 0.3, 2, 3000, |xs| {
            xs.iter()
                .map(|x| {
                    let y = shifted(x);
                    y.windows(2).map(|w| 100.0 * (w[1] - w[0] * w[0]).powi(2) + (1.0 - w[0]).powi(2)).sum()
                })
                .collect()
        });
        assert!(v < 1e-6, "{v} at {x:?}");
    }

    #[test]
    fn same_seed_same_result() {
        let run = |seed| minimize(&[0.1, 0.7, 0.4], 0.2, seed, 30, |xs| xs.iter().map(|x| x.iter().map(|v| (v - 0.3).abs()).sum()).collect());
        assert_eq!(run(5), run(5));
        assert_ne!(run(5).0, run(6).0);
    }

    #[test]
    fn jacobi_diagonalises() {
        let a = vec![vec![4.0, 1.0, 0.5], vec![1.0, 3.0, 0.2], vec![0.5, 0.2, 1.0]];
        let (b, d) = jacobi(&a);
        for i in 0..3 {
            for j in 0..3 {
                let r: f64 = (0..3).map(|k| b[i][k] * d[k] * b[j][k]).sum();
                assert!((r - a[i][j]).abs() < 1e-9);
            }
        }
    }
}
