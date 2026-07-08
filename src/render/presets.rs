// presets.rs — 8 套预设配色方案（← C# ColorPresets）
// 与 C# 版本数值对齐

/// 预设配色方案
#[derive(Clone, Debug)]
pub struct ColorPreset {
    pub name: &'static str,
    /// 色彩空间: false=HSL, true=HSLuv
    pub use_hsluv: bool,
    pub hsl_hue_from: i32,
    pub hsl_hue_to: i32,
    pub hsl_saturation: i32,
    pub hsl_lightness: i32,
    pub hsluv_hue_from: i32,
    pub hsluv_hue_to: i32,
    pub hsluv_saturation: i32,
    pub hsluv_lightness: i32,
}

/// 8 套内置预设配色
pub static PRESETS: &[ColorPreset] = &[
    // 0: 彩虹 — 完整色环循环
    ColorPreset {
        use_hsluv: false,
        name: "彩虹",
        hsl_hue_from: 0,
        hsl_hue_to: 360,
        hsl_saturation: 80,
        hsl_lightness: 50,
        hsluv_hue_from: 0,
        hsluv_hue_to: 360,
        hsluv_saturation: 100,
        hsluv_lightness: 50,
    },
    // 1: 霓虹 — 高饱和度蓝→红循环
    ColorPreset {
        use_hsluv: true,
        name: "霓虹",
        hsl_hue_from: 180,
        hsl_hue_to: 720,
        hsl_saturation: 100,
        hsl_lightness: 60,
        hsluv_hue_from: 270,
        hsluv_hue_to: -270,
        hsluv_saturation: 100,
        hsluv_lightness: 60,
    },
    // 2: 极光 — 绿→蓝→紫
    ColorPreset {
        use_hsluv: true,
        name: "极光",
        hsl_hue_from: 120,
        hsl_hue_to: 300,
        hsl_saturation: 70,
        hsl_lightness: 55,
        hsluv_hue_from: 120,
        hsluv_hue_to: 300,
        hsluv_saturation: 80,
        hsluv_lightness: 55,
    },
    // 3: 日落 — 红→橙→黄
    ColorPreset {
        use_hsluv: false,
        name: "日落",
        hsl_hue_from: -20,
        hsl_hue_to: 60,
        hsl_saturation: 85,
        hsl_lightness: 50,
        hsluv_hue_from: -20,
        hsluv_hue_to: 60,
        hsluv_saturation: 90,
        hsluv_lightness: 50,
    },
    // 4: 海洋 — 蓝→青→绿
    ColorPreset {
        use_hsluv: false,
        name: "海洋",
        hsl_hue_from: 180,
        hsl_hue_to: 240,
        hsl_saturation: 75,
        hsl_lightness: 45,
        hsluv_hue_from: 180,
        hsluv_hue_to: 240,
        hsluv_saturation: 85,
        hsluv_lightness: 45,
    },
    // 5: 火焰 — 红→橙→黄
    ColorPreset {
        use_hsluv: false,
        name: "火焰",
        hsl_hue_from: 0,
        hsl_hue_to: 50,
        hsl_saturation: 100,
        hsl_lightness: 50,
        hsluv_hue_from: 0,
        hsluv_hue_to: 50,
        hsluv_saturation: 100,
        hsluv_lightness: 50,
    },
    // 6: 森林 — 绿→黄绿
    ColorPreset {
        use_hsluv: false,
        name: "森林",
        hsl_hue_from: 60,
        hsl_hue_to: 140,
        hsl_saturation: 70,
        hsl_lightness: 40,
        hsluv_hue_from: 60,
        hsluv_hue_to: 140,
        hsluv_saturation: 80,
        hsluv_lightness: 40,
    },
    // 7: 紫罗兰 — 紫→粉
    ColorPreset {
        use_hsluv: true,
        name: "紫罗兰",
        hsl_hue_from: 240,
        hsl_hue_to: 320,
        hsl_saturation: 80,
        hsl_lightness: 50,
        hsluv_hue_from: 240,
        hsluv_hue_to: 320,
        hsluv_saturation: 90,
        hsluv_lightness: 50,
    },
];

/// 预设数量
pub const PRESET_COUNT: usize = 8;

/// 获取预设名称列表
pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.name).collect()
}

/// 根据当前颜色参数匹配预设索引（智能匹配）
/// 返回 None 表示不匹配任何预设（自定义配色）
pub fn match_preset(
    hsl_hue_from: i32, hsl_hue_to: i32, hsl_saturation: i32, hsl_lightness: i32,
    hsluv_hue_from: i32, hsluv_hue_to: i32, hsluv_saturation: i32, hsluv_lightness: i32,
    color_space_hsluv: bool,
) -> Option<usize> {
    for (i, preset) in PRESETS.iter().enumerate() {
        if preset.use_hsluv == color_space_hsluv
            && preset.hsl_hue_from == hsl_hue_from
            && preset.hsl_hue_to == hsl_hue_to
            && preset.hsl_saturation == hsl_saturation
            && preset.hsl_lightness == hsl_lightness
            && preset.hsluv_hue_from == hsluv_hue_from
            && preset.hsluv_hue_to == hsluv_hue_to
            && preset.hsluv_saturation == hsluv_saturation
            && preset.hsluv_lightness == hsluv_lightness
        {
            return Some(i);
        }
    }
    None
}
