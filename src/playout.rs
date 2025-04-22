use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::{self, Write as _},
    fs,
    ops::Range,
};

use jiff::{Timestamp, Unit};
use num::rational::Ratio;

use crate::{
    config::get_config,
    duration::{Duration, StepSize},
    package::{Package, RemoteResource, Segment, Variant, VariantKind},
    schedule::{Item, Schedule},
};

const N_STREAMS: usize = 4;
const LOOKAHEAD: usize = 16;

pub struct Playlist {
    start: Timestamp,
    sources: BTreeMap<Duration, (Duration, usize)>,
    step: StepSize,
    pub streams: [Stream; N_STREAMS],
    duration: Duration,
    items: Vec<Item>,
}

impl Playlist {
    pub fn load(start: Timestamp, packages_dir: &str) -> Self {
        let mut packages = Vec::new();
        for entry in fs::read_dir(packages_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension() != Some(OsStr::new("json")) {
                continue;
            }
            if path.file_name().unwrap().to_str().unwrap().starts_with('_') {
                continue;
            }
            packages.push(Package::from_file(entry.path().to_str().unwrap()));
        }
        Self::new(start, packages)
    }

    pub fn new(start: Timestamp, packages: Vec<Package>) -> Self {
        let step = StepSize::calculate(
            packages
                .iter()
                .flat_map(|package| package.variants.iter().map(|variant| variant.time_base)),
        );

        let mut sources = BTreeMap::default();
        let mut streams = [
            Stream::new_video(1920, 1080, 5000000),
            Stream::new_video(1280, 720, 1500000),
            Stream::new_video(960, 540, 400000),
            Stream::new_audio(192000),
        ];

        let mut running_playlist_duration = Duration::zero();
        let mut items = Vec::with_capacity(packages.len());
        for (pi, package) in packages.iter().enumerate() {
            for variant in &package.variants {
                let stream = streams
                    .iter_mut()
                    .find(|stream| stream.bitrate >= variant.bitrate && stream.kind == variant.kind)
                    .unwrap();

                let top_stream = VariantKind::Video {
                    width: 1920,
                    height: 1080,
                };
                if stream.kind == top_stream {
                    let start_duration = running_playlist_duration;
                    running_playlist_duration =
                        running_playlist_duration.add(variant.duration(step));
                    let end_duration = running_playlist_duration;
                    sources.insert(end_duration, (start_duration, pi));

                    items.push(Item {
                        vid: package.vid,
                        start: start_duration,
                        duration: end_duration,
                    });
                }

                let stream_source =
                    StreamSource::from_variant(package.vid, variant, stream.segments.len(), step);
                stream.sources.push(stream_source);

                stream.segments.reserve(stream.segments.len());
                for segment in &variant.segments {
                    let stream_segment = StreamSegment::new(segment, variant.time_base, step);
                    stream.segments.push(stream_segment);
                }
            }
        }

        Self {
            start,
            sources,
            step,
            streams,
            duration: running_playlist_duration,
            items,
        }
    }
}

struct Playhead {
    discontinuity: usize,
    loop_index: usize,
    source_index: usize,
    offset_in_source: Duration,
}

impl Playlist {
    fn at(&self, now: Timestamp) -> Playhead {
        let config = get_config();

        let now = now.since(self.start).unwrap().total(Unit::Second).unwrap() as u64;
        let now = Duration::new(now * config.speed, Ratio::ONE, self.step);
        let (loop_index, offset) = now.modulo(self.duration);

        let (_, (start_duration, source_index)) = self.sources.range(offset..).next().unwrap();
        let offset_in_source = offset.subtract(*start_duration);

        let discontinuity = ((loop_index as usize) * self.sources.len()) + source_index;

        Playhead {
            discontinuity,
            loop_index: loop_index as usize,
            source_index: *source_index,
            offset_in_source,
        }
    }
}

pub struct Stream {
    bitrate: u32,
    kind: VariantKind,
    sources: Vec<StreamSource>,
    segments: Vec<StreamSegment>,
}

struct StreamSource {
    vid: u32,
    init: RemoteResource,
    segment_lookup: BTreeMap<Duration, usize>,
    segments: Range<usize>,
}

impl StreamSource {
    fn from_variant(vid: u32, variant: &Variant, start_segment_idx: usize, step: StepSize) -> Self {
        let mut segment_lookup = BTreeMap::default();

        let mut running_duration = Duration::zero();
        for (si, segment) in variant.segments.iter().enumerate() {
            let duration = Duration::new(segment.duration(), variant.time_base, step);
            running_duration = running_duration.add(duration);
            segment_lookup.insert(running_duration, si);
        }

        Self {
            vid,
            init: variant.init_src.clone(),
            segment_lookup,
            segments: start_segment_idx..(start_segment_idx + variant.segments.len()),
        }
    }
}

#[derive(Clone)]
struct StreamSegment {
    duration: Duration,
    src: RemoteResource,
}

impl StreamSegment {
    fn new(segment: &Segment, time_base: Ratio<u32>, step: StepSize) -> Self {
        let duration = Duration::new(segment.duration, time_base, step);
        assert_ne!(duration.raw(), 0);
        Self {
            duration,
            src: segment.src.clone(),
        }
    }
}

