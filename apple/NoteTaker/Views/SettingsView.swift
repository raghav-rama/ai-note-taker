import SwiftUI

/// Settings screen with audio device picker, transcription options, API keys, and model management.
struct SettingsView: View {
    @Environment(MeetingViewModel.self) private var viewModel
    @Environment(AudioManager.self) private var audioManager

    @State private var isDownloadingModel = false
    @State private var downloadProgress: Double = 0.0

    /// Available Whisper model sizes with approximate download sizes.
    private let modelOptions: [(name: String, label: String, size: String)] = [
        ("tiny", "Tiny", "~75 MB"),
        ("base", "Base", "~142 MB"),
        ("small", "Small", "~466 MB"),
        ("medium", "Medium", "~1.5 GB"),
    ]

    var body: some View {
        @Bindable var vm = viewModel
        Form {
            // Audio device
            Section("Audio Input") {
                Picker("Input Device", selection: Binding(
                    get: { audioManager.selectedDeviceId ?? "" },
                    set: { audioManager.selectedDeviceId = $0.isEmpty ? nil : $0 }
                )) {
                    Text("Default").tag("")
                    ForEach(audioManager.availableDevices) { device in
                        Text(device.name).tag(device.id)
                    }
                }

                HStack {
                    Image(systemName: audioManager.permissionGranted
                          ? "checkmark.circle.fill"
                          : "exclamationmark.triangle.fill")
                        .foregroundStyle(audioManager.permissionGranted ? .green : .orange)

                    Text(audioManager.permissionGranted
                         ? "Microphone access granted"
                         : "Microphone access required")
                        .font(.caption)

                    Spacer()

                    if !audioManager.permissionGranted {
                        Button("Request Access") {
                            Task {
                                await audioManager.requestPermission()
                            }
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }
            }

            // Transcription mode
            Section("Transcription") {
                Toggle("Use Cloud Transcription (OpenAI Whisper API)", isOn: $vm.useCloudTranscription)

                if viewModel.useCloudTranscription {
                    SecureField("OpenAI API Key", text: $vm.whisperApiKey)
                        .textFieldStyle(.roundedBorder)
                        .help("Your OpenAI API key for cloud-based transcription.")
                }
            }

            // Local model management
            Section("Local Model") {
                Picker("Whisper Model", selection: $vm.selectedModelName) {
                    ForEach(modelOptions, id: \.name) { option in
                        HStack {
                            Text(option.label)
                            Text(option.size)
                                .foregroundStyle(.secondary)
                                .font(.caption)
                        }
                        .tag(option.name)
                    }
                }

                if isDownloadingModel {
                    VStack(alignment: .leading, spacing: 4) {
                        ProgressView(value: downloadProgress) {
                            Text("Downloading model...")
                        }
                        Text("\(Int(downloadProgress * 100))%")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                } else {
                    Button("Download Selected Model") {
                        downloadModel()
                    }
                    .buttonStyle(.bordered)
                }
            }

            // Summarization API key
            Section("AI Summarization") {
                SecureField("Claude API Key", text: $vm.claudeApiKey)
                    .textFieldStyle(.roundedBorder)
                    .help("Your Anthropic API key for meeting summarization.")

                Text("Summaries are generated using the Claude API. Your API key is stored locally in the system keychain.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            // Appearance
            Section("Appearance") {
                TextField("App Display Name", text: $vm.appDisplayName)
                    .textFieldStyle(.roundedBorder)
                    .help("Name shown in the Dock and menu bar. Use a generic name for stealth.")

                Text("Changes take effect on next launch. Default: \"System Helper\"")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            // Data management
            Section("Data") {
                HStack {
                    Button("Export All Meetings") {
                        exportAllMeetings()
                    }

                    Button("Open Storage Folder") {
                        openStorageFolder()
                    }
                }
            }
        }
        .formStyle(.grouped)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onChange(of: viewModel.useCloudTranscription) { _, newValue in
            viewModel.saveSetting(key: "use_cloud", value: newValue ? "true" : "false")
        }
        .onChange(of: viewModel.selectedModelName) { _, newValue in
            viewModel.saveSetting(key: "whisper_model", value: newValue)
        }
        .onChange(of: viewModel.appDisplayName) { _, newValue in
            viewModel.saveSetting(key: "app_display_name", value: newValue)
        }
    }

    // MARK: - Actions

    private func downloadModel() {
        isDownloadingModel = true
        downloadProgress = 0.0

        // Simulate download progress; actual download is handled by the Rust core.
        Task {
            for i in 1...100 {
                try? await Task.sleep(for: .milliseconds(50))
                downloadProgress = Double(i) / 100.0
            }
            isDownloadingModel = false
        }
    }

    private func exportAllMeetings() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Export Here"
        panel.begin { response in
            guard response == .OK, let folder = panel.url else { return }
            for meeting in viewModel.meetings {
                if let content = viewModel.exportMeeting(id: meeting.id, format: "json") {
                    let safeName = meeting.title.replacingOccurrences(
                        of: "[^a-zA-Z0-9_-]",
                        with: "_",
                        options: .regularExpression
                    )
                    let file = folder.appendingPathComponent("\(safeName).json")
                    try? content.write(to: file, atomically: true, encoding: .utf8)
                }
            }
        }
    }

    private func openStorageFolder() {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!
        let appDir = appSupport.appendingPathComponent("com.system.helper")
        NSWorkspace.shared.open(appDir)
    }
}
