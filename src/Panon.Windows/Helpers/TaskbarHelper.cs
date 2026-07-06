using System.Runtime.InteropServices;

namespace Panon.Windows.Helpers;

/// <summary>
/// 任务栏位置和尺寸检测
/// 使用 Win32 Shell API 获取任务栏信息
/// </summary>
public sealed class TaskbarHelper
{
    [StructLayout(LayoutKind.Sequential)]
    private struct APPBARDATA
    {
        public int cbSize;
        public IntPtr hWnd;
        public int uCallbackMessage;
        public int uEdge;
        public RECT rc;
        public IntPtr lParam;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT
    {
        public int Left, Top, Right, Bottom;
    }

    [DllImport("shell32.dll", SetLastError = true)]
    private static extern IntPtr SHAppBarMessage(int dwMessage, ref APPBARDATA pData);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr FindWindow(string? lpClassName, string? lpWindowName);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    private static extern IntPtr FindWindowEx(IntPtr hWndParent, IntPtr hWndChildAfter, string? lpszClass, string? lpszWindow);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);

    private const uint MONITOR_DEFAULTTONEAREST = 2;

    private const int ABM_GETTASKBARPOS = 0x00000005;

    // ── UIA 缓存 ──────────────────────────────────────────────
    private List<(int X, int Width)>? _cachedUiaRects;
    private IntPtr _cachedTaskbarHwnd = IntPtr.Zero;
    private int _cachedTaskbarWidth;
    private DateTime _lastUiaRefresh = DateTime.MinValue;
    // 正常 500ms 刷新；静音/暂停时拉长到 3s，降低 UIA COM 引用压力
    private static readonly TimeSpan UiaRefreshIntervalActive = TimeSpan.FromMilliseconds(500);
    private static readonly TimeSpan UiaRefreshIntervalIdle = TimeSpan.FromSeconds(3);
    private TimeSpan _uiaRefreshInterval = UiaRefreshIntervalActive;

    /// <summary>
    /// 设置 UIA 刷新模式：true=正常 500ms，false=静音/暂停 3s
    /// 静音时任务栏按钮布局稳定，慢刷新可显著降低 COM 引用压力
    /// </summary>
    public void SetIdleMode(bool idle)
    {
        var newInterval = idle ? UiaRefreshIntervalIdle : UiaRefreshIntervalActive;
        if (_uiaRefreshInterval != newInterval)
        {
            _uiaRefreshInterval = newInterval;
            // 模式切换时强制下次刷新（从静音恢复时立即拿到最新按钮布局）
            _lastUiaRefresh = DateTime.MinValue;
        }
    }

    // ── 合并/结果缓存（避免每帧分配 List） ────────────────────
    // _cachedUiaRects 变化时一并失效；regions 还随 minBarWidth 变化失效
    private List<(int X, int Width)>? _cachedMerged;
    private List<(int X, int Width)>? _cachedRegions;
    private int _cachedMinBarWidth = -1;
    private int _cachedTaskbarTop;

    // ── Flyout 防御回退 ──────────────────────────────────────
    // 当 Quick Settings / 音量 / IME 状态 flyout 弹出时，任务栏按钮 UIA 报告的
    // BoundingRectangle 会临时变宽（含激活态高亮指示器），导致 free regions 算出空。
    //
    // 修复策略：用两套独立计数器
    //   - 非空稳定计数 _goodConfirmCount：连续 3 次（1.5s）非空且一致 → 确认 _stableRegions
    //   - 空稳定计数 _emptyConfirmCount：连续 8 次（4s）空 → 接受"真的没空白"，清空 _stableRegions
    //
    // 关键不变量：_stableRegions 一旦确认，flyout 期间永远不会被打消。
    // 唯一的清空条件是连续 8 次（4 秒）空结果（用户长时间无操作且任务栏真的被填满）。
    private List<(int X, int Width)>? _stableRegions;
    private List<(int X, int Width)>? _stableCandidate;  // 候选稳定结果
    private int _goodConfirmCount;        // 当前非空候选的连续一致次数
    private int _emptyConfirmCount;       // 连续空结果次数（独立计数，不污染 good 计数）
    private const int StableConfirmCount = 3;  // 连续 3 次（1.5s）非空一致 → 确认稳定
    private const int EmptyStableCount = 8;    // 连续 8 次（4s）空 → 接受"真的没空白"

    // 即时回退：_stableRegions 需 1.5s 确认，在此之前用 _lastGoodRegions 防御
    // 每次 UIA 返回非空结果时更新，空结果时不清零，用于启动初期 flyout 防护
    private List<(int X, int Width)>? _lastGoodRegions;

