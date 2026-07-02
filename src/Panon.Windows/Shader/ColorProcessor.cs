namespace Panon.Windows.Shader;

/// <summary>
/// 颜色处理器，处理 HSL 和 HSLuv 色彩空间
/// 对应 panon 的颜色配置系统
/// </summary>
public sealed class ColorProcessor
{
    /// <summary>
    /// 将 HSL 转换为 RGB
    /// </summary>
    public static (float R, float G, float B) HslToRgb(float h, float s, float l)
    {
        // h: 0~360, s: 0~1, l: 0~1
        h = h % 360;
        if (h < 0) h += 360;

        float c = (1 - Math.Abs(2 * l - 1)) * s;
        float x = c * (1 - Math.Abs((h / 60) % 2 - 1));
        float m = l - c / 2;

        float r, g, b;
        if (h < 60) { r = c; g = x; b = 0; }
        else if (h < 120) { r = x; g = c; b = 0; }
        else if (h < 180) { r = 0; g = c; b = x; }
        else if (h < 240) { r = 0; g = x; b = c; }
        else if (h < 300) { r = x; g = 0; b = c; }
        else { r = c; g = 0; b = x; }

        return (r + m, g + m, b + m);
    }

    /// <summary>
    /// 将 HSLuv 转换为 RGB (简化实现)
    /// HSLuv 是感知均匀的色彩空间，完整实现需要 HSLuv 库
    /// </summary>
    public static (float R, float G, float B) HsluvToRgb(float h, float s, float l)
    {
        // 简化实现: 先转为 HSL 近似值
        // 完整实现需要引入 HSLuv 转换库
        // HSLuv 的色相范围与 HSL 相同，饱和度和亮度的映射不同
        float hslH = h % 360;
        if (hslH < 0) hslH += 360;
        float hslS = s / 100f;
        float hslL = l / 100f;

        return HslToRgb(hslH, hslS, hslL);
    }

    /// <summary>
    /// 根据频谱位置计算渐变颜色
    /// </summary>
    public static (float R, float G, float B) GetGradientColor(
        float position, // 0~1
        bool useHSLuv,
        int hueFrom, int hueTo,
        int saturation, int lightness)
    {
        float t = position;
        float hue = hueFrom + (hueTo - hueFrom) * t;

        if (useHSLuv)
        {
            return HsluvToRgb(hue, saturation, lightness);
        }
        else
        {
            return HslToRgb(hue, saturation / 100f, lightness / 100f);
        }
    }
}
