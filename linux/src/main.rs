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
    let data_dir = gtk::glib::user_data_dir().join("session-monitor");
    std::fs::create_dir_all(&data_dir).ok();

    let db_path = data_dir.join("data.db");

    // Whisper models: prefer whichever location already holds a ggml-*.bin
    // model. The downloadable artifact bundles the model under ./models next to
    // the binary; otherwise models live under the XDG data dir
    // (~/.local/share/session-monitor/models), where the in-app downloader
    // writes them. Only when neither has a model yet do we create a directory
    // (XDG first, then ./models) for a future download.
    let model_path = {
        let has_model = |dir: &std::path::Path| -> bool {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries.flatten().any(|e| {
                        let name = e.file_name();
                        let name = name.to_string_lossy();
                        name.starts_with("ggml-") && name.ends_with(".bin")
                    })
                })
                .unwrap_or(false)
        };
        // Resolve the bundled "models" dir next to the executable (not the
        // process CWD), so launching from another directory or a desktop
        // launcher still finds the model shipped in the artifact.
        let local = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("models");
        let xdg_models = data_dir.join("models");
        if has_model(&local) {
            local
        } else if has_model(&xdg_models) {
            xdg_models
        } else if std::fs::create_dir_all(&xdg_models).is_ok() {
            xdg_models
        } else {
            std::fs::create_dir_all(&local).ok();
            local
        }
    };

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
