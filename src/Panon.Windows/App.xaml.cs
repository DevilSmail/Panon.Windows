using Microsoft.UI.Xaml;
using Panon.Windows.Audio;
using Panon.Windows.Helpers;
using Panon.Windows.Overlay;
using Panon.Windows.Settings;
using Panon.Windows.Tray;
using System.Runtime.InteropServices;

namespace Panon.Windows;

/// <summary>
/// 应用入口，管理全局组件生命周期
/// </summary>
public partial class App : Application
{
    private SingleInstance? _singleInstance;
    private TrayIconController? _trayIcon;
    private SettingsManager? _settingsManager;
    private TransparencyChecker? _transparencyChecker;
    private AudioCaptureService? _audioCapture;
    private FftProcessor? _fftProcessor;
    private DecayProcessor? _decayProcessor;
    private LayeredOverlayWindow? _overlayWindow; // 保留向后兼容，始终指向 _overlayWindows[0]
    private readonly List<LayeredOverlayWindow> _overlayWindows = new(); // 多显示器 overlay 列表
    private Window? _settingsWindow;
    private Window? _hiddenMainWindow; // 隐藏主窗口，防止设置关闭时退出
    private bool _isPaused;
    private Microsoft.UI.Dispatching.DispatcherQueue? _uiDispatcher; // UI线程调度器

    // 全局服务访问
    public static SettingsManager Settings => ((App)Current)._settingsManager!;
    public static AudioCaptureService AudioCapture => ((App)Current)._audioCapture!;
    public static FftProcessor Fft => ((App)Current)._fftProcessor!;
    public static DecayProcessor Decay => ((App)Current)._decayProcessor!;
    public static TransparencyChecker Transparency => ((App)Current)._transparencyChecker!;
    public static LayeredOverlayWindow? Overlay => ((App)Current)._overlayWindow; // 始终指向第一个 overlay（主显示器）

    /// <summary>
    /// 将设置应用到所有 overlay（即时生效）
    /// </summary>
    public static void ApplySettingsToAllOverlays(AppSettings settings)
    {
        var app = (App)Current;
        foreach (var overlay in app._overlayWindows)
            overlay.ApplySettings(settings);
    }

    /// <summary>
    /// 当目标显示器切换时，销毁旧 overlay 并重新创建
    /// </summary>
    public static void RecreateOverlays(string targetMonitor)
    {
        var app = (App)Current;
        app.CreateOverlays(targetMonitor);
        app.ApplySettings(app._settingsManager!.Current);
        // 恢复设置窗口句柄（如果设置窗口已打开）
        if (app._settingsWindow != null)
        {
            var settingsHwnd = WinRT.Interop.WindowNative.GetWindowHandle(app._settingsWindow);
            foreach (var overlay in app._overlayWindows)
                overlay.SetSettingsHwnd(settingsHwnd);
        }
        DebugLog.Write($"Overlays recreated for TargetMonitor={targetMonitor}, count={app._overlayWindows.Count}");
    }

