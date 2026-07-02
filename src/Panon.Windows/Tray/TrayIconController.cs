using Panon.Windows.Helpers;

namespace Panon.Windows.Tray;

/// <summary>
/// 系统托盘图标控制器 - 使用 Win32 原生 API
/// 通过 MessageWindow 接收回调消息，支持右键菜单
/// </summary>
public sealed class TrayIconController : IDisposable
{
    private MessageWindow? _msgWindow;
    private Thread? _msgThread;
    private bool _disposed;

    public event Action? OpenSettingsRequested;
    public event Action? TogglePauseRequested;
    public event Action? ExitRequested;
    /// <summary>Explorer 重启后触发，需重建覆盖窗口和托盘图标</summary>
    public event Action? TaskbarRestarted;

    /// <summary>
    /// 初始化托盘图标（在后台线程运行消息循环）
    /// </summary>
    public void Initialize()
    {
        if (_msgWindow != null) return;

        // 在独立线程创建消息窗口（Win32 要求）
        _msgThread = new Thread(MessageLoop)
        {
            IsBackground = true,
            Name = "TrayMessageLoop"
        };
        _msgThread.Start();

        DebugLog.Write("TrayIconController: Initialize started");
    }

    /// <summary>
    /// 消息循环线程入口
    /// </summary>
    private void MessageLoop()
    {
        try
        {
            var msgWnd = new MessageWindow();
            msgWnd.Create();
            _msgWindow = msgWnd;

            // 绑定事件
            msgWnd.OnTrayLeftClick += () =>
            {
                DebugLog.Write("Tray LeftClick → OpenSettings");
                OpenSettingsRequested?.Invoke();
            };

            msgWnd.OnTrayDoubleClick += () =>
            {
                DebugLog.Write("Tray DoubleClick → OpenSettings");
                OpenSettingsRequested?.Invoke();
            };

            msgWnd.OnTrayRightClick += () =>
            {
                DebugLog.Write("Tray RightClick → ShowContextMenu");
                msgWnd.ShowContextMenu(
                    () => OpenSettingsRequested?.Invoke(),
                    () => TogglePauseRequested?.Invoke(),
                    () => ExitRequested?.Invoke());
            };

            // Explorer 重启：重注册托盘图标 + 通知 App 重建覆盖窗口
            msgWnd.OnTaskbarRestarted += () =>
            {
                DebugLog.Write("Tray: TaskbarRestarted → re-initializing tray icon");
                msgWnd.TrayIcon?.Dispose();
                var newTray = new NativeTrayIcon();
                newTray.Initialize(msgWnd.Handle, 1);
                newTray.LeftClick += () => OpenSettingsRequested?.Invoke();
                newTray.DoubleClick += () => OpenSettingsRequested?.Invoke();
                newTray.RightClick += () => msgWnd.ShowContextMenu(
                    () => OpenSettingsRequested?.Invoke(),
                    () => TogglePauseRequested?.Invoke(),
                    () => ExitRequested?.Invoke());
                TaskbarRestarted?.Invoke();
            };

            // 运行消息循环
            msgWnd.RunMessageLoop();

            // 消息循环退出后，在所属线程清理（DestroyWindow 必须在创建窗口的线程调用）
            msgWnd.Dispose();
        }
        catch (Exception ex)
        {
            DebugLog.Write($"Tray thread error: {ex.Message}");
        }
    }

    public void UpdateToolTip(string text)
    {
        // 原生托盘的 tooltip 通过 NativeTrayIcon 更新
        // 这里暂时记录日志
    }

    /// <summary>
    /// 仅移除托盘图标（Shell_NotifyIcon NIM_DELETE），不停止消息循环、不 Join 线程。
    /// 用于退出时快速移除图标，避免 Dispose 中的 _msgThread.Join 自死锁。
    /// </summary>
    public void RemoveIcon()
    {
        _msgWindow?.TrayIcon?.Dispose();
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        // 通过 PostMessage 通知消息循环线程退出（线程安全）
        _msgWindow?.RequestShutdown();
        // 等待消息循环线程在所属线程自行清理并退出
        // （DestroyWindow 必须由创建窗口的线程调用，不能跨线程）
        _msgThread?.Join(1000);
        _msgWindow = null;
        DebugLog.Write("TrayIconController Disposed");
    }

}
