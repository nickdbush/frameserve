use crate::recipe::{CmdBuilder, VideoSpec};
use serde::Deserialize;

pub fn compare_vmaf(encode_dir: String, original: String) -> CmdBuilder {
    let mut cmd = CmdBuilder::new();
    cmd.set("-i", original);
    cmd.set("-i", format!("{}/stream.m3u8", encode_dir));
    cmd.arg("-an");

    let mut filters = Vec::new();
    filters.push("[0:v]setpts=PTS-STARTPTS,scale=1920:1080[reference]");
    filters.push("[1:v]setpts=PTS-STARTPTS,scale=1920:1080[distorted]");
    filters.push("[distorted][reference]libvmaf=pool=harmonic_mean:n_threads=8");
    cmd.set("-filter_complex", filters.join(";"));

    cmd.set("-f", "null");
    cmd.arg("-");

    cmd
}

pub fn test_stream(original: String, spec: &VideoSpec) -> CmdBuilder {
    let mut cmd = CmdBuilder::new();
    cmd.set("-i", &original);
    cmd.set("-i", original);
    cmd.arg("-an");

    let mut filters = Vec::new();
    filters.push("[0:v]setpts=PTS-STARTPTS,scale=1920:1080[reference]".to_string());
    filters.push(format!(
        "[1:v]setpts=PTS-STARTPTS,scale={}:{}[forEncode]",
        spec.width, spec.height,
    ));

    cmd
}

#[derive(Debug, Clone, Deserialize)]
pub struct VmafReport {
    pub frames: Vec<Frame>,
    pub pooled_metrics: PooledMetrics,
}

impl VmafReport {
    pub fn open(file: &str) -> Self {
        let file = std::fs::File::open(file).unwrap();
        let reader = std::io::BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }

    pub fn harmonic_mean(&self) -> f64 {
        self.pooled_metrics.vmaf.harmonic_mean
    }

    // e.g. what is the 99th percentile of VMAF scores
    pub fn percentile(&self, percentile: f64) -> f64 {
        let mut scores: Vec<f64> = self.frames.iter().map(|f| f.metrics.vmaf).collect();
        scores.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let index = ((percentile / 100.0) * (scores.len() as f64)).round() as usize;
        scores[index]
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Frame {
    pub metrics: FrameMetrics,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FrameMetrics {
    pub vmaf: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PooledMetrics {
    pub vmaf: PooledMetric,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PooledMetric {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub harmonic_mean: f64,
}