    /// <summary>
    /// 任务栏位置
    /// </summary>
    public enum TaskbarPosition
    {
        Unknown = -1,
        Left = 0,
        Top = 1,
        Right = 2,
        Bottom = 3
    }

    /// <summary>
    /// 获取任务栏位置和尺寸（主显示器）
    /// </summary>
    public TaskbarInfo GetTaskbarInfo()
    {
        var info = new TaskbarInfo();

        try
        {
            var taskbarHandle = FindWindow("Shell_TrayWnd", null);
            info.TaskbarHwnd = taskbarHandle;

            // 尝试通过 Shell API 获取
            var data = new APPBARDATA();
            data.cbSize = Marshal.SizeOf(data);
            IntPtr result = SHAppBarMessage(ABM_GETTASKBARPOS, ref data);

            if (result != IntPtr.Zero)
            {
                info.Position = (TaskbarPosition)data.uEdge;
                info.Bounds = new RectInt32(
                    data.rc.Left, data.rc.Top,
                    data.rc.Right - data.rc.Left,
                    data.rc.Bottom - data.rc.Top);
            }
            else
            {
                // 回退方案：通过窗口句柄获取
                if (taskbarHandle != IntPtr.Zero && GetWindowRect(taskbarHandle, out RECT rect))
                {
                    info.Bounds = new RectInt32(
                        rect.Left, rect.Top,
                        rect.Right - rect.Left,
                        rect.Bottom - rect.Top);

                    // 推断位置
                    if (rect.Top < 10) info.Position = TaskbarPosition.Top;
                    else if (rect.Left < 10 && rect.Right - rect.Left < 200) info.Position = TaskbarPosition.Left;
                    else if (rect.Left > 500) info.Position = TaskbarPosition.Right;
                    else info.Position = TaskbarPosition.Bottom;
                }
            }
        }
        catch
        {
            // 默认底部
            info.Position = TaskbarPosition.Bottom;
        }

        return info;
    }

    /// <summary>
    /// 获取所有显示器的任务栏信息
    /// 通过查找 Shell_TrayWnd（主）和 Shell_SecondaryTrayWnd（副）窗口，
    /// 使用 MonitorFromWindow 映射到对应显示器
    /// </summary>
    public List<TaskbarInfo> GetAllTaskbarInfos()
    {
        var result = new List<TaskbarInfo>();

        // 收集所有任务栏窗口句柄
        var taskbarWindows = new List<IntPtr>();

        // 主任务栏
        var primaryHwnd = FindWindow("Shell_TrayWnd", null);
        if (primaryHwnd != IntPtr.Zero)
            taskbarWindows.Add(primaryHwnd);

        // 副任务栏（Windows 11 22H2+）
        IntPtr secondaryHwnd = IntPtr.Zero;
        while (true)
        {
            secondaryHwnd = FindWindowEx(IntPtr.Zero, secondaryHwnd, "Shell_SecondaryTrayWnd", null);
            if (secondaryHwnd == IntPtr.Zero) break;
            taskbarWindows.Add(secondaryHwnd);
        }

        // 为每个任务栏窗口创建 TaskbarInfo，按显示器位置排序
        foreach (var hwnd in taskbarWindows)
        {
            if (!GetWindowRect(hwnd, out RECT rect))
                continue;

            // 使用 MonitorFromWindow 确定所属显示器
            IntPtr hMonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

            var info = new TaskbarInfo
            {
                TaskbarHwnd = hwnd,
                Bounds = new RectInt32(rect.Left, rect.Top, rect.Right - rect.Left, rect.Bottom - rect.Top)
            };

            // 推断位置（基于窗口矩形的屏幕坐标）
            if (rect.Top < 100) info.Position = TaskbarPosition.Top;
            else if (rect.Left < 100 && rect.Right - rect.Left < 200) info.Position = TaskbarPosition.Left;
            else info.Position = TaskbarPosition.Bottom;

            result.Add(info);
        }

        // 按显示器从左到右排序
        result.Sort((a, b) => a.X.CompareTo(b.X));

        // 确保主显示器（Shell_TrayWnd）始终排在索引 0
        int primaryIdx = result.FindIndex(t => t.TaskbarHwnd == primaryHwnd);
        if (primaryIdx > 0)
        {
            var primary = result[primaryIdx];
            result.RemoveAt(primaryIdx);
            result.Insert(0, primary);
        }

        // 标记显示器索引：0=主显示器, 1,2...=其他（从左到右）
        for (int i = 0; i < result.Count; i++)
            result[i].MonitorIndex = i;

        return result;
    }

