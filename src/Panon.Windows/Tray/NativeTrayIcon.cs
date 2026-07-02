using System.Runtime.InteropServices;
using System.Text;
using Panon.Windows.Helpers;

namespace Panon.Windows.Tray;

/// <summary>
/// 系统托盘图标控制器 - 使用 Win32 原生 Shell_NotifyIcon API
/// </summary>
public sealed class NativeTrayIcon : IDisposable
{
    private const int WM_APP = 0x8000;
    internal static readonly uint WM_TRAY_CALLBACK = (uint)(WM_APP + 1);
    private const int NIF_MESSAGE = 0x01;
    private const int NIF_ICON = 0x02;
    private const int NIF_TIP = 0x04;
    private const int NIM_ADD = 0x00000000;
    private const int NIM_DELETE = 0x00000002;
    private const int NIM_MODIFY = 0x00000001;

    // 鼠标消息
    internal const int WM_LBUTTONUP = 0x0202;
    internal const int WM_RBUTTONUP = 0x0205;
    internal const int WM_LBUTTONDBLCLK = 0x0203;

    private IntPtr _hwnd;       // 消息窗口句柄
    private IntPtr _iconHandle; // 图标句柄
    private uint _notifyId;
    private bool _isCreated;

    public event Action? LeftClick;
    public event Action? DoubleClick;
    public event Action? RightClick;

    /// <summary>
    /// 初始化托盘图标（需要传入消息窗口句柄）
    /// </summary>
    public void Initialize(IntPtr messageHwnd, uint notifyId)
    {
        _hwnd = messageHwnd;
        _notifyId = notifyId;

        // 加载项目图标（PanonWindows.ico），失败时回退到系统默认图标
        string iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "PanonWindows.ico");
        _iconHandle = LoadImage(IntPtr.Zero, iconPath, IMAGE_ICON, 0, 0, LR_LOADFROMFILE);
        if (_iconHandle == IntPtr.Zero)
        {
            DebugLog.Write($"LoadImage from file failed, fallback to IDI_APPLICATION, err={Marshal.GetLastWin32Error()}");
            _iconHandle = LoadIcon(IntPtr.Zero, (IntPtr)32512); // IDI_APPLICATION
        }

        var data = new NOTIFYICONDATA();
        data.cbSize = Marshal.SizeOf<NOTIFYICONDATA>();
        data.hWnd = _hwnd;
        data.uID = _notifyId;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY_CALLBACK;
        data.hIcon = _iconHandle;
        data.szTip = "Panon - 右键菜单操作";

