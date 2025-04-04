// use std::fs;

// use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
// use serde::Serialize;

// use crate::{playlist::Playlist, utils::extract_vid};

// pub fn package(encode_dir: &str, segments_dir: &str, packages_dir: &str) {
//     let mut variants = Vec::new();
//     for entry in fs::read_dir(encode_dir).unwrap() {
//         let entry = entry.unwrap();
//         if !entry.file_type().unwrap().is_dir() {
//             continue;
//         }

//         let path = entry.path();
//         let path = path.to_string_lossy();
//         let (variant, mappings) = package_variant(&path);

//         mappings.into_iter().for_each(|m| m.exec_copy(segments_dir));
//         variants.push(variant);
//     }

//     let vid = extract_vid(encode_dir);
//     fs::write(
//         format!("{packages_dir}/{vid}.json"),
//         serde_json::to_string_pretty(&variants).unwrap(),
//     )
//     .unwrap();
// }

// fn package_variant(variant_dir: &str) -> (Package, Vec<Mapping>) {
//     let m3u8 = fs::read_to_string(format!("{variant_dir}/stream.m3u8")).unwrap();
//     let playlist = Playlist::from_m3u8(&m3u8, variant_dir);
//     Package::from_playlist(playlist)
// }

// #[derive(Debug, Serialize)]
// struct Package {
//     intro: Resource,
//     segments: Vec<Segment>,
// }

// #[derive(Debug, Serialize)]
// struct Segment {
//     media: Resource,
//     duration: serde_json::Number,
// }

// #[derive(Debug, Clone, Serialize)]
// struct Resource(String);

// impl Resource {
//     fn for_file(local_path: &str) -> (Self, Mapping) {
//         let mut hasher = blake3::Hasher::new();
//         hasher.update_mmap(local_path).unwrap();
//         let remote = Self::from_hash(hasher.finalize());
//         let mapping = Mapping {
//             local: local_path.to_string(),
//             remote: remote.clone(),
//         };
//         (remote, mapping)
//     }

//     fn from_hash(hash: blake3::Hash) -> Self {
//         Self(URL_SAFE_NO_PAD.encode(hash.as_bytes()))
//     }
// }

// impl Package {
//     fn from_playlist(playlist: Playlist) -> (Self, Vec<Mapping>) {
//         let mut segments = Vec::with_capacity(playlist.segments.len());
//         let mut mappings = Vec::with_capacity(playlist.segments.len() + 1);
//         for segment in playlist.segments {
//             let (resource, mapping) = Resource::for_file(&segment.path);
//             segments.push(Segment {
//                 media: resource,
//                 duration: segment.duration,
//             });
//             mappings.push(mapping);
//         }

//         let (intro, mapping) = Resource::for_file(&playlist.init_path);
//         mappings.push(mapping);

//         (Self { intro, segments }, mappings)
//     }
// }

// struct Mapping {
//     local: String,
//     remote: Resource,
// }

// impl Mapping {
//     fn exec_copy(&self, dir: &str) {
//         let src = &self.local;
//         let dst = format!("{dir}/{}", self.remote.0);

//         println!("{src} -> {dst}",);
//         fs::copy(src, dst).unwrap();
//     }
// }

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use std::{ffi::OsStr, fs};

use crate::{
    inspect::{Ratio, combine_inspect},
    recipe::VideoSpec,
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
    segments: Vec<Segment>,
    spec: VideoSpec,
}

#[derive(Debug, Serialize)]
struct Segment {
    src: RemoteResource,
    start: u64,
    duration: u64,
}

fn package_variant(variant_dir: &str) -> (Variant, Vec<Mapping>) {
    let init_path = format!("{variant_dir}/init.mp4");

    let mut spec = None;

    let mut init_src = None;
    let mut segments = Vec::new();
    let mut mappings = Vec::new();
    let mut time_base = None;

    for entry in std::fs::read_dir(variant_dir).unwrap() {
        let entry = entry.unwrap();

        if entry.path().file_name() == Some(OsStr::new("spec.json")) {
            let contents = fs::read_to_string(entry.path()).unwrap();
            spec = Some(serde_json::from_str::<VideoSpec>(&contents).unwrap());
            continue;
        }

        if entry.path().extension() != Some(OsStr::new("mp4")) {
            continue;
        }

        let path = entry.path();
        let path = path.to_str().unwrap();

        let (src, mapping) = RemoteResource::from_file(path);
        mappings.push(mapping);

        if entry.path().file_name() == Some(OsStr::new("init.mp4")) {
            init_src = Some(src);
            continue;
        }

        let info = combine_inspect(&init_path, path);
        let v = info.video_stream();
        time_base = Some(v.time_base);
        segments.push(Segment {
            src,
            start: v.start_pts,
            duration: v.duration_ts - v.start_pts,
        });
    }

    segments.sort_by_key(|s| s.start);

    (
        Variant {
            init_src: init_src.unwrap(),
            time_base: time_base.unwrap(),
            segments,
            spec: spec.unwrap(),
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
