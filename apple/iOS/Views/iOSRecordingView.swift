import SwiftUI

struct iOSRecordingView: View {
    @EnvironmentObject var coreBridge: CoreBridge
    @EnvironmentObject var audioManager: iOSAudioManager

    @State private var recordingStartTime: Date?
    @State private var meetingTitle = ""
    @State private var showPermissionAlert = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Timer
                timerSection
                    .padding(.top, 16)

                // Title
                titleField
                    .padding(.horizontal, 16)
                    .padding(.top, 12)

                // Record button
                recordButton
                    .padding(.top, 24)

                // Audio route info
                audioRouteInfo
                    .padding(.top, 12)

                // Live transcript
                transcriptSection
                    .padding(.top, 16)

                Spacer(minLength: 0)
            }
            .navigationTitle("Record")
            .navigationBarTitleDisplayMode(.inline)
            .onAppear {
                audioManager.onInterruptionBegan = { [coreBridge] in
                    _ = coreBridge.pauseRecording()
                }
                audioManager.onInterruptionEnded = { [coreBridge] in
                    _ = coreBridge.startRecording()
                }
            }
            .alert("Microphone Access Required",
                   isPresented: $showPermissionAlert) {
                Button("Open Settings") {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        UIApplication.shared.open(url)
                    }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("This app needs microphone access to capture audio. Please enable it in Settings.")
            }
        }
    }

    // MARK: - Timer

    private var timerSection: some View {
        Group {
            if coreBridge.isRecording, let start = recordingStartTime {
                TimelineView(.periodic(from: start, by: 1)) { context in
                    let elapsed = context.date.timeIntervalSince(start)
                    Text(formatDuration(elapsed))
                        .font(.system(size: 48, weight: .light, design: .monospaced))
                        .foregroundStyle(coreBridge.isPaused ? .orange : .red)
                }
            } else {
                Text("00:00:00")
                    .font(.system(size: 48, weight: .light, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Title

    private var titleField: some View {
        TextField("Session title", text: $meetingTitle)
            .font(.headline)
            .textFieldStyle(.roundedBorder)
            .disabled(coreBridge.isRecording)
    }

    // MARK: - Record button

    private var recordButton: some View {
        Button {
            Task { await toggleRecording() }
        } label: {
            ZStack {
                Circle()
                    .fill(coreBridge.isRecording ? .red.opacity(0.15) : .red.opacity(0.1))
                    .frame(width: 96, height: 96)

                if coreBridge.isRecording {
                    RoundedRectangle(cornerRadius: 6)
                        .fill(.red)
                        .frame(width: 32, height: 32)
                } else {
                    Circle()
                        .fill(.red)
                        .frame(width: 64, height: 64)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(coreBridge.isRecording ? "Stop recording" : "Start recording")
    }

    // MARK: - Audio route

    private var audioRouteInfo: some View {
        HStack(spacing: 6) {
            Image(systemName: "mic")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(audioManager.currentRoute)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Live transcript

    private var transcriptSection: some View {
        Group {
            if coreBridge.liveSegments.isEmpty && !coreBridge.isRecording {
                VStack(spacing: 8) {
                    Image(systemName: "text.quote")
                        .font(.largeTitle)
                        .foregroundStyle(.quaternary)
                    Text("Tap the record button to begin")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 8) {
                            ForEach(coreBridge.liveSegments) { segment in
                                HStack(alignment: .top, spacing: 8) {
                                    Text(formatTimestamp(segment.timestamp))
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.secondary)
                                        .frame(width: 52, alignment: .trailing)
                                    Text(segment.text)
                                        .font(.body)
                                }
                                .id(segment.id)
                            }
                        }
                        .padding(.horizontal, 16)
                    }
                    .onChange(of: coreBridge.liveSegments.count) { _, _ in
                        if let last = coreBridge.liveSegments.last {
                            withAnimation {
                                proxy.scrollTo(last.id, anchor: .bottom)
                            }
                        }
                    }
                }
            }
        }
    }

    // MARK: - Actions

    private func toggleRecording() async {
        if coreBridge.isRecording {
            _ = coreBridge.stopRecording()
            recordingStartTime = nil
        } else {
            // Check permission first.
            if audioManager.permissionState == .denied {
                showPermissionAlert = true
                return
            }
            if audioManager.permissionState == .undetermined {
                let granted = await audioManager.requestPermission()
                if !granted {
                    showPermissionAlert = true
                    return
                }
            }

            do {
                try audioManager.activateSession()
            } catch {
                print("Failed to activate audio session: \(error)")
                return
            }

            let started = coreBridge.startRecording()
            if started {
                recordingStartTime = Date()
            }
        }
    }

    // MARK: - Formatting

    private func formatDuration(_ interval: TimeInterval) -> String {
        let total = Int(interval)
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        return String(format: "%02d:%02d:%02d", hours, minutes, seconds)
    }

    private func formatTimestamp(_ seconds: Double) -> String {
        let total = Int(seconds)
        let m = total / 60
        let s = total % 60
        return String(format: "%d:%02d", m, s)
    }
}
