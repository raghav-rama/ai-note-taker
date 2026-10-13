import SwiftUI

/// Application delegate that activates stealth mode (screen capture exclusion) on launch.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        StealthManager.shared.activate()
    }

    func applicationWillTerminate(_ notification: Notification) {
        StealthManager.shared.deactivate()
    }
}

@main
struct NoteTakerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate

    @State private var viewModel: MeetingViewModel
    @State private var audioManager: AudioManager

    init() {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!
        let appDir = appSupport.appendingPathComponent("com.system.helper", isDirectory: true)
        try? FileManager.default.createDirectory(at: appDir, withIntermediateDirectories: true)

        let dbPath = appDir.appendingPathComponent("meetings.db").path
        let modelPath = appDir.appendingPathComponent("models/ggml-tiny.bin").path

        let core = CoreBridge(dbPath: dbPath, modelPath: modelPath)
        _viewModel = State(initialValue: MeetingViewModel(core: core))
        _audioManager = State(initialValue: AudioManager(core: core))
    }

    var body: some Scene {
        // Main application window.
        WindowGroup {
            ContentView()
                .environment(viewModel)
                .environment(audioManager)
                .frame(minWidth: 700, minHeight: 500)
        }
        .defaultSize(width: 900, height: 650)

        // Menu bar presence for quick access.
        MenuBarExtra("System Helper", systemImage: "waveform") {
            MenuBarView()
                .environment(viewModel)
                .environment(audioManager)
        }
        .menuBarExtraStyle(.window)
    }
}

/// Compact view shown from the menu bar icon.
struct MenuBarView: View {
    @Environment(MeetingViewModel.self) private var viewModel
    @Environment(AudioManager.self) private var audioManager

    var body: some View {
        VStack(spacing: 12) {
            HStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 8, height: 8)
                Text(statusText)
                    .font(.headline)
                Spacer()
            }

            if viewModel.recordingState == .recording {
                Text(formattedTime(viewModel.elapsedSeconds))
                    .font(.system(.title2, design: .monospaced))
                    .frame(maxWidth: .infinity)
            }

            HStack(spacing: 12) {
                if viewModel.recordingState == .idle {
                    Button("Start Recording") {
                        viewModel.startRecording(deviceId: audioManager.selectedDeviceId)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                } else {
                    Button("Stop") {
                        viewModel.stopRecording()
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                }

                Button("Quit") {
                    NSApplication.shared.terminate(nil)
                }
            }
        }
        .padding()
        .frame(width: 260)
    }

    private var statusColor: Color {
        switch viewModel.recordingState {
        case .idle: return .secondary
        case .recording: return .red
        case .paused: return .orange
        }
    }

    private var statusText: String {
        switch viewModel.recordingState {
        case .idle: return "Ready"
        case .recording: return "Recording"
        case .paused: return "Paused"
        }
    }

    private func formattedTime(_ totalSeconds: Int) -> String {
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%02d:%02d", minutes, seconds)
    }
}
