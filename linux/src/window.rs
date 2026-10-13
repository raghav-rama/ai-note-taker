use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::sync::Arc;

use crate::core_bridge::CoreBridge;
use crate::views;

// ── GObject subclass boilerplate ───────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct AppWindow {
        pub nav_view: RefCell<Option<adw::NavigationView>>,
        pub bridge: RefCell<Option<Arc<CoreBridge>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppWindow {
        const NAME: &'static str = "SmAppWindow";
        type Type = super::AppWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for AppWindow {}
    impl WidgetImpl for AppWindow {}
    impl WindowImpl for AppWindow {}
    impl ApplicationWindowImpl for AppWindow {}
    impl AdwApplicationWindowImpl for AppWindow {}
}

glib::wrapper! {
    pub struct AppWindow(ObjectSubclass<imp::AppWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget,
                    gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl AppWindow {
    pub fn new(app: &adw::Application, bridge: Arc<CoreBridge>) -> Self {
        let win: Self = glib::Object::builder().property("application", app).build();

        apply_capture_exclusion_hints(&win);

        let imp = win.imp();
        imp.bridge.replace(Some(bridge.clone()));

        win.set_default_size(420, 720);
        // Generic title — stealth: nothing identifies this as a note-taker.
        win.set_title(Some("Session Monitor"));
        win.set_hide_on_close(true);

        // ── Navigation structure ───────────────────────────────────────

        let nav_view = adw::NavigationView::new();

        // ── Bottom navigation via AdwViewStack + AdwViewSwitcherBar ────

        let view_stack = adw::ViewStack::new();
        view_stack.add_titled_with_icon(
            &views::recording::build(bridge.clone()),
            Some("recording"),
            "Record",
            "media-record-symbolic",
        );
        view_stack.add_titled_with_icon(
            &views::history::build(bridge.clone()),
            Some("history"),
            "History",
            "document-open-recent-symbolic",
        );
        view_stack.add_titled_with_icon(
            &views::notes::build(bridge.clone()),
            Some("notes"),
            "Notes",
            "document-edit-symbolic",
        );
        view_stack.add_titled_with_icon(
            &views::settings::build(bridge.clone()),
            Some("settings"),
            "Settings",
            "emblem-system-symbolic",
        );

        let switcher_bar = adw::ViewSwitcherBar::new();
        switcher_bar.set_stack(Some(&view_stack));
        switcher_bar.set_reveal(true);

        let header = adw::HeaderBar::new();
        let switcher_title = adw::ViewSwitcherTitle::new();
        switcher_title.set_stack(Some(&view_stack));
        header.set_title_widget(Some(&switcher_title));

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&view_stack));
        toolbar_view.add_bottom_bar(&switcher_bar);

        // Wire reveal of bottom bar to title squeeze state.
        let bar = switcher_bar.clone();
        switcher_title.connect_title_visible_notify(move |title| {
            bar.set_reveal(title.is_title_visible());
        });

        // Build the single navigation page with the toolbar view.
        let main_page = adw::NavigationPage::builder()
            .tag("main")
            .title("Session Monitor")
            .child(&toolbar_view)
            .build();

        nav_view.push(&main_page);
        imp.nav_view.replace(Some(nav_view.clone()));

        win.set_content(Some(&nav_view));
        win
    }

    /// Access the bridge from anywhere that holds a window reference.
    pub fn bridge(&self) -> Arc<CoreBridge> {
        self.imp()
            .bridge
            .borrow()
            .as_ref()
            .expect("bridge not initialized")
            .clone()
    }
}

/// Best-effort screen capture exclusion.
///
/// GTK4 does not expose an API to exclude a window from screen capture.
/// Wayland compositors generally do not support capture exclusion either.
/// On X11, there is no standard mechanism.
///
/// What we *can* do:
/// - Set a generic window title and application ID (done above).
/// - Enable hide-on-close so the window can vanish quickly.
/// - In the future, an X11-specific path could attempt a compositor hint,
///   but this would require the `gdk4-x11` crate and is unreliable.
fn apply_capture_exclusion_hints(win: &AppWindow) {
    // Setting the generic WM_CLASS via the application ID is already handled
    // by using "org.freedesktop.SessionMonitor" as the app ID.
    //
    // On GNOME/Mutter, there is no API to request capture exclusion.
    // On KDE/KWin, there is no standard D-Bus interface for this.
    // On wlroots compositors, same story.
    //
    // This function exists as a placeholder for future compositor-specific
    // exclusion logic if any becomes available.
    let _ = win;
}
