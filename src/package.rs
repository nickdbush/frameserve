use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use std::{ffi::OsStr, fs};

use crate::{
    inspect::{Ratio, combine_inspect},
    utils::extract_vid,
};

#[derive(Debug, Default, Serialize)]
pub struct Package {
    variants: Vec<Variant>,
}

pub fn package(input_dir: &str, segments_dir: &str, packages_dir: &str) {
    let mut package = Package::default();

    let vid = extract_vid(input_dir);
    fs::create_dir_all(format!("{segments_dir}/{vid}")).unwrap();

    for entry in std::fs::read_dir(input_dir).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }

        let (variant, mappings) = package_variant(entry.path().to_str().unwrap());
        package.variants.push(variant);

        for Mapping(src, remote) in mappings {
            let dst = format!("{segments_dir}/{vid}/{}", remote.0);
            fs::copy(src, dst).unwrap();
        }
    }

    let package_json = serde_json::to_string_pretty(&package).unwrap();
    fs::write(format!("{packages_dir}/{vid}.json"), package_json).unwrap();
}

#[derive(Debug, Serialize)]
struct Variant {
    init_src: RemoteResource,
    time_base: Ratio,
    bitrate: u32,
    #[serde(flatten)]
    kind: VariantKind,
    segments: Vec<Segment>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "info", rename_all = "lowercase")]
enum VariantKind {
    Video { width: u16, height: u16 },
    Audio,
}

#[derive(Debug, Serialize)]
struct Segment {
    src: RemoteResource,
    start: u64,
    duration: u64,
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
            (a.start_pts, a.duration_ts - a.start_pts)
        } else {
            let v = info.video_stream();
            (v.start_pts, v.duration_ts - v.start_pts)
        };

        segments.push(Segment {
            src,
            start,
            duration,
        });
    }

    segments.sort_by_key(|s| s.start);

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

#[derive(Debug, Clone, Serialize)]
struct RemoteResource(String);

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
