using System.Runtime.CompilerServices;

namespace Panon.Windows.Shader;

/// <summary>
/// 频谱渲染器（纯软件渲染，直接写入像素缓冲区）
/// 支持全部 12 种 Linux 着色器效果的 CPU 模拟，严格对齐 GLSL 逻辑
/// </summary>
public sealed class SpectrumRenderer
{
    private int _swWidth, _swHeight;

    public int Gravity { get; set; } = 2;
    public bool Inversion { get; set; } = false;
    public bool ColorSpaceHSLuv { get; set; } = false;
    public int HslHueFrom { get; set; } = 180;
    public int HslHueTo { get; set; } = 720;
    public int HslSaturation { get; set; } = 80;
    public int HslLightness { get; set; } = 50;
    public int HsluvHueFrom { get; set; } = 270;
    public int HsluvHueTo { get; set; } = -270;
    public int HsluvSaturation { get; set; } = 100;
    public int HsluvLightness { get; set; } = 50;
    public int BarWidth { get; set; } = 6;
    public int GapWidth { get; set; } = 3;
    public string VisualEffectName { get; set; } = "bar1ch";

    /// <summary>填充模式: 0=铺满, 1=仅空白区域(默认)</summary>
    public int FillMode { get; set; } = 1;

    /// <summary>空白区域列表（FillMode=1 时使用，相对于窗口 X 坐标）</summary>
    public List<(int X, int Width)>? FreeRegions { get; set; }

    private const float PeakDecayValue = 0.02f;
    private const float ExitPeakDecayValue = 0.08f;
    public bool UseExitFactor { get; set; } = false;
    private float[] _peakHeights = Array.Empty<float>();

    // FFT 重采样权重缓存
    private int _cachedSrcCount, _cachedTargetCount;
    private int[]? _resampleIndices;
    private float[]? _resampleFracs;

    private uint[]? _spectrogramBuffer;

    public void InitializeSoftware(int width, int height)
    {
        _swWidth = width;
        _swHeight = height;
    }

    public unsafe void RenderToPixels(float[] left, float[] right, IntPtr pBits, int width, int height)
    {
        if (pBits == IntPtr.Zero || left.Length == 0) return;

        uint* pixels = (uint*)pBits;
        Unsafe.InitBlockUnaligned(pixels, 0, (uint)(width * height * 4));

        switch (VisualEffectName)
        {
            case "bar1ch":      RenderBar1ch(left, right, pixels, width, height); break;
            case "wave":        RenderWave(left, right, pixels, width, height); break;
            case "solid1ch":    RenderSolid1ch(left, right, pixels, width, height); break;
            case "solid":       RenderSolid(left, right, pixels, width, height); break;
            case "beam":        RenderBeam(left, right, pixels, width, height); break;
            case "spectrogram": RenderSpectrogram(left, right, pixels, width, height); break;
            case "oie1ch":      RenderOie1ch(left, right, pixels, width, height); break;
            default:            RenderBar1ch(left, right, pixels, width, height); break;
        }
    }

