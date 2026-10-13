import SwiftUI

struct iOSHistoryView: View {
    @EnvironmentObject var coreBridge: CoreBridge

    @State private var meetings: [Meeting] = []
    @State private var searchText = ""
    @State private var selectedMeeting: Meeting?

    private var filteredMeetings: [Meeting] {
        if searchText.isEmpty {
            return meetings
        }
        return meetings.filter { meeting in
            meeting.title.localizedCaseInsensitiveContains(searchText)
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if meetings.isEmpty {
                    emptyState
                } else {
                    meetingList
                }
            }
            .navigationTitle("History")
            .navigationBarTitleDisplayMode(.large)
            .navigationDestination(item: $selectedMeeting) { meeting in
                iOSNotesView(meetingId: meeting.id, meetingTitle: meeting.title)
            }
            .onAppear {
                loadMeetings()
            }
            .refreshable {
                loadMeetings()
            }
        }
    }

    // MARK: - Empty state

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "clock")
                .font(.system(size: 48))
                .foregroundStyle(.quaternary)
            Text("No sessions yet")
                .font(.title3)
                .foregroundStyle(.secondary)
            Text("Recorded sessions will appear here.")
                .font(.subheadline)
                .foregroundStyle(.tertiary)
        }
    }

    // MARK: - Meeting list

    private var meetingList: some View {
        List {
            ForEach(filteredMeetings) { meeting in
                Button {
                    selectedMeeting = meeting
                } label: {
                    meetingRow(meeting)
                }
                .tint(.primary)
            }
            .onDelete(perform: deleteMeetings)
        }
        .listStyle(.insetGrouped)
        .searchable(text: $searchText, prompt: "Search sessions")
    }

    private func meetingRow(_ meeting: Meeting) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(meeting.title)
                .font(.headline)
                .lineLimit(1)

            HStack(spacing: 12) {
                Label(formatDate(meeting.startedAt), systemImage: "calendar")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if let duration = meeting.durationSecs {
                    Label(formatDuration(duration), systemImage: "timer")
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

    // MARK: - Actions

    private func loadMeetings() {
        meetings = coreBridge.listMeetings(limit: 100, offset: 0)
    }

    private func deleteMeetings(at offsets: IndexSet) {
        for index in offsets {
            let meeting = filteredMeetings[index]
            if coreBridge.deleteMeeting(id: meeting.id) {
                meetings.removeAll { $0.id == meeting.id }
            }
        }
    }

    // MARK: - Formatting

    private func formatDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }

    private func formatDuration(_ seconds: Int) -> String {
        if seconds < 60 {
            return "\(seconds)s"
        }
        let minutes = seconds / 60
        if minutes < 60 {
            return "\(minutes)m"
        }
        let hours = minutes / 60
        let remainingMinutes = minutes % 60
        return "\(hours)h \(remainingMinutes)m"
    }
}
