use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::sync::Arc;

use crate::core_bridge::CoreBridge;

/// Build the recording view widget tree.
///
/// Layout (top to bottom):
/// - Clamp for comfortable reading width
///   - Title entry (editable meeting name)
///   - Timer label (HH:MM:SS)
///   - Audio level bar
///   - Record / Stop toggle button
///   - Live transcript in a scrolled window
pub fn build(bridge: Arc<CoreBridge>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    // ── Meeting title ──────────────────────────────────────────────────

    let title_entry = gtk::Entry::builder()
        .placeholder_text("Meeting title")
        .halign(gtk::Align::Center)
        .width_chars(30)
        .build();
    title_entry.add_css_class("title-2");

    // ── Timer ──────────────────────────────────────────────────────────

    let timer_label = gtk::Label::builder()
        .label("00:00:00")
        .halign(gtk::Align::Center)
        .build();
    timer_label.add_css_class("title-1");

    // ── Audio level ────────────────────────────────────────────────────

    let level_bar = gtk::LevelBar::builder()
        .min_value(0.0)
        .max_value(1.0)
        .value(0.0)
        .height_request(8)
        .build();

    // ── Record / Stop button ───────────────────────────────────────────

    let record_btn = gtk::Button::builder()
        .label("Record")
        .halign(gtk::Align::Center)
        .width_request(160)
        .height_request(48)
        .build();
    record_btn.add_css_class("suggested-action");
    record_btn.add_css_class("pill");

    // ── Pause / Resume button ──────────────────────────────────────────

    let pause_btn = gtk::Button::builder()
        .label("Pause")
        .halign(gtk::Align::Center)
        .width_request(160)
        .height_request(48)
        .build();
    pause_btn.add_css_class("pill");
    pause_btn.set_sensitive(false);

    // ── Live transcript ────────────────────────────────────────────────

    let transcript_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    let transcript_view = gtk::TextView::builder()
        .buffer(&transcript_buffer)
        .editable(false)
        .cursor_visible(false)
        .wrap_mode(gtk::WrapMode::Word)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    transcript_view.add_css_class("card");

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&transcript_view)
        .build();

    // ── Pack into container ────────────────────────────────────────────

    container.append(&title_entry);
    container.append(&timer_label);
    container.append(&level_bar);
    container.append(&record_btn);
    container.append(&pause_btn);
    container.append(&scroll);

    let clamp = adw::Clamp::builder()
        .maximum_size(600)
        .child(&container)
        .build();

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    outer.append(&clamp);

    // ── Recording state and logic ──────────────────────────────────────

    let is_recording = Arc::new(Cell::new(false));
    let paused = Arc::new(Cell::new(false));
    let elapsed_secs = Arc::new(Cell::new(0u64));

    // Timer tick — updates the label and audio level every second while recording.
    {
        let timer = timer_label.clone();
        let level = level_bar.clone();
        let bridge_ref = bridge.clone();
        let recording = is_recording.clone();
        let secs = elapsed_secs.clone();

        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            if recording.get() {
                let s = secs.get() + 1;
                secs.set(s);
                let h = s / 3600;
                let m = (s % 3600) / 60;
                let sec = s % 60;
                timer.set_label(&format!("{h:02}:{m:02}:{sec:02}"));
                level.set_value(bridge_ref.audio_level() as f64);
            }
            glib::ControlFlow::Continue
        });
    }

    // Transcript receiver — drain incoming segments onto the text buffer.
    {
        let rx = bridge.subscribe_transcripts();
        let buf = transcript_buffer.clone();

        glib::spawn_future_local(async move {
            while let Ok(seg) = rx.recv().await {
                let mut end = buf.end_iter();
                let line = format!("[{:.1}s] {}\n", seg.timestamp_ms as f64 / 1000.0, seg.text);
                buf.insert(&mut end, &line);
            }
        });
    }

    // Pause/resume button handler.
    {
        let bridge_ref = bridge.clone();
        let pause_b = pause_btn.clone();
        let paused_c = paused.clone();

        pause_btn.connect_clicked(move |_| {
            let currently = paused_c.get();
            let bridge_c = bridge_ref.clone();
            let pause_bc = pause_b.clone();
            let paused_cc = paused_c.clone();

            glib::spawn_future_local(async move {
                if !currently {
                    match bridge_c.pause_recording().await {
                        Ok(()) => {
                            paused_cc.set(true);
                            pause_bc.set_label("Resume");
                        }
                        Err(e) => {
                            eprintln!("Failed to pause recording: {e}");
                        }
                    }
                } else {
                    match bridge_c.resume_recording().await {
                        Ok(()) => {
                            paused_cc.set(false);
                            pause_bc.set_label("Pause");
                        }
                        Err(e) => {
                            eprintln!("Failed to resume recording: {e}");
                        }
                    }
                }
            });
        });
    }

    // Record/stop button handler.
    {
        let recording = is_recording;
        let secs = elapsed_secs;
        let btn = record_btn.clone();
        let timer = timer_label;
        let buf = transcript_buffer;
        let level = level_bar;
        let bridge_ref = bridge;
        let pause_b = pause_btn;
        let paused_r = paused;

        record_btn.connect_clicked(move |_| {
            let currently = recording.get();

            if !currently {
                // Start recording.
                let bridge_c = bridge_ref.clone();
                let btn_c = btn.clone();
                let rec_c = recording.clone();
                let pause_sc = pause_b.clone();

                glib::spawn_future_local(async move {
                    match bridge_c.start_recording(None).await {
                        Ok(()) => {
                            rec_c.set(true);
                            btn_c.set_label("Stop");
                            btn_c.remove_css_class("suggested-action");
                            btn_c.add_css_class("destructive-action");
                            pause_sc.set_sensitive(true);
                        }
                        Err(e) => {
                            eprintln!("Failed to start recording: {e}");
                        }
                    }
                });
            } else {
                // Stop recording.
                let bridge_c = bridge_ref.clone();
                let btn_c = btn.clone();
                let rec_c = recording.clone();
                let secs_c = secs.clone();
                let timer_c = timer.clone();
                let buf_c = buf.clone();
                let level_c = level.clone();
                let pause_stc = pause_b.clone();
                let paused_stc = paused_r.clone();

                glib::spawn_future_local(async move {
                    match bridge_c.stop_recording().await {
                        Ok(meeting_id) => {
                            rec_c.set(false);
                            secs_c.set(0);
                            btn_c.set_label("Record");
                            btn_c.remove_css_class("destructive-action");
                            btn_c.add_css_class("suggested-action");
                            timer_c.set_label("00:00:00");
                            level_c.set_value(0.0);
                            pause_stc.set_sensitive(false);
                            pause_stc.set_label("Pause");
                            paused_stc.set(false);

                            // Clear transcript buffer for next session.
                            let (start, end) = buf_c.bounds();
                            buf_c.delete(&mut start.clone(), &mut end.clone());

                            eprintln!("Recording saved as meeting {meeting_id}");
                        }
                        Err(e) => {
                            eprintln!("Failed to stop recording: {e}");
                        }
                    }
                });
            }
        });
    }

    outer
}
