use crate::inspect::{Codec, FieldOrder, Info, Profile, VideoStreamInfo};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

const GOP_DURATION: f64 = 10.0;

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoSpec {
    pub width: u16,
    pub height: u16,
    pub bit_rate: u32,
    pub profile: Profile,
}

impl VideoSpec {
    pub fn out_dir(self, path: impl Into<String>) -> Output {
        Output {
            dir: format!("{}/{}", path.into(), self.dir_name()),
            spec: self,
        }
    }

    fn calculate_resize(&self, info: &VideoStreamInfo) -> Option<Resize> {
        if info.width <= self.width && info.height <= self.height {
            return None;
        }

        let width_ratio = self.width as f64 / info.width as f64;
        let height_ratio = self.height as f64 / info.height as f64;

        if width_ratio < height_ratio {
            Some(Resize::Width(self.width))
        } else {
            Some(Resize::Height(self.height))
        }
    }

    fn dir_name(&self) -> String {
        let kbps = self.bit_rate / 1000;
        format!(
            "{}x{}_{kbps}k_{}",
            self.width,
            self.height,
            self.profile.flag()
        )
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Decision {
    Copy,
    Transcode(&'static str),
}

impl VideoStreamInfo {
    pub fn resolve(&self, spec: &VideoSpec) -> Decision {
        match self.codec {
            Codec::H264 { profile } if profile <= spec.profile => {}
            Codec::H264 { .. } => return Decision::Transcode("profile"),
            _ => return Decision::Transcode("codec"),
        }

        if self.width > spec.width || self.height > spec.height {
            return Decision::Transcode("size");
        }

        if self.bit_rate > spec.bit_rate {
            Decision::Transcode("bitrate")
        } else {
            Decision::Copy
        }
    }
}

#[derive(Default)]
pub struct CmdBuilder {
    args: Vec<String>,
    x264_opts: Vec<(String, String)>,
}

impl CmdBuilder {
    pub fn new() -> Self {
        let mut cmd = Self::default();
        cmd.arg("-hide_banner");
        cmd
    }

    pub fn arg(&mut self, arg: impl Into<String>) {
        self.args.push(arg.into());
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.arg(key);
        self.arg(value);
    }

    pub fn x264_opt(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.x264_opts.push((key.into(), value.into()));
    }

    pub fn flush_x264opts(&mut self) {
        if self.x264_opts.is_empty() {
            return;
        }

        let opts = self
            .x264_opts
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(":");
        self.set("-x264opts", opts);
        self.x264_opts.clear();
    }

    pub fn execute(&self) {
        self.print();
        let p = std::process::Command::new("ffmpeg")
            .args(&self.args)
            .status()
            .unwrap();
        assert!(p.success());
    }

    pub fn print(&self) {
        println!(">>> {}", self.to_string());
        println!();
    }
}

impl std::fmt::Display for CmdBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for arg in &self.args {
            write!(f, "{} ", arg)?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, ValueEnum)]
pub enum Pass {
    First,
    Second,
}

pub fn transcode_video(
    input: &str,
    info: &Info,
    pass: Pass,
    outputs: &[Output],
    audio_dir: &str,
) -> CmdBuilder {
    let v = info.video_stream();
    let a = info.audio_stream();

    let mut cmd = CmdBuilder::new();

    cmd.set("-i", input);
    cmd.set("-map_metadata", "-1");

    let mut filter_graph = FilterGraph::default();
    if v.field_order != FieldOrder::Progressive {
        filter_graph.add_global_filter("yadif=1");
    }
    if v.pix_fmt != "yuv420p" {
        filter_graph.add_global_filter("format=yuv420p");
    }
    for output in outputs {
        filter_graph.add_output(output.spec.calculate_resize(v));
    }
    filter_graph.write(&mut cmd);

    for (i, output) in outputs.iter().enumerate() {
        output.write(&mut cmd, v, StreamRef::new_output(i), pass);
    }

    if pass == Pass::Second {
        cmd.set("-map", "0:a");
        if a.channels <= 2 && a.bit_rate <= 192000 && a.codec_name == "aac" {
            cmd.set("-c:a", "copy");
        } else {
            cmd.set("-ac", "2");
            cmd.set("-c:a", "aac_at");
            cmd.set("-b:a", "192k");
        }
        with_hls_muxer(&mut cmd, &audio_dir);
    }

    cmd
}

pub struct Output {
    dir: String,
    spec: VideoSpec,
}

impl Output {
    fn write(&self, cmd: &mut CmdBuilder, info: &VideoStreamInfo, stream: StreamRef, pass: Pass) {
        cmd.set("-map", stream);
        cmd.set("-c:v", "libx264");
        cmd.set("-preset", "slow");
        cmd.set("-tune", "film");
        cmd.set("-profile:v", self.spec.profile.flag());
        cmd.set("-b:v", self.spec.bit_rate.to_string());
        cmd.set("-maxrate", self.spec.bit_rate.to_string());
        cmd.set("-bufsize", (self.spec.bit_rate * 2).to_string());
        cmd.set("-flags", "+cgop");

        let gop = info.avg_frame_rate.calculate_gop_length(GOP_DURATION);

        // https://superuser.com/a/1223359
        cmd.set("-force_key_frames", format!("expr:eq(mod(n,{gop}),0)"));
        cmd.x264_opt("rc-lookahead", gop.to_string());
        cmd.x264_opt("keyint", (gop * 2).to_string());
        cmd.x264_opt("min-keyint", gop.to_string());
        cmd.flush_x264opts();

        cmd.set(
            "-passlogfile",
            format!("{}x{}", self.spec.width, self.spec.height),
        );

        if pass == Pass::First {
            cmd.set("-pass", "1");
            cmd.set("-f", "null");
            cmd.arg("/dev/null");
        } else {
            cmd.set("-pass", "2");
            with_hls_muxer(cmd, &self.dir);
        }
    }
}

fn with_hls_muxer(cmd: &mut CmdBuilder, out_dir: &str) {
    cmd.set("-f", "hls");
    cmd.set("-hls_time", GOP_DURATION.to_string());
    cmd.set("-hls_segment_filename", format!("{}/s%05d.mp4", out_dir));
    cmd.set("-hls_segment_type", "fmp4");
    cmd.set("-hls_list_size", "0");

    cmd.arg(format!("{}/stream.m3u8", out_dir));

    std::fs::create_dir_all(out_dir).unwrap();
}

#[derive(Default)]
struct FilterGraph {
    global_filters: Vec<String>,
    outputs: Vec<Option<Resize>>,
}

#[derive(Copy, Clone)]
enum Resize {
    Width(u16),
    Height(u16),
}

impl FilterGraph {
    fn add_global_filter(&mut self, filter: impl Into<String>) {
        self.global_filters.push(filter.into());
    }

    fn add_output(&mut self, filter: Option<Resize>) {
        self.outputs.push(filter);
    }

    fn write(&self, cmd: &mut CmdBuilder) {
        let mut components = Vec::new();

        let mut stream_head = StreamRef::new("[0:v]");
        for (i, filter) in self.global_filters.iter().enumerate() {
            let destination = StreamRef::new_global_intermediate(i);
            components.push(format!("{}{}{}", stream_head, filter, destination));
            stream_head = destination;
        }

        let split_destinations = self
            .outputs
            .iter()
            .enumerate()
            .map(|(i, resize_filter)| {
                if resize_filter.is_some() {
                    StreamRef::new_variant_intermediate(i).0
                } else {
                    StreamRef::new_output(i).0
                }
            })
            .collect::<Vec<_>>();
        components.push(format!(
            "{}split={}{}",
            stream_head.0,
            split_destinations.len(),
            split_destinations.join("")
        ));

        for (i, resize_filter) in self.outputs.iter().enumerate() {
            if let Some(resize) = resize_filter {
                let filter = match resize {
                    Resize::Width(w) => format!("scale={w}:-2"),
                    Resize::Height(h) => format!("scale=-2:{h}"),
                };
                let src = StreamRef::new_variant_intermediate(i);
                let dst = StreamRef::new_output(i);
                components.push(format!("{}{}{}", src, filter, dst));
            }
        }

        let filter = components.join(";");
        cmd.set("-filter_complex", filter);
    }
}

struct StreamRef(String);

impl StreamRef {
    fn new(specifier: impl Into<String>) -> Self {
        StreamRef(specifier.into())
    }

    fn new_output(index: usize) -> Self {
        StreamRef(format!("[out{index}]"))
    }

    fn new_global_intermediate(index: usize) -> Self {
        StreamRef(format!("[g{index}]"))
    }

    fn new_variant_intermediate(index: usize) -> Self {
        StreamRef(format!("[v{index}]"))
    }
}

impl Into<String> for StreamRef {
    fn into(self) -> String {
        self.0
    }
}

impl std::fmt::Display for StreamRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
