import Foundation
import AVFoundation
import Combine

/// Manages iOS audio session lifecycle, permissions, and interruption handling.
///
/// The actual audio capture is performed by the Rust core via its own
/// AVAudioEngine input tap. This manager's responsibilities are:
///   - Requesting microphone permission (iOS 17+ API)
///   - Configuring AVAudioSession category and mode
///   - Handling audio interruptions (phone calls, alarms)
///   - Handling route changes (headphones plugged/unplugged)
@MainActor
final class iOSAudioManager: ObservableObject {

    enum PermissionState {
        case undetermined
        case granted
        case denied
    }

    @Published private(set) var permissionState: PermissionState = .undetermined
    @Published private(set) var isInterrupted = false
    @Published private(set) var currentRoute: String = ""

    /// Called when an audio interruption begins (e.g. phone call). Wire this
    /// to pause recording in the core.
    var onInterruptionBegan: (() -> Void)?

    /// Called when an interruption ends and the system says audio should resume.
    var onInterruptionEnded: (() -> Void)?

    private var cancellables = Set<AnyCancellable>()

    init() {
        updatePermissionState()
        updateRouteDescription()
        observeInterruptions()
        observeRouteChanges()
    }

    // MARK: - Permission

    /// Request microphone access using the iOS 17+ API.
    func requestPermission() async -> Bool {
        let granted = await AVAudioApplication.requestRecordPermission()
        permissionState = granted ? .granted : .denied
        return granted
    }

    // MARK: - Session activation

    func activateSession() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .default, options: [])
        try session.setPreferredSampleRate(16_000)
        try session.setActive(true, options: [])
        updateRouteDescription()
    }

    func deactivateSession() {
        let session = AVAudioSession.sharedInstance()
        try? session.setActive(false, options: [.notifyOthersOnDeactivation])
    }

    // MARK: - Private

    private func updatePermissionState() {
        switch AVAudioApplication.shared.recordPermission {
        case .granted:
            permissionState = .granted
        case .denied:
            permissionState = .denied
        case .undetermined:
            permissionState = .undetermined
        @unknown default:
            permissionState = .undetermined
        }
    }

    private func updateRouteDescription() {
        let route = AVAudioSession.sharedInstance().currentRoute
        let inputs = route.inputs.map(\.portName).joined(separator: ", ")
        currentRoute = inputs.isEmpty ? "No input" : inputs
    }

    private func observeInterruptions() {
        NotificationCenter.default
            .publisher(for: AVAudioSession.interruptionNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] notification in
                self?.handleInterruption(notification)
            }
            .store(in: &cancellables)
    }

    private func observeRouteChanges() {
        NotificationCenter.default
            .publisher(for: AVAudioSession.routeChangeNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.updateRouteDescription()
            }
            .store(in: &cancellables)
    }

    private func handleInterruption(_ notification: Notification) {
        guard let userInfo = notification.userInfo,
              let typeValue = userInfo[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: typeValue) else {
            return
        }

        switch type {
        case .began:
            isInterrupted = true
            onInterruptionBegan?()

        case .ended:
            isInterrupted = false
            if let optionsValue = userInfo[AVAudioSessionInterruptionOptionKey] as? UInt {
                let options = AVAudioSession.InterruptionOptions(rawValue: optionsValue)
                if options.contains(.shouldResume) {
                    try? activateSession()
                    onInterruptionEnded?()
                }
            }

        @unknown default:
            break
        }
    }
}
