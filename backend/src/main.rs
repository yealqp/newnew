use std::net::SocketAddr;

use tracing::info;

use gateway::config::Config;
use gateway::state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cfg = Config::load();

    let pool = match gateway::db::init(&cfg).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("db init: {e}");
            std::process::exit(1);
        }
    };

    let port = cfg.port.clone();
    let state = AppState::new(pool, cfg);
    let app = gateway::build_router(state);

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