    /// <summary>
    /// 获取指定索引的显示器任务栏信息
    /// </summary>
    public TaskbarInfo? GetTaskbarInfoByIndex(int monitorIndex)
    {
        var all = GetAllTaskbarInfos();
        if (monitorIndex >= 0 && monitorIndex < all.Count)
            return all[monitorIndex];
        return null;
    }

    /// <summary>
    /// 获取任务栏空白区域（供 SpectrumRenderer 使用）
    /// 使用 UI Automation 探测任务栏按钮位置（500ms 缓存刷新），
    /// 覆盖 Win11 XAML 渲染的按钮（无独立 HWND）
    /// </summary>
    /// <param name="minBarWidth">最小柱子宽度，用于排除微小间隙</param>
    /// <param name="taskbarHwnd">目标任务栏窗口句柄，为 IntPtr.Zero 时自动获取主任务栏</param>
    public List<(int X, int Width)> GetTaskbarFreeRegions(int minBarWidth, IntPtr taskbarHwnd = default)
    {
        if (taskbarHwnd == IntPtr.Zero)
            taskbarHwnd = FindWindow("Shell_TrayWnd", null);
        if (taskbarHwnd == IntPtr.Zero)
            return _cachedRegions ?? new List<(int X, int Width)>();

        GetWindowRect(taskbarHwnd, out RECT taskbarRect);
        int tw = taskbarRect.Right - taskbarRect.Left;

        // ── UIA 缓存刷新（500ms 正常 / 3s 静音） ──
        // 任一身份/尺寸/时间变化都触发重新走 UIA，并连带失效 merged/regions 缓存
        var now = DateTime.Now;
        bool uiaStale = _cachedUiaRects == null || _cachedTaskbarHwnd != taskbarHwnd
            || _cachedTaskbarWidth != tw || _cachedTaskbarTop != taskbarRect.Top
            || (now - _lastUiaRefresh) > _uiaRefreshInterval;
        if (uiaStale)
        {
            // taskbarTop/height 传入用于 Y 坐标过滤（排除弹出的 flyout 元素）
            int taskbarHeight = taskbarRect.Bottom - taskbarRect.Top;
            var uiaRects = UiaInterop.GetTaskbarButtonRects(taskbarHwnd, taskbarRect.Left, tw, taskbarRect.Top, taskbarHeight);
            _cachedUiaRects = uiaRects;
            _cachedTaskbarHwnd = taskbarHwnd;
            _cachedTaskbarWidth = tw;
            _cachedTaskbarTop = taskbarRect.Top;
            _lastUiaRefresh = now;
            _cachedMerged = null;   // 失效下游缓存
            _cachedRegions = null;
        }

        // ── 合并重叠区域 + 计算空白区域 ──
        // 注意：UIA 空结果时 _cachedMerged 不计算，_cachedRegions 直接设空。
        // 不能 early return —— 必须走到后面的稳定回退逻辑，让 flyout 防御生效。
        if (_cachedUiaRects!.Count == 0)
        {
            _cachedMerged = null;
            _cachedRegions = new List<(int X, int Width)>(0);
        }
        else
        {
            if (_cachedMerged == null)
            {
                _cachedUiaRects.Sort((a, b) => a.X.CompareTo(b.X));
                var merged = new List<(int X, int Width)> { _cachedUiaRects[0] };
                for (int i = 1; i < _cachedUiaRects.Count; i++)
                {
                    var (x, w) = _cachedUiaRects[i];
                    var last = merged[merged.Count - 1];
                    int lastEnd = last.X + last.Width;
                    if (x <= lastEnd)
                        merged[merged.Count - 1] = (last.X, Math.Max(lastEnd, x + w) - last.X);
                    else
                        merged.Add((x, w));
                }
                _cachedMerged = merged;
                _cachedRegions = null;   // merged 变了，regions 也要重算
            }

            if (_cachedRegions == null || _cachedMinBarWidth != minBarWidth)
            {
                var regions = new List<(int X, int Width)>();
                int pos = 0;
                foreach (var (x, w) in _cachedMerged!)
                {
                    int gapWidth = x - pos;
                    if (gapWidth >= minBarWidth)
                        regions.Add((pos, gapWidth));
                    pos = Math.Max(pos, x + w);
                }
                int lastGap = tw - pos;
                if (lastGap >= minBarWidth)
                    regions.Add((pos, lastGap));

                _cachedRegions = regions;
                _cachedMinBarWidth = minBarWidth;
            }
        }

        // ── Flyout 防御回退（双计数器稳定窗口）──
        // 关键设计：空结果和非空结果使用独立的计数器，互不污染。
        // - 非空：连续 StableConfirmCount 次（1.5s）一致 → 确认 _stableRegions
        // - 空：连续 EmptyStableCount 次（4s） → 才接受"真的没空白"
        // 这样 flyout 期间（UIA 抖动、空结果）永远不达到 4s，且 _stableRegions 不会被空结果清空
        var currentResult = _cachedRegions!;

        if (currentResult.Count > 0)
        {
            // ── 非空分支：独立计数 ──
            // 重置空计数（说明 UIA 正常了）
            if (_emptyConfirmCount > 0)
            {
                _emptyConfirmCount = 0;
                DebugLog.Write("[Taskbar] Regions recovered from empty");
            }

            // 更新即时回退（无需等待稳定确认，用于启动初期 flyout 防护）
            _lastGoodRegions = new List<(int X, int Width)>(currentResult);

            if (RegionsEqual(_stableCandidate, currentResult))
            {
                _goodConfirmCount++;
            }
            else
            {
                _stableCandidate = new List<(int X, int Width)>(currentResult);
                _goodConfirmCount = 1;
            }

            // 达到确认阈值 → 记录稳定结果（仅在尚未确认时记录）
            if (_goodConfirmCount >= StableConfirmCount && _stableRegions == null)
            {
                _stableRegions = new List<(int X, int Width)>(currentResult);
                DebugLog.Write($"[Taskbar] Stable regions confirmed ({_stableRegions.Count} gaps, took {_goodConfirmCount * 500}ms)");
            }

            return currentResult;
        }
        else
        {
            // ── 空分支：独立计数，绝不污染 _goodConfirmCount ──
            // 重置非空计数（说明 UIA 又出问题了）
            _goodConfirmCount = 0;
            _stableCandidate = null;

            _emptyConfirmCount++;
            if (_emptyConfirmCount == EmptyStableCount && _stableRegions != null)
            {
                // 连续 4 秒空结果 → 接受"真的没空白"，清空稳定结果
                _stableRegions = null;
                DebugLog.Write("[Taskbar] Sustained empty for 4s, accepting as 'no free region'");
            }

            // 回退顺序: _stableRegions（已确认）> _lastGoodRegions（即时）> 空
            if (_stableRegions != null)
            {
                DebugLog.Write($"[Taskbar] Empty #{_emptyConfirmCount}/{EmptyStableCount}, falling back to stable ({_stableRegions.Count} gaps)");
                return _stableRegions;
            }
            if (_lastGoodRegions != null)
            {
                DebugLog.Write($"[Taskbar] Empty #{_emptyConfirmCount}/{EmptyStableCount}, falling back to last good ({_lastGoodRegions.Count} gaps)");
                return _lastGoodRegions;
            }
            return currentResult;
        }
    }

