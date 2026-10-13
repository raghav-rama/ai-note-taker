mod core_bridge;
mod views;
mod window;

use adw::prelude::*;
use core_bridge::CoreBridge;
use std::sync::Arc;

/// Application ID intentionally generic — part of stealth design.
const APP_ID: &str = "org.freedesktop.SessionMonitor";

fn main() {
    // Initialize the tokio runtime for async core operations.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");
    let rt_handle = rt.handle().clone();

    // Determine paths for data and model storage.
    let data_dir = glib::user_data_dir().join("session-monitor");
    std::fs::create_dir_all(&data_dir).ok();

    let db_path = data_dir.join("data.db");
    let model_path = data_dir.join("models");
    std::fs::create_dir_all(&model_path).ok();

    // Initialize the core bridge that wraps notetaker-core.
    let bridge = match CoreBridge::new(
        db_path.to_str().unwrap_or("data.db"),
        model_path.to_str().unwrap_or("models"),
        rt_handle,
    ) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            eprintln!("Failed to initialize core: {e}");
            std::process::exit(1);
        }
    };

    let app = adw::Application::builder().application_id(APP_ID).build();

    let bridge_clone = bridge.clone();
    app.connect_activate(move |app| {
        let win = window::AppWindow::new(app, bridge_clone.clone());
        win.present();
    });

    // Run the GTK main loop. The tokio runtime lives for the process lifetime
    // because `rt` is not dropped until main() returns.
    let _rt_guard = rt.enter();
    app.run();
}
