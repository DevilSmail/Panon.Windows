using System.Runtime.InteropServices;
using System.Timers;
using Panon.Windows.Audio;
using Panon.Windows.Helpers;
using Panon.Windows.Settings;
using Panon.Windows.Shader;

namespace Panon.Windows.Overlay;

/// <summary>
/// 任务栏覆盖窗口 - 使用 Win32 分层窗口实现真透明
/// 通过 UpdateLayeredWindow + 纯软件渲染实现 per-pixel alpha
/// </summary>
public sealed class LayeredOverlayWindow : IDisposable
{
    private IntPtr _hwnd = IntPtr.Zero;
    private IntPtr _hBitmap = IntPtr.Zero;       // DIB section bitmap
    private IntPtr _hDcScreen = IntPtr.Zero;      // 屏幕 DC
    private IntPtr _hDcMem = IntPtr.Zero;          // 内存 DC
    private IntPtr _pBits = IntPtr.Zero;           // 像素数据指针

    private int _width, _height;
    private bool _isRunning;
    private readonly object _spectrumLock = new();
    private SpectrumData _lastSpectrum = new()
    {
        LeftChannel = new float[76],  // 预分配，确保启动时能渲染底部线
        RightChannel = new float[76]
    };
    private DateTime _lastSpectrumUpdateTime = DateTime.MinValue;
    private System.Timers.Timer? _updateTimer;  // 不依赖 UI 线程消息循环

    // 渲染器（纯软件）
    private SpectrumRenderer? _renderer;

    // 每个 overlay 独立的衰减处理器（避免多 overlay 共享 DecayProcessor 导致状态冲突）
    private readonly DecayProcessor _decayProcessor = new();

