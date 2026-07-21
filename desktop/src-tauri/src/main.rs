//! OpenGate 桌面端：进程内启动网关（含嵌入的 Web UI），
//! WebView 直接加载本地 HTTP 服务，端口自动分配。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::mpsc;

use tauri::Manager;

fn start_gateway(db_path: std::path::PathBuf) -> Result<u16, String> {
    let (tx, rx) = mpsc::channel::<Result<u16, String>>();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(format!("tokio runtime: {e}")));
                return;
            }
        };
        rt.block_on(async move {
            std::env::set_var("DB_PATH", &db_path);
            let cfg = gateway::config::Config::load();
            let pool = match gateway::db::init(&cfg).await {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(Err(format!("db init: {e}")));
                    return;
                }
            };
            let state = gateway::state::AppState::new(pool, cfg);
            let app = gateway::build_router(state);

            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
                Ok(l) => l,
                Err(e) => {
                    let _ = tx.send(Err(format!("bind: {e}")));
                    return;
                }
            };
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let _ = tx.send(Ok(port));

            if let Err(e) = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                eprintln!("gateway serve: {e}");
            }
        });
    });

    rx.recv().map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("gateway.db");

            let port = start_gateway(db_path).map_err(|e| -> Box<dyn std::error::Error> {
                format!("启动内置网关失败: {e}").into()
            })?;

            let url: tauri::Url = format!("http://127.0.0.1:{port}/").parse()?;
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("OpenGate")
                .inner_size(1360.0, 860.0)
                .min_inner_size(960.0, 640.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OpenGate desktop");
}
