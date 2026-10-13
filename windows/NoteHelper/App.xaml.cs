using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using NoteHelper.Services;
using System;
using System.Threading.Tasks;
using Windows.ApplicationModel;

namespace NoteHelper;

/// <summary>
/// Application entry point. Manages lifecycle, system tray, and single-instance behavior.
/// </summary>
public partial class App : Application
{
    private MainWindow? _mainWindow;
    private H.NotifyIcon.TaskbarIcon? _trayIcon;

    public App()
    {
        this.InitializeComponent();
        UnhandledException += OnUnhandledException;
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _mainWindow = new MainWindow();

        SetupTrayIcon();

        // Show window on first launch
        _mainWindow.Activate();
    }

    private void SetupTrayIcon()
    {
        _trayIcon = new H.NotifyIcon.TaskbarIcon
        {
            ToolTipText = "System Helper",
            IconSource = new H.NotifyIcon.GeneratedIconSource
            {
                Text = "SH",
                Foreground = new Microsoft.UI.Xaml.Media.SolidColorBrush(
                    Microsoft.UI.Colors.White),
                Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(
                    Microsoft.UI.Colors.DarkSlateGray),
            }
        };

        var showItem = new H.NotifyIcon.Core.PopupMenuItem("Show");
        showItem.Click += (_, _) => ShowMainWindow();

        var exitItem = new H.NotifyIcon.Core.PopupMenuItem("Exit");
        exitItem.Click += (_, _) => ExitApplication();

        _trayIcon.ContextMenu = new H.NotifyIcon.Core.PopupMenu
        {
            Items = { showItem, exitItem }
        };

        _trayIcon.TrayIconClicked += (_, _) => ShowMainWindow();

        _trayIcon.ForceCreate();
    }

    private void ShowMainWindow()
    {
        if (_mainWindow is null) return;

        _mainWindow.Activate();
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(_mainWindow);
        PInvoke.User32.ShowWindow(hwnd, PInvoke.User32.ShowWindowCommand.SW_RESTORE);
        PInvoke.User32.SetForegroundWindow(hwnd);
    }

    private void ExitApplication()
    {
        _trayIcon?.Dispose();
        _mainWindow?.Close();
        Environment.Exit(0);
    }

    private void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        // Log and suppress to prevent crash
        System.Diagnostics.Debug.WriteLine($"Unhandled exception: {e.Exception}");
        e.Handled = true;
    }

    /// <summary>
    /// Minimizes to tray instead of closing. Called from MainWindow.
    /// </summary>
    public void MinimizeToTray()
    {
        if (_mainWindow is null) return;
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(_mainWindow);
        PInvoke.User32.ShowWindow(hwnd, PInvoke.User32.ShowWindowCommand.SW_HIDE);
    }
}

/// <summary>
/// Minimal P/Invoke declarations for window management used by the App class.
/// </summary>
internal static partial class PInvoke
{
    internal static class User32
    {
        public enum ShowWindowCommand : int
        {
            SW_HIDE = 0,
            SW_RESTORE = 9,
        }

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, ShowWindowCommand nCmdShow);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        [return: System.Runtime.InteropServices.MarshalAs(
            System.Runtime.InteropServices.UnmanagedType.Bool)]
        public static extern bool SetForegroundWindow(IntPtr hWnd);
    }
}
