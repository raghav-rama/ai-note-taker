import SwiftUI

/// Lists past meetings with search and navigation to the detail/notes view.
struct HistoryView: View {
    @Environment(MeetingViewModel.self) private var viewModel
    @State private var selectedMeetingId: Int64?

    var body: some View {
        NavigationSplitView {
            listSection
                .navigationSplitViewColumnWidth(min: 250, ideal: 300)
        } detail: {
            if let meeting = viewModel.selectedMeeting {
                NotesView(meeting: meeting)
            } else {
                ContentUnavailableView(
                    "Select a Meeting",
                    systemImage: "doc.text",
                    description: Text("Choose a meeting from the list to view its notes.")
                )
            }
        }
        .onAppear {
            viewModel.refreshMeetings()
        }
    }

    @ViewBuilder
    private var listSection: some View {
        @Bindable var vm = viewModel
        VStack(spacing: 0) {
            // Search bar
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search meetings...", text: $vm.searchQuery)
                    .textFieldStyle(.plain)
                if !viewModel.searchQuery.isEmpty {
                    Button {
                        viewModel.searchQuery = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(8)
            .background(.bar)

            Divider()

            // Meeting list
            List(selection: $selectedMeetingId) {
                ForEach(filteredMeetings) { meeting in
                    MeetingRow(meeting: meeting)
                        .tag(meeting.id)
                        .contextMenu {
                            Button("Delete", role: .destructive) {
                                viewModel.deleteMeeting(id: meeting.id)
                            }
                        }
                }
            }
            .listStyle(.sidebar)
            .onChange(of: selectedMeetingId) { _, newValue in
                if let id = newValue {
                    viewModel.loadMeeting(id: id)
                }
            }
        }
    }

    private var filteredMeetings: [Meeting] {
        if viewModel.searchQuery.isEmpty {
            return viewModel.meetings
        }
        let query = viewModel.searchQuery.lowercased()
        return viewModel.meetings.filter {
            $0.title.lowercased().contains(query) ||
            ($0.summary?.lowercased().contains(query) ?? false)
        }
    }
}

/// A single row in the meeting list.
struct MeetingRow: View {
    let meeting: Meeting

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(meeting.title)
                .font(.headline)
                .lineLimit(1)

            HStack {
                Text(meeting.startedAt, style: .date)
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let duration = meeting.durationSeconds {
                    Text("\u{2022}")
                        .foregroundStyle(.secondary)
                    Text(formattedDuration(duration))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if let summary = meeting.summary {
                Text(summary)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }

    private func formattedDuration(_ seconds: Int) -> String {
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        return "\(minutes)m"
    }
}
