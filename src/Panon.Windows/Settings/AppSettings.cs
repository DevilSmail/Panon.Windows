namespace Panon.Windows.Settings;

/// <summary>
/// 应用所有设置的数据模型，对应 panon 的 KConfig 配置
/// </summary>
public sealed class AppSettings
{
    // === Backend 设置 ===
    public bool ReduceBass { get; set; } = true;
    public int BassResolutionLevel { get; set; } = 4;

    // === General 设置 ===
    public int Fps { get; set; } = 30;
    public int Gravity { get; set; } = 2; // 0=Center, 1=North(从上到下), 2=South(从下到上,默认), 3=East, 4=West
    public bool Inversion { get; set; } = false;

    // === Colors 设置 ===
    public bool ColorSpaceHSLuv { get; set; } = false;
    public int HslHueFrom { get; set; } = 180;
    public int HslHueTo { get; set; } = 720;
    public int HslSaturation { get; set; } = 60;
    public int HslLightness { get; set; } = 50;
    public int HsluvHueFrom { get; set; } = 270;
    public int HsluvHueTo { get; set; } = -270;
    public int HsluvSaturation { get; set; } = 100;
    public int HsluvLightness { get; set; } = 50;

    // === Bar 设置 ===
    /// <summary>柱子宽度（像素），与 GapWidth 共同决定柱子数量填满任务栏</summary>
    public int BarWidth { get; set; } = 6;

    /// <summary>柱间间隙宽度（像素），0=无缝隙</summary>
    public int GapWidth { get; set; } = 3;

    // === 图形效果设置（GLSL → CPU 软件模拟） ===
    /// <summary>
    /// 当前图形效果名称（对应 Shader/Shaders/ 下的 .frag 文件）
    /// "bar1ch"=柱状图(当前), "hill1ch"=山丘, "wave"=波浪, "solid1ch"=实心, 等
    /// </summary>
    public string VisualEffectName { get; set; } = "bar1ch";

    // === 填充模式 ===
    /// <summary>
    /// 任务栏填充模式: 0=铺满任务栏, 1=仅空白区域(默认)
    /// </summary>
    public int FillMode { get; set; } = 1;

    // === Windows 专属设置 ===
    /// <summary>开机自启（写入 HKCU\...\Run 注册表）</summary>
    public bool StartWithWindows { get; set; } = false;

    /// <summary>
    /// 目标显示器: "0"="主显示器"(默认), "1","2",...=指定显示器, "-1"="所有显示器"
    /// </summary>
    public string TargetMonitor { get; set; } = "0";
    public int OverlayMode { get; set; } = 1; // 1=UnderTaskbar(任务栏覆盖在频谱上面,默认), 2=AboveTaskbar(频谱覆盖在任务栏上面)
}
