namespace Panon.Windows.Helpers;

/// <summary>
/// 线程安全的调试日志工具
/// 使用锁避免多线程并发写入同一文件导致 IOException
/// </summary>
public static class DebugLog
{
    private static readonly object _lock = new();
    private static readonly string _logPath = Path.Combine(Path.GetTempPath(), "panon_debug.txt");

    private const long MaxLogSize = 1_048_576; // 1 MB

    public static void Write(string message)
    {
        try
        {
            lock (_lock)
            {
                if (File.Exists(_logPath) && new FileInfo(_logPath).Length > MaxLogSize)
                {
                    // 截断：保留后半部分（最近的日志更有用）
                    var bytes = File.ReadAllBytes(_logPath);
                    int keep = bytes.Length / 2;
                    File.WriteAllBytes(_logPath, bytes[^keep..]);
                }
                File.AppendAllText(_logPath, $"[{DateTime.Now:HH:mm:ss.fff}] {message}\n");
            }
        }
        catch
        {
            // 日志写入失败不应影响业务逻辑
        }
    }
}
