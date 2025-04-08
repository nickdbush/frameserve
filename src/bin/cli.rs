use std::fs;

use clap::Parser;
use frameserve::inspect::{Profile, inspect};
use frameserve::package::package;
use frameserve::recipe::{Pass, VideoSpec, transcode_video};
use frameserve::utils::extract_vid;

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
    Package {
        dir: String,
    },
    Clean,
}

fn main() {
    let args = Args::parse();
    match args.cmd {
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

            let audio_dir = format!("{out_dir}/aac_192k");

            transcode_video(&original, &media_info, Pass::First, &outputs, &audio_dir).execute();
            transcode_video(&original, &media_info, Pass::Second, &outputs, &audio_dir).execute();
        }
        Command::Package { dir } => {
            fs::create_dir_all("segments").unwrap();
            fs::create_dir_all("packages").unwrap();
            package(&dir, "segments", "packages");
        }
        Command::Clean => {
            let _ = std::fs::remove_dir_all("segments");
            let _ = std::fs::remove_dir_all("packages");
        }
    }
}
