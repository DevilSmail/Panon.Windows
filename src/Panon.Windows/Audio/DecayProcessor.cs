namespace Panon.Windows.Audio;

/// <summary>
/// 频谱衰减处理器
/// 使用指数衰减实现平滑过渡，避免突变
/// 对应 panon 的 decay.py
/// </summary>
public sealed class DecayProcessor
{
    private float[] _prevLeft = Array.Empty<float>();
    private float[] _prevRight = Array.Empty<float>();

    /// <summary>
    /// 正常衰减因子 (每帧乘以此值，值越小衰减越快)
    /// 0.98 = 每帧保留 98%，约 30fps 下每秒降至 55%
    /// </summary>
    public float NormalFactor { get; set; } = 0.96f;

    /// <summary>
    /// 静音衰减因子（比正常稍快，让频谱自然消失）
    /// 0.75 = 每帧保留 75%，约 30fps 下 500ms 降至 3%，800ms 降至 0.5%
    /// 暂停时频谱快速但平滑地回落
    /// </summary>
    public float SilenceFactor { get; set; } = 0.75f;

    /// <summary>
    /// 退出专用衰减因子（极快衰减，确保 300ms 内完全回落）
    /// 0.80 = 每帧保留 80%，30fps 下 9 帧（300ms）降至 13%，14 帧降至 4%
    /// 仅在退出时通过 UseExitFactor 启用，不影响正常/暂停衰减
    /// </summary>
    public float ExitFactor { get; set; } = 0.80f;

    /// <summary>
    /// 是否启用退出衰减因子（退出时设为 true，衰减完成后程序销毁）
    /// </summary>
    public bool UseExitFactor { get; set; } = false;

    /// <summary>
    /// 静音阈值，低于此值视为静音
    /// </summary>
    public float SilenceThreshold { get; set; } = 0.002f;

    /// <summary>
    /// 最小可见值，低于此值归零
    /// </summary>
    public float MinValue { get; set; } = 0.002f;

    /// <summary>
    /// 对频谱数据应用衰减处理
    /// </summary>
    public SpectrumData Process(SpectrumData input)
    {
        var left = ApplyDecay(input.LeftChannel, ref _prevLeft, input.Volume);
        var right = ApplyDecay(input.RightChannel, ref _prevRight, input.Volume);

        return new SpectrumData
        {
            LeftChannel = left,
            RightChannel = right,
            Volume = input.Volume,
            BeatDetected = input.BeatDetected
        };
    }

    private float[] ApplyDecay(float[] current, ref float[] previous, float volume)
    {
        if (previous.Length != current.Length)
        {
            previous = new float[current.Length];
        }

        bool isSilent = volume < SilenceThreshold;
        // 优先级：退出因子 > 静音因子 > 正常因子
        float factor = UseExitFactor ? ExitFactor : (isSilent ? SilenceFactor : NormalFactor);

        var result = new float[current.Length];

        for (int i = 0; i < current.Length; i++)
        {
            if (current[i] >= previous[i])
            {
                // 当前值更大，直接使用
                result[i] = current[i];
            }
            else
            {
                // 指数衰减：每帧乘以衰减因子，非常平滑
                result[i] = previous[i] * factor;
                // 不低于当前输入值
                if (result[i] < current[i])
                    result[i] = current[i];
                // 过低则归零
                if (result[i] < MinValue)
                    result[i] = 0;
            }

            previous[i] = result[i];
        }

        return result;
    }

    /// <summary>
    /// 重置衰减状态
    /// </summary>
    public void Reset()
    {
        _prevLeft = Array.Empty<float>();
        _prevRight = Array.Empty<float>();
    }

    /// <summary>
    /// 获取当前衰减后的频谱最大值（用于检测频谱是否已回落到 2px 细线状态）
    /// </summary>
    public float GetMaxDecayedValue()
    {
        float max = 0;
        for (int i = 0; i < _prevLeft.Length; i++)
            if (_prevLeft[i] > max) max = _prevLeft[i];
        for (int i = 0; i < _prevRight.Length; i++)
            if (_prevRight[i] > max) max = _prevRight[i];
        return max;
    }
}
