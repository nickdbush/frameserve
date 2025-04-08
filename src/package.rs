use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jiff::Timestamp;
use num::rational::Ratio;
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, fmt::Display, fs};

use crate::{config::get_config, inspect::combine_inspect, utils::extract_vid};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub vid: u32,
    pub packaged_at: Timestamp,
    pub variants: Vec<Variant>,
}

impl Package {
    pub fn from_file(file: &str) -> Self {
        let src = fs::read_to_string(file).unwrap();
        serde_json::from_str::<Self>(&src).unwrap()
    }

    pub fn base(&self) -> Ratio<u32> {
        self.variants[0].time_base
    }

    pub fn duration(&self) -> u64 {
        self.variants[0].duration()
    }
}

pub fn package(input_dir: &str, segments_dir: &str, packages_dir: &str) {
    let mut variants = Vec::new();

    let vid = extract_vid(input_dir);
    fs::create_dir_all(format!("{segments_dir}/{vid}")).unwrap();

    for entry in std::fs::read_dir(input_dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }

        let (variant, mappings) = package_variant(entry.path().to_str().unwrap());
        variants.push(variant);

        for Mapping(src, remote) in mappings {
            let dst = format!("{segments_dir}/{vid}/{}", remote.0);
            fs::copy(src, dst).unwrap();
        }
    }

    let package = Package {
        vid,
        packaged_at: Timestamp::now(),
        variants,
    };
    let package_json = serde_json::to_string_pretty(&package).unwrap();
    fs::write(format!("{packages_dir}/{vid}.json"), package_json).unwrap();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub init_src: RemoteResource,
    pub time_base: Ratio<u32>,
    pub bitrate: u32,
    #[serde(flatten)]
    pub kind: VariantKind,
    pub segments: Vec<Segment>,
}

impl Variant {
    pub fn duration(&self) -> u64 {
        self.segments.iter().map(|segment| segment.duration()).sum()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "info", rename_all = "lowercase")]
pub enum VariantKind {
    Video { width: u16, height: u16 },
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub src: RemoteResource,
    pub start: u64,
    pub duration: u64,
}

impl Segment {
    pub fn duration(&self) -> u64 {
        self.duration
    }
}

fn package_variant(variant_dir: &str) -> (Variant, Vec<Mapping>) {
    let base = variant_dir.split("/").last().unwrap();
    let bitrate = base
        .split("_")
        .find(|tag| tag.ends_with('k'))
        .unwrap()
        .trim_end_matches('k')
        .parse::<u32>()
        .unwrap();
    let bitrate = bitrate * 1000;
    let is_audio_stream = base.starts_with("aac_");

    let init_path = format!("{variant_dir}/init.mp4");
    let init_info = combine_inspect(&init_path, &format!("{variant_dir}/s00000.mp4"));
    let (time_base, kind) = if is_audio_stream {
        let stream = init_info.audio_stream();
        (stream.time_base, VariantKind::Audio)
    } else {
        let (width, height) = base
            .split("_")
            .find(|tag| tag.contains('x'))
            .unwrap()
            .split_once('x')
            .unwrap();
        let width = width.parse::<u16>().unwrap();
        let height = height.parse::<u16>().unwrap();

        let stream = init_info.video_stream();
        (stream.time_base, VariantKind::Video { width, height })
    };

    let mut mappings = Vec::new();
    let (init_src, init_mapping) = RemoteResource::from_file(&init_path);
    mappings.push(init_mapping);

    let mut segments = Vec::new();

    for entry in std::fs::read_dir(variant_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension() != Some(OsStr::new("mp4")) {
            continue;
        }
        if entry.path().file_name() == Some(OsStr::new("init.mp4")) {
            continue;
        }

        let path = entry.path();
        let path = path.to_str().unwrap();

        let (src, mapping) = RemoteResource::from_file(path);
        mappings.push(mapping);

        let info = combine_inspect(&init_path, path);
        let (start, duration) = if is_audio_stream {
            let a = info.audio_stream();
            (a.start_pts, a.duration_ts)
        } else {
            let v = info.video_stream();
            (v.start_pts, v.duration_ts)
        };

        segments.push(Segment {
            src,
            start,
            duration,
        });
    }

    segments.sort_by_key(|s| s.start);

    let offset = segments[0].start;
    segments.iter_mut().for_each(|segment| {
        segment.duration -= segment.start;
        segment.duration += offset;
    });

    (
        Variant {
            init_src,
            time_base,
            kind,
            bitrate,
            segments,
        },
        mappings,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteResource(pub String);

impl RemoteResource {
    pub fn uri(&self, vid: u32) -> ResourceLocator {
        ResourceLocator {
            vid,
            resource: self,
        }
    }
}

pub struct ResourceLocator<'resource> {
    vid: u32,
    resource: &'resource RemoteResource,
}

impl<'a> Display for ResourceLocator<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "/{}/{}", self.vid, self.resource.0)
    }
}

struct Mapping(String, RemoteResource);

impl RemoteResource {
    fn from_file(file: &str) -> (Self, Mapping) {
        let mut hasher = blake3::Hasher::new();
        hasher.update_mmap(file).unwrap();
        let resource = Self::from_hash(hasher.finalize());
        (resource.clone(), Mapping(file.to_string(), resource))
    }

    fn from_hash(hash: blake3::Hash) -> Self {
        let b64 = URL_SAFE_NO_PAD.encode(hash.as_bytes());
        Self(format!("{b64}.mp4"))
    }
}
