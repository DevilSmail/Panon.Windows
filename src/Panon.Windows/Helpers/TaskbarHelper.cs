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
    private static readonly TimeSpan UiaRefreshInterval = TimeSpan.FromMilliseconds(500);

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

        // 按显示器从左到右排序，确保主显示器排第一
        // 主任务栏（Shell_TrayWnd）始终是主显示器
        result.Sort((a, b) => a.X.CompareTo(b.X));

        // 标记显示器索引：0=主显示器, 1,2...=其他
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
        var regions = new List<(int X, int Width)>();
        if (taskbarHwnd == IntPtr.Zero)
            taskbarHwnd = FindWindow("Shell_TrayWnd", null);
        if (taskbarHwnd == IntPtr.Zero) return regions;

        GetWindowRect(taskbarHwnd, out RECT taskbarRect);
        int tw = taskbarRect.Right - taskbarRect.Left;

        // ── UIA 缓存刷新（500ms 间隔） ──
        var now = DateTime.Now;
        if (_cachedUiaRects == null || _cachedTaskbarHwnd != taskbarHwnd
            || _cachedTaskbarWidth != tw || (now - _lastUiaRefresh) > UiaRefreshInterval)
        {
            var uiaRects = UiaInterop.GetTaskbarButtonRects(taskbarHwnd, taskbarRect.Left, tw);
            _cachedUiaRects = uiaRects;
            _cachedTaskbarHwnd = taskbarHwnd;
            _cachedTaskbarWidth = tw;
            _lastUiaRefresh = now;
        }

        if (_cachedUiaRects.Count == 0) return regions;

        // 合并重叠区域（直接在 List 上原地合并避免 struct 拷贝问题）
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

        // 计算空白区域
        int pos = 0;
        foreach (var (x, w) in merged)
        {
            int gapWidth = x - pos;
            if (gapWidth >= minBarWidth)
                regions.Add((pos, gapWidth));
            pos = Math.Max(pos, x + w);
        }
        int lastGap = tw - pos;
        if (lastGap >= minBarWidth)
            regions.Add((pos, lastGap));

        return regions;
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
