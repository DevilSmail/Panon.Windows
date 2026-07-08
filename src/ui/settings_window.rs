// settings_window.rs — egui 设置窗口（← SettingsPage.xaml + .xaml.cs）
// 阶段 7：eframe 窗口 + 四卡片分组 + 全控件
//
// 线程模型：在独立线程运行 eframe 事件循环，通过 Arc<Mutex<AppSettings>>
// 与主循环共享状态。设置窗口是唯一写入者，主循环只读取并应用。

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::render::presets::{match_preset, PRESETS};
use crate::settings::config::AppSettings;

/// 视觉效果选项（显示名, 内部名）
const VISUAL_EFFECTS: &[(&str, &str)] = &[
    ("柱状图", "bar1ch"),
    ("波形", "wave"),
    ("单声道实心", "solid1ch"),
    ("实心", "solid"),
    ("光束", "beam"),
    ("频谱图", "spectrogram"),
    ("OIE", "oie1ch"),
];

/// 填充模式选项
const FILL_MODES: &[(&str, u8)] = &[("铺满整个任务栏", 0), ("仅空白区域", 1)];

pub struct SettingsApp {
    settings: Arc<Mutex<AppSettings>>,
    monitor_count: usize,
}

impl SettingsApp {
    pub fn new(settings: Arc<Mutex<AppSettings>>, monitor_count: usize) -> Self {
        Self {
            settings,
            monitor_count,
        }
    }