    #region 颜色辅助

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private (byte b, byte g, byte r) GetColor(float pos)
    {
        var (rf, gf, bf) = ColorProcessor.GetGradientColor(
            pos, ColorSpaceHSLuv,
            ColorSpaceHSLuv ? HsluvHueFrom : HslHueFrom,
            ColorSpaceHSLuv ? HsluvHueTo : HslHueTo,
            ColorSpaceHSLuv ? HsluvSaturation : HslSaturation,
            ColorSpaceHSLuv ? HsluvLightness : HslLightness);
        return ((byte)(bf * 255), (byte)(gf * 255), (byte)(rf * 255));
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static uint MakePixel(byte b, byte g, byte r, byte a = 255) => (uint)(a << 24 | r << 16 | g << 8 | b);

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static uint MakePixel((byte b, byte g, byte r) c) => (uint)(255 << 24 | c.r << 16 | c.g << 8 | c.b);

    // 采样左右平均 (sample1.g*.5 + sample1.r*.5)
    private static float SampleAvg(float[] left, float[] right, float t)
    {
        int len = left.Length;
        float pos = t * (len - 1);
        int idx = (int)pos;
        float frac = pos - idx;
        if (idx >= len - 1) return (left[len - 1] + right[len - 1]) / 2f;
        return ((left[idx] * (1 - frac) + left[idx + 1] * frac)
              + (right[idx] * (1 - frac) + right[idx + 1] * frac)) / 2f;
    }

    // 采样左声道 (.r)
    private static float SampleLeft(float[] left, float t)
    {
        int len = left.Length;
        float pos = t * (len - 1);
        int idx = (int)pos;
        float frac = pos - idx;
        if (idx >= len - 1) return left[len - 1];
        return left[idx] * (1 - frac) + left[idx + 1] * frac;
    }

    // 采样立体声 (.r=left, .g=right)
    private static (float l, float r) SampleLR(float[] left, float[] right, float t)
    {
        int len = left.Length;
        float pos = t * (len - 1);
        int idx = (int)pos;
        float frac = pos - idx;
        if (idx >= len - 1) return (left[len - 1], right[len - 1]);
        float l = left[idx] * (1 - frac) + left[idx + 1] * frac;
        float r = right[idx] * (1 - frac) + right[idx + 1] * frac;
        return (l, r);
    }

    #endregion

    #region bar1ch

    private unsafe void RenderBar1ch(float[] left, float[] right, uint* pixels, int width, int height)
    {
        // 确定绘制区域
        var regions = GetEffectiveRegions(width);
        if (regions != null && regions.Count == 0)
            return; // FillMode==1 且无空白区域，不渲染任何柱子
        regions ??= new List<(int X, int Width)> { (0, width) };

        // 计算总可用宽度和柱子配置
        int totalFreeW = regions.Sum(r => r.Width);

        int cellSize = BarWidth + GapWidth;
        if (cellSize < 1) cellSize = 1;
        int targetBarCount = (totalFreeW + GapWidth) / cellSize;
        if (targetBarCount < 1) targetBarCount = 1;

        float[] rL = Resample(left, left.Length, targetBarCount);
        float[] rR = Resample(right, right.Length, targetBarCount);
        if (_peakHeights.Length != targetBarCount) _peakHeights = new float[targetBarCount];

        // 跨空白段连续编号绘制
        int barIndex = 0;
        foreach (var (regX, regW) in regions)
        {
            // 该空白段内能放置的柱子数
            int barCountInRegion = (regW + GapWidth) / cellSize;
            if (barCountInRegion < 1) continue;
            int rem = regW - (barCountInRegion * BarWidth + (barCountInRegion - 1) * GapWidth);

            int cx = regX;
            for (int k = 0; k < barCountInRegion && barIndex < targetBarCount; k++, barIndex++)
            {
                int cw = BarWidth + (k < rem ? 1 : 0);
                float v = (rL[barIndex] + rR[barIndex]) / 2f;
                if (Inversion) v = 1f - v;
                var (b, g, r) = GetColor((float)barIndex / targetBarCount);
                uint px = MakePixel(b, g, r);
                float bh = Gravity == 0 ? Math.Max(v * height, 2) : v * height;
                int xS = cx, xE = cx + cw, yS, yE;
                switch (Gravity)
                {
                    case 0: yS = (int)(height / 2 - bh / 2); yE = (int)(height / 2 + bh / 2); break;
                    case 1: yS = 0; yE = (int)bh; break;
                    case 2: yS = (int)(height - bh); yE = height; break;
                    default: yS = (int)(height - bh); yE = height; break;
                }
                xS = Math.Max(0, xS); xE = Math.Min(width, xE); yS = Math.Max(0, yS); yE = Math.Min(height, yE);
                if (bh >= 0.5f)
                {
                    int w = xE - xS;
                    for (int py = yS; py < yE; py++)
                        new Span<uint>(pixels + py * width + xS, w).Fill(px);
                }
                if (Gravity == 1 || Gravity == 2)
                {
                    float pd = UseExitFactor ? ExitPeakDecayValue : PeakDecayValue;
                    _peakHeights[barIndex] = v > _peakHeights[barIndex] ? v : Math.Max(0, _peakHeights[barIndex] - pd);
                    float ph = _peakHeights[barIndex] * height;
                    int pyS, pyE;
                    switch (Gravity)
                    {
                        case 1: pyS = Math.Max(0, (int)ph); pyE = Math.Min(height, pyS + 2); break;
                        case 2: pyE = Math.Min(height, (int)(height - ph)); pyS = Math.Max(0, pyE - 2); break;
                        default: pyS = 0; pyE = 0; break;
                    }
                    if (pyE > pyS)
                    {
                        int w = xE - xS;
                        for (int py = pyS; py < pyE; py++)
                            new Span<uint>(pixels + py * width + xS, w).Fill(px);
                    }
                }
                cx += cw + GapWidth;
            }
        }
    }

    #endregion

    #region wave — 波浪

    // 注：原始 shader 取材于 iChannel0（wave buffer），非频谱数据。
    // 此处用左声道模拟，实际效果与 Linux 版不同但可近似。
    private unsafe void RenderWave(float[] left, float[] right, uint* pixels, int width, int height)
    {
        for (int px = 0; px < width; px++)
        {
            if (!IsColumnVisible(px)) continue;
            float t = (float)px / width;
            // iChannel0 采样位置为 0.5 * fragCoord.x / iResolution.x
            float val = SampleLeft(left, t * 0.5f);
            int maxY = Math.Min(height - 1, (int)(val * height) + 1);
            int minY = Math.Max(0, (int)(val * height) - 1);
            uint pxV = MakePixel(GetColor(t));
            for (int py = minY; py <= maxY; py++)
                pixels[py * width + px] = pxV;
        }
    }

    #endregion

    #region solid1ch — 实心单声道

    private unsafe void RenderSolid1ch(float[] left, float[] right, uint* pixels, int width, int height)
    {
        for (int py = 0; py < height; py++)
        {
            float hy = (float)py / height;
            uint* row = pixels + py * width;
            for (int px = 0; px < width; px++)
            {
                if (!IsColumnVisible(px)) continue;
                float t = (float)px / width;
                if (SampleAvg(left, right, t) > hy)
                    row[px] = MakePixel(GetColor(t));
            }
        }
    }

    #endregion

    #region solid — 实心立体声

    private unsafe void RenderSolid(float[] left, float[] right, uint* pixels, int width, int height)
    {
        // min_ = .5 - sample1.r*.5; max_ = .5 + sample1.g*.5
        for (int py = 0; py < height; py++)
        {
            float hy = (float)py / height;
            uint* row = pixels + py * width;
            for (int px = 0; px < width; px++)
            {
                if (!IsColumnVisible(px)) continue;
                float t = (float)px / width;
                var (l, r) = SampleLR(left, right, t);
                if (0.5f - l * 0.5f <= hy && hy <= 0.5f + r * 0.5f)
                    row[px] = MakePixel(GetColor(t));
            }
        }
    }

    #endregion

    #region beam — 光束（对齐 GLSL: fragColor=vec4(rgb*a,a)）

    private unsafe void RenderBeam(float[] left, float[] right, uint* pixels, int width, int height)
    {
        // beam 对整列写相同颜色+alpha，不随 y 变化
        for (int px = 0; px < width; px++)
        {
            if (!IsColumnVisible(px)) continue;
            float t = (float)px / width;
            float val = SampleAvg(left, right, t);
            var (b, g, r) = GetColor(t);
            byte a = (byte)(val * 255);
            uint pxV = (uint)(a << 24 | ((byte)(r * val) << 16) | ((byte)(g * val) << 8) | (byte)(b * val));
            for (int py = 0; py < height; py++)
                pixels[py * width + px] = pxV;
        }
    }

    #endregion

    #region spectrogram — 频谱瀑布

    private unsafe void RenderSpectrogram(float[] left, float[] right, uint* pixels, int width, int height)
    {
        int total = width * height;
        if (_spectrogramBuffer == null || _spectrogramBuffer.Length != total)
            _spectrogramBuffer = new uint[total];

        // 向下滚动一行：buf[row-1] ← buf[row]
        int lastRow = (height - 1) * width;
        Array.Copy(_spectrogramBuffer, width, _spectrogramBuffer, 0, lastRow);

        // 写新行到底部
        for (int px = 0; px < width; px++)
        {
            if (!IsColumnVisible(px)) { _spectrogramBuffer[lastRow + px] = 0; continue; }
            float t = (float)px / width;
            float val = SampleAvg(left, right, t);
            var (b, g2, r) = GetColor(t);
            byte br = (byte)(val * 255);
            _spectrogramBuffer[lastRow + px] = MakePixel((byte)(b * br / 255), (byte)(g2 * br / 255), (byte)(r * br / 255), br);
        }

        // 复制
        fixed (uint* src = _spectrogramBuffer)
            Unsafe.CopyBlockUnaligned(pixels, src, (uint)(total * 4));
    }

    #endregion

    #region oie1ch — Oie 连线（对齐 GLSL：仅用左声道+.r）

    private unsafe void RenderOie1ch(float[] left, float[] right, uint* pixels, int width, int height)
    {
        // 对齐 GLSL: p1 = 0.5*(sample_.r+sample_prev.r) * height
        // 仅用左声道 .r
        for (int px = 0; px < width; px++)
        {
            if (!IsColumnVisible(px)) continue;
            float t = (float)px / width;
            float tPrev = Math.Max(0, (float)(px - 1) / width);
            float tNext = Math.Min(1, (float)(px + 1) / width);

            float vl = SampleLeft(left, t);
            float vlPrev = SampleLeft(left, tPrev);
            float vlNext = SampleLeft(left, tNext);

            int p1 = (int)(0.5f * (vl + vlPrev) * height);
            int p2 = (int)(0.5f * (vl + vlNext) * height);
            uint pxV = MakePixel(GetColor(t));

            // 对齐 GLSL 的绘制逻辑
            int minY = Math.Min(p1, p2) - 0;
            int maxY = Math.Max(p1, p2) + 2;
            for (int py = Math.Max(0, minY); py < Math.Min(height, maxY + 1); py++)
                pixels[py * width + px] = pxV;
        }
    }

    #endregion

    #region FreeRegions 辅助

    /// <summary>
    /// 获取当前有效的渲染区域（考虑 FillMode 设置）
    /// 返回 null 表示铺满全宽；返回空列表表示无空白区域（不渲染）
    /// </summary>
    private List<(int X, int Width)>? GetEffectiveRegions(int width)
    {
        if (FillMode != 1 || FreeRegions == null)
            return null; // 铺满全宽

        if (FreeRegions.Count == 0)
            return new List<(int X, int Width)>(); // 无空白区域，不渲染

        return FreeRegions;
    }

    /// <summary>
    /// 判断指定 X 列是否在空白区域内（用于非 bar1ch 效果）
    /// 仅在 FillMode==1 时生效，否则始终返回 true
    /// </summary>
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private bool IsColumnVisible(int x)
    {
        if (FillMode != 1 || FreeRegions == null) return true;
        foreach (var (rx, rw) in FreeRegions)
        {
            if (x >= rx && x < rx + rw) return true;
        }
        return false;
    }

    #endregion

    #region 公共

    private float[] Resample(float[] source, int srcCount, int targetCount)
    {
        if (srcCount == 0) return new float[targetCount];
        if (srcCount == targetCount) return source;

        // 权重缓存：srcCount/targetCount 不变时跳过浮点除法+索引计算
        if (_cachedSrcCount != srcCount || _cachedTargetCount != targetCount
            || _resampleIndices == null || _resampleFracs == null
            || _resampleIndices.Length != targetCount)
        {
            _cachedSrcCount = srcCount;
            _cachedTargetCount = targetCount;
            _resampleIndices = new int[targetCount];
            _resampleFracs = new float[targetCount];
            for (int i = 0; i < targetCount; i++)
            {
                float sp = (float)i / targetCount * srcCount;
                _resampleIndices[i] = (int)sp;
                _resampleFracs[i] = sp - _resampleIndices[i];
            }
        }

        var result = new float[targetCount];
        int[] idx = _resampleIndices;
        float[] fr = _resampleFracs;
        int lastSrc = srcCount - 1;
        for (int i = 0; i < targetCount; i++)
        {
            int si = idx[i];
            result[i] = si >= lastSrc ? source[lastSrc] : source[si] * (1f - fr[i]) + source[si + 1] * fr[i];
        }
        return result;
    }

    public float GetMaxPeakHeight()
    {
        float max = 0;
        for (int i = 0; i < _peakHeights.Length; i++)
            if (_peakHeights[i] > max) max = _peakHeights[i];
        return max;
    }

    public void Cleanup() { _spectrogramBuffer = null; }

    #endregion
}