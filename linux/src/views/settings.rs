use adw::prelude::*;
use gtk::glib;
use std::sync::Arc;

use crate::core_bridge::CoreBridge;

/// Build the settings view.
///
/// Layout:
/// - Audio device dropdown (AdwComboRow)
/// - API key entry (AdwPasswordEntryRow)
/// - Transcription mode toggle
/// - Model management section
/// - Data management section
pub fn build(bridge: Arc<CoreBridge>) -> gtk::Box {
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();

    let clamp = adw::Clamp::builder().maximum_size(600).build();

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    // ── Audio section ──────────────────────────────────────────────────

    let audio_group = adw::PreferencesGroup::builder()
        .title("Audio")
        .description("Configure audio input for recording")
        .build();

    let device_model = gtk::StringList::new(&["Default"]);

    let device_row = adw::ComboRow::builder()
        .title("Input Device")
        .subtitle("Microphone used for recording")
        .model(&device_model)
        .build();

    audio_group.add(&device_row);

    // Load devices asynchronously.
    {
        let bridge_ref = bridge.clone();
        let model = device_model.clone();

        glib::spawn_future_local(async move {
            match bridge_ref.list_audio_devices().await {
                Ok(devices) => {
                    // Remove the placeholder "Default" and repopulate.
                    model.splice(0, model.n_items(), &[] as &[&str]);
                    for dev in &devices {
                        let label = if dev.is_default {
                            format!("{} (default)", dev.name)
                        } else {
                            dev.name.clone()
                        };
                        model.append(&label);
                    }
                    if devices.is_empty() {
                        model.append("No devices found");
                    }
                }
                Err(e) => {
                    eprintln!("Failed to list audio devices: {e}");
                }
            }
        });
    }

    // ── API keys section ───────────────────────────────────────────────

    let api_group = adw::PreferencesGroup::builder()
        .title("API Keys")
        .description("Keys are stored in the local database")
        .build();

    let whisper_api_row = adw::PasswordEntryRow::builder()
        .title("OpenAI API Key (optional)")
        .build();

    let claude_api_row = adw::PasswordEntryRow::builder()
        .title("Anthropic API Key (for summaries)")
        .build();

    api_group.add(&whisper_api_row);
    api_group.add(&claude_api_row);

    // Load existing keys.
    {
        let bridge_ref = bridge.clone();
        let whisper_row = whisper_api_row.clone();
        let claude_row = claude_api_row.clone();

        glib::spawn_future_local(async move {
            if let Ok(Some(key)) = bridge_ref.get_setting("openai_api_key").await {
                whisper_row.set_text(&key);
            }
            if let Ok(Some(key)) = bridge_ref.get_setting("anthropic_api_key").await {
                claude_row.set_text(&key);
            }
        });
    }

    // Save keys when focus leaves the entry.
    {
        let bridge_ref = bridge.clone();
        let row = whisper_api_row.clone();
        whisper_api_row.connect_changed(move |_| {
            let text = row.text().to_string();
            let bridge_c = bridge_ref.clone();
            glib::spawn_future_local(async move {
                let _ = bridge_c.set_setting("openai_api_key", &text).await;
            });
        });
    }
    {
        let bridge_ref = bridge.clone();
        let row = claude_api_row.clone();
        claude_api_row.connect_changed(move |_| {
            let text = row.text().to_string();
            let bridge_c = bridge_ref.clone();
            glib::spawn_future_local(async move {
                let _ = bridge_c.set_setting("anthropic_api_key", &text).await;
            });
        });
    }

    // ── Transcription section ──────────────────────────────────────────

    let transcription_group = adw::PreferencesGroup::builder()
        .title("Transcription")
        .description("Configure how audio is transcribed")
        .build();

    let cloud_toggle = adw::SwitchRow::builder()
        .title("Use Cloud Transcription")
        .subtitle("Falls back to OpenAI Whisper API (requires API key)")
        .active(false)
        .build();

    transcription_group.add(&cloud_toggle);

    // Load and save cloud toggle state.
    {
        let bridge_ref = bridge.clone();
        let toggle = cloud_toggle.clone();
        glib::spawn_future_local(async move {
            if let Ok(Some(val)) = bridge_ref.get_setting("use_cloud_transcription").await {
                toggle.set_active(val == "true");
            }
        });
    }
    {
        let bridge_ref = bridge.clone();
        cloud_toggle.connect_active_notify(move |toggle| {
            let val = if toggle.is_active() { "true" } else { "false" };
            let bridge_c = bridge_ref.clone();
            let val = val.to_string();
            glib::spawn_future_local(async move {
                let _ = bridge_c.set_setting("use_cloud_transcription", &val).await;
            });
        });
    }

    // ── Model management section ───────────────────────────────────────

    let model_group = adw::PreferencesGroup::builder()
        .title("Whisper Model")
        .description("Local transcription model (download required)")
        .build();

    let model_names = gtk::StringList::new(&[
        "tiny (~75 MB)",
        "base (~142 MB)",
        "small (~466 MB)",
        "medium (~1.5 GB)",
    ]);

    let model_row = adw::ComboRow::builder()
        .title("Model Size")
        .subtitle("Larger models are more accurate but slower")
        .model(&model_names)
        .build();

    let download_btn = gtk::Button::builder()
        .label("Download Selected Model")
        .halign(gtk::Align::Center)
        .build();
    download_btn.add_css_class("pill");

    // Download the selected model on click.
    {
        let bridge_ref = bridge.clone();
        let btn = download_btn.clone();
        let row = model_row.clone();
        download_btn.connect_clicked(move |_| {
            let name = match row.selected() {
                0 => "tiny",
                1 => "base",
                2 => "small",
                3 => "medium",
                _ => "tiny",
            }
            .to_string();

            btn.set_sensitive(false);
            btn.set_label("Downloading…");

            let bridge_c = bridge_ref.clone();
            let btn_c = btn.clone();
            glib::spawn_future_local(async move {
                match bridge_c.download_model(&name).await {
                    Ok(()) => {
                        btn_c.set_label("Download complete");
                        glib::timeout_future_seconds(2).await;
                        btn_c.set_label("Download Selected Model");
                        btn_c.set_sensitive(true);
                    }
                    Err(e) => {
                        eprintln!("Model download failed: {e}");
                        btn_c.set_label(&format!("Download failed: {e}"));
                        btn_c.set_sensitive(true);
                    }
                }
            });
        });
    }

    model_group.add(&model_row);

    // Load saved model selection.
    {
        let bridge_ref = bridge.clone();
        let row = model_row.clone();
        glib::spawn_future_local(async move {
            if let Ok(Some(val)) = bridge_ref.get_setting("whisper_model").await {
                let idx = match val.as_str() {
                    "tiny" => 0,
                    "base" => 1,
                    "small" => 2,
                    "medium" => 3,
                    _ => 0,
                };
                row.set_selected(idx);
            }
        });
    }

    // Save model selection on change.
    {
        let bridge_ref = bridge.clone();
        model_row.connect_selected_notify(move |row| {
            let model_name = match row.selected() {
                0 => "tiny",
                1 => "base",
                2 => "small",
                3 => "medium",
                _ => "tiny",
            };
            let bridge_c = bridge_ref.clone();
            let name = model_name.to_string();
            glib::spawn_future_local(async move {
                let _ = bridge_c.set_setting("whisper_model", &name).await;
            });
        });
    }

    // ── Data management section ────────────────────────────────────────

    let data_group = adw::PreferencesGroup::builder()
        .title("Data")
        .description("Manage stored meetings and data")
        .build();

    let export_all_row = adw::ActionRow::builder()
        .title("Export All Meetings")
        .subtitle("Save all meetings as Markdown files")
        .activatable(true)
        .build();
    let export_arrow = gtk::Image::from_icon_name("document-save-symbolic");
    export_all_row.add_suffix(&export_arrow);

    let clear_row = adw::ActionRow::builder()
        .title("Clear All Data")
        .subtitle("Permanently delete all meetings and transcripts")
        .activatable(true)
        .build();
    clear_row.add_css_class("error");
    let clear_arrow = gtk::Image::from_icon_name("user-trash-symbolic");
    clear_row.add_suffix(&clear_arrow);

    data_group.add(&export_all_row);
    data_group.add(&clear_row);

    // ── Pack everything ────────────────────────────────────────────────

    container.append(&audio_group);
    container.append(&api_group);
    container.append(&transcription_group);
    container.append(&model_group);
    container.append(&download_btn);
    container.append(&data_group);

    clamp.set_child(Some(&container));

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&clamp)
        .build();

    outer.append(&scroll);
    outer
}