    public App()
    {
        // 全局异常捕获
        AppDomain.CurrentDomain.UnhandledException += (s, e) =>
        {
            var ex = e.ExceptionObject as Exception;
            System.IO.File.WriteAllText(
                System.IO.Path.Combine(System.IO.Path.GetTempPath(), "panon_crash.txt"),
                $"[{DateTime.Now}] UnhandledException: {ex?.ToString() ?? e.ExceptionObject?.ToString() ?? "null"}\n");
        };

        TaskScheduler.UnobservedTaskException += (s, e) =>
        {
            System.IO.File.WriteAllText(
                System.IO.Path.Combine(System.IO.Path.GetTempPath(), "panon_crash_task.txt"),
                $"[{DateTime.Now}] UnobservedTaskException: {e.Exception?.ToString() ?? "null"}\n");
        };

        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            LaunchInternal(args);
        }
        catch (Exception ex)
        {
            System.IO.File.WriteAllText(
                System.IO.Path.Combine(System.IO.Path.GetTempPath(), "panon_crash_launch.txt"),
                $"[{DateTime.Now}] LaunchException: {ex}\n");
            throw;
        }
    }

    private void LaunchInternal(LaunchActivatedEventArgs args)
    {
        // 0. 保存UI线程的 DispatcherQueue（用于从后台线程调度UI操作）
        _uiDispatcher = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();

        // 0.5 创建隐藏主窗口（防止设置窗口关闭时退出应用）
        _hiddenMainWindow = new Window();
        _hiddenMainWindow.AppWindow.MoveInZOrderAtBottom(); // 放到最底层

        // 1. 单实例检查
        _singleInstance = new SingleInstance();
        if (!_singleInstance.TryAcquire())
        {
            _singleInstance.Dispose();
            Exit();
            return;
        }

        // 2. 初始化设置
        _settingsManager = new SettingsManager();

        // 3. 初始化透明度检测器，记录原始注册表状态（首次启动快照）
        _transparencyChecker = new TransparencyChecker();
        _transparencyChecker.CaptureOriginalState();
        // 同步运行态：如果注册表已为 1,1（上次运行开启后保留），标记为已开启
        if (_transparencyChecker.IsTransparencyEnabled && _transparencyChecker.IsOLEDTaskbarTransparencyEnabled)
            _transparencyChecker.MarkEnabled();

        // 4. 初始化音频引擎
        _audioCapture = new AudioCaptureService();
        _fftProcessor = new FftProcessor();
        _decayProcessor = new DecayProcessor();
        // 连接音频处理管线
        _audioCapture.DataAvailable += OnAudioDataAvailable;

        // 5. 初始化系统托盘
        _trayIcon = new TrayIconController();
        _trayIcon.OpenSettingsRequested += OpenSettingsWindow;
        _trayIcon.TogglePauseRequested += TogglePause;
        _trayIcon.ExitRequested += ExitApp;
        _trayIcon.TaskbarRestarted += () =>
        {
            DebugLog.Write("App: Explorer restarted — recreating overlays");
            _uiDispatcher?.TryEnqueue(() =>
            {
                RecreateOverlays(_settingsManager!.Current.TargetMonitor);
            });
        };
        _trayIcon.Initialize();

        // 6. 创建任务栏覆盖窗口（Win32 分层窗口，真透明）
        // 支持多显示器：TargetMonitor="0"(主显示器), "1"/"2"(指定), "-1"(所有)
        CreateOverlays(_settingsManager.Current.TargetMonitor);

        DebugLog.Write("Overlay Window Created");

        // 7. 应用设置并启动音频捕获
        ApplySettings(_settingsManager.Current);
        _audioCapture.Start();
    }

    private void OnAudioDataAvailable(float[] samples, NAudio.Wave.WaveFormat format)
    {
        _fftProcessor?.Process(samples, format);
    }

    private void ApplySettings(AppSettings settings)
    {
        if (_fftProcessor != null)
        {
            _fftProcessor.BassResolutionLevel = settings.BassResolutionLevel;
            _fftProcessor.ReduceBass = settings.ReduceBass;
        }
        foreach (var overlay in _overlayWindows)
            overlay.ApplySettings(settings);
    }

    /// <summary>
    /// 根据 TargetMonitor 设置创建所有 overlay
    /// "0"=主显示器(默认), "1"/"2"...=指定显示器, "-1"=所有显示器
    /// </summary>
    private void CreateOverlays(string targetMonitor)
    {
        // 先清理旧的
        DestroyOverlays();

        var helper = new TaskbarHelper();

        if (targetMonitor == "-1")
        {
            // 所有显示器：每个显示器创建独立 overlay
            var allTaskbars = helper.GetAllTaskbarInfos();
            foreach (var tbi in allTaskbars)
            {
                var overlay = new LayeredOverlayWindow();
                overlay.Create(tbi);
                _overlayWindows.Add(overlay);
                DebugLog.Write($"Overlay created on monitor {tbi.MonitorIndex}: {tbi.Width}x{tbi.Height} at ({tbi.X},{tbi.Y})");
            }
        }
        else
        {
            int index;
            if (int.TryParse(targetMonitor, out index))
            {
                var tbi = helper.GetTaskbarInfoByIndex(index) ?? helper.GetTaskbarInfo();
                var overlay = new LayeredOverlayWindow();
                overlay.Create(tbi);
                _overlayWindows.Add(overlay);
                DebugLog.Write($"Overlay created on monitor {tbi.MonitorIndex}: {tbi.Width}x{tbi.Height} at ({tbi.X},{tbi.Y})");
            }
            else
            {
                // 解析失败，回退主显示器
                var overlay = new LayeredOverlayWindow();
                overlay.Create();
                _overlayWindows.Add(overlay);
            }
        }

        // 更新兼容引用
        _overlayWindow = _overlayWindows.Count > 0 ? _overlayWindows[0] : null;

        DebugLog.Write($"Total overlays created: {_overlayWindows.Count}");
    }

    /// <summary>
    /// 销毁所有 overlay
    /// </summary>
    private void DestroyOverlays()
    {
        foreach (var overlay in _overlayWindows)
            overlay.Dispose();
        _overlayWindows.Clear();
        _overlayWindow = null;
    }

    /// <summary>
    /// 拦截设置窗口关闭事件，改为隐藏而非销毁
    /// </summary>
    private void OnSettingsClosing(object sender, Microsoft.UI.Windowing.AppWindowClosingEventArgs e)
    {
        e.Cancel = true;
        _settingsWindow?.AppWindow.Hide();
        foreach (var overlay in _overlayWindows)
        {
            overlay.SetSettingsHwnd(IntPtr.Zero);
            overlay.Refresh();
        }
        DebugLog.Write("Settings Window Hidden (not closed)");
    }

    private void OpenSettingsWindow()
    {
        try
        {
            DebugLog.Write("OpenSettingsWindow");

            _uiDispatcher?.TryEnqueue(() =>
            {
                try
                {
                    if (_settingsWindow == null)
                    {
                        _settingsWindow = new MainWindow();
                        _settingsWindow.AppWindow.Closing += OnSettingsClosing;
                        _settingsWindow.Closed += (_, _) =>
                        {
                            _settingsWindow = null;
                        };
                        DebugLog.Write("Settings Window Created");
                    }

                    if (_settingsWindow != null)
                    {
                        if (!_settingsWindow.AppWindow.IsVisible)
                            _settingsWindow.AppWindow.Show();
                        _settingsWindow.Activate();

                        var settingsHwnd = WinRT.Interop.WindowNative.GetWindowHandle(_settingsWindow);
                        foreach (var overlay in _overlayWindows)
                            overlay.SetSettingsHwnd(settingsHwnd);
                    }
                }
                catch (Exception ex2)
                {
                    DebugLog.Write($"Settings Window Error: {ex2.GetType().FullName}: {ex2.Message}");
                    DebugLog.Write($"Settings Window StackTrace: {ex2.StackTrace}");
                    if (ex2.InnerException != null)
                        DebugLog.Write($"Settings Window Inner: {ex2.InnerException.GetType().FullName}: {ex2.InnerException.Message}");
                }
            });
        }
        catch (Exception ex)
        {
            DebugLog.Write($"OpenSettingsWindow Error: {ex.Message}");
        }
    }

    private void TogglePause()
    {
        _isPaused = !_isPaused;

        if (_isPaused)
        {
            // 仅停止音频捕获，不隐藏窗口
            // 依赖 _lastSpectrumUpdateTime 过期检测（>200ms 用零值）+ 指数衰减
            // 让频谱平滑回落到 2px 彩色细线（待机状态）
            _audioCapture?.Stop();
            // 立即触发衰减，跳过 200ms 冻结期，避免频谱冻结后突然下降
            foreach (var overlay in _overlayWindows)
                overlay.ForceDecay();
            _trayIcon?.UpdateToolTip("Panon - 已暂停");
        }
        else
        {
            _audioCapture?.Start();
            _trayIcon?.UpdateToolTip("Panon - 运行中");
        }
    }

    private void ExitApp()
    {
        // 先停止音频捕获，让频谱通过衰减机制自然回落
        _audioCapture?.Stop();
        // 立即触发衰减，使用 ExitFactor（柱身指数 0.80 + 峰值线减法 0.08）
        foreach (var overlay in _overlayWindows)
            overlay.ForceDecay(useExitFactor: true);

        // 轮询检测所有 overlay 的频谱是否都已衰减到 2px 细线状态
        // 必须同时检测柱身（DecayProcessor）和峰值线（SpectrumRenderer）
        var maxWait = DateTime.Now.AddMilliseconds(800);
        while (DateTime.Now < maxWait)
        {
            bool allDone = true;
            foreach (var overlay in _overlayWindows)
            {
                float barMax = overlay.GetMaxDecayedValue();
                float peakMax = overlay.GetMaxPeakHeight();
                if (barMax >= 0.05f || peakMax >= 0.05f)
                {
                    allDone = false;
                    break;
                }
            }
            if (allDone)
                break;
            System.Threading.Thread.Sleep(16);
        }

        // 释放所有 overlay
        foreach (var overlay in _overlayWindows)
            overlay.Dispose();
        _overlayWindows.Clear();

        // 仅移除托盘图标（不 Join 消息线程，避免自死锁）
        _trayIcon?.RemoveIcon();

        // TerminateProcess 强制终止进程
        TerminateProcess(GetCurrentProcess(), 0);
    }

    [DllImport("kernel32.dll")]
    private static extern void TerminateProcess(IntPtr hProcess, uint uExitCode);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();
}
