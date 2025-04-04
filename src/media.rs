use jiff::Timestamp;
use serde::Deserialize;

#[derive(Debug, Copy, Clone, Deserialize)]
pub enum Codec {
    #[serde(rename = "avc1")]
    AVC1,
}

#[derive(Debug, Copy, Clone, Deserialize)]
pub enum Profile {
    #[serde(rename = "Main")]
    Main,
    #[serde(rename = "High")]
    High,
}

#[derive(Debug, Copy, Clone, Deserialize)]
pub enum StreamType {
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "audio")]
    Audio,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stream {
    pub file: String,
    #[serde(rename = "type")]
    pub ty: StreamType,
    pub codec: Codec,
    pub profile: Profile,
    pub level: u8,
    pub width: u16,
    pub height: u16,
    pub timescale: u16,
    pub fragment_duration: u32,
    pub last_fragment_duration: u32,
    pub index_start: u16,
    pub index_size: u16,
    pub fragment_start: u16,
    pub fragment_sizes: Box<[u32]>,
}

impl Stream {
    pub fn uri(&self, vid: u64) -> String {
        format!("/media/{vid}/{}", self.file)
    }

    pub fn duration(&self) -> f64 {
        ((self.fragment_duration * (self.fragment_sizes.len() as u32 - 1))
            + self.last_fragment_duration) as f64
            / self.timescale as f64
    }

    pub fn chunk_start(&self, index: usize) -> u64 {
        (self.fragment_start as u64)
            + self.fragment_sizes[0..index]
                .iter()
                .copied()
                .map(u64::from)
                .sum::<u64>()
    }

    pub fn get_chunk(&self, source_id: usize, index: usize) -> Chunk {
        let duration = if index == self.fragment_sizes.len() - 1 {
            self.last_fragment_duration
        } else {
            self.fragment_duration
        };
        Chunk::new(source_id, index, duration, self.timescale)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub vid: u64,
    pub streams: Box<[Stream]>,
}

impl Source {
    pub fn duration(&self) -> f64 {
        self.streams[0].duration()
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub source: usize,
    pub fragment: usize,
    pub duration: f64,
}

impl Chunk {
    pub fn new(source: usize, fragment: usize, duration: u32, timescale: u16) -> Self {
        let duration = f64::from(duration) / f64::from(timescale);
        Self {
            source,
            fragment,
            duration,
        }
    }
}

pub struct Playlist {
    pub start: Timestamp,
    pub sources: Vec<Source>,
}

impl Playlist {
    pub fn duration(&self) -> f64 {
        self.sources.iter().map(|s| s.duration()).sum()
    }

    pub fn offset(&self, at: Timestamp) -> f64 {
        at.duration_since(self.start).as_secs_f64() % self.duration()
    }

    pub fn chunks_len(&self) -> usize {
        self.sources
            .iter()
            .map(|s| s.streams[0].fragment_sizes.len())
            .sum()
    }

    pub fn chunks_iter(&self) -> impl Iterator<Item = Chunk> + Clone {
        self.sources
            .iter()
            .enumerate()
            .flat_map(|(source_index, source)| {
                let stream = &source.streams[0];
                (0..stream.fragment_sizes.len())
                    .map(move |chunk_index| stream.get_chunk(source_index, chunk_index))
            })
    }

    pub fn chunks_at(&self, at: Timestamp, n: usize) -> impl Iterator<Item = Chunk> {
        let offset = self.offset(at);

        self.chunks_iter()
            .cycle()
            .scan(0.0, |offset, chunk| {
                *offset += chunk.duration;
                Some((chunk, *offset))
            })
            .skip_while(move |(_, end_offset)| *end_offset < offset)
            .map(|(chunk, _)| chunk)
            .take(n)
    }

    pub fn calculate_history(&self, at: Timestamp) -> (usize, usize) {
        let loops = at.duration_since(self.start).as_secs_f64() / self.duration();
        let loops = loops as usize;
        let sources_in_previous_loops = loops * self.sources.len();
        let chunks_in_previous_loops = loops * self.chunks_len();

        let offset = self.offset(at);
        let chunks_in_current_loop = self
            .chunks_iter()
            .scan(0.0, |offset, chunk| {
                *offset += chunk.duration;
                Some((chunk, *offset))
            })
            .take_while(|(_, end_offset)| *end_offset < offset)
            .count();

        let sources_in_current_loop = self
            .sources
            .iter()
            .scan(0.0, |offset, source| {
                *offset += source.duration();
                Some(*offset)
            })
            .take_while(|end_offset| *end_offset < offset)
            .count();

        let passed_chunks = chunks_in_previous_loops + chunks_in_current_loop;
        let passed_sources = sources_in_previous_loops + sources_in_current_loop;

        (passed_chunks, passed_sources)
    }

    pub fn hls_stream(&self, at: Timestamp) -> String {
        let mut out = String::with_capacity(2048);
        out.push_str("#EXTM3U\n");
        out.push_str("#EXT-X-TARGETDURATION:6\n");
        out.push_str("#EXT-X-VERSION:6\n");
        // out.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");

        let (passed_chunks, passed_discontinuities) = self.calculate_history(at);
        out.push_str(&format!("#EXT-X-MEDIA-SEQUENCE:{passed_chunks}\n"));
        out.push_str(&format!(
            "#EXT-X-DISCONTINUITY-SEQUENCE:{passed_discontinuities}\n"
        ));

        for (i, chunk) in self.chunks_at(at, 10).enumerate() {
            let source = &self.sources[chunk.source];
            let stream_idx = 0;
            let stream = &source.streams[stream_idx];
            let uri = stream.uri(source.vid);

            if chunk.fragment == 0 && i != 0 {
                out.push_str("#EXT-X-DISCONTINUITY\n");
            }
            if chunk.fragment == 0 || i == 0 {
                out.push_str(&format!(
                    "#EXT-X-MAP:URI=\"{uri}\",BYTERANGE=\"{}@0\"\n",
                    stream.index_start
                ));
            }

            let size = stream.fragment_sizes[chunk.fragment];
            let start = stream.chunk_start(chunk.fragment);
            out.push_str(&format!("#EXTINF:{:.3},\n", chunk.duration));
            out.push_str(&format!("#EXT-X-BYTERANGE:{size}@{start}\n"));
            out.push_str(&uri);
            out.push_str("\n");
        }

        out
    }
}
