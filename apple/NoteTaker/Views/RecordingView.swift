import SwiftUI

/// The main recording screen with a record/stop button, live transcript, timer, and audio level.
struct RecordingView: View {
    @Environment(MeetingViewModel.self) private var viewModel
    @Environment(AudioManager.self) private var audioManager

    var body: some View {
        VStack(spacing: 0) {
            // Header: title + timer
            headerSection
                .padding(.horizontal)
                .padding(.top, 16)

            Divider()
                .padding(.top, 12)

            // Audio level waveform indicator
            audioLevelIndicator
                .padding(.horizontal)
                .padding(.top, 12)

            // Live transcript
            transcriptSection
                .padding(.horizontal)
                .padding(.top, 8)

            Spacer(minLength: 0)

            // Record/stop button
            controlsSection
                .padding(.bottom, 24)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task {
            if !audioManager.permissionDetermined {
                await audioManager.requestPermission()
            }
        }
    }

    // MARK: - Header

    @ViewBuilder
    private var headerSection: some View {
        @Bindable var vm = viewModel
        VStack(spacing: 8) {
            TextField("Meeting Title", text: $vm.meetingTitle)
                .textFieldStyle(.plain)
                .font(.title2.bold())
                .multilineTextAlignment(.center)

            if viewModel.recordingState == .recording || viewModel.recordingState == .paused {
                Text(formattedTime(viewModel.elapsedSeconds))
                    .font(.system(.title, design: .monospaced))
                    .foregroundStyle(
                        viewModel.recordingState == .recording ? .primary : .secondary
                    )
                    .contentTransition(.numericText())
                    .animation(.default, value: viewModel.elapsedSeconds)
            }
        }
    }

    // MARK: - Audio level

    @ViewBuilder
    private var audioLevelIndicator: some View {
        if viewModel.recordingState == .recording {
            HStack(spacing: 2) {
                ForEach(0..<30, id: \.self) { index in
                    RoundedRectangle(cornerRadius: 1)
                        .fill(barColor(for: index))
                        .frame(width: 4, height: barHeight(for: index))
                }
            }
            .frame(height: 32)
            .animation(.easeOut(duration: 0.1), value: audioManager.audioLevel)
        }
    }

    private func barHeight(for index: Int) -> CGFloat {
        let normalized = CGFloat(audioManager.audioLevel)
        let position = CGFloat(index) / 30.0
        // Create a wave-like visualization.
        let wave = sin(position * .pi) * normalized
        return max(4, wave * 28 + 4)
    }

    private func barColor(for index: Int) -> Color {
        let position = CGFloat(index) / 30.0
        if position < 0.6 {
            return .green
        } else if position < 0.85 {
            return .yellow
        } else {
            return .red
        }
    }

    // MARK: - Transcript

    @ViewBuilder
    private var transcriptSection: some View {
        if viewModel.transcriptSegments.isEmpty && viewModel.recordingState == .idle {
            ContentUnavailableView(
                "No Transcript",
                systemImage: "text.bubble",
                description: Text("Press the record button to start capturing audio.")
            )
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(viewModel.transcriptSegments) { segment in
                            HStack(alignment: .top, spacing: 8) {
                                Text(formattedTimestamp(segment.timestamp))
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .frame(width: 60, alignment: .trailing)

                                Text(segment.text)
                                    .font(.body)
                                    .textSelection(.enabled)
                            }
                            .id(segment.id)
                        }
                    }
                    .padding(.vertical, 8)
                }
                .onChange(of: viewModel.transcriptSegments.count) {
                    if let last = viewModel.transcriptSegments.last {
                        withAnimation {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }
        }
    }

    // MARK: - Controls

    @ViewBuilder
    private var controlsSection: some View {
        VStack(spacing: 12) {
            if !audioManager.permissionGranted && audioManager.permissionDetermined {
                Label("Microphone access denied. Open System Settings to grant permission.",
                      systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.orange)
                    .font(.caption)
            }

            HStack(spacing: 24) {
                if viewModel.recordingState == .recording {
                    // Pause button
                    Button {
                        viewModel.pauseRecording()
                    } label: {
                        Image(systemName: "pause.circle.fill")
                            .font(.system(size: 36))
                            .foregroundStyle(.orange)
                    }
                    .buttonStyle(.plain)
                    .help("Pause recording")
                }

                // Main record/stop button
                Button {
                    switch viewModel.recordingState {
                    case .idle:
                        viewModel.startRecording(deviceId: audioManager.selectedDeviceId)
                    case .recording, .paused:
                        viewModel.stopRecording()
                    }
                } label: {
                    ZStack {
                        Circle()
                            .fill(viewModel.recordingState == .idle ? .red : .red.opacity(0.8))
                            .frame(width: 64, height: 64)

                        if viewModel.recordingState == .idle {
                            Circle()
                                .fill(.white)
                                .frame(width: 24, height: 24)
                        } else {
                            RoundedRectangle(cornerRadius: 4)
                                .fill(.white)
                                .frame(width: 22, height: 22)
                        }
                    }
                    .shadow(color: .red.opacity(0.3), radius: viewModel.recordingState == .recording ? 12 : 0)
                    .animation(
                        viewModel.recordingState == .recording
                            ? .easeInOut(duration: 1.0).repeatForever(autoreverses: true)
                            : .default,
                        value: viewModel.recordingState
                    )
                }
                .buttonStyle(.plain)
                .disabled(!audioManager.permissionGranted)
                .help(viewModel.recordingState == .idle ? "Start recording" : "Stop recording")

                if viewModel.recordingState == .paused {
                    // Resume button
                    Button {
                        viewModel.resumeRecording(deviceId: audioManager.selectedDeviceId)
                    } label: {
                        Image(systemName: "play.circle.fill")
                            .font(.system(size: 36))
                            .foregroundStyle(.green)
                    }
                    .buttonStyle(.plain)
                    .help("Resume recording")
                }
            }
        }
    }

    // MARK: - Formatting

    private func formattedTime(_ totalSeconds: Int) -> String {
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%02d:%02d", minutes, seconds)
    }

    private func formattedTimestamp(_ seconds: Double) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%02d:%02d", mins, secs)
    }
}
