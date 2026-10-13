using System;
using System.Runtime.InteropServices;

namespace NoteHelper.Services;

/// <summary>
/// Manages window stealth features to exclude the application from screen capture
/// and screen sharing. Uses SetWindowDisplayAffinity with WDA_EXCLUDEFROMCAPTURE.
/// </summary>
public sealed class StealthService
{
    private readonly IntPtr _hwnd;
    private bool _isExcluded;

    /// <summary>Window Display Affinity: exclude from capture (Windows 10 2004+).</summary>
    private const uint WDA_NONE = 0x00000000;
    private const uint WDA_EXCLUDEFROMCAPTURE = 0x00000011;

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowDisplayAffinity(IntPtr hWnd, uint dwAffinity);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowDisplayAffinity(IntPtr hWnd, out uint dwAffinity);

    public StealthService(IntPtr hwnd)
    {
        _hwnd = hwnd;
    }

    /// <summary>
    /// Whether the window is currently excluded from screen capture.
    /// </summary>
    public bool IsExcludedFromCapture => _isExcluded;

    /// <summary>
    /// Applies WDA_EXCLUDEFROMCAPTURE to make the window invisible
    /// to screen sharing and capture tools.
    /// </summary>
    /// <returns>True if the affinity was set successfully.</returns>
    public bool ExcludeFromCapture()
    {
        if (_hwnd == IntPtr.Zero) return false;

        bool result = SetWindowDisplayAffinity(_hwnd, WDA_EXCLUDEFROMCAPTURE);
        if (result)
        {
            _isExcluded = true;
            System.Diagnostics.Debug.WriteLine(
                "StealthService: Window excluded from screen capture.");
        }
        else
        {
            int error = Marshal.GetLastWin32Error();
            System.Diagnostics.Debug.WriteLine(
                $"StealthService: Failed to set display affinity. Win32 error: {error}");
        }
        return result;
    }

    /// <summary>
    /// Removes the capture exclusion, making the window visible to capture again.
    /// </summary>
    /// <returns>True if the affinity was cleared successfully.</returns>
    public bool RemoveExclusion()
    {
        if (_hwnd == IntPtr.Zero) return false;

        bool result = SetWindowDisplayAffinity(_hwnd, WDA_NONE);
        if (result)
        {
            _isExcluded = false;
        }
        return result;
    }

    /// <summary>
    /// Queries the current display affinity of the window.
    /// </summary>
    public uint GetCurrentAffinity()
    {
        if (_hwnd == IntPtr.Zero) return 0;

        GetWindowDisplayAffinity(_hwnd, out uint affinity);
        return affinity;
    }
}
