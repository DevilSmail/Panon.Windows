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
    private int _fftLogCount;
    private DateTime _lastFftLogTime = DateTime.MinValue;

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

        // 分离左右声道
        int frameCount = samples.Length / channels;
        var leftSamples = new float[frameCount];
        var rightSamples = new float[frameCount];

        for (int i = 0; i < frameCount; i++)
        {
            leftSamples[i] = samples[i * channels];
            rightSamples[i] = channels > 1 ? samples[i * channels + 1] : samples[i * channels];
        }

        // 计算频谱
        var leftSpectrum = ComputeSpectrum(leftSamples);
        var rightSpectrum = ComputeSpectrum(rightSamples);

        // 计算 RMS 音量
        float rms = ComputeRms(samples);

        var data = new SpectrumData
        {
            LeftChannel = leftSpectrum,
            RightChannel = rightSpectrum,
            Volume = rms,
            BeatDetected = false // 节拍检测后续实现
        };

        // 诊断：持续记录前 10 次 + 每 5 秒一次
        _fftLogCount++;
        if (_fftLogCount <= 10 || (DateTime.Now - _lastFftLogTime).TotalSeconds > 5)
        {
            DebugLog.Write($"FFT #{_fftLogCount}: {leftSpectrum.Length} bars, max={leftSpectrum.Max():F4}, rms={rms:F4}");
            _lastFftLogTime = DateTime.Now;
        }

        SpectrumUpdated?.Invoke(data);
    }

    private float[] ComputeSpectrum(float[] samples)
    {
        // FFT 大小: 取 2 的幂次
        int fftSize = 2048;
        int useSamples = Math.Min(samples.Length, fftSize);

        // 应用汉宁窗
        var windowed = new float[fftSize];
        for (int i = 0; i < useSamples; i++)
        {
            float window = 0.5f * (1 - (float)Math.Cos(2 * Math.PI * i / (useSamples - 1)));
            windowed[i] = samples[i] * window;
        }

        // 就地 FFT (Cooley-Tukey)
        var real = new float[fftSize];
        var imag = new float[fftSize];
        Array.Copy(windowed, real, fftSize);

        Fft(real, imag, fftSize);

        // 计算幅度谱
        int halfSize = fftSize / 2;
        var magnitudes = new float[halfSize];
        for (int i = 0; i < halfSize; i++)
        {
            magnitudes[i] = (float)Math.Sqrt(real[i] * real[i] + imag[i] * imag[i]) / fftSize;
        }

        // 根据频率范围截取
        var (lowFreq, highFreq) = FrequencyRanges[_bassResolutionLevel];
        int lowBin = (int)(lowFreq * fftSize / _sampleRate);
        int highBin = (int)(highFreq * fftSize / _sampleRate);
        highBin = Math.Min(highBin, halfSize - 1);

        int barCount = highBin - lowBin;
        if (barCount <= 0) barCount = 1;

        var spectrum = new float[barCount];
        for (int i = 0; i < barCount; i++)
        {
            spectrum[i] = magnitudes[lowBin + i];
        }

        // 低音衰减
        if (_reduceBass)
        {
            ApplyBassReduction(spectrum, lowBin, _sampleRate, fftSize);
        }

        // 归一化到 0~1
        float max = spectrum.Max();
        if (max > 0.001f)
        {
            for (int i = 0; i < spectrum.Length; i++)
            {
                spectrum[i] = Math.Clamp(spectrum[i] / max, 0f, 1f);
            }
        }

        return spectrum;
    }

    private static void ApplyBassReduction(float[] spectrum, int lowBin, int sampleRate, int fftSize)
    {
        // 降低低频权重，避免低音主导频谱
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
