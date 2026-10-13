import AppKit

/// Ensures every window the app creates is excluded from screen capture and sharing.
///
/// SwiftUI creates windows lazily (e.g. the `MenuBarExtra` popover is created on first click),
/// so a one-shot sweep at launch is insufficient. This manager observes window lifecycle
/// notifications and applies `sharingType = .none` as soon as each window appears.
@MainActor
final class StealthManager {

    static let shared = StealthManager()

    private var observers: [NSObjectProtocol] = []

    private init() {}

    /// Start observing all window creation / activation events and apply stealth to existing windows.
    func activate() {
        // Sweep windows already in existence.
        applyStealthToAllWindows()

        // Watch for new or re-activated windows.
        let center = NotificationCenter.default
        let names: [Notification.Name] = [
            NSWindow.didBecomeKeyNotification,
            NSWindow.didBecomeMainNotification,
            NSApplication.didBecomeActiveNotification,
        ]
        for name in names {
            let observer = center.addObserver(
                forName: name,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in
                    self?.applyStealthToAllWindows()
                }
            }
            observers.append(observer)
        }
    }

    /// Remove all observers.
    func deactivate() {
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
        observers.removeAll()
    }

    /// Walk every NSWindow belonging to this application and set sharingType to .none.
    private func applyStealthToAllWindows() {
        for window in NSApp.windows {
            if window.sharingType != .none {
                window.sharingType = .none
            }
        }
    }
}
