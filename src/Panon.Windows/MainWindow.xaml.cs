using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace Panon.Windows;

/// <summary>
/// 主窗口，作为设置窗口使用
/// </summary>
public sealed partial class MainWindow : Window
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadImage(IntPtr hInst, string lpszName, uint uType, int cxDesired, int cyDesired, uint fuLoad);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr SendMessage(IntPtr hWnd, int Msg, IntPtr wParam, IntPtr lParam);

    private const int WM_SETICON = 0x0080;
    private const int ICON_SMALL = 0;
    private const int ICON_BIG = 1;
    private const uint IMAGE_ICON = 1;
    private const uint LR_LOADFROMFILE = 0x00000010;

    private IntPtr _hIconSmall;
    private IntPtr _hIconBig;

    public MainWindow()
    {
        InitializeComponent();

        // 设置窗口大小
        var size = new SizeInt32(820, 720);
        AppWindow.Resize(size);

        // 居中到屏幕工作区（排除任务栏）
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        var displayArea = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
        if (displayArea != null)
        {
            int x = displayArea.WorkArea.X + (displayArea.WorkArea.Width - size.Width) / 2;
            int y = displayArea.WorkArea.Y + (displayArea.WorkArea.Height - size.Height) / 2;
            AppWindow.Move(new PointInt32(x, y));
        }

        // 设置窗口任务栏图标（Win32 WM_SETICON）
        SetWindowIcon(hwnd);

        RootFrame.Navigate(typeof(SettingsPage));
    }

    /// <summary>
    /// 通过 Win32 API 设置窗口任务栏/Alt+Tab 图标
    /// </summary>
    private void SetWindowIcon(IntPtr hwnd)
    {
        string iconPath = System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "PanonWindows.ico");
        if (!System.IO.File.Exists(iconPath)) return;

        // 加载大图标(256x256)和小图标(16x16)，0表示使用图标文件中的实际尺寸
        _hIconBig = LoadImage(IntPtr.Zero, iconPath, IMAGE_ICON, 0, 0, LR_LOADFROMFILE);
        _hIconSmall = LoadImage(IntPtr.Zero, iconPath, IMAGE_ICON, 16, 16, LR_LOADFROMFILE);

        if (_hIconBig != IntPtr.Zero)
            SendMessage(hwnd, WM_SETICON, (IntPtr)ICON_BIG, _hIconBig);
        if (_hIconSmall != IntPtr.Zero)
            SendMessage(hwnd, WM_SETICON, (IntPtr)ICON_SMALL, _hIconSmall);
    }
}
