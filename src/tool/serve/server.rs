//! HTTP server for serving built assets.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderValue};
use axum::response::IntoResponse;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

/// Starts the HTTP server serving files from the target directory.
pub async fn start(port: u16, target: &Path) -> std::io::Result<()> {
    let target_buf = target.to_path_buf();
    let serve_dir = ServeDir::new(target)
        .append_index_html_on_directories(true)
        .fallback(axum::routing::any(move |req: axum::extract::Request| {
            html_fallback(req, target_buf.clone())
        }));

    let app = Router::new()
        .fallback_service(serve_dir)
        // Discourage client-side caching of assets served from the local server.
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .await
        .map_err(|e| std::io::Error::other(format!("server error: {}", e)))
}

/// Fallback handler that tries `{path}/index.html` and `{path}.html`
/// when the primary file lookup returns 404.
async fn html_fallback(req: axum::extract::Request, target: PathBuf) -> impl IntoResponse {
    let path = req
        .uri()
        .path()
        .trim_start_matches('/')
        .trim_end_matches('/');

    if !path.is_empty() && !path.contains("..") {
        // Try {path}/index.html
        let index_path = target.join(path).join("index.html");
        if let Ok(content) = tokio::fs::read(&index_path).await {
            return (
                StatusCode::OK,
                [(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                )],
                content,
            )
                .into_response();
        }

        // Try {path}.html
        let html_path = target.join(format!("{}.html", path));
        if let Ok(content) = tokio::fs::read(&html_path).await {
            return (
                StatusCode::OK,
                [(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                )],
                content,
            )
                .into_response();
        }
    }

    StatusCode::NOT_FOUND.into_response()
}
