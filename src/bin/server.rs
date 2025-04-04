use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use frameserve::media::{Playlist, Source};
use jiff::Timestamp;
use jiff::civil::datetime;
use jiff::tz::TimeZone;
use std::io;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> io::Result<()> {
    let sources = load_media()?;

    let start = datetime(2025, 3, 1, 0, 0, 0, 0)
        .to_zoned(TimeZone::UTC)
        .unwrap()
        .timestamp();

    let playlist = Playlist { start, sources };
    let playlist: &'static Playlist = Box::leak(Box::new(playlist));

    let app = Router::new()
        .route("/hls/index.m3u8", get(hls_index_playlist))
        .nest_service("/media", ServeDir::new("../../caveh/encodes/"))
        .layer(CorsLayer::permissive())
        .with_state(playlist);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}

async fn hls_index_playlist(State(playlist): State<&'static Playlist>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        playlist.hls_stream(Timestamp::now()),
    )
}

fn load_media() -> io::Result<Vec<Source>> {
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir("media")? {
        let entry = entry?;
        if entry.path().extension() == Some("json".as_ref()) {
            let file = std::fs::File::open(entry.path())?;
            let manifest = serde_json::from_reader::<_, Source>(file)?;
            manifests.push(manifest);
        }
    }
    Ok(manifests)
}
