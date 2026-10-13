import SwiftUI

/// Displays a meeting's AI-generated summary, structured notes, and full transcript.
struct NotesView: View {
    let meeting: Meeting

    @Environment(MeetingViewModel.self) private var viewModel
    @State private var keyPointsExpanded = true
    @State private var actionItemsExpanded = true
    @State private var decisionsExpanded = true
    @State private var transcriptExpanded = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // Header
                headerSection

                Divider()

                // Summary
                if let summary = meeting.summary, !summary.isEmpty {
                    summarySection(summary)
                } else {
                    noSummarySection
                }

                // Structured sections
                collapsibleSection(
                    title: "Key Points",
                    icon: "list.bullet",
                    isExpanded: $keyPointsExpanded,
                    content: extractSection("key_points")
                )

                collapsibleSection(
                    title: "Action Items",
                    icon: "checkmark.circle",
                    isExpanded: $actionItemsExpanded,
                    content: extractSection("action_items")
                )

                collapsibleSection(
                    title: "Decisions",
                    icon: "arrow.triangle.branch",
                    isExpanded: $decisionsExpanded,
                    content: extractSection("decisions")
                )

                // Full transcript
                transcriptSection
            }
            .padding(24)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .toolbar {
            ToolbarItemGroup {
                if viewModel.isSummarizing {
                    ProgressView()
                        .controlSize(.small)
                }

                Button {
                    viewModel.summarizeMeeting(id: meeting.id)
                } label: {
                    Label("Summarize", systemImage: "sparkles")
                }
                .disabled(viewModel.isSummarizing)
                .help("Generate AI summary")

                Menu {
                    Button("Markdown") { exportMeeting(format: "markdown") }
                    Button("Plain Text") { exportMeeting(format: "text") }
                    Button("JSON") { exportMeeting(format: "json") }
                } label: {
                    Label("Export", systemImage: "square.and.arrow.up")
                }
                .help("Export meeting notes")
            }
        }
    }

    // MARK: - Header

    @ViewBuilder
    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(meeting.title)
                .font(.largeTitle.bold())

            HStack(spacing: 12) {
                Label(meeting.startedAt.formatted(date: .abbreviated, time: .shortened),
                      systemImage: "calendar")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                if let duration = meeting.durationSeconds {
                    Label(formattedDuration(duration), systemImage: "clock")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - Summary

    @ViewBuilder
    private func summarySection(_ summary: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Summary", systemImage: "doc.text")
                .font(.headline)

            Text(summary)
                .font(.body)
                .textSelection(.enabled)
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background.secondary)
                .clipShape(RoundedRectangle(cornerRadius: 8))
        }
    }

    @ViewBuilder
    private var noSummarySection: some View {
        VStack(spacing: 8) {
            Image(systemName: "sparkles")
                .font(.title)
                .foregroundStyle(.secondary)
            Text("No summary yet")
                .font(.headline)
                .foregroundStyle(.secondary)
            Text("Click \"Summarize\" to generate an AI summary of this meeting.")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity)
        .padding(20)
        .background(.background.secondary)
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    // MARK: - Collapsible sections

    @ViewBuilder
    private func collapsibleSection(
        title: String,
        icon: String,
        isExpanded: Binding<Bool>,
        content: String?
    ) -> some View {
        DisclosureGroup(isExpanded: isExpanded) {
            if let content, !content.isEmpty {
                Text(content)
                    .font(.body)
                    .textSelection(.enabled)
                    .padding(.top, 4)
            } else {
                Text("No data available.")
                    .font(.body)
                    .foregroundStyle(.tertiary)
                    .padding(.top, 4)
            }
        } label: {
            Label(title, systemImage: icon)
                .font(.headline)
        }
    }

    // MARK: - Full transcript from meeting segments

    @ViewBuilder
    private var transcriptSection: some View {
        DisclosureGroup(isExpanded: $transcriptExpanded) {
            if meeting.segments.isEmpty {
                Text("No transcript segments recorded.")
                    .font(.body)
                    .foregroundStyle(.tertiary)
                    .padding(.top, 4)
            } else {
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(meeting.segments) { segment in
                        HStack(alignment: .top, spacing: 8) {
                            Text(formattedTimestamp(segment.timestamp))
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                                .frame(width: 60, alignment: .trailing)

                            Text(segment.text)
                                .font(.body)
                                .textSelection(.enabled)
                        }
                    }
                }
                .padding(.top, 4)
            }
        } label: {
            Label("Full Transcript", systemImage: "text.quote")
                .font(.headline)
        }
    }

    // MARK: - Helpers

    private func extractSection(_ key: String) -> String? {
        let notes = viewModel.selectedMeetingNotes.first { $0.noteType == key }
        return notes?.content
    }

    private func exportMeeting(format: String) {
        guard let content = viewModel.exportMeeting(id: meeting.id, format: format) else { return }

        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        let ext = format == "json" ? "json" : format == "markdown" ? "md" : "txt"
        panel.nameFieldStringValue = "\(meeting.title).\(ext)"

        panel.begin { response in
            if response == .OK, let url = panel.url {
                try? content.write(to: url, atomically: true, encoding: .utf8)
            }
        }
    }

    private func formattedDuration(_ seconds: Int) -> String {
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        if hours > 0 {
            return "\(hours)h \(minutes)m"
        }
        return "\(minutes) min"
    }

    private func formattedTimestamp(_ seconds: Double) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%02d:%02d", mins, secs)
    }
}
