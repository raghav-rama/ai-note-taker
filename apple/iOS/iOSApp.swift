import SwiftUI
import AVFoundation

@main
struct UtilityApp: App {
    @StateObject private var coreBridge = CoreBridge()
    @StateObject private var audioManager = iOSAudioManager()
    @StateObject private var stealthManager = iOSStealthManager()

    init() {
        configureAudioSessionCategory()
    }

    var body: some Scene {
        WindowGroup {
            // Wrap the entire app in the capture-proof secure container.
            // Environment objects are applied inside the content closure so they
            // propagate through the nested UIHostingController.
            SecureRootView {
                ContentView()
                    .environmentObject(coreBridge)
                    .environmentObject(audioManager)
                    .environmentObject(stealthManager)
                    .onAppear {
                        initializeCore()
                    }
            }
            .ignoresSafeArea()
        }
    }

    // MARK: - Private

    /// Set the audio session category at launch. Do NOT activate the session
    /// here -- activation happens when recording starts, after permission is
    /// granted. Activating a .record session before permission is pointless.
    private func configureAudioSessionCategory() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.record, mode: .default, options: [])
            try session.setPreferredSampleRate(16_000)
        } catch {
            print("Audio session category configuration failed: \(error)")
        }
    }

    private func initializeCore() {
        let documentsURL = FileManager.default.urls(
            for: .documentDirectory, in: .userDomainMask
        ).first!
        let dbPath = documentsURL.appendingPathComponent("store.db").path
        let modelPath = documentsURL.appendingPathComponent("models").path

        // Ensure model directory exists.
        try? FileManager.default.createDirectory(
            atPath: modelPath, withIntermediateDirectories: true
        )

        let success = coreBridge.initialize(dbPath: dbPath, modelPath: modelPath)
        if !success {
            print("Core library initialization failed")
        }
    }
}

// MARK: - Root Content View (Tab-based navigation)

struct ContentView: View {
    @EnvironmentObject var stealthManager: iOSStealthManager

    var body: some View {
        TabView {
            iOSRecordingView()
                .tabItem {
                    Label("Record", systemImage: "waveform")
                }
            iOSHistoryView()
                .tabItem {
                    Label("History", systemImage: "clock")
                }
            iOSSettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gear")
                }
        }
        .safeAreaInset(edge: .top) {
            // Non-blocking banner when screen capture is active.
            // The secure container already hides content from the capture;
            // this banner informs the user without blocking the UI.
            if stealthManager.isScreenBeingCaptured {
                screenRecordingBanner
            }
        }
    }

    private var screenRecordingBanner: some View {
        HStack(spacing: 8) {
            Image(systemName: "eye.slash.fill")
                .foregroundStyle(.white)
            Text("Screen recording active -- content is protected")
                .font(.caption.bold())
                .foregroundStyle(.white)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .padding(.horizontal, 16)
        .background(.red.opacity(0.85))
    }
}
