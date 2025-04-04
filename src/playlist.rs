use std::str::FromStr;

#[derive(Debug)]
pub struct Playlist {
    pub init_path: String,
    pub segments: Vec<Segment>,
}

#[derive(Debug)]
pub struct Segment {
    pub duration: serde_json::Number,
    pub path: String,
}

impl Playlist {
    pub fn from_m3u8(src: &str, cwd: &str) -> Playlist {
        let mut segments = Vec::new();
        let mut segment_duration = None;
        for line in src.lines() {
            if line.starts_with("#EXTINF:") {
                let duration_str = line.split(':').nth(1).unwrap();
                let duration_str = duration_str.trim_end_matches(',');
                segment_duration = Some(duration_str.to_string());
            } else if !line.starts_with('#') {
                if let Some(duration) = segment_duration {
                    segments.push(Segment {
                        duration: serde_json::Number::from_str(&duration).unwrap(),
                        path: format!("{cwd}/{}", line),
                    });
                    segment_duration = None;
                }
            }
        }
        Playlist {
            init_path: format!("{cwd}/init.mp4"),
            segments,
        }
    }
}
