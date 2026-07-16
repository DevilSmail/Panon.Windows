// config.rs — AppSettings 模型 + JSON 读写（← AppSettings.cs + SettingsManager.cs）
// 阶段 8：serde 持久化 + 字段验证
//
// P0 修复：JSON 字段名对齐 C#
// - visualEffectName (原 visualEffect，保留 alias 兼容旧配置)
// - startWithWindows (原 startup，保留 alias 兼容旧配置)
// - targetMonitor: String 类型 (对齐 C#)，支持整数和字符串双向兼容

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// targetMonitor 反序列化：兼容整数（旧配置）和字符串（C# 格式）
fn deserialize_target_monitor<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TargetMonitorVisitor;
    impl<'de> serde::de::Visitor<'de> for TargetMonitorVisitor {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string like \"0\" or integer 0 for target monitor")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<String, E> {
            Ok((v as i64).to_string())
        }
    }
    deserializer.deserialize_any(TargetMonitorVisitor)
}

/// 应用设置（主循环与设置窗口共享）
///
/// 序列化为 `%APPDATA%/Panon/settings.json`，camelCase 对齐 C#。
/// 加载时缺失字段用 Default 填充，加载后自动 validate() 修正非法值。
/// 旧字段名通过 serde alias 兼容。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    // === 音频 ===
    /// 低音分辨率级别 (0-6)
    pub bass_resolution_level: u8,
    /// 降低低音
    pub reduce_bass: bool,

    // === 显示 ===
    /// 视觉效果名称: bar1ch / wave / solid1ch / solid / beam / spectrogram / oie1ch
    #[serde(alias = "visualEffect")]
    pub visual_effect_name: String,
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
    /// 目标显示器: "-1"=所有, "0"=主显示器, "1"+=副显示器索引（对齐 C# string 类型）
    #[serde(deserialize_with = "deserialize_target_monitor")]
    pub target_monitor: String,

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

    // === Windows 设置 ===
    /// 覆盖模式: 1=任务栏在上(默认), 2=频谱在上
    pub overlay_mode: u8,
    /// 频谱窗口最大高度（像素），0=自动跟随任务栏高度（对齐 C# int MaxHeight）
    pub max_height: i32,
    /// 开机自启（对齐 C# StartWithWindows）
    #[serde(alias = "startup")]
    pub start_with_windows: bool,
    /// 系统透明效果（注册表 EnableTransparency + UseOLEDTaskbarTransparency，对齐 C# 单开关）
    pub enable_transparency: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        // 默认值对齐 C# AppSettings.Default
        Self {
            bass_resolution_level: 4,
            reduce_bass: true,
            visual_effect_name: "bar1ch".to_string(),
            gravity: 2,
            inversion: false,
            fps: 30,
            bar_width: 6,
            gap_width: 3,
            fill_mode: 1,
            target_monitor: "0".to_string(),
            color_space_hsluv: false,
            hsl_hue_from: 180,
            hsl_hue_to: 720,
            hsl_saturation: 60,
            hsl_lightness: 50,
            hsluv_hue_from: 270,
            hsluv_hue_to: -270,
            hsluv_saturation: 100,
            hsluv_lightness: 50,
            overlay_mode: 1,
            max_height: 0,
            start_with_windows: false,
            enable_transparency: false,
        }
    }
}

/// 合法的视觉效果名称
const VALID_EFFECTS: &[&str] = &[
    "bar1ch",
    "wave",
    "solid1ch",
    "solid",
    "beam",
    "spectrogram",
    "oie1ch",
];

impl AppSettings {
    /// 修正非法字段值（加载后或用户误编辑后调用）
    pub fn validate(&mut self) {
        self.bass_resolution_level = self.bass_resolution_level.min(6);
        self.gravity = self.gravity.min(4);
        self.fps = self.fps.clamp(10, 60);
        self.bar_width = self.bar_width.clamp(1, 30);
        self.gap_width = self.gap_width.clamp(0, 20);
        self.fill_mode = if self.fill_mode > 1 { 1 } else { self.fill_mode };
        // 验证 target_monitor：必须是有效的数字字符串或 "-1"
        if self.target_monitor != "-1" {
            if self.target_monitor.parse::<i32>().is_err() {
                self.target_monitor = "0".to_string();
            }
        }
        // UI sliders allow -4000..4000 for hue ranges; keep validate in sync
        self.hsl_hue_from = self.hsl_hue_from.clamp(-4000, 4000);
        self.hsl_hue_to = self.hsl_hue_to.clamp(-4000, 4000);
        self.hsl_saturation = self.hsl_saturation.clamp(0, 100);
        self.hsl_lightness = self.hsl_lightness.clamp(0, 100);
        self.hsluv_hue_from = self.hsluv_hue_from.clamp(-4000, 4000);
        self.hsluv_hue_to = self.hsluv_hue_to.clamp(-4000, 4000);
        self.hsluv_saturation = self.hsluv_saturation.clamp(0, 100);
        self.hsluv_lightness = self.hsluv_lightness.clamp(0, 100);
        if !VALID_EFFECTS.contains(&self.visual_effect_name.as_str()) {
            self.visual_effect_name = "bar1ch".to_string();
        }
    }

    /// 设置文件路径: %APPDATA%/Panon/settings.json
    fn settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Panon").join("settings.json"))
    }

    /// 从 JSON 加载设置；文件不存在或解析失败时返回 Default
    pub fn load() -> Self {
        let path = match Self::settings_path() {
            Some(p) => p,
            None => {
                eprintln!("[settings] cannot resolve config dir, using defaults");
                return Self::default();
            }
        };

        match fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<AppSettings>(&json) {
                Ok(mut s) => {
                    s.validate();
                    println!("[settings] loaded from {:?}", path);
                    s
                }
                Err(e) => {
                    eprintln!("[settings] parse error: {}, using defaults", e);
                    Self::default()
                }
            },
            Err(_) => {
                // 文件不存在是正常情况（首次运行）
                Self::default()
            }
        }
    }

    /// 保存设置到 JSON；失败时打印错误但不中断
    pub fn save(&self) {
        let path = match Self::settings_path() {
            Some(p) => p,
            None => {
                eprintln!("[settings] cannot resolve config dir, skip save");
                return;
            }
        };

        // 确保父目录存在
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("[settings] create_dir_all failed: {}", e);
                return;
            }
        }

        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = fs::write(&path, json) {
                    eprintln!("[settings] write failed: {}", e);
                }
            }
            Err(e) => eprintln!("[settings] serialize failed: {}", e),
        }
    }
}
