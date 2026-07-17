mod billing;
mod channel_select;
mod config;
mod convert;
mod db;
mod dto;
mod handlers;
mod logsvc;
mod middleware;
mod models;
mod state;
mod stream;
mod upstream;
mod util;

use std::net::SocketAddr;

use axum::extract::DefaultBodyLimit;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::Json;
use axum::routing::{delete, get, post, put};
use axum::Router;
use serde_json::json;
use tracing::info;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cfg = Config::load();

    let pool = match db::init(&cfg).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("db init: {e}");
            std::process::exit(1);
        }
    };

    let port = cfg.port.clone();
    let state = AppState::new(pool, cfg);

    let admin_routes = Router::new()
        .route("/me", get(handlers::admin::me))
        .route("/change-password", post(handlers::admin::change_password))
        .route("/channels", get(handlers::admin::list_channels))
        .route(
            "/channels/fetch-models",
            post(handlers::admin::fetch_upstream_models),
        )
        .route("/channels/{id}", get(handlers::admin::get_channel))
        .route("/channels", post(handlers::admin::create_channel))
        .route("/channels/{id}", put(handlers::admin::update_channel))
        .route("/channels/{id}", delete(handlers::admin::delete_channel))
        .route("/channels/{id}/test", get(handlers::admin::test_channel))
        .route("/tokens", get(handlers::admin::list_tokens))
        .route("/tokens", post(handlers::admin::create_token))
        .route("/tokens/{id}", put(handlers::admin::update_token))
        .route(
            "/tokens/{id}/reset-key",
            post(handlers::admin::reset_token_key),
        )
        .route("/tokens/{id}", delete(handlers::admin::delete_token))
        .route("/logs", get(handlers::admin::list_logs))
        .route("/logs/{id}", get(handlers::admin::get_log))
        .route("/dashboard", get(handlers::admin::dashboard))
        .route("/settings", get(handlers::admin::get_settings))
        .route("/settings", put(handlers::admin::update_settings))
        .route(
            "/playground/conversations",
            get(handlers::playground::list_conversations),
        )
        .route(
            "/playground/conversations",
            post(handlers::playground::create_conversation),
        )
        .route(
            "/playground/conversations/{id}",
            put(handlers::playground::update_conversation),
        )
        .route(
            "/playground/conversations/{id}",
            delete(handlers::playground::delete_conversation),
        )
        .route(
            "/playground/conversations/{id}/messages",
            get(handlers::playground::list_messages),
        )
        .route(
            "/playground/conversations/{id}/messages",
            post(handlers::playground::create_message),
        )
        .route(
            "/playground/conversations/{id}/messages",
            delete(handlers::playground::clear_messages),
        )
        .route_layer(from_fn_with_state(
            state.clone(),
            middleware::admin_auth_mw,
        ));

    let v1_routes = Router::new()
        .route("/models", get(handlers::relay::list_models))
        .route("/chat/completions", post(handlers::relay::chat_completions))
        .route("/messages", post(handlers::relay::messages))
        .route_layer(from_fn_with_state(
            state.clone(),
            middleware::token_auth_mw,
        ));

    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/api/admin/login", post(handlers::admin::login))
        .nest("/api/admin", admin_routes)
        .nest("/v1", v1_routes)
        .layer(from_fn(middleware::request_id_mw))
        .layer(from_fn(middleware::cors_mw))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("gateway listening on :{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        eprintln!("serve: {e}");
        std::process::exit(1);
    }
}
