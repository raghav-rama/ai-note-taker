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

    // Whisper models: prefer whichever known location already holds a ggml-*.bin
    // model, checked in order:
    //   1. next to the executable  — packaged/downloadable artifact
    //   2. ./models in the CWD      — source build (scripts/download-model.sh)
    //   3. the XDG data dir         — written by the in-app downloader
    // Only when none has a model do we create a directory (XDG, else exe-dir,
    // else CWD) for a future download.
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
        let exe_models = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("models")));
        let cwd_models = std::path::PathBuf::from("models");
        let xdg_models = data_dir.join("models");

        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Some(ref d) = exe_models {
            candidates.push(d.clone());
        }
        candidates.push(cwd_models.clone());
        candidates.push(xdg_models.clone());

        match candidates.into_iter().find(|d| has_model(d)) {
            Some(dir) => dir,
            None => {
                if std::fs::create_dir_all(&xdg_models).is_ok() {
                    xdg_models
                } else if let Some(exe_dir) = exe_models {
                    std::fs::create_dir_all(&exe_dir).ok();
                    exe_dir
                } else {
                    std::fs::create_dir_all(&cwd_models).ok();
                    cwd_models
                }
            }
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
        // Re-activating (e.g. after the window was hidden via hide-on-close)
        // should re-present the existing window instead of leaking a new one.
        let windows = app.windows();
        if let Some(existing) = windows.first() {
            existing.present();
            return;
        }
        let win = window::AppWindow::new(app, bridge_clone.clone());
        win.present();
    });

    // Run the GTK main loop. The tokio runtime lives for the process lifetime
    // because `rt` is not dropped until main() returns.
    let _rt_guard = rt.enter();
    app.run();
}