    // Win32 窗口样式
    private const int GWL_STYLE = -16;
    private const int GWL_EXSTYLE = -20;
    private const uint WS_POPUP = 0x80000000;
    private const uint WS_VISIBLE = 0x10000000;
    private const uint WS_EX_TOPMOST = 0x00000008;
    private const uint WS_EX_TRANSPARENT = 0x00000020;
    private const uint WS_EX_TOOLWINDOW = 0x00000080;
    private const uint WS_EX_LAYERED = 0x00080000;
    private const uint WS_EX_NOACTIVATE = 0x08000000;
    private const uint SWP_NOMOVE = 0x0002;
    private const uint SWP_NOSIZE = 0x0001;
    private const uint SWP_NOACTIVATE = 0x0010;
    private static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);
    private static readonly IntPtr HWND_NOTOPMOST = new IntPtr(-2);

    // 任务栏窗口句柄（用于 Z-order 定位）
    private IntPtr _taskbarHwnd = IntPtr.Zero;

    // 设置窗口句柄（打开设置时设置，关闭时清除）
    private IntPtr _settingsHwnd = IntPtr.Zero;

    // 缓存的 TaskbarHelper（避免每帧分配）
    private readonly TaskbarHelper _taskbarHelper = new();

    /// <summary>覆盖模式: 1=Under(任务栏覆盖在频谱上面,默认), 2=Above(频谱覆盖在任务栏上面)</summary>
    private int _overlayMode = 1;

    // DWM 常量
    private const int DWMWA_EXCLUDED_FROM_PEEK = 12;

    // Z-order 维护：定时器原子操作保持 overlay 在 TOPMOST 组底层
    // overlay→TOPMOST（高于所有普通窗口），taskbar→TOPMOST（高于 overlay）
    // 使用 BeginDeferWindowPos/EndDeferWindowPos 原子提交，无闪烁
    // 使用定时器而非 WinEvent 钩子，避免干扰设置窗口激活
    private System.Timers.Timer? _zOrderTimer;

    // 窗口类名
    private const string WND_CLASS = "Panon_Overlay_2024";

    /// <summary>
    /// 创建分层窗口并定位到指定任务栏
    /// </summary>
    /// <param name="taskbarInfo">任务栏信息，为 null 时自动获取主显示器任务栏</param>
    public void Create(TaskbarInfo? taskbarInfo = null)
    {
        if (_hwnd != IntPtr.Zero) return;

        taskbarInfo ??= new TaskbarHelper().GetTaskbarInfo();

        _width = taskbarInfo.Width;
        _height = taskbarInfo.Height; // 窗口高度 = 任务栏高度

        // 频谱窗口与任务栏完全重叠
        int overlayX = taskbarInfo.X;
        int overlayY = taskbarInfo.Y;

        // 保存任务栏句柄（用于 Z-order 维护）
        _taskbarHwnd = taskbarInfo.TaskbarHwnd;

        // 注册窗口类
        var wc = new WNDCLASSA();
        wc.style = 0;
        wc.lpfnWndProc = Marshal.GetFunctionPointerForDelegate<WndProcDelegate>(DefWndProc);
        wc.hInstance = GetModuleHandleA(null);
        wc.hCursor = LoadCursorA(IntPtr.Zero, (IntPtr)32512); // IDC_ARROW
        wc.lpszClassName = WND_CLASS;

        RegisterClassA(ref wc);

        // 创建分层窗口（TOPMOST 确保高于所有普通窗口，包括设置窗口）
        _hwnd = CreateWindowExA(
            WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            WND_CLASS,
            "Panon Overlay",
            WS_POPUP, // 无标题栏
            overlayX, overlayY, _width, _height,
            IntPtr.Zero, IntPtr.Zero, GetModuleHandleA(null), IntPtr.Zero);

        if (_hwnd == IntPtr.Zero)
        {
            DebugLog.Write($"CreateWindow failed: {Marshal.GetLastWin32Error()}");
            return;
        }

        // 创建 DIB section 用于渲染
        CreateDibSection(_width, _height);

        // 初始化纯软件渲染器
        _renderer = new SpectrumRenderer();
        _renderer.InitializeSoftware(_width, _height);

        // 订阅频谱数据
        App.Fft.SpectrumUpdated += OnSpectrumUpdated;

        // 启动渲染定时器（使用 System.Timers.Timer，不依赖 UI 消息循环）
        _updateTimer = new System.Timers.Timer(33); // ~30 FPS
        _updateTimer.Elapsed += OnUpdateTick;
        _updateTimer.AutoReset = true;
        _updateTimer.Start();
            DebugLog.Write($"Timer started: {_updateTimer.Enabled}");
        _isRunning = true;

        // 设置 DWM 属性：排除在 Peek 预览中
        int excludedFromPeek = 1;
        DwmSetWindowAttribute(_hwnd, DWMWA_EXCLUDED_FROM_PEEK, ref excludedFromPeek, 4);

        // Z-order：原子操作确保 overlay→TOPMOST，taskbar→TOPMOST（覆盖 overlay）
            DebugLog.Write($"Taskbar HWND from TaskbarInfo: {_taskbarHwnd}");

        EnsureZOrder();

        // 定时器：每 200ms 原子维护 Z-order + 任务栏显隐同步（不干扰设置窗口）
        _zOrderTimer = new System.Timers.Timer(200);
        _zOrderTimer.Elapsed += (_, _) =>
        {
            if (_hwnd == IntPtr.Zero) return;

            // 任务栏隐藏时同步隐藏频谱，显示时恢复
            bool taskbarVisible = _taskbarHwnd != IntPtr.Zero && IsWindowVisible(_taskbarHwnd);
            if (taskbarVisible && !_isRunning)
            {
                ShowWindow(_hwnd, 1);
                _isRunning = true;
            }
            else if (!taskbarVisible && _isRunning)
            {
                ShowWindow(_hwnd, 0);
                _isRunning = false;
                return; // 已隐藏，无需维护 Z-order
            }

            if (_isRunning)
                EnsureZOrder();
        };
        _zOrderTimer.AutoReset = true;
        _zOrderTimer.Start();

        // 首次显示
        UpdateLayeredWindow();
        ShowWindow(_hwnd, 1); // SW_SHOW

            DebugLog.Write($"LayeredOverlay OK: {_width}x{_height} at ({overlayX},{overlayY}), taskbar=({taskbarInfo.X},{taskbarInfo.Y},{taskbarInfo.Width}x{taskbarInfo.Height})");
    }

    /// <summary>
    /// 创建 32bpp ARGB DIB Section
    /// </summary>
    private void CreateDibSection(int width, int height)
    {
        _hDcScreen = GetDC(IntPtr.Zero);
        _hDcMem = CreateCompatibleDC(_hDcScreen);

        var bmi = new BITMAPINFOHEADER();
        bmi.biSize = Marshal.SizeOf<BITMAPINFOHEADER>();
        bmi.biWidth = width;
        bmi.biHeight = -height; // 自上而下
        bmi.biPlanes = 1;
        bmi.biBitCount = 32;
        bmi.biCompression = 0; // BI_RGB

        _hBitmap = CreateDIBSection(
            _hDcScreen, ref bmi, 0, out _pBits, IntPtr.Zero, 0);

        SelectObject(_hDcMem, _hBitmap);
    }

    /// <summary>
    /// 定时器回调：更新渲染
    /// </summary>
    private void OnUpdateTick(object? sender, ElapsedEventArgs e)
    {
        if (!_isRunning || _renderer == null || _hwnd == IntPtr.Zero) return;

        SpectrumData currentSpectrum;
        lock (_spectrumLock)
        {
            // 如果频谱数据超过 200ms 未更新（音频捕获已停止），使用零值
            // 让衰减处理器自然衰减到零，最终显示底部彩色细线
            if (_lastSpectrumUpdateTime != DateTime.MinValue &&
                (DateTime.Now - _lastSpectrumUpdateTime).TotalMilliseconds > 200)
            {
                currentSpectrum = new SpectrumData
                {
                    LeftChannel = new float[76],
                    RightChannel = new float[76],
                    Volume = 0
                };
            }
            else
            {
                currentSpectrum = _lastSpectrum;
            }
        }

        try
        {
            // 空白区域填充模式：每帧重算空白区域（UIA 500ms 缓存）
            if (_renderer.FillMode == 1)
            {
                var regions = _taskbarHelper.GetTaskbarFreeRegions(Math.Max(4, _renderer.BarWidth), _taskbarHwnd);
                _renderer.FreeRegions = regions;
            }
            else if (_renderer.FillMode == 0)
            {
                _renderer.FreeRegions = null;
            }

            var processed = _decayProcessor.Process(currentSpectrum);

            // 渲染到内存像素缓冲区
            _renderer.RenderToPixels(processed.LeftChannel, processed.RightChannel, _pBits, _width, _height);

            // 更新分层窗口显示
            UpdateLayeredWindow();

            // 诊断：记录渲染（前 10 次 + 每隔 5 秒一次）
            _renderCount++;
            if (_renderCount <= 10 || (DateTime.Now - _lastRenderTime).TotalSeconds > 5)
            {
                // 检查像素数据是否非零
                int nonZero = 0;
                int total = _width * _height;
                unsafe
                {
                    int* p = (int*)_pBits;
                    for (int i = 0; i < total; i++)
                    {
                        if (p[i] != 0) nonZero++;
                    }
                }
            DebugLog.Write($"Render #{_renderCount}: decayMax={processed.LeftChannel.Max():F4}, bars={processed.LeftChannel.Length}, hwnd={_hwnd}, pixels_nonzero={nonZero}/{total}");
                _lastRenderTime = DateTime.Now;
            }
        }
        catch (Exception ex)
        {
            DebugLog.Write($"Render Error: {ex.Message}\n{ex.StackTrace}");
        }
    }

    /// <summary>
    /// 调用 UpdateLayeredWindow 显示带 alpha 的内容
    /// </summary>
    private unsafe void UpdateLayeredWindow()
    {
        if (_hwnd == IntPtr.Zero || _pBits == IntPtr.Zero) return;

        var blend = new BLENDFUNCTION
        {
            BlendOp = 0,     // AC_SRC_OVER
            BlendFlags = 0,
            SourceConstantAlpha = 255, // 不透明度
            AlphaFormat = 1   // AC_SRC_ALPHA (per-pixel alpha)
        };

        var ptSize = new POINT { X = _width, Y = _height };
        var ptSrc = new POINT { X = 0, Y = 0 };

        bool ok = UpdateLayeredWindow(
            _hwnd,
            IntPtr.Zero,         // hWndDest (null=当前位置)
            IntPtr.Zero,         // pptDest (null=当前位置)
            ref ptSize,          // psize
            _hDcMem,             // hdcSrc
            ref ptSrc,           // pptSrc
            0,                   // crKey (不用色键)
            ref blend,           // pblend
            0x00000002);         // ULW_ALPHA

        if (!ok)
        {
            int err = Marshal.GetLastWin32Error();
            _ulwErrorCount++;
            // 前 10 次 + 每隔 30 秒记录一次
            if (_ulwErrorCount <= 10 || (DateTime.Now - _lastUlwErrorTime).TotalSeconds > 30)
            {
            DebugLog.Write($"UpdateLayeredWindow FAILED #{_ulwErrorCount}: error={err}, hwnd={_hwnd}");
                _lastUlwErrorTime = DateTime.Now;
            }
        }
    }

    private int _ulwErrorCount;
    private DateTime _lastUlwErrorTime = DateTime.MinValue;

    private int _spectrumLogCount;
    private int _renderCount;
    private DateTime _lastRenderTime = DateTime.MinValue;
    private DateTime _lastSpectrumLogTime = DateTime.MinValue;

    private void OnSpectrumUpdated(SpectrumData data)
    {
        lock (_spectrumLock)
        {
            _lastSpectrum = data;
            _lastSpectrumUpdateTime = DateTime.Now;
        }
        // 诊断：持续记录前 20 次 + 每 5 秒一次
        _spectrumLogCount++;
        if (_spectrumLogCount <= 20 || (DateTime.Now - _lastSpectrumLogTime).TotalSeconds > 5)
        {
            float maxVal = data.LeftChannel.Length > 0 ? data.LeftChannel.Max() : 0;
            DebugLog.Write($"Spectrum #{_spectrumLogCount}: {data.LeftChannel.Length} bars, max={maxVal:F4}");
            _lastSpectrumLogTime = DateTime.Now;
        }
    }

    /// <summary>
    /// 默认窗口过程
    /// </summary>
    private static IntPtr DefWndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        return DefWindowProcA(hWnd, msg, wParam, lParam);
    }

    public void Show()
    {
        if (_hwnd != IntPtr.Zero)
        {
            ShowWindow(_hwnd, 1); // SW_SHOW
            _isRunning = true;
        }
    }

    public void Hide()
    {
        if (_hwnd != IntPtr.Zero)
        {
            ShowWindow(_hwnd, 0); // SW_HIDE
            _isRunning = false;
        }
    }

    /// <summary>
    /// 立即触发衰减模式（跳过 200ms 冻结期）
    /// 用于暂停/退出时让频谱立即开始平滑回落，避免冻结后突然下降
    /// </summary>
    public void ForceDecay(bool useExitFactor = false)
    {
        lock (_spectrumLock)
        {
            // 将更新时间设为很久以前，让 OnUpdateTick 下一帧立即进入过期衰减模式
            _lastSpectrumUpdateTime = DateTime.Now.AddSeconds(-1);
        }
        // 启用退出专用衰减因子（极快衰减），仅影响后续 Process 调用
        if (useExitFactor)
        {
            _decayProcessor.UseExitFactor = true;
            _renderer!.UseExitFactor = true; // 峰值线也用退出专用衰减，与柱身同步回落
        }
    }

    /// <summary>
    /// 获取当前峰值线的最大高度（用于退出时检测峰值线是否已回落到 2px 细线状态）
    /// </summary>
    public float GetMaxPeakHeight()
    {
        return _renderer?.GetMaxPeakHeight() ?? 0f;
    }

    /// <summary>
    /// 获取当前衰减的最大值（用于退出时检测柱身是否已回落到细线状态）
    /// </summary>
    public float GetMaxDecayedValue()
    {
        return _decayProcessor.GetMaxDecayedValue();
    }

    /// <summary>
    /// 应用设置变更（即时生效）
    /// </summary>
    public void ApplySettings(AppSettings settings)
    {
        if (_renderer != null)
        {
            _renderer.VisualEffectName = settings.VisualEffectName;
            _renderer.Gravity = settings.Gravity;
            _renderer.Inversion = settings.Inversion;
            _renderer.ColorSpaceHSLuv = settings.ColorSpaceHSLuv;
            _renderer.HslHueFrom = settings.HslHueFrom;
            _renderer.HslHueTo = settings.HslHueTo;
            _renderer.HslSaturation = settings.HslSaturation;
            _renderer.HslLightness = settings.HslLightness;
            _renderer.HsluvHueFrom = settings.HsluvHueFrom;
            _renderer.HsluvHueTo = settings.HsluvHueTo;
            _renderer.HsluvSaturation = settings.HsluvSaturation;
            _renderer.HsluvLightness = settings.HsluvLightness;
            _renderer.BarWidth = settings.BarWidth;
            _renderer.GapWidth = settings.GapWidth;
            _renderer.FillMode = settings.FillMode;
        }

        // 更新覆盖模式（即时生效，下次 EnsureZOrder 触发时应用）
        if (settings.OverlayMode != _overlayMode)
        {
            _overlayMode = settings.OverlayMode;
            EnsureZOrder(); // 立即应用新层级
        }

        // 更新帧率
        if (_updateTimer != null && settings.Fps > 0)
            _updateTimer.Interval = 1000.0 / settings.Fps;
    }

    /// <summary>
    /// 原子 Z-order 操作，根据覆盖模式决定 overlay 与 taskbar 的层级关系
    /// 
    /// 模式 1 (Under): taskbar 覆盖在频谱上面（默认 — 频谱被任务栏图标遮挡，柱子上半部分可见）
    /// 模式 2 (Above): 频谱覆盖在任务栏上面（透明区域鼠标穿透）
    /// 
    /// DeferWindowPos 中后设置的窗口在 Z-order 更高层
    /// </summary>
    private void EnsureZOrder()
    {
        if (_hwnd == IntPtr.Zero) return;

        bool hasTaskbar = _taskbarHwnd != IntPtr.Zero;
        int count = (hasTaskbar ? 1 : 0) + 1;
        var dwp = BeginDeferWindowPos(count);

        if (dwp == IntPtr.Zero)
        {
            DebugLog.Write($"EnsureZOrder: BeginDeferWindowPos FAILED, last error={Marshal.GetLastWin32Error()}");
            return;
        }

        bool isAbove = _overlayMode == 2;
        if (isAbove)
        {
            // 步骤1: taskbar → TOPMOST（底层）
            if (hasTaskbar)
            {
                dwp = DeferWindowPos(dwp, _taskbarHwnd, HWND_TOPMOST, 0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                if (dwp == IntPtr.Zero) { DebugLog.Write("EnsureZOrder: DeferWindowPos(taskbar) FAILED"); return; }
            }
            // 步骤2: overlay → TOPMOST（上层，覆盖 taskbar）
            dwp = DeferWindowPos(dwp, _hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            if (dwp == IntPtr.Zero) { DebugLog.Write("EnsureZOrder: DeferWindowPos(overlay) FAILED"); return; }
        }
        else
        {
            // 步骤1: overlay → TOPMOST（底层）
            dwp = DeferWindowPos(dwp, _hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            if (dwp == IntPtr.Zero) { DebugLog.Write($"EnsureZOrder: DeferWindowPos(overlay) FAILED"); return; }
            // 步骤2: taskbar → TOPMOST（上层，覆盖 overlay）
            if (hasTaskbar)
            {
                dwp = DeferWindowPos(dwp, _taskbarHwnd, HWND_TOPMOST, 0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                if (dwp == IntPtr.Zero) { DebugLog.Write($"EnsureZOrder: DeferWindowPos(taskbar) FAILED"); return; }
            }
        }

        if (!EndDeferWindowPos(dwp))
            DebugLog.Write($"EnsureZOrder: EndDeferWindowPos FAILED, last error={Marshal.GetLastWin32Error()}");
    }

    /// <summary>
    /// 强制刷新覆盖窗口（用于设置窗口打开/关闭后恢复显示）
    /// </summary>
    public void Refresh()
    {
        if (_hwnd != IntPtr.Zero && _isRunning)
        {
            ShowWindow(_hwnd, 1); // SW_SHOW
            EnsureZOrder();
            UpdateLayeredWindow(); // 强制刷新分层窗口内容
            DebugLog.Write($"Refresh: hwnd={_hwnd}, isRunning={_isRunning}");
        }
    }

    /// <summary>
    /// 强制恢复 Z-order（供外部在设置窗口激活后调用）
    /// 确保 overlay 在 TOPMOST 组，taskbar 在 overlay 上面
    /// </summary>
    public void ForceZOrderRestore()
    {
        if (_hwnd != IntPtr.Zero)
        {
            EnsureZOrder();
            DebugLog.Write($"ForceZOrderRestore: hwnd={_hwnd}, settingsHwnd={_settingsHwnd}");
        }
    }

    /// <summary>
    /// 设置/清除设置窗口句柄（打开设置时调用设置，关闭时传 IntPtr.Zero）
    /// </summary>
    public void SetSettingsHwnd(IntPtr hwnd)
    {
        _settingsHwnd = hwnd;
            DebugLog.Write($"SetSettingsHwnd: {hwnd}");
        if (_hwnd != IntPtr.Zero)
            EnsureZOrder();
    }

    public void Dispose()
    {
        _zOrderTimer?.Stop();
        _zOrderTimer?.Dispose();
        _zOrderTimer = null;

        App.Fft.SpectrumUpdated -= OnSpectrumUpdated;

        _isRunning = false;
        _updateTimer?.Stop();
        _updateTimer?.Dispose();
        _updateTimer = null;

        _renderer?.Cleanup();
        _renderer = null;

        if (_hBitmap != IntPtr.Zero)
        {
            DeleteObject(_hBitmap);
            _hBitmap = IntPtr.Zero;
        }

        if (_hDcMem != IntPtr.Zero)
        {
            DeleteDC(_hDcMem);
            _hDcMem = IntPtr.Zero;
        }

        if (_hDcScreen != IntPtr.Zero)
        {
            ReleaseDC(IntPtr.Zero, _hDcScreen);
            _hDcScreen = IntPtr.Zero;
        }

        if (_hwnd != IntPtr.Zero)
        {
            DestroyWindow(_hwnd);
            _hwnd = IntPtr.Zero;
        }

        UnregisterClassA(WND_CLASS, GetModuleHandleA(null));
        DebugLog.Write("LayeredOverlay Disposed");
    }

    #region Win32 P/Invoke

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate IntPtr WndProcDelegate(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
    private static extern ushort RegisterClassA([In] ref WNDCLASSA lpWndClass);

    [DllImport("user32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
    private static extern IntPtr CreateWindowExA(
        uint dwExStyle, string lpClassName, string lpWindowName,
        uint dwStyle, int x, int y, int nWidth, int nHeight,
        IntPtr hWndParent, IntPtr hMenu, IntPtr hInstance, IntPtr lpParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Ansi)]
    private static extern bool UnregisterClassA(string lpClassName, IntPtr hInstance);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProcA(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern IntPtr BeginDeferWindowPos(int nNumWindows);

    [DllImport("user32.dll")]
    private static extern IntPtr DeferWindowPos(IntPtr hWinPosInfo, IntPtr hWnd, IntPtr hWndInsertAfter, int x, int y, int cx, int cy, uint uFlags);

    [DllImport("user32.dll")]
    private static extern bool EndDeferWindowPos(IntPtr hWinPosInfo);

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int dwAttribute, ref int pvAttribute, int cbAttribute);

    [DllImport("user32.dll")]
    private static extern IntPtr GetDC(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern int ReleaseDC(IntPtr hWnd, IntPtr hDC);

    [DllImport("gdi32.dll")]
    private static extern IntPtr CreateCompatibleDC(IntPtr hDC);

    [DllImport("gdi32.dll")]
    private static extern bool DeleteDC(IntPtr hDC);

    [DllImport("gdi32.dll")]
    private static extern IntPtr SelectObject(IntPtr hDC, IntPtr hObject);

    [DllImport("gdi32.dll")]
    private static extern IntPtr CreateDIBSection(
        IntPtr hdc, [In] ref BITMAPINFOHEADER pbmi, uint iUsage,
        out IntPtr ppvBits, IntPtr hSection, uint dwOffset);

    [DllImport("gdi32.dll")]
    private static extern bool DeleteObject(IntPtr hObject);

    [DllImport("user32.dll")]
    private static extern bool UpdateLayeredWindow(
        IntPtr hwnd, IntPtr hdcDst, IntPtr pptDst, ref POINT psize,
        IntPtr hdcSrc, ref POINT pptSrc, uint crKey, ref BLENDFUNCTION pblend, uint dwFlags);

    [DllImport("kernel32.dll", CharSet = CharSet.Ansi)]
    private static extern IntPtr GetModuleHandleA(string? lpModuleName);

    [DllImport("user32.dll", CharSet = CharSet.Ansi)]
    private static extern IntPtr LoadCursorA(IntPtr hInstance, IntPtr lpCursorName);

    #endregion

    #region 结构体

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)]
    private struct WNDCLASSA
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
    private struct BITMAPINFOHEADER
    {
        public int biSize;
        public int biWidth;
        public int biHeight;
        public short biPlanes;
        public short biBitCount;
        public int biCompression;
        public int biSizeImage;
        public int biXPelsPerMeter;
        public int biYPelsPerMeter;
        public int biClrUsed;
        public int biClrImportant;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BLENDFUNCTION
    {
        public byte BlendOp;
        public byte BlendFlags;
        public byte SourceConstantAlpha;
        public byte AlphaFormat;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int X;
        public int Y;
    }

    #endregion

}
