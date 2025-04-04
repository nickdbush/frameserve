use crate::inspect::inspect;
use crate::recipe::CmdBuilder;
use crate::utils::extract_vid;
use crate::vmaf::VmafReport;
use serde::Serialize;
use std::io::Write;

const OUTPUT: &str = "rd_temp.mp4";
const VMAF_JSON: &str = "rd_vmaf.json";

#[derive(Debug, Clone, Serialize)]
pub struct RateDistortionPoint {
    pub vid: u32,
    pub width: u16,
    pub height: u16,
    pub crf: u8,
    pub bitrate: u32,
    pub vmaf_harmonic_mean: f64,
    pub vmaf_99_percentile: f64,
}

impl RateDistortionPoint {
    pub fn log(&self) {
        // append to rd_log.ndjson
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open("rd_log.ndjson")
            .unwrap();
        let mut writer = std::io::BufWriter::new(file);
        let json = serde_json::to_string(self).unwrap();
        writeln!(writer, "{json}").unwrap();
        writer.flush().unwrap();
    }
}

pub fn calculate_rd_point(file: &str, width: u16, height: u16, crf: u8) -> RateDistortionPoint {
    let cmd = make_calculate_rd_point_cmd(file, height, crf);
    cmd.execute();

    let info = inspect(OUTPUT);
    let vmaf_report = VmafReport::open(VMAF_JSON);

    RateDistortionPoint {
        vid: extract_vid(file),
        width,
        height,
        crf,
        bitrate: info.video_stream().bit_rate,
        vmaf_harmonic_mean: vmaf_report.harmonic_mean(),
        vmaf_99_percentile: vmaf_report.percentile(99.0),
    }
}

fn make_calculate_rd_point_cmd(file: &str, height: u16, crf: u8) -> CmdBuilder {
    let mut cmd = CmdBuilder::new();
    cmd.arg("-y");

    cmd.set("-i", file);
    // cmd.arg("-an");

    cmd.set("-map", "0:v:0");
    cmd.set("-vf", format!("scale=-2:{height}"));
    cmd.set("-pix_fmt", "yuv420p");
    cmd.set("-c:v", "libx264");
    cmd.set("-crf", crf.to_string());
    cmd.set("-preset", "slow");
    cmd.set("-tune", "film");
    cmd.set("-profile:v", "high");
    cmd.arg(OUTPUT);
    cmd.set("-dec", "0:0");

    let mut filters = Vec::new();
    filters.push("[0:v:0]setpts=PTS-STARTPTS,scale=1920:1080[reference]".to_string());
    filters.push("[dec:0]scale=1920:1080[distorted]".to_string());
    filters.push(format!(
        "[distorted][reference]libvmaf=n_threads=8:log_fmt=json:log_path={VMAF_JSON}[vmaf]"
    ));
    cmd.set("-filter_complex", filters.join(";"));

    cmd.arg("-an");
    cmd.set("-map", "[vmaf]");
    cmd.set("-f", "null");
    cmd.arg("-");

    cmd
}
