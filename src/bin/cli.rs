use std::collections::HashSet;
use std::fs;

use clap::Parser;
use frameserve::dynamic::calculate_rd_point;
use frameserve::inspect::{Profile, inspect};
use frameserve::package::package;
use frameserve::recipe::{Pass, VideoSpec, transcode_video};
use frameserve::utils::extract_vid;
use frameserve::vmaf::compare_vmaf;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    cmd: Command,
}

#[derive(Parser)]
enum Command {
    Encode {
        original: String,
        #[clap(default_value = "encodes")]
        out_dir: String,
    },
    Compare {
        original: String,
        encode_dir: String,
    },
    Compute {
        original: String,
    },
    Package {
        dir: String,
    },
}

fn main() {
    let args = Args::parse();
    match args.cmd {
        Command::Compare {
            encode_dir,
            original,
        } => {
            compare_vmaf(encode_dir, original).execute();
        }
        Command::Encode { original, out_dir } => {
            let media_info = inspect(&original);
            media_info.check();

            let high_spec = VideoSpec {
                width: 1920,
                height: 1080,
                bit_rate: 5000_000,
                profile: Profile::High,
            };
            let mid_spec = VideoSpec {
                width: 1280,
                height: 720,
                bit_rate: 1500_000,
                profile: Profile::High,
            };
            let low_spec = VideoSpec {
                width: 960,
                height: 540,
                bit_rate: 400_000,
                profile: Profile::Main,
            };

            let vid = extract_vid(&original);

            let out_dir = format!("{out_dir}/{vid}");
            let outputs = [
                high_spec.out_dir(&out_dir),
                mid_spec.out_dir(&out_dir),
                low_spec.out_dir(&out_dir),
            ];

            transcode_video(&original, media_info.video_stream(), Pass::First, &outputs).execute();
            transcode_video(&original, media_info.video_stream(), Pass::Second, &outputs).execute();
        }
        Command::Compute { original } => {
            let info = inspect(&original);
            let v = info.video_stream();

            let mut seen_sizes = HashSet::new();

            for (width, height) in [
                (v.width, v.height),
                // (1920, 1080),
                // (1280, 720),
                // (960, 540),
                // (640, 360),
            ] {
                if !seen_sizes.insert((width, height)) {
                    continue;
                }

                if info.video_stream().width < width && info.video_stream().height < height {
                    continue;
                }

                for crf in [16, 20, 24, 28, 32, 36] {
                    calculate_rd_point(&original, width, height, crf).log();
                }
            }
        }
        Command::Package { dir } => {
            fs::create_dir_all("segments").unwrap();
            fs::create_dir_all("packages").unwrap();
            package(&dir, "segments", "packages");
        }
    }
}
