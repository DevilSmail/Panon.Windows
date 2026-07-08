// config.rs — AppSettings 模型 + JSON 读写（← AppSettings.cs + SettingsManager.cs）
// 阶段 7：基础结构（serde 持久化 + 字段验证在阶段 8 实现）

/// 应用设置（主循环与设置窗口共享）
///
/// 字段分组（对齐 C# SettingsPage.xaml 的四个卡片）：
/// - 音频：bass_resolution_level / reduce_bass
/// - 显示：visual_effect / gravity / inversion / fps / bar_width / gap_width / fill_mode / target_monitor
/// - 颜色：color_space_hsluv + hsl_* + hsluv_*
/// - Windows：startup / enable_transparency / use_oled_taskbar_transparency（阶段 8 完整接线）
#[derive(Clone, Debug)]
pub struct AppSettings {
    // === 音频 ===
    /// 低音分辨率级别 (0-6)
    pub bass_resolution_level: u8,
    /// 降低低音
    pub reduce_bass: bool,

    // === 显示 ===
    /// 视觉效果名称: bar1ch / wave / solid1ch / solid / beam / spectrogram / oie1ch
    pub visual_effect: String,
    /// 重力级别 (0-4)
    pub gravity: u8,
    /// 反转频谱
    pub inversion: bool,
    /// 帧率 (10-60)
    pub fps: u8,
    /// 柱宽 (1-20)
    pub bar_width: i32,
    /// 间隙宽 (0-10)
    pub gap_width: i32,
    /// 填充模式: 0=铺满, 1=仅空白区域
    pub fill_mode: u8,
    /// 目标显示器: -1=所有, 0=主显示器, 1+=副显示器索引
    pub target_monitor: i32,

    // === 颜色 ===
    /// 色彩空间: false=HSL, true=HSLuv
    pub color_space_hsluv: bool,
    pub hsl_hue_from: i32,
    pub hsl_hue_to: i32,
    pub hsl_saturation: i32,
    pub hsl_lightness: i32,
    pub hsluv_hue_from: i32,
    pub hsluv_hue_to: i32,
    pub hsluv_saturation: i32,
    pub hsluv_lightness: i32,

    // === Windows 设置（阶段 8 接线注册表/启动项）===
    /// 开机自启
    pub startup: bool,
    /// 系统透明效果（注册表 EnableTransparency）
    pub enable_transparency: bool,
    /// OLED 任务栏透明（注册表 UseOLEDTaskbarTransparency）
    pub use_oled_taskbar_transparency: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        // 默认值对齐 C# AppSettings.Default + SpectrumRenderer::new()
        Self {
            bass_resolution_level: 4,
            reduce_bass: true,
            visual_effect: "bar1ch".to_string(),
            gravity: 2,
            inversion: false,
            fps: 30,
            bar_width: 6,
            gap_width: 3,
            fill_mode: 0,
            target_monitor: -1,
            color_space_hsluv: false,
            hsl_hue_from: 180,
            hsl_hue_to: 720,
            hsl_saturation: 80,
            hsl_lightness: 50,
            hsluv_hue_from: 270,
            hsluv_hue_to: -270,
            hsluv_saturation: 100,
            hsluv_lightness: 50,
            startup: false,
            enable_transparency: false,
            use_oled_taskbar_transparency: false,
        }
    }
}
