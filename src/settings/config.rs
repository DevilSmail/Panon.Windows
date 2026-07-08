// config.rs — AppSettings 模型 + JSON 读写（← AppSettings.cs + SettingsManager.cs）
// 阶段 8：serde 持久化 + 字段验证

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 应用设置（主循环与设置窗口共享）
///
/// 序列化为 `%APPDATA%/Panon/settings.json`，camelCase 对齐 C#。
/// 加载时缺失字段用 Default 填充，加载后自动 validate() 修正非法值。
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

    // === Windows 设置 ===
    /// 覆盖模式: 1=任务栏在上(默认), 2=频谱在上
    pub overlay_mode: u8,
    /// 频谱窗口最大高度（像素），0=自动跟随任务栏高度
    pub max_height: u8,
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
            overlay_mode: 1,
            max_height: 0,
            startup: false,
            enable_transparency: false,
            use_oled_taskbar_transparency: false,
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
        self.bar_width = self.bar_width.clamp(1, 20);
        self.gap_width = self.gap_width.clamp(0, 10);
        self.fill_mode = if self.fill_mode > 1 { 0 } else { self.fill_mode };
        if self.target_monitor < -1 {
            self.target_monitor = -1;
        }
        self.hsl_hue_from = self.hsl_hue_from.clamp(-360, 720);
        self.hsl_hue_to = self.hsl_hue_to.clamp(-360, 720);
        self.hsl_saturation = self.hsl_saturation.clamp(0, 100);
        self.hsl_lightness = self.hsl_lightness.clamp(0, 100);
        self.hsluv_hue_from = self.hsluv_hue_from.clamp(-360, 720);
        self.hsluv_hue_to = self.hsluv_hue_to.clamp(-360, 720);
        self.hsluv_saturation = self.hsluv_saturation.clamp(0, 100);
        self.hsluv_lightness = self.hsluv_lightness.clamp(0, 100);
        if !VALID_EFFECTS.contains(&self.visual_effect.as_str()) {
            self.visual_effect = "bar1ch".to_string();
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
