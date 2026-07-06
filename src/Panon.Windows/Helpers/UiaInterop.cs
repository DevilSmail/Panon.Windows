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
    /// <param name="taskbarTop">任务栏上边缘 Y 坐标（用于过滤弹出的 flyout 元素）</param>
    /// <param name="taskbarHeight">任务栏高度</param>
    public static List<(int X, int Width)> GetTaskbarButtonRects(IntPtr taskbarHwnd, int taskbarLeft, int taskbarWidth, int taskbarTop, int taskbarHeight)
    {
        var result = new List<(int X, int Width)>();
        int tw = taskbarWidth;

        try
        {
            // 通过 HWND 获取任务栏 AutomationElement
            var taskbarEl = System.Windows.Automation.AutomationElement.FromHandle(taskbarHwnd);
            if (taskbarEl == null) return result;

            // 遍历所有后代元素，收集非容器元素的 BoundingRectangle
            CollectElementRects(taskbarEl, taskbarLeft, tw, taskbarTop, taskbarHeight, result);

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
        int taskbarLeft, int tw, int taskbarTop, int taskbarHeight,
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
                int cy = (int)rect.Y;
                int ch = (int)rect.Height;
                int taskbarBottom = taskbarTop + taskbarHeight;

                // 判断当前元素是否在任务栏纵向范围内
                bool withinTaskbarY = cy >= taskbarTop && cy < taskbarBottom;

                // flyout 容器可能起始于任务栏上边缘（cy == taskbarTop），但高度远超 taskbarHeight
                // 这类元素的 Y 过滤通过但实际是弹窗，通过高度上限排除
                bool heightReasonable = ch <= taskbarHeight;

                // 三重过滤后收录：
                // 1) 宽度合理（0 < w < 80% 任务栏宽），排除极宽容器
                // 2) Y 坐标在任务栏纵向范围内
                // 3) 高度不超出任务栏高度，排除 flyout 容器（顶边与任务栏重叠但向上延伸）
                if (cw > 0 && cw < tw * 0.8 && withinTaskbarY && heightReasonable)
                {
                    int cx = (int)rect.X - taskbarLeft;
                    if (cx < 0) { cw += cx; cx = 0; }
                    if (cx + cw > tw) cw = tw - cx;
                    if (cw > 0)
                        result.Add((cx, cw));
                }

                // 仅递归同时满足 Y + 高度条件的元素
                // flyout 容器即使顶边与任务栏重叠，因其高度 >> taskbarHeight，递归在此截断
                if (withinTaskbarY && heightReasonable)
                {
                    CollectElementRects(child, taskbarLeft, tw, taskbarTop, taskbarHeight, result);
                }
            }
            catch { /* UIA element may be invalid after taskbar change */ }

            child = walker.GetNextSibling(child);
        }
    }
}
