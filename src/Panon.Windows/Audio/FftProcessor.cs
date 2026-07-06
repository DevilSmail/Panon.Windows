using NAudio.Wave;
using Panon.Windows.Helpers;

namespace Panon.Windows.Audio;

/// <summary>
/// FFT 频谱处理器
/// 将 PCM 音频数据转换为频域频谱数据
/// </summary>
public sealed class FftProcessor
{
    // 频率范围定义，对应 panon 的 7 级 bassResolutionLevel
    private static readonly (double Low, double High)[] FrequencyRanges =
    {
        (0, 22050),     // Level 0: 全频段
        (0, 9000),      // Level 1
        (0, 3000),      // Level 2: F7
        (0, 1800),      // Level 3: A6
        (0, 1800),      // Level 4: A6 (低延迟)
        (300, 1800),    // Level 5: 过滤低音
        (0, 600),       // Level 6: D5
    };

    private int _bassResolutionLevel = 4;
    private bool _reduceBass = true;
    private int _sampleRate = 44100;
#if DEBUG
    private int _fftLogCount;
    private DateTime _lastFftLogTime = DateTime.MinValue;
#endif

    // 预分配 FFT 缓冲区（避免每帧分配，音频线程专用，无需锁）
    private const int FftSize = 2048;
    private const int HalfSize = FftSize / 2;
    private float[] _leftSamples = Array.Empty<float>();
    private float[] _rightSamples = Array.Empty<float>();
    private readonly float[] _windowed = new float[FftSize];
    private readonly float[] _real = new float[FftSize];
    private readonly float[] _imag = new float[FftSize];
    private float[] _magnitudes = new float[HalfSize];
    private float[] _spectrum = Array.Empty<float>();

    // 复用 SpectrumData 对象（LeftChannel/RightChannel 数组不复用，因渲染线程跨帧持有引用）
    private SpectrumData? _resultData;

    /// <summary>
    /// 频谱数据更新事件
    /// </summary>
    public event Action<SpectrumData>? SpectrumUpdated;

    /// <summary>
    /// 频率分辨率等级 (0-6)
    /// </summary>
    public int BassResolutionLevel
    {
        get => _bassResolutionLevel;
        set => _bassResolutionLevel = Math.Clamp(value, 0, 6);
    }

    /// <summary>
    /// 是否降低低音权重
    /// </summary>
    public bool ReduceBass
    {
        get => _reduceBass;
        set => _reduceBass = value;
    }

    /// <summary>
    /// 处理音频采样数据
    /// </summary>
    public void Process(float[] samples, WaveFormat format)
    {
        _sampleRate = format.SampleRate;
        int channels = format.Channels;

        // 分离左右声道（复用缓冲区）
        int frameCount = samples.Length / channels;
        if (_leftSamples.Length != frameCount)
        {
            _leftSamples = new float[frameCount];
            _rightSamples = new float[frameCount];
        }

        for (int i = 0; i < frameCount; i++)
        {
            _leftSamples[i] = samples[i * channels];
            _rightSamples[i] = channels > 1 ? samples[i * channels + 1] : samples[i * channels];
        }

        // 计算频谱
        var leftSpectrum = ComputeSpectrum(_leftSamples);
        var rightSpectrum = ComputeSpectrum(_rightSamples);

        // 计算 RMS 音量
        float rms = ComputeRms(samples);

        // 复用 SpectrumData 对象（LeftChannel/RightChannel 数组仍是新分配，因渲染线程跨帧持有引用）
        var data = _resultData ??= new SpectrumData();
        data.LeftChannel = leftSpectrum;
        data.RightChannel = rightSpectrum;
        data.Volume = rms;
        data.BeatDetected = false;

#if DEBUG
        _fftLogCount++;
        if (_fftLogCount <= 10 || (DateTime.Now - _lastFftLogTime).TotalSeconds > 5)
        {
            DebugLog.Write($"FFT #{_fftLogCount}: {leftSpectrum.Length} bars, max={leftSpectrum.Max():F4}, rms={rms:F4}");
            _lastFftLogTime = DateTime.Now;
        }
#endif

        SpectrumUpdated?.Invoke(data);
    }

