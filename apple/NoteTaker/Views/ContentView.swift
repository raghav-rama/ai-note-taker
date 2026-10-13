import SwiftUI

/// Root view with tab navigation between Recording, History, and Settings.
struct ContentView: View {
    @Environment(MeetingViewModel.self) private var viewModel

    enum Tab: Hashable {
        case recording
        case history
        case settings
    }

    @State private var selectedTab: Tab = .recording

    var body: some View {
        TabView(selection: $selectedTab) {
            RecordingView()
                .tabItem {
                    Label("Record", systemImage: "record.circle")
                }
                .tag(Tab.recording)

            HistoryView()
                .tabItem {
                    Label("History", systemImage: "clock.arrow.circlepath")
                }
                .tag(Tab.history)

            SettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gear")
                }
                .tag(Tab.settings)
        }
        .alert("Error", isPresented: Binding(
            get: { viewModel.showError },
            set: { viewModel.showError = $0 }
        )) {
            Button("OK") { viewModel.showError = false }
        } message: {
            Text(viewModel.errorMessage ?? "An unknown error occurred.")
        }
    }
}
