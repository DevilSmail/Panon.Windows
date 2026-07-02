using System.Runtime.InteropServices;
using NAudio.Wave;
using Panon.Windows.Helpers;

namespace Panon.Windows.Audio;

/// <summary>
/// WASAPI Loopback 音频捕获服务
/// 在独立的 MTA 后台线程上运行，与 UI 线程的 COM 环境隔离。
/// </summary>
public sealed class AudioCaptureService : IDisposable
{
    private WasapiLoopbackCapture? _capture;
    private WaveFormat? _waveFormat;
    private volatile bool _isRunning;
    private bool _hasLogged = false;
    private Thread? _captureThread;
    private readonly object _lock = new();

    /// <summary>
    /// 音频数据可用事件，参数为 PCM 浮点采样数据
    /// </summary>
    public event Action<float[], WaveFormat>? DataAvailable;

    public bool IsRunning => _isRunning;

    // COM 初始化 P/Invoke
    [DllImport("ole32.dll")]
    private static extern int CoInitializeEx(IntPtr pvReserved, uint dwCoInit);
    private const uint COINIT_MULTITHREADED = 0x0;
    [DllImport("ole32.dll")]
    private static extern void CoUninitialize();

    /// <summary>
    /// 开始捕获系统音频
    /// 在专用 MTA 后台线程上创建 WASAPI 捕获，与 UI 线程隔离
    /// </summary>
    public void Start(int deviceIndex = -1)
    {
        Stop();

        _isRunning = true;
        _hasLogged = false;

        _captureThread = new Thread(() => CaptureThreadProc())
        {
            IsBackground = true,
            Name = "WASAPI Capture Thread"
        };
        _captureThread.SetApartmentState(ApartmentState.MTA);
        _captureThread.Start();
    }

    /// <summary>
    /// WASAPI 捕获线程主逻辑
    /// </summary>
    private void CaptureThreadProc()
    {
        try
        {
            CoInitializeEx(IntPtr.Zero, COINIT_MULTITHREADED);

            lock (_lock)
            {
                _capture = new WasapiLoopbackCapture();
                _waveFormat = _capture.WaveFormat;
                _capture.DataAvailable += OnDataAvailable;
                _capture.RecordingStopped += OnRecordingStopped;
            }

            _capture.StartRecording();
            DebugLog.Write($"Audio Start OK: {_waveFormat.SampleRate}Hz {_waveFormat.Channels}ch {_waveFormat.BitsPerSample}bit (MTA thread)");

            while (_isRunning)
            {
                Thread.Sleep(100);
            }

            lock (_lock)
            {
                if (_capture != null)
                {
                    _capture.DataAvailable -= OnDataAvailable;
                    _capture.RecordingStopped -= OnRecordingStopped;
                    try { _capture.StopRecording(); } catch { DebugLog.Write("Audio: StopRecording failed"); }
                    _capture.Dispose();
                    _capture = null;
                }
            }
        }
        catch (Exception ex)
        {
            DebugLog.Write($"Audio CaptureThread error: {ex.Message}");
            _isRunning = false;
        }
        finally
        {
            CoUninitialize();
        }
    }

    /// <summary>
    /// 停止捕获
    /// </summary>
    public void Stop()
    {
        _isRunning = false;

        var thread = _captureThread;
        if (thread != null && thread.IsAlive)
        {
            thread.Join(2000);
        }
        _captureThread = null;
    }

    private void OnDataAvailable(object? sender, WaveInEventArgs e)
    {
        if (_waveFormat == null || !_isRunning) return;
        if (e.BytesRecorded <= 0) return;

        int bytesPerSample = _waveFormat.BitsPerSample / 8;
        int sampleCount = e.BytesRecorded / bytesPerSample;
        var samples = new float[sampleCount];

        if (_waveFormat.BitsPerSample == 32 && _waveFormat.Encoding == WaveFormatEncoding.IeeeFloat)
        {
            Buffer.BlockCopy(e.Buffer, 0, samples, 0, e.BytesRecorded);
        }
        else if (_waveFormat.BitsPerSample == 16)
        {
            for (int i = 0; i < sampleCount; i++)
            {
                short sample = BitConverter.ToInt16(e.Buffer, i * 2);
                samples[i] = sample / 32768f;
            }
        }

        if (!_hasLogged && sampleCount > 0)
        {
            DebugLog.Write($"Audio 首次捕获: {sampleCount} samples, format={_waveFormat.SampleRate}Hz {_waveFormat.Channels}ch {_waveFormat.BitsPerSample}bit");
            _hasLogged = true;
        }

        DataAvailable?.Invoke(samples, _waveFormat);
    }

    private void OnRecordingStopped(object? sender, StoppedEventArgs e)
    {
        _isRunning = false;
        DebugLog.Write($"*** Audio Recording STOPPED! Exception={e.Exception?.Message ?? "null"} ***");
    }

    public void Dispose()
    {
        Stop();
    }
}
