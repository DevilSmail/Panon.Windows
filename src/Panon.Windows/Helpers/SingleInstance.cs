using System.Threading;

namespace Panon.Windows.Helpers;

/// <summary>
/// 单实例控制器，确保应用只运行一个实例
/// </summary>
public sealed class SingleInstance : IDisposable
{
    private Mutex? _mutex;
    private bool _owned;

    /// <summary>
    /// 尝试获取单实例锁
    /// </summary>
    /// <returns>true 表示当前是第一个实例，false 表示已有实例在运行</returns>
    public bool TryAcquire()
    {
        _mutex = new Mutex(true, "Panon.Windows.SingleInstance", out _owned);
        return _owned;
    }

    /// <summary>
    /// 是否当前持有实例锁
    /// </summary>
    public bool IsOwned => _owned;

    public void Dispose()
    {
        if (_mutex != null)
        {
            if (_owned)
            {
                _mutex.ReleaseMutex();
            }
            _mutex.Dispose();
            _mutex = null;
        }
    }
}
