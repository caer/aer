//! HTTP server for serving built assets.

use std::path::Path;

use axum::Router;
use tower_http::services::ServeDir;

/// Starts the HTTP server serving files from the target directory.
pub async fn start(port: u16, target: &Path) -> std::io::Result<()> {
    let serve_dir = ServeDir::new(target).append_index_html_on_directories(true);

    let app = Router::new().fallback_service(serve_dir);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .await
        .map_err(|e| std::io::Error::other(format!("server error: {}", e)))
}
