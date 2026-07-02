namespace Panon.Windows.Helpers;

/// <summary>
/// 使用 System.Windows.Automation (UIA) 探测任务栏按钮位置
/// 解决 Win11 XAML 渲染的任务栏按钮没有独立 HWND 的问题
/// </summary>
internal static class UiaInterop
{
    /// <summary>
    /// 通过 UIA 获取任务栏上所有非容器元素的 BoundingRectangle
    /// 使用 TreeWalker 遍历任务栏子树，排除极宽容器元素
    /// 返回任务栏相对坐标 (X=相对于任务栏左边缘, Width=元素宽度)
    /// </summary>
    public static List<(int X, int Width)> GetTaskbarButtonRects(IntPtr taskbarHwnd, int taskbarLeft, int taskbarWidth)
    {
        var result = new List<(int X, int Width)>();
        int tw = taskbarWidth;

        try
        {
            // 通过 HWND 获取任务栏 AutomationElement
            var taskbarEl = System.Windows.Automation.AutomationElement.FromHandle(taskbarHwnd);
            if (taskbarEl == null) return result;

            // 遍历所有后代元素，收集非容器元素的 BoundingRectangle
            CollectElementRects(taskbarEl, taskbarLeft, tw, result);

            // 去重 + 排序
            result = result.Distinct().OrderBy(r => r.X).ToList();
        }
        catch (Exception ex)
        {
            DebugLog.Write($"[UIA] Error: {ex.Message}");
        }

        return result;
    }

    private static void CollectElementRects(
        System.Windows.Automation.AutomationElement element,
        int taskbarLeft, int tw,
        List<(int X, int Width)> result)
    {
        var walker = System.Windows.Automation.TreeWalker.RawViewWalker;
        var child = walker.GetFirstChild(element);

        while (child != null)
        {
            try
            {
                var rect = child.Current.BoundingRectangle;
                int cw = (int)rect.Width;
                if (cw > 0 && cw < tw * 0.8) // 排除极宽容器（背景面板 > 80% 任务栏宽）
                {
                    int cx = (int)rect.X - taskbarLeft;
                    if (cx < 0) { cw += cx; cx = 0; }
                    if (cx + cw > tw) cw = tw - cx;
                    if (cw > 0)
                        result.Add((cx, cw));
                }

                // 递归处理子元素
                CollectElementRects(child, taskbarLeft, tw, result);
            }
            catch { /* UIA element may be invalid after taskbar change */ }

            child = walker.GetNextSibling(child);
        }
    }
}
