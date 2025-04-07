use num::rational::Ratio;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Display;
use std::fs::File;
use std::io;
use std::io::Read;
use std::process::{Command, Stdio};
use std::str::FromStr;

pub fn inspect(input: &str) -> Info {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            input,
        ])
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

pub fn combine_inspect(header: &str, segment: &str) -> Info {
    let mut cmd = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_streams", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = cmd.stdin.as_mut().unwrap();

    let header = File::open(header).unwrap();
    let segment = File::open(segment).unwrap();
    let mut handle = header.chain(segment);

    let _ = io::copy(&mut handle, &mut stdin);

    let output = cmd.wait_with_output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub streams: Vec<StreamInfo>,
}

impl Info {
    pub fn check(&self) {
        for stream in &self.streams {
            stream.check();
        }
    }

    pub fn video_stream(&self) -> &VideoStreamInfo {
        for stream in &self.streams {
            if let StreamKind::Video(video) = &stream.kind {
                return video;
            }
        }
        panic!("no video stream found");
    }

    pub fn audio_stream(&self) -> &AudioStreamInfo {
        for stream in &self.streams {
            if let StreamKind::Audio(audio) = &stream.kind {
                return audio;
            }
        }
        panic!("no video stream found");
    }
}

#[derive(Debug, Deserialize)]
pub struct StreamInfo {
    pub index: usize,
    #[serde(flatten)]
    pub kind: StreamKind,
}

impl StreamInfo {
    fn check(&self) {
        self.kind.check();
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "codec_type", rename_all = "snake_case")]
pub enum StreamKind {
    Video(VideoStreamInfo),
    Audio(AudioStreamInfo),
    Data,
}

impl StreamKind {
    fn check(&self) {
        match self {
            StreamKind::Video(video) => {
                video.check();
            }
            _ => {}
        };
    }
}

#[derive(Debug, Deserialize)]
pub struct VideoStreamInfo {
    #[serde(flatten)]
    pub codec: Codec,
    pub width: u16,
    pub height: u16,
    pub start_pts: u64,
    pub duration_ts: u64,
    pub field_order: FieldOrder,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub bit_rate: u32,
    // #[serde(deserialize_with = "deserialize_ratio_from_string")]
    // time_base: Ratio,
    #[serde(deserialize_with = "deserialize_ratio_from_string")]
    pub r_frame_rate: Ratio<u32>,
    #[serde(deserialize_with = "deserialize_ratio_from_string")]
    pub avg_frame_rate: Ratio<u32>,
    pub pix_fmt: String,
    #[serde(deserialize_with = "deserialize_ratio_from_string")]
    pub time_base: Ratio<u32>,
}

impl VideoStreamInfo {
    fn check(&self) {
        assert!(self.width > 0);
        assert!(self.height > 0);
        assert!(self.bit_rate > 0);
        // Fails on vid:24849730
        // assert_eq!(self.r_frame_rate, self.avg_frame_rate);
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "codec_name")]
pub enum Codec {
    #[serde(rename = "h264")]
    H264 { profile: Profile },
    #[serde(other)]
    Other,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub enum Profile {
    #[serde(rename = "Constrained Baseline")]
    Baseline,
    #[serde(rename = "Main")]
    Main,
    #[serde(rename = "High")]
    High,
}

impl Profile {
    pub fn flag(self) -> &'static str {
        match self {
            Profile::Baseline => "baseline",
            Profile::Main => "main",
            Profile::High => "high",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize)]
pub enum FieldOrder {
    #[serde(rename = "progressive")]
    Progressive,
    #[serde(rename = "bt")]
    BottomFirst,
}

#[derive(Debug, Deserialize)]
pub struct AudioStreamInfo {
    pub codec_name: String,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub bit_rate: u32,
    pub start_pts: u64,
    pub duration_ts: u64,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub sample_rate: u32,
    pub channels: u8,
    #[serde(deserialize_with = "deserialize_ratio_from_string")]
    pub time_base: Ratio<u32>,
}

pub fn deserialize_number_from_string<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: Display,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt<T> {
        String(String),
        Number(T),
    }

    match StringOrInt::<T>::deserialize(deserializer)? {
        StringOrInt::String(s) => s.parse::<T>().map_err(serde::de::Error::custom),
        StringOrInt::Number(i) => Ok(i),
    }
}

pub fn deserialize_ratio_from_string<'de, D>(deserializer: D) -> Result<Ratio<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let string = String::deserialize(deserializer)?;
    let (num, den) = string
        .split_once('/')
        .ok_or_else(|| serde::de::Error::custom("missing /"))?;
    let num = num.parse().map_err(serde::de::Error::custom)?;
    let den = den.parse().map_err(serde::de::Error::custom)?;
    Ok(Ratio::new(num, den))
}