        if (Shell_NotifyIcon(NIM_ADD, ref data))
        {
            _isCreated = true;
            DebugLog.Write("NativeTray OK");
        }
        else
        {
            DebugLog.Write($"NativeTray ADD failed, err={Marshal.GetLastWin32Error()}");
        }
    }

    /// <summary>
    /// 处理来自托盘的消息（由消息窗口调用）
    /// </summary>
    public bool HandleMessage(IntPtr wParam, IntPtr lParam)
    {
        if ((uint)wParam != _notifyId) return false;

        int msg = lParam.ToInt32();

        switch (msg)
        {
            case WM_LBUTTONUP:
                DebugLog.Write("Tray LeftClick");
                LeftClick?.Invoke();
                return true;

            case WM_LBUTTONDBLCLK:
                DebugLog.Write("Tray DoubleClick");
                DoubleClick?.Invoke();
                return true;

            case WM_RBUTTONUP:
                DebugLog.Write("Tray RightClick");
                RightClick?.Invoke();
                return true;
        }

        return false;
    }

    /// <summary>
    /// 显示右键弹出菜单
    /// </summary>
    public void ShowContextMenu(int x, int y, Action onSettings, Action onPause, Action onExit)
    {
        var hmenu = CreatePopupMenu();

        AppendMenu(hmenu, 0, 100, "⚙ 设置");
        AppendMenu(hmenu, 0, 101, "⏸ 暂停 / 恢复");
        AppendMenu(hmenu, 0x800, 0, null); // 分隔线 MF_SEPARATOR
        AppendMenu(hmenu, 0, 102, "✕ 退出");

        // 设置默认选中项
        SetMenuDefaultItem(hmenu, 100, 0);

        // 去除菜单两侧留白：设置菜单为右对齐文本风格，紧贴内容
        var mi = new MENUINFO();
        mi.cbSize = (uint)Marshal.SizeOf<MENUINFO>();
        mi.fMask = MIM_STYLE;
        mi.dwStyle = MNS_NOCHECK; // 不显示勾选框列，减少左侧留白
        SetMenuInfo(hmenu, ref mi);

        // 获取前景窗口，确保菜单能正确显示
        var foregroundWindow = GetForegroundWindow();
        uint processId;
        var foregroundThread = GetWindowThreadProcessId(foregroundWindow, out processId);
        var currentThread = GetCurrentThreadId();
        AttachThreadInput(currentThread, foregroundThread, true);

        // 显示菜单并获取选择
        var cmd = TrackPopupMenu(hmenu, 0x100 | 0x002, x, y, 0, _hwnd, IntPtr.Zero);
        // 0x100 = TPM_RETURNCMD, 0x002 = TPM_RIGHTBUTTON

        AttachThreadInput(currentThread, foregroundThread, false);
        DestroyMenu(hmenu);

        switch (cmd)
        {
            case 100:
                DebugLog.Write("Menu: Settings selected");
                onSettings();
                break;
            case 101:
                DebugLog.Write("Menu: Pause selected");
                onPause();
                break;
            case 102:
                DebugLog.Write("Menu: Exit selected");
                onExit();
                break;
        }
    }

    public void UpdateToolTip(string text)
    {
        if (!_isCreated) return;

        var data = new NOTIFYICONDATA();
        data.cbSize = Marshal.SizeOf<NOTIFYICONDATA>();
        data.hWnd = _hwnd;
        data.uID = _notifyId;
        data.uFlags = NIF_TIP;
        data.szTip = text.Length > 127 ? text.Substring(0, 127) : text;

        Shell_NotifyIcon(NIM_MODIFY, ref data);
    }

    public void Dispose()
    {
        if (_isCreated && _hwnd != IntPtr.Zero)
        {
            var data = new NOTIFYICONDATA();
            data.cbSize = Marshal.SizeOf<NOTIFYICONDATA>();
            data.hWnd = _hwnd;
            data.uID = _notifyId;
            Shell_NotifyIcon(NIM_DELETE, ref data);
            _isCreated = false;
        }

        if (_iconHandle != IntPtr.Zero)
        {
            DestroyIcon(_iconHandle);
            _iconHandle = IntPtr.Zero;
        }
    }

    #region Win32 P/Invoke

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern bool Shell_NotifyIcon(int dwMessage, [In] ref NOTIFYICONDATA pnid);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr LoadIcon(IntPtr hInstance, IntPtr lpIconName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadImage(IntPtr hInst, string lpszName, uint uType, int cxDesired, int cyDesired, uint fuLoad);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool SetMenuInfo(IntPtr hmenu, ref MENUINFO lpmi);

    private const uint MIM_STYLE = 0x00000010;
    private const uint MNS_NOCHECK = 0x00000000;

    [StructLayout(LayoutKind.Sequential)]
    private struct MENUINFO
    {
        public uint cbSize;
        public uint fMask;
        public uint dwStyle;
        public uint cyMax;
        public IntPtr hbrBack;
        public uint dwContextHelpID;
        public uint dwMenuData;
    }

    private const uint IMAGE_ICON = 1;
    private const uint LR_LOADFROMFILE = 0x00000010;

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr hIcon);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool AppendMenu(IntPtr hMenu, uint uFlags, uint uIDNewItem, string? lpNewItem);

    [DllImport("user32.dll")]
    private static extern bool SetMenuDefaultItem(IntPtr hMenu, uint uItem, uint fByPosition);

    [DllImport("user32.dll")]
    private static extern int TrackPopupMenu(IntPtr hMenu, uint uFlags, int x, int y, int nReserved, IntPtr hWnd, IntPtr prcRect);

    [DllImport("user32.dll")]
    private static extern bool DestroyMenu(IntPtr hMenu);

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("kernel32.dll")]
    private static extern uint GetCurrentThreadId();

    [DllImport("user32.dll")]
    private static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);

    #endregion

    #region 结构体

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NOTIFYICONDATA
    {
        public int cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)]
        public string szInfo;
        public union_uTimeoutVersion uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)]
        public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct union_uTimeoutVersion
    {
        [FieldOffset(0)] public uint uTimeout;
        [FieldOffset(0)] public uint uVersion;
    }

    #endregion

}
