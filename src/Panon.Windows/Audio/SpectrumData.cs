namespace Panon.Windows.Audio;

/// <summary>
/// 频谱数据模型，存储 FFT 计算后的频谱数据
/// </summary>
public sealed class SpectrumData
{
    /// <summary>
    /// 左声道频谱数据 (归一化 0.0~1.0)
    /// </summary>
    public float[] LeftChannel { get; set; } = Array.Empty<float>();

    /// <summary>
    /// 右声道频谱数据 (归一化 0.0~1.0)
    /// </summary>
    public float[] RightChannel { get; set; } = Array.Empty<float>();

    /// <summary>
    /// 频谱条数
    /// </summary>
    public int BarCount => LeftChannel.Length;

    /// <summary>
    /// 是否检测到节拍
    /// </summary>
    public bool BeatDetected { get; set; } = false;

    /// <summary>
    /// 音频 RMS 音量 (0.0~1.0)
    /// </summary>
    public float Volume { get; set; } = 0f;

    /// <summary>
    /// 是否静音
    /// </summary>
    public bool IsSilent => Volume < 0.001f;
}
