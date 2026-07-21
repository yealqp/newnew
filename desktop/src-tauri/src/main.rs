//! OpenGate 桌面端：进程内启动网关（含嵌入的 Web UI），
//! WebView 直接加载本地 HTTP 服务，端口自动分配。
//! 系统托盘：左键显示/聚焦主窗口，右键菜单『显示 / 隐藏 / 退出』，
//! 关闭窗口时隐藏而不退出，常驻后台。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::Mutex;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

/// 启动内置网关，返回监听端口。独立线程内建 tokio runtime + axum 服务。
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

            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", GATEWAY_PORT)).await {
                Ok(l) => l,
                Err(e) => {
                    let _ = tx.send(Err(format!("bind: {e}")));
                    return;
                }
            };
            let _ = tx.send(Ok(GATEWAY_PORT));

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

#[tauri::command]
fn gateway_url(app: tauri::AppHandle) -> String {
    let port = app.state::<SharedPort>().0.lock().unwrap().unwrap_or(GATEWAY_PORT);
    format!("http://127.0.0.1:{port}/")
}

/// 启动后保存在 AppState 中的网关端口，供前端（如需要）查询。
struct SharedPort(Mutex<Option<u16>>);

const GATEWAY_PORT: u16 = 54444;

fn main() {
    tauri::Builder::default()
        .manage(SharedPort(Mutex::new(None)))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("gateway.db");

            let port = start_gateway(db_path).map_err(|e| -> Box<dyn std::error::Error> {
                format!("启动内置网关失败: {e}").into()
            })?;
            *app.state::<SharedPort>().0.lock().unwrap() = Some(port);

            let url: tauri::Url = format!("http://127.0.0.1:{port}/").parse()?;
            let win = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url),
            )
            .title("OpenGate")
            .inner_size(1360.0, 860.0)
            .min_inner_size(960.0, 640.0)
            .build()?;
            let _ = win; // 主窗口由托盘/窗口事件管理，这里只需创建

            // 托盘菜单：显示 / 隐藏 / 退出
            let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏窗口", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 OpenGate", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("missing default icon"))
                .tooltip("OpenGate")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // 单击左键：显示并聚焦主窗口
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            // 端口已固定，不再需要 emit 端口号；保留事件用于扩展开机自启等逻辑。
            app.emit("gateway://started", port)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 主窗口关闭按钮 → 隐藏到托盘，而非退出进程。
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![gateway_url])
        .run(tauri::generate_context!())
        .expect("error while running OpenGate desktop");
}