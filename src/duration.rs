use num::{Integer, integer::lcm, rational::Ratio};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StepSize(u64);

impl StepSize {
    pub fn calculate(time_bases: impl Iterator<Item = Ratio<u32>>) -> Self {
        let step_size = time_bases
            .map(|d| *d.denom() as u64)
            .reduce(|acc, base| lcm(acc, base))
            .unwrap();
        Self(step_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Duration(u64);

impl Duration {
    pub fn new(duration_in_time_base: u64, time_base: Ratio<u32>, step_size: StepSize) -> Self {
        let duration_in_steps = duration_in_time_base * (step_size.0 / *time_base.denom() as u64);
        Self(duration_in_steps)
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn to_seconds(self, step_size: StepSize) -> f64 {
        (self.0 as f64) / (step_size.0 as f64)
    }

    #[must_use]
    pub fn modulo(self, base: Duration) -> (u64, Duration) {
        let (quotient, remainder) = self.0.div_mod_floor(&base.0);
        (quotient, Duration(remainder))
    }

    #[must_use]
    pub fn add(self, other: Duration) -> Duration {
        Duration(self.0 + other.0)
    }

    #[must_use]
    pub fn subtract(self, other: Duration) -> Duration {
        Duration(self.0 - other.0)
    }
}

#[cfg(test)]
mod tests {
    use num::rational::Ratio;

    use super::*;

    fn sum(durations: &[Duration], step_size: StepSize) -> f64 {
        let duration_in_steps = durations.iter().map(|d| d.0).sum::<u64>();
        Duration(duration_in_steps).to_seconds(step_size)
    }

    #[test]
    fn test_duration_perfect_sum_same_base() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 24), Ratio::new(1, 24), Ratio::new(1, 24)].into_iter(),
        );
        let a = Duration::new(24, Ratio::new(1, 24), step_size);
        let b = Duration::new(24, Ratio::new(1, 24), step_size);
        let c = Duration::new(24, Ratio::new(1, 24), step_size);
        assert_eq!(sum(&[a, b, c], step_size), 3.0);
    }

    #[test]
    fn test_duration_perfect_sum_different_bases() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 25), Ratio::new(1, 50), Ratio::new(1, 100)].into_iter(),
        );
        let a = Duration::new(25, Ratio::new(1, 25), step_size);
        let b = Duration::new(50, Ratio::new(1, 50), step_size);
        let c = Duration::new(100, Ratio::new(1, 100), step_size);
        assert_eq!(sum(&[a, b, c], step_size), 3.0);
    }

    #[test]
    fn test_duration_imperfect_sum_different_bases() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 24), Ratio::new(1, 24), Ratio::new(1, 48)].into_iter(),
        );
        let a = Duration::new(6, Ratio::new(1, 24), step_size);
        let b = Duration::new(12, Ratio::new(1, 24), step_size);
        let c = Duration::new(36, Ratio::new(1, 48), step_size);
        assert_eq!(sum(&[a, b, c], step_size), 1.5);
    }
}
