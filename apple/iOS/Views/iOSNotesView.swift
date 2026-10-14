import SwiftUI

struct iOSNotesView: View {
    @EnvironmentObject var coreBridge: CoreBridge

    let meetingId: Int64
    let meetingTitle: String

    @State private var detail: MeetingDetail?
    @State private var parsedSummary: Summary?
    @State private var isLoadingSummary = false
    @State private var showExportSheet = false
    @State private var showSummaryError = false
    @State private var exportedText = ""

    // Collapsible sections
    @State private var showKeyPoints = true
    @State private var showActionItems = true
    @State private var showDecisions = true
    @State private var showTranscript = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // Meeting metadata
                meetingHeader

                // Summary
                if let summary = parsedSummary {
                    summarySection(summary)
                } else if detail?.meeting.summary != nil {
                    // Summary text exists but could not be parsed as structured
                    rawSummarySection
                } else {
                    generateSummaryButton
                }

                // Transcript
                transcriptSection

                Spacer(minLength: 32)
            }
            .padding(16)
        }
        .navigationTitle(meetingTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Export Markdown") { exportMeeting(format: "markdown") }
                    Button("Export Text") { exportMeeting(format: "text") }
                    Button("Export JSON") { exportMeeting(format: "json") }
                } label: {
                    Image(systemName: "square.and.arrow.up")
                }
            }
        }
        .sheet(isPresented: $showExportSheet) {
            ShareSheet(text: exportedText)
        }
        .alert("Summary Unavailable",
               isPresented: $showSummaryError) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Could not generate a summary. Check that an API key is configured in Settings.")
        }
        .onAppear {
            loadDetail()
        }
    }

    // MARK: - Meeting header

    private var meetingHeader: some View {
        Group {
            if let meeting = detail?.meeting {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 12) {
                        Label(formatDate(meeting.startedAt), systemImage: "calendar")
                        if let duration = meeting.durationSecs {
                            Label(formatDuration(duration), systemImage: "timer")
                        }
                    }
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - Summary

    private func summarySection(_ summary: Summary) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            // Overview
            VStack(alignment: .leading, spacing: 6) {
                Label("Overview", systemImage: "doc.text")
                    .font(.headline)
                Text(summary.summary)
                    .font(.body)
            }

            // Key Points
            if !summary.keyPoints.isEmpty {
                collapsibleSection(
                    title: "Key Points",
                    icon: "lightbulb",
                    isExpanded: $showKeyPoints
                ) {
                    ForEach(summary.keyPoints, id: \.self) { point in
                        bulletRow(point)
                    }
                }
            }

            // Action Items
            if !summary.actionItems.isEmpty {
                collapsibleSection(
                    title: "Action Items",
                    icon: "checklist",
                    isExpanded: $showActionItems
                ) {
                    ForEach(summary.actionItems, id: \.self) { item in
                        bulletRow(item)
                    }
                }
            }

            // Decisions
            if !summary.decisions.isEmpty {
                collapsibleSection(
                    title: "Decisions",
                    icon: "checkmark.seal",
                    isExpanded: $showDecisions
                ) {
                    ForEach(summary.decisions, id: \.self) { decision in
                        bulletRow(decision)
                    }
                }
            }
        }
    }

    private var rawSummarySection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Summary", systemImage: "doc.text")
                .font(.headline)
            Text(detail?.meeting.summary ?? "")
                .font(.body)
        }
    }

    private var generateSummaryButton: some View {
        Button {
            generateSummary()
        } label: {
            HStack {
                if isLoadingSummary {
                    ProgressView()
                        .controlSize(.small)
                }
                Text(isLoadingSummary ? "Generating..." : "Generate Summary")
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
        }
        .buttonStyle(.borderedProminent)
        .disabled(isLoadingSummary)
    }

    // MARK: - Transcript

    private var transcriptSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            collapsibleSection(
                title: "Transcript",
                icon: "text.quote",
                isExpanded: $showTranscript
            ) {
                if let segments = detail?.segments, !segments.isEmpty {
                    ForEach(segments) { segment in
                        HStack(alignment: .top, spacing: 8) {
                            Text(formatTimestamp(segment.timestampMs))
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                                .frame(width: 52, alignment: .trailing)
                            Text(segment.text)
                                .font(.body)
                        }
                    }
                } else {
                    Text("No transcript available.")
                        .font(.body)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - Helpers

    private func collapsibleSection<Content: View>(
        title: String,
        icon: String,
        isExpanded: Binding<Bool>,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.wrappedValue.toggle()
                }
            } label: {
                HStack {
                    Label(title, systemImage: icon)
                        .font(.headline)
                    Spacer()
                    Image(systemName: "chevron.right")
                        .font(.caption.bold())
                        .foregroundStyle(.secondary)
                        .rotationEffect(.degrees(isExpanded.wrappedValue ? 90 : 0))
                }
            }
            .tint(.primary)

            if isExpanded.wrappedValue {
                content()
                    .padding(.leading, 4)
            }
        }
    }

    private func bulletRow(_ text: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text("\u{2022}")
                .foregroundStyle(.secondary)
            Text(text)
                .font(.body)
        }
    }

    // MARK: - Data loading

    private func loadDetail() {
        detail = coreBridge.getMeeting(id: meetingId)
        if let summaryJSON = detail?.meeting.summary {
            parsedSummary = try? JSONDecoder.noteCore.decode(
                Summary.self, from: Data(summaryJSON.utf8)
            )
        }
    }

    private func generateSummary() {
        isLoadingSummary = true
        Task {
            let summary = await coreBridge.summarize(meetingId: meetingId)
            parsedSummary = summary
            isLoadingSummary = false
            if summary == nil {
                showSummaryError = true
            }
        }
    }

    private func exportMeeting(format: String) {
        if let text = coreBridge.exportMeeting(id: meetingId, format: format) {
            exportedText = text
            showExportSheet = true
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
        let minutes = seconds / 60
        if minutes < 60 {
            return "\(minutes) min"
        }
        let hours = minutes / 60
        let rem = minutes % 60
        return "\(hours)h \(rem)m"
    }

    private func formatTimestamp(_ ms: Int64) -> String {
        let totalSeconds = Int(ms / 1000)
        let m = totalSeconds / 60
        let s = totalSeconds % 60
        return String(format: "%d:%02d", m, s)
    }
}

// MARK: - Share Sheet (UIActivityViewController wrapper)

struct ShareSheet: UIViewControllerRepresentable {
    let text: String

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(
            activityItems: [text],
            applicationActivities: nil
        )
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}
