namespace Panon.Windows.Helpers;

/// <summary>
/// 线程安全的调试日志工具
/// 使用锁避免多线程并发写入同一文件导致 IOException
/// </summary>
public static class DebugLog
{
    private static readonly object _lock = new();
    private static readonly string _logPath = Path.Combine(Path.GetTempPath(), "panon_debug.txt");

    public static void Write(string message)
    {
        try
        {
            lock (_lock)
            {
                File.AppendAllText(_logPath, $"[{DateTime.Now:HH:mm:ss.fff}] {message}\n");
            }
        }
        catch
        {
            // 日志写入失败不应影响业务逻辑
        }
    }
}