    fn render_audio_card(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("🎵 音频");
            ui.separator();

            let mut s = self.settings.lock().unwrap();
            let level = s.bass_resolution_level;

            ui.horizontal(|ui| {
                ui.label("频率分辨率");
                ui.add(egui::Slider::new(&mut s.bass_resolution_level, 0..=6));
                ui.label(format!("当前: {}", level));
            });

            ui.horizontal(|ui| {
                ui.label("降低低音");
                ui.checkbox(&mut s.reduce_bass, "");
            });
        });
    }

    fn render_display_card(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("🖥 显示");
            ui.separator();

            let mut s = self.settings.lock().unwrap();

            // 视觉效果
            let current_effect_name = VISUAL_EFFECTS
                .iter()
                .find(|(_, name)| *name == s.visual_effect)
                .map(|(display, _)| *display)
                .unwrap_or("柱状图");
            ui.horizontal(|ui| {
                ui.label("视觉效果");
                egui::ComboBox::from_id_source("visual_effect")
                    .selected_text(current_effect_name)
                    .show_ui(ui, |ui| {
                        for (display, name) in VISUAL_EFFECTS {
                            ui.selectable_value(&mut s.visual_effect, name.to_string(), *display);
                        }
                    });
            });

            // 重力
            let gravity = s.gravity;
            ui.horizontal(|ui| {
                ui.label("重力");
                ui.add(egui::Slider::new(&mut s.gravity, 0u8..=4));
                ui.label(format!("当前: {}", gravity));
            });

            // 反转频谱
            ui.horizontal(|ui| {
                ui.label("反转频谱");
                ui.checkbox(&mut s.inversion, "");
            });

            // 帧率
            let fps = s.fps;
            ui.horizontal(|ui| {
                ui.label("帧率");
                ui.add(egui::Slider::new(&mut s.fps, 10u8..=60));
                ui.label(format!("当前: {} FPS", fps));
            });

            // 柱宽 / 间隙（仅在 bar1ch 时启用）
            let bars_enabled = s.visual_effect == "bar1ch";
            let bar_width = s.bar_width;
            ui.horizontal(|ui| {
                ui.label("柱宽");
                ui.add_enabled(bars_enabled, egui::Slider::new(&mut s.bar_width, 1..=20));
                ui.label(format!("当前: {}", bar_width));
            });

            let gap_width = s.gap_width;
            ui.horizontal(|ui| {
                ui.label("间隙宽");
                ui.add_enabled(bars_enabled, egui::Slider::new(&mut s.gap_width, 0..=10));
                ui.label(format!("当前: {}", gap_width));
            });

            // 填充模式
            let current_fill = FILL_MODES
                .iter()
                .find(|(_, v)| *v == s.fill_mode)
                .map(|(name, _)| *name)
                .unwrap_or("铺满整个任务栏");
            ui.horizontal(|ui| {
                ui.label("填充模式");
                egui::ComboBox::from_id_source("fill_mode")
                    .selected_text(current_fill)
                    .show_ui(ui, |ui| {
                        for (name, val) in FILL_MODES {
                            ui.selectable_value(&mut s.fill_mode, *val, *name);
                        }
                    });
            });

            // 目标显示器
            let monitor_label = monitor_display_name(s.target_monitor);
            ui.horizontal(|ui| {
                ui.label("目标显示器");
                egui::ComboBox::from_id_source("target_monitor")
                    .selected_text(monitor_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.target_monitor, -1, "所有显示器");
                        for i in 0..self.monitor_count as i32 {
                            let label = monitor_display_name(i);
                            ui.selectable_value(&mut s.target_monitor, i, label);
                        }
                    });
            });
        });
    }

    fn render_color_card(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("🎨 颜色");
            ui.separator();

            let mut s = self.settings.lock().unwrap();

            // 预设配色下拉
            let current_preset = match_preset(
                s.hsl_hue_from,
                s.hsl_hue_to,
                s.hsl_saturation,
                s.hsl_lightness,
                s.hsluv_hue_from,
                s.hsluv_hue_to,
                s.hsluv_saturation,
                s.hsluv_lightness,
            );
            let preset_label = match current_preset {
                Some(i) => PRESETS[i].name,
                None => "自定义",
            };

            let mut apply_preset: Option<usize> = None;
            ui.horizontal(|ui| {
                ui.label("预设配色");
                egui::ComboBox::from_id_source("color_preset")
                    .selected_text(preset_label)
                    .show_ui(ui, |ui| {
                        for (i, preset) in PRESETS.iter().enumerate() {
                            if ui
                                .selectable_label(current_preset == Some(i), preset.name)
                                .clicked()
                            {
                                apply_preset = Some(i);
                            }
                        }
                        let _ = ui.selectable_label(current_preset.is_none(), "自定义");
                    });
            });

            // 应用预设
            if let Some(i) = apply_preset {
                let preset = &PRESETS[i];
                s.hsl_hue_from = preset.hsl_hue_from;
                s.hsl_hue_to = preset.hsl_hue_to;
                s.hsl_saturation = preset.hsl_saturation;
                s.hsl_lightness = preset.hsl_lightness;
                s.hsluv_hue_from = preset.hsluv_hue_from;
                s.hsluv_hue_to = preset.hsluv_hue_to;
                s.hsluv_saturation = preset.hsluv_saturation;
                s.hsluv_lightness = preset.hsluv_lightness;
            }

            // 色彩空间切换
            ui.horizontal(|ui| {
                ui.label("色彩空间");
                ui.radio_value(&mut s.color_space_hsluv, false, "HSL");
                ui.radio_value(&mut s.color_space_hsluv, true, "HSLuv");
            });

            // 色相 / 饱和度 / 亮度（根据色彩空间显示对应滑块）
            // 复制到局部变量避免同时多重可变借用 s
            if s.color_space_hsluv {
                let (mut hf, mut ht, mut sat, mut light) = (
                    s.hsluv_hue_from,
                    s.hsluv_hue_to,
                    s.hsluv_saturation,
                    s.hsluv_lightness,
                );
                color_sliders(ui, &mut hf, &mut ht, &mut sat, &mut light);
                s.hsluv_hue_from = hf;
                s.hsluv_hue_to = ht;
                s.hsluv_saturation = sat;
                s.hsluv_lightness = light;
            } else {
                let (mut hf, mut ht, mut sat, mut light) = (
                    s.hsl_hue_from,
                    s.hsl_hue_to,
                    s.hsl_saturation,
                    s.hsl_lightness,
                );
                color_sliders(ui, &mut hf, &mut ht, &mut sat, &mut light);
                s.hsl_hue_from = hf;
                s.hsl_hue_to = ht;
                s.hsl_saturation = sat;
                s.hsl_lightness = light;
            }

            // 随机颜色按钮
            if ui.button("🎲 随机颜色").clicked() {
                let (hf, ht, sat, light) = random_color();
                if s.color_space_hsluv {
                    s.hsluv_hue_from = hf;
                    s.hsluv_hue_to = ht;
                    s.hsluv_saturation = sat;
                    s.hsluv_lightness = light;
                } else {
                    s.hsl_hue_from = hf;
                    s.hsl_hue_to = ht;
                    s.hsl_saturation = sat;
                    s.hsl_lightness = light;
                }
            }
        });
    }

    fn render_windows_card(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.heading("⚙ Windows 设置");
            ui.separator();

            let mut s = self.settings.lock().unwrap();

            ui.horizontal(|ui| {
                ui.label("开机自启");
                ui.checkbox(&mut s.startup, "");
            });

            ui.horizontal(|ui| {
                ui.label("系统透明效果");
                ui.checkbox(&mut s.enable_transparency, "");
            });

            ui.horizontal(|ui| {
                ui.label("OLED 任务栏透明");
                ui.checkbox(&mut s.use_oled_taskbar_transparency, "");
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("注：透明效果与开机自启在阶段 8 接线注册表")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(780.0);
                    ui.add_space(6.0);
                    self.render_audio_card(ui);
                    ui.add_space(6.0);
                    self.render_display_card(ui);
                    ui.add_space(6.0);
                    self.render_color_card(ui);
                    ui.add_space(6.0);
                    self.render_windows_card(ui);
                    ui.add_space(6.0);
                });
        });
    }
}