    private float[] ComputeSpectrum(float[] samples)
    {
        int useSamples = Math.Min(samples.Length, FftSize);

        // 应用汉宁窗（复用 _windowed，用后清零尾部）
        Array.Clear(_windowed, useSamples, FftSize - useSamples);
        for (int i = 0; i < useSamples; i++)
        {
            float window = 0.5f * (1 - (float)Math.Cos(2 * Math.PI * i / (useSamples - 1)));
            _windowed[i] = samples[i] * window;
        }

        // 就地 FFT（复用 _real / _imag）
        Array.Copy(_windowed, _real, FftSize);
        Array.Clear(_imag, 0, FftSize);
        Fft(_real, _imag, FftSize);

        // 计算幅度谱（复用 _magnitudes）
        for (int i = 0; i < HalfSize; i++)
        {
            _magnitudes[i] = (float)Math.Sqrt(_real[i] * _real[i] + _imag[i] * _imag[i]) / FftSize;
        }

        // 根据频率范围截取
        var (lowFreq, highFreq) = FrequencyRanges[_bassResolutionLevel];
        int lowBin = (int)(lowFreq * FftSize / _sampleRate);
        int highBin = (int)(highFreq * FftSize / _sampleRate);
        highBin = Math.Min(highBin, HalfSize - 1);

        int barCount = highBin - lowBin;
        if (barCount <= 0) barCount = 1;

        // 复用 _spectrum 当长度匹配时
        if (_spectrum.Length != barCount)
            _spectrum = new float[barCount];

        for (int i = 0; i < barCount; i++)
        {
            _spectrum[i] = _magnitudes[lowBin + i];
        }

        // 低音衰减
        if (_reduceBass)
        {
            ApplyBassReduction(_spectrum, lowBin, _sampleRate, FftSize);
        }

        // 归一化到 0~1 → 返回副本（消费者持有引用，不能返回复用数组）
        float max = _spectrum.Max();
        var result = new float[barCount];
        if (max > 0.001f)
        {
            for (int i = 0; i < barCount; i++)
            {
                result[i] = Math.Clamp(_spectrum[i] / max, 0f, 1f);
            }
        }

        return result;
    }

    private static void ApplyBassReduction(float[] spectrum, int lowBin, int sampleRate, int fftSize)
    {
        for (int i = 0; i < spectrum.Length; i++)
        {
            double freq = (lowBin + i) * (double)sampleRate / fftSize;
            if (freq < 300)
            {
                double factor = freq / 300.0;
                spectrum[i] *= (float)factor;
            }
        }
    }

    private static float ComputeRms(float[] samples)
    {
        double sum = 0;
        for (int i = 0; i < samples.Length; i++)
        {
            sum += samples[i] * samples[i];
        }
        return (float)Math.Sqrt(sum / samples.Length);
    }

    /// <summary>
    /// 就地 Cooley-Tukey FFT
    /// </summary>
    private static void Fft(float[] real, float[] imag, int n)
    {
        // 位反转排列
        int j = 0;
        for (int i = 0; i < n - 1; i++)
        {
            if (i < j)
            {
                (real[i], real[j]) = (real[j], real[i]);
                (imag[i], imag[j]) = (imag[j], imag[i]);
            }
            int k = n >> 1;
            while (k <= j)
            {
                j -= k;
                k >>= 1;
            }
            j += k;
        }

        // 蝶形运算
        for (int len = 2; len <= n; len <<= 1)
        {
            double angle = -2 * Math.PI / len;
            float wReal = (float)Math.Cos(angle);
            float wImag = (float)Math.Sin(angle);

            for (int i = 0; i < n; i += len)
            {
                float curReal = 1, curImag = 0;
                for (int m = 0; m < len / 2; m++)
                {
                    int evenIdx = i + m;
                    int oddIdx = i + m + len / 2;

                    float tReal = curReal * real[oddIdx] - curImag * imag[oddIdx];
                    float tImag = curReal * imag[oddIdx] + curImag * real[oddIdx];

                    real[oddIdx] = real[evenIdx] - tReal;
                    imag[oddIdx] = imag[evenIdx] - tImag;
                    real[evenIdx] += tReal;
                    imag[evenIdx] += tImag;

                    float newCurReal = curReal * wReal - curImag * wImag;
                    curImag = curReal * wImag + curImag * wReal;
                    curReal = newCurReal;
                }
            }
        }
    }
}
