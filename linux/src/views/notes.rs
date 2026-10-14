use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::core_bridge::CoreBridge;

/// Build the notes view for a meeting's AI-generated summary.
///
/// Layout:
/// - Meeting selector (or shows most recent by default)
/// - Summary text at top
/// - Collapsible sections via GtkExpander:
///   - Key Points
///   - Action Items
///   - Decisions
/// - Full transcript in an expander at the bottom
/// - Export button
pub fn build(bridge: Arc<CoreBridge>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    // ── Summary section ────────────────────────────────────────────────

    let summary_label = gtk::Label::builder()
        .label("Select a meeting from History to view notes.")
        .wrap(true)
        .xalign(0.0)
        .build();
    summary_label.add_css_class("dim-label");

    let summary_frame = gtk::Frame::builder()
        .label("Summary")
        .child(&summary_label)
        .build();

    // ── Collapsible sections ───────────────────────────────────────────

    let key_points_label = gtk::Label::builder()
        .label("")
        .wrap(true)
        .xalign(0.0)
        .build();
    let key_points = gtk::Expander::builder()
        .label("Key Points")
        .child(&key_points_label)
        .build();

    let action_items_label = gtk::Label::builder()
        .label("")
        .wrap(true)
        .xalign(0.0)
        .build();
    let action_items = gtk::Expander::builder()
        .label("Action Items")
        .child(&action_items_label)
        .build();

    let decisions_label = gtk::Label::builder()
        .label("")
        .wrap(true)
        .xalign(0.0)
        .build();
    let decisions = gtk::Expander::builder()
        .label("Decisions")
        .child(&decisions_label)
        .build();

    // ── Full transcript expander ───────────────────────────────────────

    let transcript_label = gtk::Label::builder()
        .label("")
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .build();
    let transcript_expander = gtk::Expander::builder()
        .label("Full Transcript")
        .child(&transcript_label)
        .build();

    // ── Action buttons ─────────────────────────────────────────────────

    let button_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();

    let generate_btn = gtk::Button::builder().label("Generate Summary").build();
    generate_btn.add_css_class("suggested-action");
    generate_btn.add_css_class("pill");

    let export_btn = gtk::Button::builder().label("Export").build();
    export_btn.add_css_class("pill");

    button_box.append(&generate_btn);
    button_box.append(&export_btn);

    // ── Pack ───────────────────────────────────────────────────────────

    container.append(&summary_frame);
    container.append(&key_points);
    container.append(&action_items);
    container.append(&decisions);
    container.append(&transcript_expander);
    container.append(&button_box);

    let clamp = adw::Clamp::builder()
        .maximum_size(600)
        .child(&container)
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&clamp)
        .build();

    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .build();
    outer.append(&scroll);

    // ── State: current meeting ID ──────────────────────────────────────

    let current_meeting_id: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
    // Monotonic token bumped on every `notes.load`. Async tasks capture it and
    // bail if it advanced, so any later activation — even re-selecting the SAME
    // meeting — invalidates an in-flight load or summary rather than letting a
    // slow result overwrite the current view.
    let load_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    // "load" action — invoked from the History view when a meeting is selected.
    // It carries the meeting id as an int64 parameter, sets it as the current
    // meeting, and loads the meeting's title and transcript into the view.
    {
        let bridge_ref = bridge.clone();
        let meeting_id = current_meeting_id.clone();
        let load_gen = load_gen.clone();
        let summary = summary_label.clone();
        let transcript = transcript_label.clone();
        let kp = key_points_label.clone();
        let ai = action_items_label.clone();
        let dec = decisions_label.clone();

        let actions = gio::SimpleActionGroup::new();
        let load_action = gio::SimpleAction::new("load", Some(glib::VariantTy::INT64));
        load_action.connect_activate(move |_, param| {
            let Some(mid) = param.and_then(|v| v.get::<i64>()) else {
                return;
            };
            let gen = load_gen.get().wrapping_add(1);
            load_gen.set(gen);
            *meeting_id.borrow_mut() = Some(mid);

            // Clear sections parsed from any previously viewed meeting.
            kp.set_label("");
            ai.set_label("");
            dec.set_label("");

            let bridge_c = bridge_ref.clone();
            let summary_c = summary.clone();
            let transcript_c = transcript.clone();
            let gen_c = load_gen.clone();
            glib::spawn_future_local(async move {
                let meeting = bridge_c.get_meeting(mid).await;
                // Discard if a newer load activation has happened since (another
                // meeting, or this same meeting re-selected), so a slow fetch
                // can't clobber the current view.
                if gen_c.get() != gen {
                    return;
                }
                match meeting {
                    Ok(meeting) => {
                        summary_c.set_label(&format!(
                            "{}\n\nPress \u{201c}Generate Summary\u{201d} to create notes.",
                            meeting.title
                        ));
                        summary_c.remove_css_class("dim-label");
                    }
                    Err(e) => summary_c.set_label(&format!("Failed to load meeting: {e}")),
                }

                let transcript = bridge_c.get_transcript(mid).await;
                if gen_c.get() != gen {
                    return;
                }
                match transcript {
                    Ok(segments) => {
                        let text = segments
                            .iter()
                            .map(|s| {
                                format!("[{:.1}s] {}", s.timestamp_ms as f64 / 1000.0, s.text)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        transcript_c.set_label(&text);
                    }
                    Err(e) => transcript_c.set_label(&format!("Failed to load transcript: {e}")),
                }
            });
        });
        actions.add_action(&load_action);
        outer.insert_action_group("notes", Some(&actions));
    }

    // Generate summary button handler.
    {
        let bridge_ref = bridge.clone();
        let meeting_id = current_meeting_id.clone();
        let load_gen = load_gen.clone();
        let summary = summary_label.clone();
        let kp = key_points_label.clone();
        let ai = action_items_label.clone();
        let dec = decisions_label.clone();

        generate_btn.connect_clicked(move |btn| {
            let mid = *meeting_id.borrow();
            let Some(mid) = mid else {
                summary.set_label("No meeting selected. Go to History and select one.");
                return;
            };

            btn.set_sensitive(false);
            let gen = load_gen.get();
            let bridge_c = bridge_ref.clone();
            let summary_c = summary.clone();
            let kp_c = kp.clone();
            let ai_c = ai.clone();
            let dec_c = dec.clone();
            let btn_c = btn.clone();
            let gen_c = load_gen.clone();

            glib::spawn_future_local(async move {
                let result = bridge_c.summarize(mid).await;
                // Discard if any load activation happened after this summary was
                // requested (another meeting, or this one re-selected), so it
                // can't overwrite the now-current meeting's view while Export
                // would operate on the newer selection.
                if gen_c.get() != gen {
                    btn_c.set_sensitive(true);
                    return;
                }
                match result {
                    Ok(text) => {
                        // The summary is expected as plain text. A more
                        // sophisticated implementation would parse structured
                        // sections from the AI output.
                        summary_c.set_label(&text);
                        summary_c.remove_css_class("dim-label");

                        // Parse basic sections if the summary uses markdown-style headers.
                        let sections = parse_summary_sections(&text);
                        kp_c.set_label(&sections.key_points);
                        ai_c.set_label(&sections.action_items);
                        dec_c.set_label(&sections.decisions);
                    }
                    Err(e) => {
                        summary_c.set_label(&format!("Error generating summary: {e}"));
                    }
                }
                btn_c.set_sensitive(true);
            });
        });
    }

    // Export button handler.
    {
        let bridge_ref = bridge;
        let meeting_id = current_meeting_id;

        export_btn.connect_clicked(move |_| {
            let mid = *meeting_id.borrow();
            let Some(mid) = mid else {
                return;
            };

            let bridge_c = bridge_ref.clone();
            glib::spawn_future_local(async move {
                match bridge_c.export_meeting(mid, "markdown").await {
                    Ok(content) => {
                        eprintln!("Exported meeting {mid}:\n{content}");
                        // TODO: save to file via GtkFileDialog
                    }
                    Err(e) => {
                        eprintln!("Export failed: {e}");
                    }
                }
            });
        });
    }

    outer
}

/// Naively parsed summary sections from AI output that may contain markdown
/// headers like `## Key Points`, `## Action Items`, `## Decisions`.
struct SummarySections {
    key_points: String,
    action_items: String,
    decisions: String,
}

fn parse_summary_sections(text: &str) -> SummarySections {
    let mut key_points = Vec::new();
    let mut action_items = Vec::new();
    let mut decisions = Vec::new();
    let mut current: Option<&mut Vec<&str>> = None;

    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("key point") {
            current = Some(&mut key_points);
            continue;
        } else if lower.contains("action item") {
            current = Some(&mut action_items);
            continue;
        } else if lower.contains("decision") {
            current = Some(&mut decisions);
            continue;
        }

        if let Some(ref mut section) = current {
            if !line.trim().is_empty() {
                section.push(line);
            }
        }
    }

    SummarySections {
        key_points: key_points.join("\n"),
        action_items: action_items.join("\n"),
        decisions: decisions.join("\n"),
    }
}