struct QueueItem<'a> {
    discontinuity: usize,
    source: &'a StreamSource,
    segment: &'a StreamSegment,
}

impl Stream {
    fn new_video(width: u16, height: u16, bitrate: u32) -> Self {
        Self {
            bitrate,
            kind: VariantKind::Video { width, height },
            sources: Vec::default(),
            segments: Vec::default(),
        }
    }

    fn new_audio(bitrate: u32) -> Self {
        Self {
            bitrate,
            kind: VariantKind::Audio,
            sources: Vec::default(),
            segments: Vec::default(),
        }
    }

    fn queue(&self, playhead: &Playhead) -> impl Iterator<Item = QueueItem> {
        let this_source = &self.sources[playhead.source_index];
        let start_segment_index = this_source
            .segment_lookup
            .range(playhead.offset_in_source..)
            .next()
            .map(|(_, idx)| *idx)
            .unwrap_or(this_source.segment_lookup.len());

        let first_source_segments = self.segments
            [this_source.segments.start + start_segment_index..this_source.segments.end]
            .iter()
            .map(|segment| QueueItem {
                discontinuity: playhead.discontinuity,
                source: this_source,
                segment,
            });

        let playlist_remainder = self
            .sources
            .iter()
            .cycle()
            .skip(playhead.source_index + 1)
            .enumerate()
            .flat_map(|(source_i, source)| {
                let discontinuity = playhead.discontinuity + source_i + 1;
                self.segments[source.segments.clone()]
                    .iter()
                    .map(move |segment| QueueItem {
                        discontinuity,
                        source,
                        segment,
                    })
            });

        first_source_segments.chain(playlist_remainder)
    }
}

impl Stream {
    fn media_seq(&self, playhead: &Playhead) -> usize {
        let this_source = &self.sources[playhead.source_index];
        let start_segment_index = this_source
            .segment_lookup
            .range(playhead.offset_in_source..)
            .next()
            .map(|(_, idx)| *idx)
            .unwrap_or(this_source.segment_lookup.len());

        (playhead.loop_index * self.segments.len())
            + this_source.segments.start
            + start_segment_index
    }

    pub fn render_variant_playlist(
        &self,
        r: &mut String,
        playlist: &Playlist,
        now: Timestamp,
    ) -> fmt::Result {
        let config = get_config();

        let playhead = playlist.at(now);
        let mut current_discontinuity = playhead.discontinuity;

        writeln!(r, "#EXTM3U")?;
        writeln!(
            r,
            "## DEBUG: loop={};source={};",
            playhead.loop_index, playhead.source_index
        )?;
        writeln!(r, "#EXT-X-VERSION:7")?;
        writeln!(r, "#EXT-X-TARGETDURATION:10")?;
        writeln!(r, "#EXT-X-MEDIA-SEQUENCE:{}", self.media_seq(&playhead))?;
        writeln!(r, "#EXT-X-DISCONTINUITY-SEQUENCE:{current_discontinuity}")?;

        let mut mapped_vid = None;

        for (i, this) in self.queue(&playhead).take(LOOKAHEAD).enumerate() {
            for _ in current_discontinuity..this.discontinuity {
                writeln!(r, "#EXT-X-DISCONTINUITY")?;
            }

            if i == 0 || mapped_vid != Some(this.source.vid) {
                let uri = this.source.init.uri(this.source.vid);
                writeln!(r, "#EXT-X-MAP:URI=\"{}{uri}\"", config.media_base)?;
                mapped_vid = Some(this.source.vid);
            }

            let duration = this.segment.duration.to_seconds(playlist.step);
            let uri = this.segment.src.uri(this.source.vid);
            writeln!(r, "#EXTINF:{duration:.6},")?;
            writeln!(r, "{}{uri}", config.media_base,)?;

            current_discontinuity = this.discontinuity;
        }

        Ok(())
    }
}

impl Playlist {
    pub fn master_playlist(&self, out: &mut String) -> fmt::Result {
        let config = get_config();

        writeln!(out, "#EXTM3U")?;
        writeln!(out, "#EXT-X-INDEPENDENT-SEGMENTS")?;
        writeln!(out)?;

        for (i, stream) in self.streams.iter().enumerate() {
            let bitrate = stream.bitrate;
            match &stream.kind {
                VariantKind::Video { width, height } => {
                    writeln!(
                        out,
                        "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{},CODECS=\"{}\",AUDIO=\"audio\"",
                        bitrate, width, height, "avc1.64e01f, mp4a.40.2"
                    )?;
                    writeln!(out, "{}/hls/variant{i}.m3u8", config.base)?;
                    writeln!(out)?;
                }
                VariantKind::Audio => {
                    writeln!(
                        out,
                        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",LANGUAGE=\"en\",NAME=\"aac_192\",AUTOSELECT=YES,DEFAULT=YES,URI=\"{}/hls/variant{i}.m3u8\"",
                        config.base,
                    )?;
                    writeln!(out)?;
                }
            }
        }
        Ok(())
    }
}

impl Playlist {
    pub fn schedule(&self) -> Schedule {
        Schedule {
            step: self.step,
            duration: self.duration,
            start: self.start,
            items: self.items.clone(),
        }
    }
}
