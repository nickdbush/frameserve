use std::io;

use frameserve::playout::Playlist;
use jiff::Timestamp;

#[tokio::main]
async fn main() -> io::Result<()> {
    let playlist = Playlist::from_dir("packages");

    let mut buf = String::new();
    playlist.hls(&mut buf, Timestamp::now()).unwrap();
    println!("{buf}");

    Ok(())
}

// 1:19.41 (29.97fps) + 48.72
// 79.41 + 48.72 =
