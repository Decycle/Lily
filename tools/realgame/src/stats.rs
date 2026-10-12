//! Summary statistics for timing samples (milliseconds).

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    /// Nearest-rank 95th percentile.
    pub p95: f64,
    pub max: f64,
}

/// `None` for no samples. NaNs are ignored.
pub fn summarize(samples: &[f64]) -> Option<Stats> {
    let mut v: Vec<f64> = samples.iter().copied().filter(|x| !x.is_nan()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let rank = (0.95 * n as f64).ceil() as usize;
    Some(Stats {
        n,
        mean: v.iter().sum::<f64>() / n as f64,
        p95: v[rank.clamp(1, n) - 1],
        max: v[n - 1],
    })
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "n={} mean={:.3}ms p95={:.3}ms max={:.3}ms",
            self.n, self.mean, self.p95, self.max
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_none() {
        assert_eq!(summarize(&[]), None);
        assert_eq!(summarize(&[f64::NAN]), None);
    }

    #[test]
    fn single_sample() {
        let s = summarize(&[2.5]).unwrap();
        assert_eq!((s.n, s.mean, s.p95, s.max), (1, 2.5, 2.5, 2.5));
    }

    #[test]
    fn mean_p95_max_unsorted() {
        // 1..=20 shuffled: nearest-rank p95 of 20 samples is the 19th.
        let v: Vec<f64> = (1..=20).rev().map(f64::from).collect();
        let s = summarize(&v).unwrap();
        assert_eq!(s.n, 20);
        assert_eq!(s.mean, 10.5);
        assert_eq!(s.p95, 19.0);
        assert_eq!(s.max, 20.0);
        assert_eq!(
            s.to_string(),
            "n=20 mean=10.500ms p95=19.000ms max=20.000ms"
        );
    }

    #[test]
    fn negative_errors_and_nan_skipped() {
        let s = summarize(&[-1.0, f64::NAN, 1.0]).unwrap();
        assert_eq!((s.n, s.mean, s.max), (2, 0.0, 1.0));
    }
}
