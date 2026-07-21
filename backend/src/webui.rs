//! Embedded web UI (feature `embed-frontend`): serves frontend/dist from the
//! binary with an SPA fallback, so one executable ships the whole product.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Asset;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // API namespaces never fall through to the SPA shell.
    if path.starts_with("api/") || path.starts_with("v1/") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"success": false, "message": "not found"})),
        )
            .into_response();
    }

    let candidate = if path.is_empty() { "index.html" } else { path };
    match Asset::get(candidate) {
        Some(file) => serve_file(candidate, file),
        // SPA fallback: unknown paths (e.g. /channels) get index.html.
        None => match Asset::get("index.html") {
            Some(file) => serve_file("index.html", file),
            None => (StatusCode::NOT_FOUND, "web ui not embedded").into_response(),
        },
    }
}

fn serve_file(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite emits content-hashed filenames under assets/ — cache those hard.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime.as_ref().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
