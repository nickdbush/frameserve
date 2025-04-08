use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::{self, Write},
    fs,
};

use jiff::Unit;
use num::{integer::lcm, rational::Ratio};

use crate::{
    config::get_config,
    package::{Package, RemoteResource, Segment, Variant, VariantKind},
};

pub struct Playlist {
    packages: Vec<Package>,
    /// Uses the end time of the segment to quickly find the current segment.
    segments: BTreeMap<Duration, SegmentInfo>,
    pub streams: [Stream; 4],
    duration: Duration,
    step_size: StepSize,
    start: jiff::Timestamp,
}

impl Playlist {
    pub fn new(start: jiff::Timestamp, dir: &str) -> Self {
        let mut packages = Vec::new();
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension() != Some(OsStr::new("json")) {
                continue;
            }
            packages.push(Package::from_file(entry.path().to_str().unwrap()));
        }

        let step_size = StepSize::calculate(packages.iter().map(|package| package.base()));

        let mut segment_info = BTreeMap::new();
        let mut end_duration = Duration::zero();
        packages
            .iter()
            .enumerate()
            .flat_map(|(package_i, package)| {
                package.variants[0]
                    .segments
                    .iter()
                    .enumerate()
                    .map(move |(segment_i, segment)| {
                        let duration = Duration::new(segment.duration, package.base(), step_size);
                        SegmentInfo {
                            package_i,
                            segment_i,
                            duration,
                        }
                    })
            })
            .for_each(|segment| {
                end_duration = end_duration.add(segment.duration);
                segment_info.insert(end_duration, segment);
            });

        let mut streams = [
            Stream::new_video(1920, 1080, 5000000),
            Stream::new_video(1280, 720, 1500000),
            Stream::new_video(960, 540, 400000),
            Stream::new_audio(192000),
        ];
        for package in &packages {
            for stream in &mut streams {
                stream.add_best_match(&package.variants);
            }
        }

        Self {
            packages,
            segments: segment_info,
            streams,
            duration: end_duration,
            step_size,
            start,
        }
    }
}

impl Playlist {
    fn duration_from_ts(&self, ts: jiff::Timestamp) -> Duration {
        let diff = ts.since(self.start).unwrap().total(Unit::Second).unwrap() as u64;
        Duration::new(diff, Ratio::ONE, self.step_size)
    }

    fn queue(&self, now: jiff::Timestamp) -> impl Iterator<Item = &SegmentInfo> {
        let offset = self.duration_from_ts(now).modulo(self.duration);
        self.segments
            .range(offset..)
            .map(|(_, segment)| segment)
            .chain(self.segments.values().cycle())
    }
}

pub struct SegmentInfo {
    package_i: usize,
    segment_i: usize,
    duration: Duration,
}

pub struct Stream {
    kind: VariantKind,
    bitrate: u32,
    inits: Vec<RemoteResource>,
    segments: Vec<Segment>,
}

impl Stream {
    fn new_video(width: u16, height: u16, bitrate: u32) -> Self {
        Self {
            kind: VariantKind::Video { width, height },
            bitrate,
            inits: Vec::default(),
            segments: Vec::default(),
        }
    }

    fn new_audio(bitrate: u32) -> Self {
        Self {
            kind: VariantKind::Audio,
            bitrate,
            inits: Vec::default(),
            segments: Vec::default(),
        }
    }

    fn add_best_match(&mut self, variants: &[Variant]) {
        let variant = variants
            .iter()
            .find(|variant| variant.kind == self.kind && variant.bitrate == self.bitrate)
            .unwrap();
        self.inits.push(variant.init_src.clone());
        self.segments.extend_from_slice(&variant.segments);
    }
}

impl Playlist {
    pub fn master_playlist(&self, out: &mut String) -> fmt::Result {
        let config = get_config();
        writeln!(out, "#EXTM3U")?;
        for (i, stream) in self.streams.iter().enumerate() {
            let bitrate = stream.bitrate;
            match &stream.kind {
                VariantKind::Video { width, height } => {
                    writeln!(
                        out,
                        "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{},CODECS=\"{}\"",
                        bitrate, width, height, "avc1.42e00a,mp4a.40.2"
                    )?;
                }
                VariantKind::Audio => {
                    writeln!(
                        out,
                        "#EXT-X-STREAM-INF:BANDWIDTH={},CODECS=\"{}\"",
                        192_000, "mp4a.40.5"
                    )?;
                }
            }
            writeln!(out, "{}/hls/variant{i}.m3u8", config.base)?;
        }
        Ok(())
    }
}

impl Stream {
    pub fn variant_playlist(
        &self,
        playlist: &Playlist,
        now: jiff::Timestamp,
        out: &mut String,
    ) -> fmt::Result {
        let config = get_config();

        writeln!(out, "#EXTM3U")?;
        writeln!(out, "#EXT-X-TARGETDURATION:10")?;
        writeln!(out, "#EXT-X-VERSION:4")?;
        writeln!(out, "#EXT-X-MEDIA-SEQUENCE:1")?;

        for (i, segment_info) in playlist.queue(now).take(20).enumerate() {
            let package = &playlist.packages[segment_info.package_i];
            let segment = &self.segments[segment_info.segment_i];
            let init = &self.inits[segment_info.package_i];

            if segment_info.segment_i == 0 {
                writeln!(out, "#EXT-X-DISCONTINUITY")?;
            }
            if i == 0 || segment_info.segment_i == 0 {
                let uri = init.uri(package.vid);
                writeln!(out, "#EXT-X-MAP:URI=\"{}{uri}\"", config.media_base)?;
            }

            let duration = segment_info.duration.to_seconds(playlist.step_size);
            writeln!(out, "#EXTINF:{duration:.6}")?;

            let uri = segment.src.uri(package.vid);
            writeln!(out, "{}{uri}", config.media_base)?;
        }

        Ok(())
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Duration(u64);

impl Duration {
    fn new(duration_in_time_base: u64, time_base: Ratio<u32>, step_size: StepSize) -> Self {
        let duration_in_steps = duration_in_time_base * (step_size.0 / *time_base.denom() as u64);
        Self(duration_in_steps)
    }

    fn zero() -> Self {
        Self(0)
    }

    fn to_seconds(self, step_size: StepSize) -> f64 {
        (self.0 as f64) / (step_size.0 as f64)
    }

    #[must_use]
    pub fn modulo(self, base: Duration) -> Duration {
        Duration(self.0 % base.0)
    }

    #[must_use]
    pub fn add(self, other: Duration) -> Duration {
        Duration(self.0 + other.0)
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
