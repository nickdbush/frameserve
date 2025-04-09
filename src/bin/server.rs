use std::io;

use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use frameserve::{config::get_config, playout::Playlist};
use jiff::Timestamp;
use tower_http::{cors::CorsLayer, services::ServeDir};

#[tokio::main]
async fn main() -> io::Result<()> {
    let config = get_config();

    let playlist = Playlist::load(Timestamp::now(), "packages");

    let app_state = AppState::new(playlist);

    let app = Router::new()
        .route("/hls/index.m3u8", get(hls_index_playlist))
        .route("/hls/{variant}", get(hls_variant_playlist))
        .nest_service("/media", ServeDir::new("segments"))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    println!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}

async fn hls_index_playlist(State(state): State<AppState>) -> impl IntoResponse {
    let mut buffer = String::new();
    state.playlist.master_playlist(&mut buffer).unwrap();
    (
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        buffer,
    )
}

async fn hls_variant_playlist(
    State(state): State<AppState>,
    Path(variant): Path<String>,
) -> impl IntoResponse {
    let stream = match variant.as_str() {
        "variant0.m3u8" => &state.playlist.streams[0],
        "variant1.m3u8" => &state.playlist.streams[1],
        "variant2.m3u8" => &state.playlist.streams[2],
        "variant3.m3u8" => &state.playlist.streams[3],
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    let mut buffer = String::new();
    stream
        .render_variant_playlist(&mut buffer, &state.playlist, Timestamp::now())
        .unwrap();

    (
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        buffer,
    )
        .into_response()
}

#[derive(Clone)]
struct AppState {
    playlist: &'static Playlist,
}

impl AppState {
    fn new(playlist: Playlist) -> Self {
        let playlist = Box::leak(Box::new(playlist));
        Self { playlist }
    }
}
