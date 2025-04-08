use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::{self, Write},
    fs,
};

use jiff::Unit;
use num::{Integer, integer::lcm, rational::Ratio};

use crate::{
    config::get_config,
    package::{Package, RemoteResource, Segment, VariantKind},
};

pub struct Playlist {
    pub streams: [Stream; 4],
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

        let mut streams = [
            Stream::new_video(1920, 1080, 5000000),
            Stream::new_video(1280, 720, 1500000),
            Stream::new_video(960, 540, 400000),
            Stream::new_audio(192000),
        ];

        for (package_i, package) in packages.iter().enumerate() {
            for variant in &package.variants {
                let stream = streams
                    .iter_mut()
                    .find(|stream| stream.bitrate >= variant.bitrate && stream.kind == variant.kind)
                    .unwrap();

                stream.inits.push(variant.init_src.clone());
                stream.vids.push(package.vid);

                let start_idx = stream.segments.len();
                stream.segments.extend_from_slice(&variant.segments);

                for (segment_i, segment) in variant.segments.iter().enumerate() {
                    let flat_segment_i = start_idx + segment_i;
                    stream.positions.push(Position {
                        package_index: package_i as u64,
                        segment_index: segment_i as u64,
                        flat_segment_index: flat_segment_i as u64,
                    });

                    let segment_duration =
                        Duration::new(segment.duration, package.base(), step_size);
                    stream.durations.push(segment_duration);

                    stream.duration = stream.duration.add(segment_duration);
                    stream
                        .time_index
                        .insert(stream.duration, stream.positions.len() - 1);
                }
            }
        }

        for stream in &streams {
            if stream.kind == VariantKind::Audio {
                continue;
            }
            assert_eq!(stream.duration, streams[0].duration);
            assert_eq!(stream.inits.len(), packages.len());

            let total_segments = packages
                .iter()
                .map(|package| package.variants[0].segments.len())
                .sum::<usize>();
            assert_eq!(stream.time_index.len(), total_segments);
            assert_eq!(stream.segments.len(), total_segments);
            assert_eq!(stream.durations.len(), total_segments);
            assert_eq!(stream.positions.len(), total_segments);
        }

        Self {
            streams,
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
}

pub struct Stream {
    duration: Duration,
    kind: VariantKind,
    bitrate: u32,
    inits: Vec<RemoteResource>,
    segments: Vec<Segment>,
    time_index: BTreeMap<Duration, usize>,
    positions: Vec<Position>,
    vids: Vec<u32>,
    durations: Vec<Duration>,
}

#[derive(Clone, Copy)]
struct Position {
    package_index: u64,
    segment_index: u64,
    flat_segment_index: u64,
}

impl Stream {
    fn new_video(width: u16, height: u16, bitrate: u32) -> Self {
        Self {
            duration: Duration::zero(),
            kind: VariantKind::Video { width, height },
            bitrate,
            inits: Vec::default(),
            segments: Vec::default(),
            time_index: BTreeMap::default(),
            positions: Vec::default(),
            vids: Vec::default(),
            durations: Vec::default(),
        }
    }

    fn new_audio(bitrate: u32) -> Self {
        Self {
            duration: Duration::zero(),
            kind: VariantKind::Audio,
            bitrate,
            inits: Vec::default(),
            segments: Vec::default(),
            time_index: BTreeMap::default(),
            positions: Vec::default(),
            vids: Vec::default(),
            durations: Vec::default(),
        }
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
    fn current_position(&self, playlist: &Playlist, ts: jiff::Timestamp) -> (u64, Position) {
        let offset_from_start = playlist.duration_from_ts(ts);
        let (loop_index, offset_in_stream) = offset_from_start.modulo(self.duration);
        let (_, position_index) = self.time_index.range(offset_in_stream..).next().unwrap();
        let position = self.positions[*position_index];
        (loop_index, position)
    }

    fn discontinuity_count(&self, loop_index: u64, package_index: u64) -> u64 {
        let packages_in_loop = self.inits.len() as u64;
        (loop_index * packages_in_loop) + package_index
    }

    fn segment_count(&self, loop_index: u64, flat_segment_index: u64) -> u64 {
        let segments_in_loop = self.segments.len() as u64;
        (loop_index * segments_in_loop) + flat_segment_index
    }

    pub fn variant_playlist(
        &self,
        playlist: &Playlist,
        now: jiff::Timestamp,
        out: &mut String,
    ) -> fmt::Result {
        let config = get_config();

        let (loop_index, current_position) = self.current_position(playlist, now);

        let discontinuities = self.discontinuity_count(loop_index, current_position.package_index);
        let segment_count = self.segment_count(loop_index, current_position.flat_segment_index);

        writeln!(out, "#EXTM3U")?;
        writeln!(out, "#EXT-X-VERSION:7")?;
        writeln!(out, "#EXT-X-TARGETDURATION:10")?;
        writeln!(out, "#EXT-X-MEDIA-SEQUENCE:{segment_count}")?;
        writeln!(out, "#EXT-X-DISCONTINUITY-SEQUENCE:{discontinuities}")?;

        let queue = self.positions[(current_position.flat_segment_index as usize)..]
            .iter()
            .chain(self.positions.iter().cycle())
            .take(10);

        for (i, position) in queue.enumerate() {
            let vid = self.vids[position.package_index as usize];

            if i != 0 && position.segment_index == 0 {
                writeln!(out, "#EXT-X-DISCONTINUITY")?;
            }
            if i == 0 || position.segment_index == 0 {
                // TODO: Merge package specific metadata into one vector
                let init = &self.inits[position.package_index as usize];
                let uri = init.uri(vid);
                writeln!(out, "#EXT-X-MAP:URI=\"{}{uri}\"", config.media_base)?;
            }

            let segment = &self.segments[position.flat_segment_index as usize];
            let duration =
                self.durations[position.flat_segment_index as usize].to_seconds(playlist.step_size);

            let uri = segment.src.uri(vid);
            writeln!(out, "#EXTINF:{duration:.6},")?;
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
    pub fn modulo(self, base: Duration) -> (u64, Duration) {
        let (quotient, remainder) = self.0.div_mod_floor(&base.0);
        (quotient, Duration(remainder))
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