/// 渲染色相/饱和度/亮度四个滑块
fn color_sliders(
    ui: &mut egui::Ui,
    hue_from: &mut i32,
    hue_to: &mut i32,
    saturation: &mut i32,
    lightness: &mut i32,
) {
    let hf = *hue_from;
    ui.horizontal(|ui| {
        ui.label("色相起点");
        ui.add(egui::Slider::new(hue_from, -360..=720));
        ui.label(format!("当前: {}", hf));
    });

    let ht = *hue_to;
    ui.horizontal(|ui| {
        ui.label("色相终点");
        ui.add(egui::Slider::new(hue_to, -360..=720));
        ui.label(format!("当前: {}", ht));
    });

    let sat = *saturation;
    ui.horizontal(|ui| {
        ui.label("饱和度");
        ui.add(egui::Slider::new(saturation, 0..=100));
        ui.label(format!("当前: {}", sat));
    });

    let light = *lightness;
    ui.horizontal(|ui| {
        ui.label("亮度");
        ui.add(egui::Slider::new(lightness, 0..=100));
        ui.label(format!("当前: {}", light));
    });
}

/// 生成目标显示器显示名
fn monitor_display_name(idx: i32) -> String {
    match idx {
        -1 => "所有显示器".to_string(),
        0 => "显示器 1 (主)".to_string(),
        n => format!("显示器 {}", n + 1),
    }
}

/// 生成随机配色（简单 LCG，避免引入 rand 依赖）
fn random_color() -> (i32, i32, i32, i32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xCAFE_BABE);
    let mut next = || {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as i64
    };
    let hue_from = (next() % 360) as i32;
    let hue_to = ((next() % 900) - 180) as i32; // -180 ~ 719
    let sat = (50 + (next() % 51)) as i32; // 50 ~ 100
    let light = (40 + (next() % 31)) as i32; // 40 ~ 70
    (hue_from, hue_to, sat, light)
}

/// 在独立线程启动设置窗口
///
/// - `settings`: 与主循环共享的设置状态
/// - `monitor_count`: 运行时检测到的显示器数量（用于目标显示器下拉枚举）
/// - `open_flag`: 用于标记窗口是否已打开，防止重复创建
///
/// 调用方应在 `open_flag` 为 false 时调用，并在调用后将其设为 true。
/// 窗口关闭时本函数内部会将其重置为 false。
pub fn run_settings_window(
    settings: Arc<Mutex<AppSettings>>,
    monitor_count: usize,
    open_flag: Arc<Mutex<bool>>,
) {
    std::thread::spawn(move || {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([820.0, 720.0])
                .with_min_inner_size([600.0, 500.0])
                .with_title("Panon 设置"),
            ..Default::default()
        };

        let result = eframe::run_native(
            "Panon 设置",
            options,
            Box::new(move |_cc| Ok(Box::new(SettingsApp::new(settings, monitor_count)))),
        );

        if let Err(e) = result {
            eprintln!("[settings] eframe error: {}", e);
        }

        // 窗口关闭，释放打开标志
        *open_flag.lock().unwrap() = false;
    });
}
