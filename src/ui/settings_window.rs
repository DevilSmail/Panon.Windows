// settings_window.rs — egui 设置窗口 (SettingsPage.xaml + .xaml.cs)
// 即时生效：修改直接写入 Arc<Mutex<AppSettings>>，渲染线程每帧读取

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::render::presets::{match_preset, PRESETS};
use crate::settings::config::AppSettings;

const VISUAL_EFFECTS: &[(&str, &str)] = &[
    ("柱状图 (bar1ch)", "bar1ch"),
    ("波浪 (wave)", "wave"),
    ("实心单声道 (solid1ch)", "solid1ch"),
    ("实心立体声 (solid)", "solid"),
    ("光束 (beam)", "beam"),
    ("频谱瀑布 (spectrogram)", "spectrogram"),
    ("连线 (oie1ch)", "oie1ch"),
];

const GRAVITY_NAMES: &[&str] = &[
    "居中 (从任务栏中线向上下扩展)",
    "靠任务栏上边缘向下",
    "靠任务栏下边缘向上 (默认)",
    "从右到左 (横向)",
    "从左到右 (横向)",
];

const FILL_NAMES: &[(&str, u8)] = &[
    ("铺满任务栏", 0),
    ("仅空白区域 (默认)", 1),
];

const OVERLAY_NAMES: &[(&str, u8)] = &[
    ("任务栏覆盖在频谱上面 (默认)", 1),
    ("频谱覆盖在任务栏上面", 2),
];

pub struct SettingsApp {
    settings: Arc<Mutex<AppSettings>>,
    monitor_count: usize,
}

