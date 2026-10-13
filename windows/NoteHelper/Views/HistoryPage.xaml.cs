using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NoteHelper.Services;
using System;
using System.Collections.ObjectModel;
using System.Linq;
using System.Text.Json;

namespace NoteHelper.Views;

/// <summary>
/// Data model for a meeting displayed in the history list.
/// </summary>
public sealed class MeetingItem
{
    public long Id { get; set; }
    public string Title { get; set; } = "Untitled Meeting";
    public DateTime StartedAt { get; set; }
    public DateTime? EndedAt { get; set; }
    public int DurationSecs { get; set; }
    public string? Summary { get; set; }

    // Display helpers
    public string DateMonth => StartedAt.ToString("MMM").ToUpper();
    public string DateDay => StartedAt.Day.ToString();
    public string TimeRange
    {
        get
        {
            var start = StartedAt.ToString("h:mm tt");
            if (EndedAt.HasValue)
                return $"{start} - {EndedAt.Value.ToString("h:mm tt")}";
            return start;
        }
    }
    public string DurationText
    {
        get
        {
            if (DurationSecs < 60) return $"{DurationSecs}s";
            if (DurationSecs < 3600) return $"{DurationSecs / 60}m";
            return $"{DurationSecs / 3600}h {(DurationSecs % 3600) / 60}m";
        }
    }
}

/// <summary>
/// History view showing past meetings with search and context actions.
/// </summary>
public sealed partial class HistoryPage : Page
{
    private readonly ObservableCollection<MeetingItem> _meetings = new();
    private readonly ObservableCollection<MeetingItem> _filteredMeetings = new();
    private CoreBridge? _core;

    public HistoryPage()
    {
        this.InitializeComponent();
        MeetingList.ItemsSource = _filteredMeetings;
        Loaded += HistoryPage_Loaded;
    }

    private void HistoryPage_Loaded(object sender, RoutedEventArgs e)
    {
        LoadMeetings();
    }

    private void LoadMeetings()
    {
        _meetings.Clear();
        _filteredMeetings.Clear();

        try
        {
            _core ??= CreateCore();
            var json = _core?.ListMeetings(100, 0);
            if (json is null) return;

            var items = JsonSerializer.Deserialize<MeetingItem[]>(json,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            if (items is not null)
            {
                foreach (var item in items.OrderByDescending(m => m.StartedAt))
                {
                    _meetings.Add(item);
                    _filteredMeetings.Add(item);
                }
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Failed to load meetings: {ex.Message}");
        }

        EmptyState.Visibility = _filteredMeetings.Count == 0
            ? Visibility.Visible
            : Visibility.Collapsed;
    }

    private void SearchBox_TextChanged(AutoSuggestBox sender,
        AutoSuggestBoxTextChangedEventArgs args)
    {
        if (args.Reason == AutoSuggestionBoxTextChangeReason.UserInput)
        {
            ApplyFilter(sender.Text);
        }
    }

    private void SearchBox_QuerySubmitted(AutoSuggestBox sender,
        AutoSuggestBoxQuerySubmittedEventArgs args)
    {
        ApplyFilter(args.QueryText);
    }

    private void ApplyFilter(string query)
    {
        _filteredMeetings.Clear();
        var q = query.Trim().ToLowerInvariant();

        var source = string.IsNullOrEmpty(q)
            ? _meetings
            : _meetings.Where(m =>
                m.Title.Contains(q, StringComparison.OrdinalIgnoreCase) ||
                (m.Summary?.Contains(q, StringComparison.OrdinalIgnoreCase) ?? false));

        foreach (var item in source)
            _filteredMeetings.Add(item);

        EmptyState.Visibility = _filteredMeetings.Count == 0
            ? Visibility.Visible
            : Visibility.Collapsed;
    }

    private void MeetingList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (MeetingList.SelectedItem is MeetingItem meeting)
        {
            // Navigate to notes view for selected meeting
            Frame.Navigate(typeof(NotesPage), meeting.Id);
        }
    }

    private void ViewNotes_Click(object sender, RoutedEventArgs e)
    {
        if (GetMeetingFromSender(sender) is MeetingItem meeting)
        {
            Frame.Navigate(typeof(NotesPage), meeting.Id);
        }
    }

    private async void Export_Click(object sender, RoutedEventArgs e)
    {
        if (GetMeetingFromSender(sender) is not MeetingItem meeting) return;

        try
        {
            var exported = _core?.ExportMeeting(meeting.Id, "markdown");
            if (exported is null) return;

            var picker = new Windows.Storage.Pickers.FileSavePicker();
            picker.SuggestedStartLocation =
                Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary;
            picker.FileTypeChoices.Add("Markdown", new[] { ".md" });
            picker.FileTypeChoices.Add("Plain Text", new[] { ".txt" });
            picker.SuggestedFileName = meeting.Title;

            // Initialize picker with window handle
            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(
                (Application.Current as App)!.GetType()
                    .GetField("_mainWindow",
                        System.Reflection.BindingFlags.NonPublic |
                        System.Reflection.BindingFlags.Instance)
                    ?.GetValue(Application.Current) as MainWindow);
            WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);

            var file = await picker.PickSaveFileAsync();
            if (file is not null)
            {
                await Windows.Storage.FileIO.WriteTextAsync(file, exported);
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Export failed: {ex.Message}");
        }
    }

    private async void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (GetMeetingFromSender(sender) is not MeetingItem meeting) return;

        var dialog = new ContentDialog
        {
            Title = "Delete Meeting",
            Content = $"Are you sure you want to delete \"{meeting.Title}\"? This cannot be undone.",
            PrimaryButtonText = "Delete",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = this.XamlRoot
        };

        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary)
        {
            _core?.DeleteMeeting(meeting.Id);
            _meetings.Remove(meeting);
            _filteredMeetings.Remove(meeting);
            EmptyState.Visibility = _filteredMeetings.Count == 0
                ? Visibility.Visible
                : Visibility.Collapsed;
        }
    }

    private static MeetingItem? GetMeetingFromSender(object sender)
    {
        if (sender is FrameworkElement fe)
            return fe.DataContext as MeetingItem;
        return null;
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
