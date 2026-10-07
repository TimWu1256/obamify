use axum::{Router, routing::{get, post}};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

mod dto;
mod routes;

#[tokio::main]
async fn main() {
    env_logger::init();

    // headless Vulkan 需要 XDG_RUNTIME_DIR
    ensure_xdg_runtime_dir();

    let app = Router::new()
        .route("/health",                   get(routes::health))
        .route("/presets",                  get(routes::presets::list))
        .route("/obamify/assignments",      post(routes::obamify::assignments))
        .route("/obamify/gif",              post(routes::obamify::gif))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = std::env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    log::info!("obamify-api listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}

fn ensure_xdg_runtime_dir() {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) => { let _ = std::fs::create_dir_all(&dir); }
        Err(_)  => { let _ = std::fs::create_dir_all("/tmp/xdg_runtime"); }
    }
}
