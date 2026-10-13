using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NoteHelper.Services;
using NoteHelper.Views;
using System;
using System.Runtime.InteropServices;

namespace NoteHelper;

/// <summary>
/// Main application window with NavigationView and screen capture exclusion.
/// </summary>
public sealed partial class MainWindow : Window
{
    private readonly StealthService _stealth;

    public MainWindow()
    {
        this.InitializeComponent();

        // Configure window
        Title = "System Helper";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);

        // Set up stealth (screen capture exclusion)
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        _stealth = new StealthService(hwnd);

        // Set initial window size
        var appWindow = GetAppWindowForCurrentWindow();
        if (appWindow is not null)
        {
            appWindow.Resize(new Windows.Graphics.SizeInt32(1024, 720));
        }

        // Minimize to tray on close instead of exiting
        this.Closed += MainWindow_Closed;
    }

    private Microsoft.UI.Windowing.AppWindow? GetAppWindowForCurrentWindow()
    {
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
        return Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
    }

    private void MainWindow_Closed(object sender, WindowEventArgs args)
    {
        // Prevent close, minimize to tray instead
        args.Handled = true;
        if (Application.Current is App app)
        {
            app.MinimizeToTray();
        }
    }

    private void NavView_Loaded(object sender, RoutedEventArgs e)
    {
        // Apply screen capture exclusion once the window handle is ready
        _stealth.ExcludeFromCapture();

        // Navigate to the recording page by default
        if (NavView.MenuItems.Count > 0)
        {
            NavView.SelectedItem = NavView.MenuItems[0];
            ContentFrame.Navigate(typeof(RecordingPage));
        }
    }

    private void NavView_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.IsSettingsSelected)
        {
            ContentFrame.Navigate(typeof(SettingsPage));
            return;
        }

        if (args.SelectedItemContainer is NavigationViewItem item)
        {
            var tag = item.Tag?.ToString();
            Type? pageType = tag switch
            {
                "Recording" => typeof(RecordingPage),
                "History" => typeof(HistoryPage),
                "Notes" => typeof(NotesPage),
                _ => null,
            };

            if (pageType is not null)
            {
                ContentFrame.Navigate(pageType);
            }
        }
    }

    /// <summary>
    /// Updates the recording indicator in the title bar.
    /// </summary>
    public void SetRecordingState(bool isRecording, string? timerText = null)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            RecordingIndicator.Visibility = isRecording
                ? Visibility.Visible
                : Visibility.Collapsed;

            if (timerText is not null)
            {
                RecordingTimerText.Text = timerText;
            }
        });
    }
}
