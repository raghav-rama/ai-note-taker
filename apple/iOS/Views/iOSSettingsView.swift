import SwiftUI
import Security

struct iOSSettingsView: View {
    @EnvironmentObject var coreBridge: CoreBridge
    @EnvironmentObject var audioManager: iOSAudioManager

    // Transcription settings
    @State private var useCloudTranscription = false
    @State private var selectedModel = "tiny"
    @State private var selectedLanguage = "auto"

    // API keys (stored in Keychain, NOT in SQLite settings)
    @State private var whisperAPIKey = ""
    @State private var claudeAPIKey = ""

    // Audio
    @State private var audioDevices: [AudioDevice] = []
    @State private var selectedDeviceId = ""

    // Data
    @State private var showDeleteConfirmation = false

    private let modelOptions = ["tiny", "base", "small", "medium"]
    private let languageOptions = [
        "auto": "Auto-detect",
        "en": "English",
        "es": "Spanish",
        "fr": "French",
        "de": "German",
        "it": "Italian",
        "pt": "Portuguese",
        "zh": "Chinese",
        "ja": "Japanese",
        "ko": "Korean",
    ]

    var body: some View {
        NavigationStack {
            Form {
                audioSection
                transcriptionSection
                apiKeysSection
                dataSection
                aboutSection
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.large)
            .onAppear {
                loadSettings()
            }
        }
    }

    // MARK: - Audio

    private var audioSection: some View {
        Section("Audio") {
            HStack {
                Label("Input", systemImage: "mic")
                Spacer()
                Text(audioManager.currentRoute)
                    .foregroundStyle(.secondary)
            }

            HStack {
                Label("Permission", systemImage: "lock.shield")
                Spacer()
                switch audioManager.permissionState {
                case .granted:
                    Text("Granted")
                        .foregroundStyle(.green)
                case .denied:
                    Button("Open Settings") {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(url)
                        }
                    }
                case .undetermined:
                    Button("Request") {
                        Task { await audioManager.requestPermission() }
                    }
                }
            }
        }
    }

    // MARK: - Transcription

    private var transcriptionSection: some View {
        Section("Transcription") {
            Toggle(isOn: $useCloudTranscription) {
                Label("Cloud Transcription", systemImage: "cloud")
            }
            .onChange(of: useCloudTranscription) { _, newValue in
                _ = coreBridge.setSetting(key: "use_cloud", value: newValue ? "true" : "false")
            }

            if !useCloudTranscription {
                Picker(selection: $selectedModel) {
                    ForEach(modelOptions, id: \.self) { model in
                        Text(model.capitalized).tag(model)
                    }
                } label: {
                    Label("Model", systemImage: "cpu")
                }
                .onChange(of: selectedModel) { _, newValue in
                    _ = coreBridge.setSetting(key: "whisper_model", value: newValue)
                }
            }

            Picker(selection: $selectedLanguage) {
                ForEach(languageOptions.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                    Text(value).tag(key)
                }
            } label: {
                Label("Language", systemImage: "globe")
            }
            .onChange(of: selectedLanguage) { _, newValue in
                _ = coreBridge.setSetting(key: "language", value: newValue)
            }
        }
    }

    // MARK: - API Keys

    private var apiKeysSection: some View {
        Section {
            SecureField("Whisper API Key", text: $whisperAPIKey)
                .textContentType(.password)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .onChange(of: whisperAPIKey) { _, newValue in
                    KeychainHelper.save(key: "whisper_api_key", value: newValue)
                }

            SecureField("Claude API Key", text: $claudeAPIKey)
                .textContentType(.password)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .onChange(of: claudeAPIKey) { _, newValue in
                    KeychainHelper.save(key: "claude_api_key", value: newValue)
                }
        } header: {
            Text("API Keys")
        } footer: {
            Text("Keys are stored securely in the device Keychain. They are only sent to their respective services when cloud features are enabled.")
        }
    }

    // MARK: - Data management

    private var dataSection: some View {
        Section("Data") {
            Button(role: .destructive) {
                showDeleteConfirmation = true
            } label: {
                Label("Delete All Data", systemImage: "trash")
            }
            .confirmationDialog(
                "Delete all sessions and recordings?",
                isPresented: $showDeleteConfirmation,
                titleVisibility: .visible
            ) {
                Button("Delete Everything", role: .destructive) {
                    deleteAllData()
                }
                Button("Cancel", role: .cancel) {}
            }
        }
    }

    // MARK: - About

    private var aboutSection: some View {
        Section("About") {
            HStack {
                Text("Version")
                Spacer()
                Text(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0")
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Data loading

    private func loadSettings() {
        useCloudTranscription = coreBridge.getSetting(key: "use_cloud") == "true"
        selectedModel = coreBridge.getSetting(key: "whisper_model") ?? "tiny"
        selectedLanguage = coreBridge.getSetting(key: "language") ?? "auto"

        whisperAPIKey = KeychainHelper.load(key: "whisper_api_key") ?? ""
        claudeAPIKey = KeychainHelper.load(key: "claude_api_key") ?? ""

        audioDevices = coreBridge.listAudioDevices()
        if let defaultDevice = audioDevices.first(where: { $0.isDefault }) {
            selectedDeviceId = defaultDevice.id
        }
    }

    private func deleteAllData() {
        let meetings = coreBridge.listMeetings(limit: 10000, offset: 0)
        for meeting in meetings {
            _ = coreBridge.deleteMeeting(id: meeting.id)
        }
    }
}

// MARK: - Keychain Helper

/// Minimal Keychain wrapper for storing API keys.
/// API keys belong in the platform Keychain, not in SQLite settings.
enum KeychainHelper {

    private static let service = "com.system.utility"

    static func save(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }

        // Delete existing item first.
        let deleteQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(deleteQuery as CFDictionary)

        guard !value.isEmpty else { return }

        let addQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]
        SecItemAdd(addQuery as CFDictionary, nil)
    }

    static func load(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess, let data = result as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    static func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
