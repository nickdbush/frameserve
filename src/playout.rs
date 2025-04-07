use std::fmt::{self, Write};
use std::{ffi::OsStr, fs};

use num::{integer::lcm, rational::Ratio};

use crate::package::{Package, Segment};

#[derive(Debug)]
pub struct Playlist {
    sources: Vec<Source>,
    step_size: StepSize,
    duration: Duration,
}

impl Playlist {
    pub fn from_dir(dir: &str) -> Self {
        let mut packages = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension() != Some(OsStr::new("json")) {
                continue;
            }
            packages.push(Package::from_file(entry.path().to_str().unwrap()));
        }

        let step_size = StepSize::calculate(packages.iter().map(|package| package.base()));
        let mut duration = Duration::zero();

        let sources = packages
            .into_iter()
            .map(|package| Source {
                duration: Duration::new(package.duration(), package.base(), step_size),
                package,
            })
            .inspect(|source| duration.0 += source.duration.0)
            .collect();

        Self {
            sources,
            step_size,
            duration,
        }
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration.to_seconds(self.step_size)
    }

    pub fn queue(&self, now: jiff::Timestamp) -> impl Iterator<Item = SegmentRef> {
        self.sources
            .iter()
            .enumerate()
            .flat_map(|(source_i, source)| {
                let variant = &source.package.variants[0];
                variant
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(segment_i, segment)| SegmentRef {
                        source_i,
                        segment_i,
                        source,
                        segment,
                    })
                    .collect::<Vec<_>>()
            })
            .cycle()
    }

    pub fn hls(&self, out: &mut String, now: jiff::Timestamp) -> fmt::Result {
        assert!(out.is_empty());

        for (i, segment_ref) in self.queue(now).take(20).enumerate() {
            let vid = segment_ref.source.package.vid;

            if segment_ref.segment_i == 0 {
                writeln!(out, "#EXT-X-DISCONTINUITY")?;
            }
            if segment_ref.segment_i == 0 || i == 0 {
                let uri = &segment_ref.source.package.variants[0].init_src.uri(vid);
                writeln!(out, "#EXT-X-MAP:URI=\"{uri}\"",)?;
            }

            let duration = Duration::new(
                segment_ref.segment.duration,
                segment_ref.source.package.base(),
                self.step_size,
            );
            writeln!(out, "#EXTINF:{:.6},", duration.to_seconds(self.step_size))?;
            writeln!(out, "{}", segment_ref.segment.src.uri(vid))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SegmentRef<'playlist> {
    source_i: usize,
    segment_i: usize,
    source: &'playlist Source,
    segment: &'playlist Segment,
}

#[derive(Debug)]
pub struct Source {
    package: Package,
    duration: Duration,
}

#[derive(Debug, Clone, Copy)]
struct StepSize(u64);

impl StepSize {
    fn calculate(time_bases: impl Iterator<Item = Ratio<u32>>) -> Self {
        let step_size = time_bases
            .map(|d| *d.denom() as u64)
            .reduce(|acc, base| lcm(acc, base))
            .unwrap();
        Self(step_size)
    }
}

#[derive(Debug, Clone, Copy)]
struct Duration(u64);

impl Duration {
    fn new(duration_in_time_base: u64, time_base: Ratio<u32>, step_size: StepSize) -> Self {
        let duration_in_steps = duration_in_time_base * (step_size.0 / *time_base.denom() as u64);
        Self(duration_in_steps)
    }

    fn zero() -> Self {
        Self(0)
    }

    fn sum(durations: impl Iterator<Item = Self>, step_size: StepSize) -> f64 {
        let duration_in_steps = durations.map(|d| d.0).sum::<u64>();
        Duration(duration_in_steps).to_seconds(step_size)
    }

    fn to_seconds(self, step_size: StepSize) -> f64 {
        (self.0 as f64) / (step_size.0 as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_perfect_sum_same_base() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 24), Ratio::new(1, 24), Ratio::new(1, 24)].into_iter(),
        );
        let a = Duration::new(24, Ratio::new(1, 24), step_size);
        let b = Duration::new(24, Ratio::new(1, 24), step_size);
        let c = Duration::new(24, Ratio::new(1, 24), step_size);
        assert_eq!(Duration::sum([a, b, c].into_iter(), step_size), 3.0);
    }

    #[test]
    fn test_duration_perfect_sum_different_bases() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 25), Ratio::new(1, 50), Ratio::new(1, 100)].into_iter(),
        );
        let a = Duration::new(25, Ratio::new(1, 25), step_size);
        let b = Duration::new(50, Ratio::new(1, 50), step_size);
        let c = Duration::new(100, Ratio::new(1, 100), step_size);
        assert_eq!(Duration::sum([a, b, c].into_iter(), step_size), 3.0);
    }

    #[test]
    fn test_duration_imperfect_sum_different_bases() {
        let step_size = StepSize::calculate(
            [Ratio::new(1, 24), Ratio::new(1, 24), Ratio::new(1, 48)].into_iter(),
        );
        let a = Duration::new(6, Ratio::new(1, 24), step_size);
        let b = Duration::new(12, Ratio::new(1, 24), step_size);
        let c = Duration::new(36, Ratio::new(1, 48), step_size);
        assert_eq!(Duration::sum([a, b, c].into_iter(), step_size), 1.5);
    }
}
