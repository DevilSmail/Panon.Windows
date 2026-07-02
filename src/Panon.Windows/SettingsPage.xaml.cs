using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.Win32;
using Panon.Windows.Settings;
using System.Runtime.InteropServices;

namespace Panon.Windows;

/// <summary>
/// 设置页面
/// </summary>
public sealed partial class SettingsPage : Page
{
    private AppSettings _settings => App.Settings.Current;
    private bool _isLoading;

    public SettingsPage()
    {
        InitializeComponent();
        _isLoading = true;
        ConfigureSliders();
        LoadSettings();
        Loaded += OnPageLoaded;
    }

    private void OnPageLoaded(object sender, RoutedEventArgs e)
    {
        // 延迟设置 _isLoading = false，确保 WinUI3 延迟触发的 SelectionChanged/ValueChanged 事件被拦截
        var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(500) };
        timer.Tick += (s, args) =>
        {
            _isLoading = false;
            timer.Stop();
        };
        timer.Start();
    }

    /// <summary>
    /// 在代码中设置 Slider 范围（避免 WinUI3 XAML 按字母顺序解析 Minimum/Value 导致异常）
    /// </summary>
    private void ConfigureSliders()
    {
        // 顺序: Maximum → Value → Minimum（避免值越界）
        BassResolutionSlider.Maximum = 6;
        BassResolutionSlider.Value = 0;
        BassResolutionSlider.Minimum = 0;

        FpsSlider.Maximum = 60;
        FpsSlider.Value = 30;
        FpsSlider.Minimum = 10;

        // BarWidth 滑块: 1~30 像素
        BarWidthSlider.Maximum = 30;
        BarWidthSlider.Value = 6;
        BarWidthSlider.Minimum = 1;

        // GapWidth 滑块: 0~20 像素
        GapWidthSlider.Maximum = 20;
        GapWidthSlider.Value = 3;
        GapWidthSlider.Minimum = 0;

        // 颜色滑块范围对齐 Linux 版本（-4000~4000）
        HueFromSlider.Maximum = 4000;
        HueFromSlider.Value = 0;
        HueFromSlider.Minimum = -4000;

        HueToSlider.Maximum = 4000;
        HueToSlider.Value = 0;
        HueToSlider.Minimum = -4000;

        SaturationSlider.Maximum = 100;
        SaturationSlider.Value = 80;
        SaturationSlider.Minimum = 0;

        LightnessSlider.Maximum = 100;
        LightnessSlider.Value = 50;
        LightnessSlider.Minimum = 0;
    }

    /// <summary>
    /// 从设置加载到 UI 控件
    /// </summary>
    private void LoadSettings()
    {
        // 音频
        ReduceBassToggle.IsOn = _settings.ReduceBass;
        BassResolutionSlider.Value = _settings.BassResolutionLevel;
        FpsSlider.Value = _settings.Fps;

        // 显示
        if (_settings.Gravity >= 0 && _settings.Gravity < GravityCombo.Items.Count)
            GravityCombo.SelectedIndex = _settings.Gravity;
        else
            GravityCombo.SelectedIndex = 2; // 默认 South
        InversionToggle.IsOn = _settings.Inversion;

        // 柱宽和间隙
        BarWidthSlider.Value = _settings.BarWidth;
        GapWidthSlider.Value = _settings.GapWidth;

        // 颜色
        if (_settings.ColorSpaceHSLuv)
            ColorSpaceRadio.SelectedIndex = 1;
        else
            ColorSpaceRadio.SelectedIndex = 0;

        UpdateColorSliders();

        // Windows
        // 覆盖模式: SelectedIndex 0=Under(1), 1=Above(2)
        int savedMode = _settings.OverlayMode;
        if (savedMode >= 1 && savedMode <= 2)
            OverlayModeCombo.SelectedIndex = savedMode - 1;
        else
            OverlayModeCombo.SelectedIndex = 0; // 默认任务栏覆盖在频谱上面

        // 图形效果
        VisualEffectCombo.SelectedIndex = 0; // 默认柱状图

        // 根据图形效果控制柱宽/间隙是否可用
        UpdateBarControlsEnabled();

        // 填充模式
        if (_settings.FillMode >= 0 && _settings.FillMode < FillModeCombo.Items.Count)
            FillModeCombo.SelectedIndex = _settings.FillMode;
        else
            FillModeCombo.SelectedIndex = 1; // 默认仅空白区域

        // 开机自启（以注册表为准，不受 settings.json 影响）
        SyncStartWithWindowsFromRegistry();
        StartWithWindowsToggle.IsOn = _settings.StartWithWindows;

        // 目标显示器（动态填充列表 + 恢复选中项）
        PopulateMonitorCombo();

        // 透明度状态
        RefreshTransparencyStatus();

        // 更新所有数值显示
        UpdateValueDisplays();

        // 检测当前配色是否匹配预设
        UpdatePresetSelection();
    }

    /// <summary>
    /// 更新所有滑块右侧的当前值显示
    /// </summary>
    private void UpdateValueDisplays()
    {
        BassResolutionValue.Text = $"当前: {(int)BassResolutionSlider.Value}";
        FpsValue.Text = $"当前: {(int)FpsSlider.Value}";
        BarWidthValue.Text = $"当前: {(int)BarWidthSlider.Value}px";
        GapWidthValue.Text = $"当前: {(int)GapWidthSlider.Value}px";
        HueFromValue.Text = $"当前: {(int)HueFromSlider.Value}";
        HueToValue.Text = $"当前: {(int)HueToSlider.Value}";
        SaturationValue.Text = $"当前: {(int)SaturationSlider.Value}";
        LightnessValue.Text = $"当前: {(int)LightnessSlider.Value}";
    }

    /// <summary>
    /// 根据当前色彩空间更新滑块值（切换色彩空间时调用）
    /// </summary>
    private void UpdateColorSliders()
    {
        if (_settings.ColorSpaceHSLuv)
        {
            HueFromSlider.Value = _settings.HsluvHueFrom;
            HueToSlider.Value = _settings.HsluvHueTo;
            SaturationSlider.Value = _settings.HsluvSaturation;
            LightnessSlider.Value = _settings.HsluvLightness;
        }
        else
        {
            HueFromSlider.Value = _settings.HslHueFrom;
            HueToSlider.Value = _settings.HslHueTo;
            SaturationSlider.Value = _settings.HslSaturation;
            LightnessSlider.Value = _settings.HslLightness;
        }
    }

    private void OnSettingChanged(object sender, RoutedEventArgs e)
    {
        if (_isLoading) return;

        _settings.ReduceBass = ReduceBassToggle.IsOn;
        _settings.Inversion = InversionToggle.IsOn;
        // 防御：SelectedIndex 可能是 -1（ComboBox 未完成初始化或无选中项）
        _settings.Gravity = GravityCombo.SelectedIndex >= 0 ? GravityCombo.SelectedIndex : _settings.Gravity;
        _settings.OverlayMode = OverlayModeCombo.SelectedIndex + 1; // SelectedIndex 0→1(Under), 1→2(Above)
        _settings.VisualEffectName = VisualEffectCombo.SelectedItem is ComboBoxItem item ? (item.Tag as string ?? "bar1ch") : "bar1ch";
        _settings.FillMode = FillModeCombo.SelectedIndex >= 0 ? FillModeCombo.SelectedIndex : _settings.FillMode;

        // 图形效果切换时更新柱宽/间隙可用性
        UpdateBarControlsEnabled();
        _settings.StartWithWindows = StartWithWindowsToggle.IsOn;

        // 检测目标显示器是否变化（变化时需要重建 overlay）
        string oldTarget = _settings.TargetMonitor;
        string newTarget = (TargetMonitorCombo.SelectedItem as ComboBoxItem)?.Tag as string ?? "0";
        _settings.TargetMonitor = newTarget;

        // 开机自启：写/删 HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Panon
        UpdateStartWithWindows(_settings.StartWithWindows);

        SaveSettings();

        if (oldTarget != newTarget)
        {
            // 目标显示器切换：销毁旧 overlay 并创建新的
            App.RecreateOverlays(_settings.TargetMonitor);
        }
        else
        {
            ApplySettingsToEngine();
        }
    }

    private void OnSliderChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_isLoading) return;

        _settings.BassResolutionLevel = (int)BassResolutionSlider.Value;
        _settings.Fps = (int)FpsSlider.Value;

        UpdateValueDisplays();
        SaveSettings();
        ApplySettingsToEngine();
    }

    private void OnBarWidthChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_isLoading) return;

        _settings.BarWidth = (int)BarWidthSlider.Value;
        UpdateValueDisplays();
        SaveSettings();
        ApplySettingsToEngine();
    }

    private void OnGapWidthChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_isLoading) return;

        _settings.GapWidth = (int)GapWidthSlider.Value;
        UpdateValueDisplays();
        SaveSettings();
        ApplySettingsToEngine();
    }

    /// <summary>
    /// 色彩空间切换：更新滑块为对应色彩空间的值
    /// </summary>
    private void OnColorSpaceChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_isLoading) return;

        var selectedRadio = ColorSpaceRadio.SelectedItem as RadioButton;
        _settings.ColorSpaceHSLuv = selectedRadio?.Tag as string == "HSLuv";

        // 屏蔽 OnColorSliderChanged，避免 UpdateColorSliders 触发滑块事件把预设切回"自定义"
        _isLoading = true;
        UpdateColorSliders();
        UpdateValueDisplays();
        _isLoading = false;

        // 切换色彩空间本质上是改变了配色方案（HSL 和 HSLuv 各有独立存储的值），
        // 直接切到"自定义"，不尝试匹配预设（避免匹配到另一个预设造成混乱）
        ColorPresetCombo.SelectedIndex = ColorPresets.Length;

        SaveSettings();
        ApplySettingsToEngine();
    }

    /// <summary>
    /// 颜色滑块变化：根据当前色彩空间写入对应字段
    /// </summary>
    private void OnColorSliderChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_isLoading) return;

        if (_settings.ColorSpaceHSLuv)
        {
            _settings.HsluvHueFrom = (int)HueFromSlider.Value;
            _settings.HsluvHueTo = (int)HueToSlider.Value;
            _settings.HsluvSaturation = (int)SaturationSlider.Value;
            _settings.HsluvLightness = (int)LightnessSlider.Value;
        }
        else
        {
            _settings.HslHueFrom = (int)HueFromSlider.Value;
            _settings.HslHueTo = (int)HueToSlider.Value;
            _settings.HslSaturation = (int)SaturationSlider.Value;
            _settings.HslLightness = (int)LightnessSlider.Value;
        }

        UpdateValueDisplays();
        // 手动调整滑块后切换到"自定义"
        ColorPresetCombo.SelectedIndex = ColorPresets.Length;
        SaveSettings();
        ApplySettingsToEngine();
    }

    /// <summary>
    /// 随机颜色按钮（对齐 Linux 版本）
    /// </summary>
    private void OnRandomColorClick(object sender, RoutedEventArgs e)
    {
        if (_isLoading) return;

        var random = new Random();
        double seed1 = random.NextDouble();
        double seed2 = random.NextDouble();
        double seed3 = random.NextDouble();
        double seed4 = random.NextDouble();
        double seed5 = random.NextDouble();

        // 切换到 HSLuv 色彩空间
        _settings.ColorSpaceHSLuv = true;
        ColorSpaceRadio.SelectedIndex = 1;

        _settings.HsluvHueFrom = (int)(360 * seed1);
        _settings.HsluvHueTo = (int)(1080 * seed2 - 360);

        if (Math.Abs(_settings.HsluvHueTo - _settings.HsluvHueFrom) > 100)
        {
            _settings.HsluvSaturation = (int)(80 + 20 * seed3);
            _settings.HsluvLightness = (int)(60 + 20 * seed4);
        }
        else
        {
            _settings.HsluvSaturation = (int)(80 + 20 * seed3);
            _settings.HsluvLightness = (int)(100 * seed5);
        }

        UpdateColorSliders();
        UpdateValueDisplays();
        // 随机颜色后切换到"自定义"
        ColorPresetCombo.SelectedIndex = ColorPresets.Length;

        SaveSettings();
        ApplySettingsToEngine();
    }

    private void SaveSettings()
    {
        App.Settings.Save();
    }

    /// <summary>
    /// 将设置应用到运行时引擎（即时生效）
    /// </summary>
    private void ApplySettingsToEngine()
    {
        // 音频处理
        App.Fft.BassResolutionLevel = _settings.BassResolutionLevel;
        App.Fft.ReduceBass = _settings.ReduceBass;

        // 渲染器 + 帧率（即时生效，应用到所有 overlay）
        App.ApplySettingsToAllOverlays(_settings);
    }

    // === 预设配色方案 ===
    // (HSLuv, HueFrom, HueTo, Saturation, Lightness)
    private static readonly (bool HSLuv, int HueFrom, int HueTo, int Saturation, int Lightness)[] ColorPresets =
    {
        (false, 180, 720, 60, 50),    // 0: 彩虹（默认）
        (true,  270, -270, 100, 50),   // 1: 霓虹
        (true,  120, 300, 80, 65),     // 2: 极光
        (false, 0,   60,  90, 55),     // 3: 日落
        (false, 180, 240, 80, 50),     // 4: 海洋
        (false, 0,   40,  100, 50),    // 5: 火焰
        (false, 80,  160, 70, 45),     // 6: 森林
        (true,  270, 330, 90, 55),     // 7: 紫罗兰
    };

    /// <summary>
    /// 预设配色选择
    /// </summary>
    private void OnColorPresetChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_isLoading) return;

        int index = ColorPresetCombo.SelectedIndex;
        if (index < 0 || index >= ColorPresets.Length) return; // 自定义或无效

        var preset = ColorPresets[index];
        _settings.ColorSpaceHSLuv = preset.HSLuv;

        if (preset.HSLuv)
        {
            _settings.HsluvHueFrom = preset.HueFrom;
            _settings.HsluvHueTo = preset.HueTo;
            _settings.HsluvSaturation = preset.Saturation;
            _settings.HsluvLightness = preset.Lightness;
        }
        else
        {
            _settings.HslHueFrom = preset.HueFrom;
            _settings.HslHueTo = preset.HueTo;
            _settings.HslSaturation = preset.Saturation;
            _settings.HslLightness = preset.Lightness;
        }

        // 屏蔽 OnColorSpaceChanged 和 OnColorSliderChanged 事件，避免重复触发导致预设被切回"自定义"
        _isLoading = true;
        ColorSpaceRadio.SelectedIndex = preset.HSLuv ? 1 : 0;
        UpdateColorSliders();
        UpdateValueDisplays();
        _isLoading = false;

        SaveSettings();
        ApplySettingsToEngine();
    }

    /// <summary>
    /// 检测当前配置是否匹配某个预设，不匹配则选择"自定义"
    /// </summary>
    private void UpdatePresetSelection()
    {
        bool hsluv = _settings.ColorSpaceHSLuv;
        int hueFrom, hueTo, sat, light;
        if (hsluv)
        {
            hueFrom = _settings.HsluvHueFrom;
            hueTo = _settings.HsluvHueTo;
            sat = _settings.HsluvSaturation;
            light = _settings.HsluvLightness;
        }
        else
        {
            hueFrom = _settings.HslHueFrom;
            hueTo = _settings.HslHueTo;
            sat = _settings.HslSaturation;
            light = _settings.HslLightness;
        }

        for (int i = 0; i < ColorPresets.Length; i++)
        {
            var p = ColorPresets[i];
            if (p.HSLuv == hsluv && p.HueFrom == hueFrom && p.HueTo == hueTo
                && p.Saturation == sat && p.Lightness == light)
            {
                ColorPresetCombo.SelectedIndex = i;
                return;
            }
        }

        // 不匹配任何预设，选择"自定义"（最后一项）
        ColorPresetCombo.SelectedIndex = ColorPresets.Length;
    }

    /// <summary>
    /// 运行时枚举真实显示器，动态填充下拉列表（主显示器 / 各显示器 / 所有显示器）
    /// </summary>
    private void PopulateMonitorCombo()
    {
        TargetMonitorCombo.Items.Clear();

        var monitors = new List<(int Index, int Width, int Height, bool IsPrimary)>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero,
            (IntPtr hMonitor, IntPtr hdc, ref RECT rc, IntPtr lp) =>
            {
                var mi = new MONITORINFOEX();
                mi.cbSize = Marshal.SizeOf<MONITORINFOEX>();
                GetMonitorInfo(hMonitor, ref mi);
                bool isPrimary = (mi.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY
                monitors.Add((monitors.Count, rc.Right - rc.Left, rc.Bottom - rc.Top, isPrimary));
                return true;
            }, IntPtr.Zero);

        // 主显示器排第一
        var primary = monitors.FirstOrDefault(m => m.IsPrimary);
        if (primary != default)
        {
            TargetMonitorCombo.Items.Add(new ComboBoxItem
            {
                Content = $"主显示器 - {primary.Width}×{primary.Height} (默认)",
                Tag = primary.Index.ToString()
            });
        }
        else
        {
            TargetMonitorCombo.Items.Add(new ComboBoxItem { Content = "主显示器 (默认)", Tag = "0" });
        }

        // 其余显示器
        int displayNum = 2;
        foreach (var m in monitors.Where(m => !m.IsPrimary))
        {
            TargetMonitorCombo.Items.Add(new ComboBoxItem
            {
                Content = $"显示器 {displayNum} - {m.Width}×{m.Height}",
                Tag = m.Index.ToString()
            });
            displayNum++;
        }

        // 只有多显示器时才显示"所有显示器"选项
        if (monitors.Count > 1)
        {
            TargetMonitorCombo.Items.Add(new ComboBoxItem { Content = "所有显示器", Tag = "-1" });
        }

        // 恢复选中项
        string saved = _settings.TargetMonitor;
        for (int i = 0; i < TargetMonitorCombo.Items.Count; i++)
        {
            var item = (ComboBoxItem)TargetMonitorCombo.Items[i];
            if ((item.Tag as string) == saved)
            {
                TargetMonitorCombo.SelectedIndex = i;
                return;
            }
        }
        TargetMonitorCombo.SelectedIndex = 0; // 默认主显示器
    }

    #region 显示器枚举 P/Invoke

    private delegate bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdc, ref RECT lprcMonitor, IntPtr dwData);

    [DllImport("user32.dll")]
    private static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumProc lpfnEnum, IntPtr dwData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFOEX lpmi);

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct MONITORINFOEX
    {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string szDevice;
    }

    #endregion

    /// <summary>
    /// 刷新透明度开关状态显示
    /// </summary>
    private void RefreshTransparencyStatus()
    {
        var tc = App.Transparency;
        bool et = tc.IsTransparencyEnabled;
        bool oled = tc.IsOLEDTaskbarTransparencyEnabled;

        // ToggleSwitch 反映运行态（用户上次操作的结果）
        _isLoading = true;
        TransparencyToggle.IsOn = tc.IsEnabled;
        _isLoading = false;

        TransparencyHintText.Text =
            $"当前状态：系统透明效果 {(et ? "已开启" : "未开启")}  ·  任务栏增强透明 {(oled ? "已开启" : "未开启")}\n" +
            (tc.IsEnabled
                ? "两项透明效果均已开启，频谱可正常显示。"
                : "透明效果未完全开启，频谱可能无法正常显示。开启此开关将同时启用两项透明效果。");
    }

    /// <summary>
    /// ToggleSwitch 切换：开启/关闭系统透明效果（直接写注册表 1/0，自由切换）
    /// </summary>
    private void OnTransparencyToggled(object sender, RoutedEventArgs e)
    {
        if (_isLoading) return;

        if (TransparencyToggle.IsOn)
            App.Transparency.Enable();
        else
            App.Transparency.Disable();

        RefreshTransparencyStatus();
    }

    /// <summary>
    /// 仅在柱状图时启用柱宽/间隙滑块，其他图形禁用
    /// </summary>
    private void UpdateBarControlsEnabled()
    {
        bool isBar = _settings.VisualEffectName == "bar1ch";
        BarWidthSlider.IsEnabled = isBar;
        GapWidthSlider.IsEnabled = isBar;
    }

    /// <summary>
    /// 根据开关状态写入/删除 HKCU Run 注册表启动项
    /// </summary>
    private void UpdateStartWithWindows(bool enable)
    {
        const string runKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(runKey, writable: true);
            if (enable)
            {
                var exePath = Environment.ProcessPath ?? System.Reflection.Assembly.GetExecutingAssembly().Location;
                key?.SetValue("Panon", $"\"{exePath}\"");
            }
            else
            {
                key?.DeleteValue("Panon", throwOnMissingValue: false);
            }
        }
        catch { /* 无管理员权限时静默失败 */ }
    }

    /// <summary>
    /// 启动时从注册表同步开机自启状态
    /// </summary>
    private void SyncStartWithWindowsFromRegistry()
    {
        const string runKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(runKey);
            _settings.StartWithWindows = key?.GetValue("Panon") != null;
        }
        catch
        {
            _settings.StartWithWindows = false;
        }
    }
}
