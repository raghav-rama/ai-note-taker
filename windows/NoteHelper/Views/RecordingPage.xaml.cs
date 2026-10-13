using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using NoteHelper.Services;
using System;
using System.Diagnostics;

namespace NoteHelper.Views;

/// <summary>
/// Recording view with record/stop button, live transcript, timer, and audio level meter.
/// </summary>
public sealed partial class RecordingPage : Page
{
    private enum RecordState { Idle, Recording, Paused }

    private RecordState _state = RecordState.Idle;
    private readonly DispatcherTimer _timer;
    private readonly Stopwatch _stopwatch;
    private CoreBridge? _core;

    public RecordingPage()
    {
        this.InitializeComponent();

        _stopwatch = new Stopwatch();
        _timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(100) };
        _timer.Tick += Timer_Tick;

        InitializeCore();
    }

    private void InitializeCore()
    {
        try
        {
            _core = new CoreBridge();

            var appData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            var dbDir = System.IO.Path.Combine(appData, "NoteHelper");
            System.IO.Directory.CreateDirectory(dbDir);

            var dbPath = System.IO.Path.Combine(dbDir, "meetings.db");
            var modelPath = System.IO.Path.Combine(dbDir, "models", "ggml-tiny.bin");

            // Only initialize if model exists; otherwise the UI works without core
            if (System.IO.File.Exists(modelPath))
            {
                _core.Initialize(dbPath, modelPath);
                SetupTranscriptCallback();
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Core init failed: {ex.Message}");
            _core = null;
        }
    }

    private void SetupTranscriptCallback()
    {
        _core?.SetTranscriptCallback((text, timestamp) =>
        {
            DispatcherQueue.TryEnqueue(() => AppendTranscript(text, timestamp));
        });
    }

    private void RecordButton_Click(object sender, RoutedEventArgs e)
    {
        switch (_state)
        {
            case RecordState.Idle:
                StartRecording();
                break;
            case RecordState.Recording:
            case RecordState.Paused:
                StopRecording();
                break;
        }
    }

    private void PauseButton_Click(object sender, RoutedEventArgs e)
    {
        switch (_state)
        {
            case RecordState.Recording:
                PauseRecording();
                break;
            case RecordState.Paused:
                ResumeRecording();
                break;
        }
    }

    private void StartRecording()
    {
        _state = RecordState.Recording;
        UpdateButtonVisuals();

        // Clear transcript
        TranscriptBlock.Blocks.Clear();

        // Start timer
        _stopwatch.Restart();
        _timer.Start();

        // Start native recording
        _core?.StartRecording(null);

        // Update title bar indicator
        if (App.Current is App { } app)
        {
            var mainWindow = GetMainWindow();
            mainWindow?.SetRecordingState(true, "00:00");
        }
    }

    private void StopRecording()
    {
        _state = RecordState.Idle;
        UpdateButtonVisuals();

        // Stop timer
        _stopwatch.Stop();
        _timer.Stop();

        // Stop native recording
        _core?.StopRecording();

        // Update title bar
        GetMainWindow()?.SetRecordingState(false);

        // Reset audio level
        AudioLevelBar.Value = 0;
    }

    private void PauseRecording()
    {
        _state = RecordState.Paused;
        UpdateButtonVisuals();
        _stopwatch.Stop();
        _core?.PauseRecording();
    }

    private void ResumeRecording()
    {
        _state = RecordState.Recording;
        UpdateButtonVisuals();
        _stopwatch.Start();
        _core?.StartRecording(null);
    }

    private void UpdateButtonVisuals()
    {
        switch (_state)
        {
            case RecordState.Idle:
                RecordButton.Background = (Microsoft.UI.Xaml.Media.Brush)
                    Application.Current.Resources["RecordingIdleBrush"];
                RecordButtonIcon.Glyph = "";   // Microphone
                RecordButtonText.Text = "Record";
                PauseButton.Visibility = Visibility.Collapsed;
                break;

            case RecordState.Recording:
                RecordButton.Background = (Microsoft.UI.Xaml.Media.Brush)
                    Application.Current.Resources["RecordingActiveBrush"];
                RecordButtonIcon.Glyph = "";   // Stop
                RecordButtonText.Text = "Stop";
                PauseButton.Visibility = Visibility.Visible;
                ((FontIcon)PauseButton.Content).Glyph = ""; // Pause
                break;

            case RecordState.Paused:
                RecordButton.Background = (Microsoft.UI.Xaml.Media.Brush)
                    Application.Current.Resources["RecordingActiveBrush"];
                RecordButtonIcon.Glyph = "";   // Stop
                RecordButtonText.Text = "Stop";
                PauseButton.Visibility = Visibility.Visible;
                ((FontIcon)PauseButton.Content).Glyph = ""; // Play/Resume
                break;
        }
    }

    private void Timer_Tick(object? sender, object e)
    {
        var elapsed = _stopwatch.Elapsed;
        TimerDisplay.Text = elapsed.ToString(@"hh\:mm\:ss");

        // Update title bar with mm:ss
        GetMainWindow()?.SetRecordingState(true, elapsed.ToString(@"mm\:ss"));

        // Simulate audio level (will be replaced by real data from core)
        if (_state == RecordState.Recording)
        {
            var rand = new Random();
            AudioLevelBar.Value = rand.Next(10, 70);
        }
    }

    private void AppendTranscript(string text, double timestamp)
    {
        var ts = TimeSpan.FromSeconds(timestamp);
        var timeStr = ts.ToString(@"mm\:ss");

        var paragraph = new Paragraph();
        paragraph.Inlines.Add(new Run
        {
            Text = $"[{timeStr}] ",
            Foreground = (Microsoft.UI.Xaml.Media.Brush)
                Application.Current.Resources["AppAccentBrush"],
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold
        });
        paragraph.Inlines.Add(new Run { Text = text });

        TranscriptBlock.Blocks.Add(paragraph);

        // Auto-scroll to bottom
        TranscriptScroller.ChangeView(null, TranscriptScroller.ScrollableHeight, null);
    }

    private MainWindow? GetMainWindow()
    {
        if (XamlRoot?.Content is FrameworkElement fe)
        {
            // Walk up to find the Window
            var window = fe.XamlRoot?.Content;
        }
        // Fallback: iterate through open windows (WinUI 3 doesn't expose this easily)
        return null;
    }
}
