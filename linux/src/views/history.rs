use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::core_bridge::CoreBridge;

/// Build the meeting history view.
///
/// Layout:
/// - Search entry at the top
/// - Scrolled list of meeting rows (AdwActionRow inside GtkListBox)
/// - Each row shows title, date, and duration
pub fn build(bridge: Arc<CoreBridge>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    // ── Search entry ───────────────────────────────────────────────────

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search meetings...")
        .build();

    // ── Meeting list ───────────────────────────────────────────────────

    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list_box.add_css_class("boxed-list");

    // Placeholder when empty.
    let placeholder = gtk::Label::builder()
        .label("No meetings yet.\nRecord a meeting to see it here.")
        .justify(gtk::Justification::Center)
        .margin_top(32)
        .build();
    placeholder.add_css_class("dim-label");
    list_box.set_placeholder(Some(&placeholder));

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&list_box)
        .build();

    container.append(&search_entry);
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

    // ── Load meetings asynchronously ───────────────────────────────────

    {
        let list = list_box.clone();
        let bridge_ref = bridge.clone();
        // Monotonic token bumped per load; a result whose token is stale (a later
        // keystroke superseded it) is discarded, so fast typing can't interleave
        // out-of-order results or clear the list for a query no longer in the box.
        let load_gen = Rc::new(Cell::new(0u64));

        let load_meetings = move |search_text: String| {
            let list_c = list.clone();
            let bridge_c = bridge_ref.clone();
            let gen_c = load_gen.clone();
            let my_gen = load_gen.get().wrapping_add(1);
            load_gen.set(my_gen);

            glib::spawn_future_local(async move {
                // Non-empty query → real FTS5 search; empty → list all.
                let result = if search_text.trim().is_empty() {
                    bridge_c.list_meetings(50, 0).await
                } else {
                    bridge_c.search_meetings(&search_text).await
                };

                // A newer load superseded this one — discard without touching the list.
                if gen_c.get() != my_gen {
                    return;
                }

                // Clear and repopulate only for the latest query.
                while let Some(child) = list_c.first_child() {
                    list_c.remove(&child);
                }

                match result {
                    Ok(meetings) => {
                        for meeting in meetings {
                            let duration_str = meeting
                                .duration_secs
                                .map(|s| {
                                    let m = s / 60;
                                    let sec = s % 60;
                                    format!("{m}m {sec}s")
                                })
                                .unwrap_or_else(|| "In progress".to_string());

                            let subtitle = format!("{} - {}", meeting.started_at, duration_str);

                            let row = adw::ActionRow::builder()
                                .title(&meeting.title)
                                .subtitle(&subtitle)
                                .activatable(true)
                                .build();

                            // Chevron to indicate the row is tappable.
                            let arrow = gtk::Image::from_icon_name("go-next-symbolic");
                            row.add_suffix(&arrow);

                            // On activation, switch to the Notes page and ask it
                            // to load this meeting. The Notes view installs a
                            // "notes.load" action taking the meeting id.
                            let mid = meeting.id;
                            row.connect_activated(move |row| {
                                if let Some(stack) = row
                                    .ancestor(adw::ViewStack::static_type())
                                    .and_then(|w| w.downcast::<adw::ViewStack>().ok())
                                {
                                    if let Some(notes) = stack.child_by_name("notes") {
                                        let _ = notes.activate_action(
                                            "notes.load",
                                            Some(&mid.to_variant()),
                                        );
                                    }
                                    stack.set_visible_child_name("notes");
                                }
                            });

                            list_c.append(&row);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load meetings: {e}");
                    }
                }
            });
        };

        // Initial load.
        let loader = load_meetings.clone();
        loader(String::new());

        // Re-filter on search text change.
        search_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            load_meetings(text);
        });
    }

    outer
}
