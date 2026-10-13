using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using NoteHelper.Services;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading.Tasks;

namespace NoteHelper.Views;

/// <summary>
/// Notes view showing AI-generated summary, key points, action items,
/// decisions, and full transcript for a meeting.
/// </summary>
public sealed partial class NotesPage : Page
{
    private long _meetingId;
    private CoreBridge? _core;

    public NotesPage()
    {
        this.InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);

        if (e.Parameter is long meetingId)
        {
            _meetingId = meetingId;
            LoadMeeting();
        }
    }

    private void LoadMeeting()
    {
        try
        {
            _core ??= CreateCore();
            var json = _core?.GetMeeting(_meetingId);
            if (json is null) return;

            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;

            MeetingTitle.Text = root.TryGetProperty("title", out var t)
                ? t.GetString() ?? "Untitled" : "Untitled";

            if (root.TryGetProperty("started_at", out var sa) &&
                DateTime.TryParse(sa.GetString(), out var startedAt))
            {
                MeetingDate.Text = startedAt.ToString("MMMM d, yyyy 'at' h:mm tt");
            }

            if (root.TryGetProperty("duration_secs", out var dur))
            {
                var secs = dur.GetInt32();
                var span = TimeSpan.FromSeconds(secs);
                MeetingDuration.Text = span.TotalHours >= 1
                    ? span.ToString(@"h\h\ mm\m")
                    : span.ToString(@"m\m\ ss\s");
            }

            // Load existing summary if available
            if (root.TryGetProperty("summary", out var summary) &&
                summary.ValueKind == JsonValueKind.String)
            {
                var summaryText = summary.GetString();
                if (!string.IsNullOrWhiteSpace(summaryText))
                {
                    ParseAndDisplaySummary(summaryText);
                }
            }

            // Load transcript segments
            LoadTranscript();
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Failed to load meeting: {ex.Message}");
        }
    }

    private void LoadTranscript()
    {
        // The core library returns transcript within the meeting JSON
        // For now, show exported text
        var exported = _core?.ExportMeeting(_meetingId, "text");
        if (!string.IsNullOrWhiteSpace(exported))
        {
            TranscriptText.Text = exported;
        }
    }

    private async void GenerateSummary_Click(object sender, RoutedEventArgs e)
    {
        SummaryProgress.Visibility = Visibility.Visible;
        SummaryProgress.IsActive = true;
        SummaryText.Text = "Generating summary...";

        try
        {
            // Run summarization off the UI thread
            var summary = await Task.Run(() => _core?.Summarize(_meetingId));

            if (!string.IsNullOrWhiteSpace(summary))
            {
                ParseAndDisplaySummary(summary);
            }
            else
            {
                SummaryText.Text = "Failed to generate summary. "
                    + "Ensure an API key is configured in Settings.";
                SummaryText.Foreground = (Microsoft.UI.Xaml.Media.Brush)
                    Application.Current.Resources["SystemFillColorCautionBrush"];
            }
        }
        catch (Exception ex)
        {
            SummaryText.Text = $"Error: {ex.Message}";
        }
        finally
        {
            SummaryProgress.IsActive = false;
            SummaryProgress.Visibility = Visibility.Collapsed;
        }
    }

    /// <summary>
    /// Parses a structured summary (expects sections separated by headers)
    /// and populates the collapsible sections.
    /// </summary>
    private void ParseAndDisplaySummary(string summary)
    {
        // Attempt to parse as JSON first (structured summary from core)
        try
        {
            using var doc = JsonDocument.Parse(summary);
            var root = doc.RootElement;

            if (root.TryGetProperty("summary", out var s))
            {
                SummaryText.Text = s.GetString() ?? summary;
                SummaryText.Foreground = (Microsoft.UI.Xaml.Media.Brush)
                    Application.Current.Resources["TextFillColorPrimaryBrush"];
            }

            if (root.TryGetProperty("key_points", out var kp) &&
                kp.ValueKind == JsonValueKind.Array)
            {
                var items = kp.EnumerateArray()
                    .Select(e => e.GetString() ?? "").ToList();
                if (items.Count > 0)
                {
                    KeyPointsList.ItemsSource = items;
                    KeyPointsExpander.Visibility = Visibility.Visible;
                }
            }

            if (root.TryGetProperty("action_items", out var ai) &&
                ai.ValueKind == JsonValueKind.Array)
            {
                var items = ai.EnumerateArray()
                    .Select(e => e.GetString() ?? "").ToList();
                if (items.Count > 0)
                {
                    ActionItemsList.ItemsSource = items;
                    ActionItemsExpander.Visibility = Visibility.Visible;
                }
            }

            if (root.TryGetProperty("decisions", out var d) &&
                d.ValueKind == JsonValueKind.Array)
            {
                var items = d.EnumerateArray()
                    .Select(e => e.GetString() ?? "").ToList();
                if (items.Count > 0)
                {
                    DecisionsList.ItemsSource = items;
                    DecisionsExpander.Visibility = Visibility.Visible;
                }
            }

            return;
        }
        catch (JsonException)
        {
            // Not JSON, treat as plain text
        }

        // Plain text summary
        SummaryText.Text = summary;
        SummaryText.Foreground = (Microsoft.UI.Xaml.Media.Brush)
            Application.Current.Resources["TextFillColorPrimaryBrush"];
    }

    private void ExportNotes_Click(object sender, RoutedEventArgs e)
    {
        // Delegate to the same export logic as HistoryPage
        var exported = _core?.ExportMeeting(_meetingId, "markdown");
        if (exported is not null)
        {
            // Copy to clipboard as a quick export
            var dataPackage = new Windows.ApplicationModel.DataTransfer.DataPackage();
            dataPackage.SetText(exported);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(dataPackage);

            // TODO: show a teaching tip / info bar
        }
    }

    private void BackButton_Click(object sender, RoutedEventArgs e)
    {
        if (Frame.CanGoBack)
            Frame.GoBack();
    }

    private static CoreBridge? CreateCore()
    {
        try
        {
            var core = new CoreBridge();
            var appData = Environment.GetFolderPath(
                Environment.SpecialFolder.LocalApplicationData);
            var dbDir = System.IO.Path.Combine(appData, "NoteHelper");
            System.IO.Directory.CreateDirectory(dbDir);
            var dbPath = System.IO.Path.Combine(dbDir, "meetings.db");
            var modelPath = System.IO.Path.Combine(dbDir, "models", "ggml-tiny.bin");

            if (System.IO.File.Exists(modelPath))
            {
                core.Initialize(dbPath, modelPath);
                return core;
            }
        }
        catch { }
        return null;
    }
}