impl SettingsApp {
    pub fn new(settings: Arc<Mutex<AppSettings>>, monitor_count: usize) -> Self {
        Self { settings, monitor_count }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(720.0);

                    // ---- 音频 ----
                    ui.group(|ui| {
                        ui.heading("音频");
                        ui.separator();

                        let mut s = self.settings.lock().unwrap();

                        let mut rb = s.reduce_bass;
                        ui.checkbox(&mut rb, "降低低音权重");
                        s.reduce_bass = rb;
                        ui.label(egui::RichText::new("降低低频段的视觉高度，避免低音过强时频谱被低频柱子占满。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut br = s.bass_resolution_level as i32;
                        ui.horizontal(|ui| {
                            ui.label("频率分辨率");
                            ui.add(egui::Slider::new(&mut br, 0..=6));
                            ui.label(format!("当前: {}", br));
                        });
                        s.bass_resolution_level = br as u8;

                        ui.add_space(4.0);

                        let mut fps = s.fps as i32;
                        ui.horizontal(|ui| {
                            ui.label("帧率 (FPS)");
                            ui.add(egui::Slider::new(&mut fps, 10..=60));
                            ui.label(format!("当前: {}", fps));
                        });
                        s.fps = fps.clamp(10, 60) as u8;
                        ui.label(egui::RichText::new("每秒渲染次数。越高越流畅但越耗 CPU，建议 30-60。").size(12.0).weak());
                    });

                    ui.add_space(6.0);

                    // ---- 显示 ----
                    ui.group(|ui| {
                        ui.heading("显示");
                        ui.separator();

                        let mut s = self.settings.lock().unwrap();

                        let mut effect = s.visual_effect.clone();
                        let current_name = VISUAL_EFFECTS.iter().find(|(_, n)| *n == effect).map(|(d, _)| *d).unwrap_or("柱状图 (bar1ch)");
                        egui::ComboBox::from_id_source("visual_effect")
                            .selected_text(current_name)
                            .show_ui(ui, |ui| {
                                for (display, name) in VISUAL_EFFECTS {
                                    ui.selectable_value(&mut effect, name.to_string(), *display);
                                }
                            });
                        s.visual_effect = effect;
                        ui.label(egui::RichText::new("选择频谱的可视化图形效果，通过 CPU 软件渲染实现。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut grav = s.gravity as usize;
                        let gname = GRAVITY_NAMES.get(grav).unwrap_or(&GRAVITY_NAMES[2]);
                        egui::ComboBox::from_id_source("gravity")
                            .selected_text(*gname)
                            .show_ui(ui, |ui| {
                                for (i, name) in GRAVITY_NAMES.iter().enumerate() {
                                    ui.selectable_value(&mut grav, i, *name);
                                }
                            });
                        s.gravity = grav.min(4) as u8;

                        ui.add_space(4.0);

                        let mut inv = s.inversion;
                        ui.checkbox(&mut inv, "反转频谱");
                        s.inversion = inv;
                        ui.label(egui::RichText::new("开启后，安静时显示满柱，有声时柱子缩短。").size(12.0).weak());

                        ui.add_space(4.0);

                        // 柱宽 + 间隙
                        let bar_enabled = s.visual_effect == "bar1ch";
                        ui.add_enabled_ui(bar_enabled, |ui| {
                            let mut bw = s.bar_width;
                            ui.horizontal(|ui| {
                                ui.label("柱子宽度");
                                ui.add(egui::Slider::new(&mut bw, 1..=30));
                                ui.label(format!("当前: {}px", bw));
                            });
                            s.bar_width = bw.clamp(1, 30);

                            let mut gw = s.gap_width;
                            ui.horizontal(|ui| {
                                ui.label("柱间间隙");
                                ui.add(egui::Slider::new(&mut gw, 0..=20));
                                ui.label(format!("当前: {}px", gw));
                            });
                            s.gap_width = gw.clamp(0, 20);
                        });
                        if !bar_enabled {
                            ui.label(egui::RichText::new("柱子宽度和间隙仅在柱状图效果下生效。").size(12.0).weak());
                        }

                        ui.add_space(4.0);

                        let mut fm = s.fill_mode;
                        let fm_name = FILL_NAMES.iter().find(|(_, v)| *v == fm).map(|(d, _)| *d).unwrap_or("仅空白区域 (默认)");
                        egui::ComboBox::from_id_source("fill_mode")
                            .selected_text(fm_name)
                            .show_ui(ui, |ui| {
                                for (name, val) in FILL_NAMES {
                                    ui.selectable_value(&mut fm, *val, *name);
                                }
                            });
                        s.fill_mode = fm;
                        ui.label(egui::RichText::new("铺满:频谱铺满整个任务栏后方; 仅空白区域:只在不被图标遮挡的空隙绘制。").size(12.0).weak());
                    });

                    ui.add_space(6.0);

                    // ---- 颜色 ----
                    ui.group(|ui| {
                        ui.heading("颜色");
                        ui.separator();

                        let mut s = self.settings.lock().unwrap();

                        let current_preset = match_preset(
                            s.hsl_hue_from, s.hsl_hue_to, s.hsl_saturation, s.hsl_lightness,
                            s.hsluv_hue_from, s.hsluv_hue_to, s.hsluv_saturation, s.hsluv_lightness,
                            s.color_space_hsluv,
                        );
                        let preset_label = match current_preset {
                            Some(i) => PRESETS[i].name,
                            None => "自定义 (当前配置)",
                        };

                        let mut preset_idx = current_preset.unwrap_or(PRESETS.len());
                        egui::ComboBox::from_id_source("color_preset")
                            .selected_text(preset_label)
                            .show_ui(ui, |ui| {
                                for (i, p) in PRESETS.iter().enumerate() {
                                    ui.selectable_value(&mut preset_idx, i, p.name);
                                }
                                ui.selectable_value(&mut preset_idx, PRESETS.len(), "自定义 (当前配置)");
                            });
                        if preset_idx < PRESETS.len() {
                            let p = &PRESETS[preset_idx];
                            s.color_space_hsluv = p.use_hsluv;
                            s.hsluv_hue_from = p.hsluv_hue_from;
                            s.hsluv_hue_to = p.hsluv_hue_to;
                            s.hsluv_saturation = p.hsluv_saturation;
                            s.hsluv_lightness = p.hsluv_lightness;
                            s.hsl_hue_from = p.hsl_hue_from;
                            s.hsl_hue_to = p.hsl_hue_to;
                            s.hsl_saturation = p.hsl_saturation;
                            s.hsl_lightness = p.hsl_lightness;
                        }

                        ui.add_space(4.0);

                        if ui.button("随机颜色").clicked() {
                            let (hf, ht, sat, light) = random_color();
                            s.color_space_hsluv = true;
                            s.hsluv_hue_from = hf;
                            s.hsluv_hue_to = ht;
                            s.hsluv_saturation = sat;
                            s.hsluv_lightness = light;
                        }
                        ui.label(egui::RichText::new("随机生成一组配色方案(会自动切换到 HSLuv 色彩空间)。多按几次直到满意。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut use_hsluv = s.color_space_hsluv;
                        ui.horizontal(|ui| {
                            ui.label("色彩空间");
                            ui.radio_value(&mut use_hsluv, false, "HSL (标准)");
                            ui.radio_value(&mut use_hsluv, true, "HSLuv (感知均匀)");
                        });
                        s.color_space_hsluv = use_hsluv;
                        ui.label(egui::RichText::new("HSL 是传统色彩模型; HSLuv 是感知均匀色彩空间，亮度变化更自然。").size(12.0).weak());

                        ui.add_space(4.0);

                        if s.color_space_hsluv {
                            let mut hf = s.hsluv_hue_from;
                            ui.horizontal(|ui| { ui.label("色相起始值"); ui.add(egui::Slider::new(&mut hf, -4000..=4000)); ui.label(format!("当前: {}", hf)); });
                            s.hsluv_hue_from = hf;
                            let mut ht = s.hsluv_hue_to;
                            ui.horizontal(|ui| { ui.label("色相结束值"); ui.add(egui::Slider::new(&mut ht, -4000..=4000)); ui.label(format!("当前: {}", ht)); });
                            s.hsluv_hue_to = ht;
                            let mut sat = s.hsluv_saturation;
                            ui.horizontal(|ui| { ui.label("饱和度"); ui.add(egui::Slider::new(&mut sat, 0..=100)); ui.label(format!("当前: {}", sat)); });
                            s.hsluv_saturation = sat;
                            let mut light = s.hsluv_lightness;
                            ui.horizontal(|ui| { ui.label("亮度"); ui.add(egui::Slider::new(&mut light, 0..=100)); ui.label(format!("当前: {}", light)); });
                            s.hsluv_lightness = light;
                        } else {
                            let mut hf = s.hsl_hue_from;
                            ui.horizontal(|ui| { ui.label("色相起始值"); ui.add(egui::Slider::new(&mut hf, -4000..=4000)); ui.label(format!("当前: {}", hf)); });
                            s.hsl_hue_from = hf;
                            let mut ht = s.hsl_hue_to;
                            ui.horizontal(|ui| { ui.label("色相结束值"); ui.add(egui::Slider::new(&mut ht, -4000..=4000)); ui.label(format!("当前: {}", ht)); });
                            s.hsl_hue_to = ht;
                            let mut sat = s.hsl_saturation;
                            ui.horizontal(|ui| { ui.label("饱和度"); ui.add(egui::Slider::new(&mut sat, 0..=100)); ui.label(format!("当前: {}", sat)); });
                            s.hsl_saturation = sat;
                            let mut light = s.hsl_lightness;
                            ui.horizontal(|ui| { ui.label("亮度"); ui.add(egui::Slider::new(&mut light, 0..=100)); ui.label(format!("当前: {}", light)); });
                            s.hsl_lightness = light;
                        }
                    });

                    ui.add_space(6.0);

                    // ---- Windows 设置 ----
                    ui.group(|ui| {
                        ui.heading("Windows 设置");
                        ui.separator();

                        let mut s = self.settings.lock().unwrap();

                        let mut om = s.overlay_mode;
                        let om_name = OVERLAY_NAMES.iter().find(|(_, v)| *v == om).map(|(d, _)| *d).unwrap_or(OVERLAY_NAMES[0].0);
                        egui::ComboBox::from_id_source("overlay_mode")
                            .selected_text(om_name)
                            .show_ui(ui, |ui| {
                                for (name, val) in OVERLAY_NAMES {
                                    ui.selectable_value(&mut om, *val, *name);
                                }
                            });
                        s.overlay_mode = om;
                        ui.label(egui::RichText::new("任务栏在上:频谱被任务栏遮挡; 频谱在上:频谱覆盖任务栏图标上方，透明区域鼠标可穿透。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut mh = s.max_height as i32;
                        ui.horizontal(|ui| {
                            ui.label("频谱窗口高度");
                            ui.add(egui::Slider::new(&mut mh, 0..=80));
                            ui.label(if mh == 0 { "自动".into() } else { format!("当前: {}px", mh) });
                        });
                        s.max_height = mh.clamp(0, 80) as u8;
                        ui.label(egui::RichText::new("0=自动(跟随任务栏高度); >0=限制频谱高度为该值，底部对齐。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut tgt = s.target_monitor;
                        let tgt_label = monitor_display_name(tgt);
                        egui::ComboBox::from_id_source("target_monitor")
                            .selected_text(tgt_label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut tgt, -1, "所有显示器");
                                for i in 0..self.monitor_count as i32 {
                                    ui.selectable_value(&mut tgt, i, monitor_display_name(i));
                                }
                            });
                        s.target_monitor = tgt;
                        ui.label(egui::RichText::new("选择在哪台显示器的任务栏上显示频谱。默认主显示器。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut et = s.enable_transparency;
                        ui.checkbox(&mut et, "系统透明效果");
                        s.enable_transparency = et;
                        ui.label(egui::RichText::new("开启/关闭 Windows 任务栏透明效果(修改注册表，退出时自动还原)。").size(12.0).weak());

                        ui.add_space(4.0);

                        let mut su = s.startup;
                        ui.checkbox(&mut su, "开机自启");
                        s.startup = su;
                        ui.label(egui::RichText::new("开启后程序会在系统启动时自动运行(修改注册表 Run 键)。").size(12.0).weak());
                    });
                });
        });
    }
}

fn monitor_display_name(idx: i32) -> String {
    match idx {
        -1 => "所有显示器".to_string(),
        0 => "主显示器 (默认)".to_string(),
        n => format!("显示器 {}", n + 1),
    }
}

fn random_color() -> (i32, i32, i32, i32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xCAFE_BABE);
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as i64
    };
    let hf = (next() % 360) as i32;
    let ht = ((next() % 900) - 180) as i32;
    let sat = (50 + (next() % 51)) as i32;
    let light = (40 + (next() % 31)) as i32;
    (hf, ht, sat, light)
}

fn setup_cjk_fonts(ctx: &egui::Context) {
    let cjk_paths = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    let font_data = cjk_paths
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .unwrap_or_default();
    if font_data.is_empty() {
        return;
    }
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("CJK".to_owned(), egui::FontData::from_owned(font_data));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "CJK".to_owned());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().push("CJK".to_owned());
    ctx.set_fonts(fonts);
}

/// 从嵌入的 icon 资源加载窗口图标
fn load_window_icon() -> Option<egui::IconData> {
    let ico_bytes = include_bytes!("../../assets/panon.ico");
    let icon_dir = ico::IconDir::read(std::io::Cursor::new(ico_bytes)).ok()?;
    // 取最大尺寸的图标
    let entry = icon_dir.entries().iter().max_by_key(|e| e.width())?;
    let rgba = entry.decode().ok()?;
    Some(egui::IconData {
        rgba: rgba.rgba_data().to_vec(),
        width: entry.width(),
        height: entry.height(),
    })
}

/// 应用 Win11 风格视觉主题
fn apply_win11_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    // 圆角
    style.visuals.window_rounding = egui::Rounding::same(8.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
    // 间距
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    // 字体大小
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

pub fn run_settings_window(
    settings: Arc<Mutex<AppSettings>>,
    monitor_count: usize,
    open_flag: Arc<Mutex<bool>>,
) {
    let icon = load_window_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([820.0, 720.0])
            .with_min_inner_size([600.0, 500.0])
            .with_title("Panon 设置")
            .with_icon(icon.unwrap_or_default()),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Panon 设置",
        options,
        Box::new(move |cc| {
            setup_cjk_fonts(&cc.egui_ctx);
            apply_win11_style(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(settings, monitor_count)))
        }),
    );

    if let Err(e) = result {
        eprintln!("[settings] eframe error: {}", e);
    }
    *open_flag.lock().unwrap() = false;
}