    /// <summary>
    /// 比较两个 regions 列表是否"实质上一致"（X 和 Width 相同）
    /// </summary>
    private static bool RegionsEqual(List<(int X, int Width)>? a, List<(int X, int Width)>? b)
    {
        if (a == null || b == null) return false;
        if (a.Count != b.Count) return false;
        for (int i = 0; i < a.Count; i++)
        {
            if (a[i].X != b[i].X || a[i].Width != b[i].Width) return false;
        }
        return true;
    }
}

/// <summary>
/// 任务栏信息
/// </summary>
public sealed class TaskbarInfo
{
    public TaskbarHelper.TaskbarPosition Position { get; set; } = TaskbarHelper.TaskbarPosition.Bottom;
    public RectInt32 Bounds { get; set; } = new(0, 0, 0, 0);
    /// <summary>任务栏窗口句柄（用于 Z-order 维护）</summary>
    public IntPtr TaskbarHwnd { get; set; } = IntPtr.Zero;
    /// <summary>显示器索引（0=主显示器, 1,2...=其他，与 EnumDisplayMonitors 顺序一致）</summary>
    public int MonitorIndex { get; set; } = 0;

    public int Width => Bounds.Width;
    public int Height => Bounds.Height;
    public int X => Bounds.X;
    public int Y => Bounds.Y;

    public bool IsHorizontal => Position is TaskbarHelper.TaskbarPosition.Top or TaskbarHelper.TaskbarPosition.Bottom;
}

/// <summary>
/// 简单的矩形结构，替代 Windows.Graphics.RectInt32
/// </summary>
public record struct RectInt32(int X, int Y, int Width, int Height);
