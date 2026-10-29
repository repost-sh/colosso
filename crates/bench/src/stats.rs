//! Samples into the numbers the budgets are written in.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// What one scenario run measured, passed from the run back to the harness as JSON.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Measured {
    pub samples_ms: Vec<f64>,
    /// Seconds into the run when the time ran out, if it did; the samples are those taken.
    pub timed_out_at_s: Option<f64>,
    /// Anything that makes the numbers mean less than they seem.
    pub notes: Vec<String>,
    pub scale_factor: f32,
}

impl Measured {
    pub fn push(&mut self, sample: Duration) {
        self.samples_ms.push(sample.as_secs_f64() * 1000.);
    }

    pub fn summary(&self) -> Option<Summary> {
        let mut sorted = self.samples_ms.clone();
        sorted.sort_by(f64::total_cmp);
        Some(Summary {
            p50: percentile(&sorted, 0.50)?,
            p95: percentile(&sorted, 0.95)?,
            max: *sorted.last()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Summary {
    #[serde(rename = "p50_ms")]
    pub p50: f64,
    #[serde(rename = "p95_ms")]
    pub p95: f64,
    #[serde(rename = "max_ms")]
    pub max: f64,
}

/// Nearest rank: the smallest sample at least `q` of the samples are at or under.
fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.max(1) - 1).copied()
}

/// When a run has to stop taking samples: `limit` after it was set.
#[derive(Clone, Copy)]
pub struct Deadline {
    started: Instant,
    limit: Duration,
}

impl Deadline {
    pub fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
        }
    }

    /// Seconds since the run started, once the limit has passed.
    pub fn passed(&self) -> Option<f64> {
        let elapsed = self.started.elapsed();
        (elapsed >= self.limit).then_some(elapsed.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let sorted: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&sorted, 0.50), Some(10.));
        assert_eq!(percentile(&sorted, 0.95), Some(19.));
        assert_eq!(percentile(&[7.], 0.95), Some(7.));
        assert_eq!(percentile(&[], 0.5), None);
    }
}
