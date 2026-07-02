using System.Runtime.InteropServices;
using System.Text;
using Panon.Windows.Helpers;

namespace Panon.Windows.Tray;

/// <summary>
/// 隐藏的消息窗口，用于接收系统托盘图标回调
/// </summary>
internal sealed class MessageWindow : IDisposable
{
    private IntPtr _hwnd = IntPtr.Zero;
    private NativeTrayIcon? _trayIcon;

    // 消息窗口类名
    private const string WND_CLASS = "Panon_MessageWindow_2024";

    // 窗口过程委托（必须保持引用防止GC回收）
    private delegate IntPtr WndProcDelegate(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
    private WndProcDelegate? _wndProcDelegate;

    public event Action? OnTrayLeftClick;
    public event Action? OnTrayDoubleClick;
    public event Action? OnTrayRightClick;
    /// <summary>Explorer 重启后触发（TaskbarCreated 消息），需重建托盘图标和覆盖窗口</summary>
    public event Action? OnTaskbarRestarted;

    public NativeTrayIcon? TrayIcon => _trayIcon;
    public IntPtr Handle => _hwnd;

    // TaskbarCreated 消息 ID（Explorer 重启时广播），运行时注册
    private static uint _taskbarCreatedMsg;
    private static bool _taskbarMsgRegistered;
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterWindowMessage(string lpString);

    /// <summary>
    /// 创建隐藏消息窗口并初始化托盘图标
    /// </summary>
    public void Create()
    {
        // 注册窗口类
        var wc = new WNDCLASS();
        wc.lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_wndProcDelegate = WindowProc);
        wc.hInstance = GetModuleHandle(null);
        wc.lpszClassName = WND_CLASS;

        var atom = RegisterClass(ref wc);
        if (atom == 0)
        {
            DebugLog.Write($"RegisterClass failed: {Marshal.GetLastWin32Error()}");
            return;
        }

        // 创建消息窗口（不可见）
        _hwnd = CreateWindowEx(
            0,
            WND_CLASS,
            "Panon Hidden Message Window",
            0,
            0, 0, 0, 0,
            IntPtr.Zero,   // 无父窗口
            IntPtr.Zero,   // 无菜单
            GetModuleHandle(null),
            IntPtr.Zero);

        if (_hwnd == IntPtr.Zero)
        {
            DebugLog.Write($"CreateWindow failed: {Marshal.GetLastWin32Error()}");
            return;
        }

        // 注册 TaskbarCreated 消息（Explorer 重启广播）
        if (!_taskbarMsgRegistered)
        {
            _taskbarCreatedMsg = RegisterWindowMessage("TaskbarCreated");
            _taskbarMsgRegistered = true;
        }

        // 初始化托盘图标
        _trayIcon = new NativeTrayIcon();
        _trayIcon.Initialize(_hwnd, 1);
        _trayIcon.LeftClick += () => OnTrayLeftClick?.Invoke();
        _trayIcon.DoubleClick += () => OnTrayDoubleClick?.Invoke();
        _trayIcon.RightClick += () => OnTrayRightClick?.Invoke();

        DebugLog.Write($"MessageWindow OK, hwnd={_hwnd}");
    }

    /// <summary>
    /// 显示右键菜单（从光标位置）
    /// </summary>
    public void ShowContextMenu(Action onSettings, Action onPause, Action onExit)
    {
        if (_trayIcon == null) return;

        GetCursorPos(out var pt);
        _trayIcon.ShowContextMenu(pt.X, pt.Y, onSettings, onPause, onExit);
    }

    /// <summary>
    /// 请求关闭消息窗口（线程安全，可从任意线程调用）
    /// 通过 PostMessage 发送 WM_CLOSE，由创建窗口的线程处理销毁与清理
    /// </summary>
    public void RequestShutdown()
    {
        if (_hwnd != IntPtr.Zero)
        {
            PostMessage(_hwnd, WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
        }
    }

    /// <summary>
    /// 窗口过程 - 处理托盘回调消息
    /// </summary>
    private IntPtr WindowProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        if (msg == _taskbarCreatedMsg && _taskbarCreatedMsg != 0)
        {
            DebugLog.Write("TaskbarCreated received — Explorer restarted");
            OnTaskbarRestarted?.Invoke();
            return IntPtr.Zero;
        }
        else if (msg == NativeTrayIcon.WM_TRAY_CALLBACK && _trayIcon != null)
        {
            if (_trayIcon.HandleMessage(wParam, lParam))
                return IntPtr.Zero;
        }
        else if (msg == 0x0002) // WM_DESTROY
        {
            PostQuitMessage(0);
        }

        return DefWindowProc(hWnd, msg, wParam, lParam);
    }

    /// <summary>
    /// 处理消息循环（在单独线程运行）
    /// </summary>
    public void RunMessageLoop()
    {
        var msg = new MSG();
        while (GetMessage(out msg, IntPtr.Zero, 0, 0))
        {
            TranslateMessage(ref msg);
            DispatchMessage(ref msg);
        }
    }

    public void Dispose()
    {
        _trayIcon?.Dispose();
        _trayIcon = null;

        if (_hwnd != IntPtr.Zero)
        {
            DestroyWindow(_hwnd);
            _hwnd = IntPtr.Zero;
        }

        UnregisterClass(WND_CLASS, GetModuleHandle(null));
    }

    #region Win32 P/Invoke

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern ushort RegisterClass([In] ref WNDCLASS lpWndClass);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateWindowEx(
        uint dwExStyle, string lpClassName, string lpWindowName,
        uint dwStyle, int x, int y, int nWidth, int nHeight,
        IntPtr hWndParent, IntPtr hMenu, IntPtr hInstance, IntPtr lpParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool UnregisterClass(string lpClassName, IntPtr hInstance);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool GetMessage(out MSG lpMsg, IntPtr hWnd, uint wMsgFilterMin, uint wMsgFilterMax);

    [DllImport("user32.dll")]
    private static extern bool TranslateMessage(ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern IntPtr DispatchMessage(ref MSG lpMsg);

    [DllImport("user32.dll")]
    private static extern void PostQuitMessage(int nExitCode);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? lpModuleName);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);

    private const uint WM_CLOSE = 0x0010;

    #endregion

    #region 结构体

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WNDCLASS
    {
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string lpszMenuName;
        public string lpszClassName;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MSG
    {
        public IntPtr hwnd;
        public uint message;
        public IntPtr wParam;
        public IntPtr lParam;
        public uint time;
        public POINT pt;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int X;
        public int Y;
    }

    #endregion

}
