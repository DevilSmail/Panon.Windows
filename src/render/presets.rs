// presets.rs — 8 套预设配色方案（← C# ColorPresets）
// P1 修复：数值完全对齐 C# SettingsPage.xaml.cs ColorPresets
//
// C# 预设 (HSLuv, HueFrom, HueTo, Saturation, Lightness):
//   0: (false, 180,  720,  60, 50)  彩虹（默认）
//   1: (true,  270, -270, 100, 50)  霓虹
//   2: (true,  120,  300,  80, 65)  极光
//   3: (false,  0,    60,  90, 55)  日落
//   4: (false, 180,  240,  80, 50)  海洋
//   5: (false,  0,    40, 100, 50)  火焰
//   6: (false,  80,  160,  70, 45)  森林
//   7: (true,  270,  330,  90, 55)  紫罗兰
//
// Rust 同时设置 HSL 和 HSLuv 两套值（与 C# 单套滑块行为对齐：切换色彩空间时值保持一致）

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

/// 8 套内置预设配色（数值严格对齐 C#）
pub static PRESETS: &[ColorPreset] = &[
    // 0: 彩虹 — HSL, 180→720, sat=60, light=50
    ColorPreset {
        use_hsluv: false,
        name: "彩虹",
        hsl_hue_from: 180,
        hsl_hue_to: 720,
        hsl_saturation: 60,
        hsl_lightness: 50,
        hsluv_hue_from: 180,
        hsluv_hue_to: 720,
        hsluv_saturation: 60,
        hsluv_lightness: 50,
    },
    // 1: 霓虹 — HSLuv, 270→-270, sat=100, light=50
    ColorPreset {
        use_hsluv: true,
        name: "霓虹",
        hsl_hue_from: 270,
        hsl_hue_to: -270,
        hsl_saturation: 100,
        hsl_lightness: 50,
        hsluv_hue_from: 270,
        hsluv_hue_to: -270,
        hsluv_saturation: 100,
        hsluv_lightness: 50,
    },
    // 2: 极光 — HSLuv, 120→300, sat=80, light=65
    ColorPreset {
        use_hsluv: true,
        name: "极光",
        hsl_hue_from: 120,
        hsl_hue_to: 300,
        hsl_saturation: 80,
        hsl_lightness: 65,
        hsluv_hue_from: 120,
        hsluv_hue_to: 300,
        hsluv_saturation: 80,
        hsluv_lightness: 65,
    },
    // 3: 日落 — HSL, 0→60, sat=90, light=55
    ColorPreset {
        use_hsluv: false,
        name: "日落",
        hsl_hue_from: 0,
        hsl_hue_to: 60,
        hsl_saturation: 90,
        hsl_lightness: 55,
        hsluv_hue_from: 0,
        hsluv_hue_to: 60,
        hsluv_saturation: 90,
        hsluv_lightness: 55,
    },
    // 4: 海洋 — HSL, 180→240, sat=80, light=50
    ColorPreset {
        use_hsluv: false,
        name: "海洋",
        hsl_hue_from: 180,
        hsl_hue_to: 240,
        hsl_saturation: 80,
        hsl_lightness: 50,
        hsluv_hue_from: 180,
        hsluv_hue_to: 240,
        hsluv_saturation: 80,
        hsluv_lightness: 50,
    },
    // 5: 火焰 — HSL, 0→40, sat=100, light=50
    ColorPreset {
        use_hsluv: false,
        name: "火焰",
        hsl_hue_from: 0,
        hsl_hue_to: 40,
        hsl_saturation: 100,
        hsl_lightness: 50,
        hsluv_hue_from: 0,
        hsluv_hue_to: 40,
        hsluv_saturation: 100,
        hsluv_lightness: 50,
    },
    // 6: 森林 — HSL, 80→160, sat=70, light=45
    ColorPreset {
        use_hsluv: false,
        name: "森林",
        hsl_hue_from: 80,
        hsl_hue_to: 160,
        hsl_saturation: 70,
        hsl_lightness: 45,
        hsluv_hue_from: 80,
        hsluv_hue_to: 160,
        hsluv_saturation: 70,
        hsluv_lightness: 45,
    },
    // 7: 紫罗兰 — HSLuv, 270→330, sat=90, light=55
    ColorPreset {
        use_hsluv: true,
        name: "紫罗兰",
        hsl_hue_from: 270,
        hsl_hue_to: 330,
        hsl_saturation: 90,
        hsl_lightness: 55,
        hsluv_hue_from: 270,
        hsluv_hue_to: 330,
        hsluv_saturation: 90,
        hsluv_lightness: 55,
    },
];

/// 预设数量
#[allow(dead_code)]
pub const PRESET_COUNT: usize = 8;

/// 获取预设名称列表
#[allow(dead_code)]
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
