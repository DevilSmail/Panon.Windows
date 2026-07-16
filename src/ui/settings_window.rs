// settings_window.rs — slint 风格设置窗口
//
// 设计：
// - 使用 slint 声明式 UI（ui/settings.slint）
// - 通过属性双向绑定 + 回调与 Rust 交互
// - 设置窗口在独立线程运行，通过 Arc<Mutex<AppSettings>> 共享状态
// - 实时修改 + 即时保存到 JSON

use std::sync::{Arc, Mutex, atomic::AtomicI32, atomic::Ordering};

use crate::render::presets::{match_preset, PRESETS};
use crate::settings::config::AppSettings;
use crate::taskbar::detect::TaskbarInfo;

// 编译 slint UI 文件，生成 SettingsWindow 和 Theme 类型
slint::include_modules!();

// ===== 常量 =====

const VISUAL_EFFECTS: &[&str] = &[
    "bar1ch", "wave", "solid1ch", "solid", "beam", "spectrogram", "oie1ch",
];

// ===== 转换函数 =====

fn visual_effect_to_idx(name: &str) -> i32 {
    VISUAL_EFFECTS.iter().position(|&v| v == name).unwrap_or(0) as i32
}

fn idx_to_visual_effect(idx: i32) -> String {
    VISUAL_EFFECTS
        .get(idx as usize)
        .unwrap_or(&"bar1ch")
        .to_string()
}

fn target_monitor_to_idx(target: &str, has_all: bool) -> i32 {
    if has_all {
        // ["所有显示器", "主显示器", "显示器 2"...]
        if target == "-1" { 0 } else { target.parse::<i32>().unwrap_or(0) + 1 }
    } else {
        // ["主显示器", "显示器 2"...]  — 直接映射
        target.parse().unwrap_or(0)
    }
}

fn idx_to_target_monitor(idx: i32, has_all: bool) -> String {
    if has_all {
        if idx <= 0 { "-1".to_string() } else { (idx - 1).to_string() }
    } else {
        idx.to_string()
    }
}

// ===== 系统主题检测 =====

fn detect_system_theme() -> bool {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_NONE,
    };

    const KEY_PATH: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    const VAL_NAME: PCWSTR = w!("AppsUseLightTheme");

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, KEY_PATH, 0, KEY_READ, &mut hkey).is_err() {
            return true; // 默认暗色
        }

        let mut value: u32 = 0;
        let mut len: u32 = 4;
        let mut val_type = REG_NONE;
        let _ = RegQueryValueExW(
            hkey,
            VAL_NAME,
            None,
            Some(&mut val_type),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut len),
        );
        let _ = RegCloseKey(hkey);

        value != 1 // 1 = 亮色, 其他 = 暗色
    }
}

// ===== 随机颜色（splitmix64 LCG） =====

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

// ===== 窗口居中 =====

/// 获取主显示器工作区 (left, top, width, height)，排除任务栏
fn get_primary_work_area() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA, 0,
            Some(&mut rect as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)
}

// ===== 主入口 =====

pub fn run_settings_window(
    settings: Arc<Mutex<AppSettings>>,
    taskbars: Vec<TaskbarInfo>,
    open_flag: Arc<Mutex<bool>>,
    on_startup: Box<dyn Fn(bool)>,
    on_transparency: Box<dyn Fn(bool)>,
    on_recreate_overlays: Box<dyn Fn(String) + Send + Sync>,
    pending_max_height: Arc<AtomicI32>,
) {
    // 创建 slint 窗口
    let window = match SettingsWindow::new() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[settings] slint 窗口创建失败: {}", e);
            *open_flag.lock().unwrap() = false;
            return;
        }
    };

    // 设置主题
    let dark_mode = detect_system_theme();
    let theme = Theme::get(&window);
    theme.set_dark_mode(dark_mode);

    let has_all = taskbars.len() > 1;

    // 设置初始属性值
    {
        let s = settings.lock().unwrap();
        window.set_reduce_bass(s.reduce_bass);
        window.set_bass_resolution_level(s.bass_resolution_level as f32);
        window.set_fps(s.fps as f32);
        window.set_visual_effect_idx(visual_effect_to_idx(&s.visual_effect_name));
        window.set_gravity(s.gravity as i32);
        window.set_inversion(s.inversion);
        window.set_bar_width(s.bar_width as f32);
        window.set_gap_width(s.gap_width as f32);
        window.set_fill_mode(s.fill_mode as i32);
        window.set_overlay_mode(s.overlay_mode as i32 - 1);
        window.set_color_space_idx(if s.color_space_hsluv { 1 } else { 0 });
        window.set_hsl_hue_from(s.hsl_hue_from as f32);
        window.set_hsl_hue_to(s.hsl_hue_to as f32);
        window.set_hsl_saturation(s.hsl_saturation as f32);
        window.set_hsl_lightness(s.hsl_lightness as f32);
        window.set_hsluv_hue_from(s.hsluv_hue_from as f32);
        window.set_hsluv_hue_to(s.hsluv_hue_to as f32);
        window.set_hsluv_saturation(s.hsluv_saturation as f32);
        window.set_hsluv_lightness(s.hsluv_lightness as f32);
        window.set_target_monitor_idx(target_monitor_to_idx(&s.target_monitor, has_all));
        window.set_startup(s.start_with_windows);
        window.set_enable_transparency(s.enable_transparency);
        window.set_spectrum_max_height(s.max_height as f32);

        // 设置预设索引
        let preset_idx = match_preset(
            s.hsl_hue_from, s.hsl_hue_to, s.hsl_saturation, s.hsl_lightness,
            s.hsluv_hue_from, s.hsluv_hue_to, s.hsluv_saturation, s.hsluv_lightness,
            s.color_space_hsluv,
        )
        .map(|i| i as i32)
        .unwrap_or(PRESETS.len() as i32);
        window.set_preset_idx(preset_idx);
    }

    // 设置显示器名称列表（对齐 C# 版: 基于任务栏数量，格式 宽×高）
    let monitor_names: Vec<slint::SharedString> = {
        let mut names: Vec<slint::SharedString> = taskbars
            .iter()
            .enumerate()
            .map(|(i, tb)| {
                if i == 0 {
                    format!("主显示器 - {}×{} (默认)", tb.width, tb.height).into()
                } else {
                    format!("显示器 {} - {}×{}", i + 1, tb.width, tb.height).into()
                }
            })
            .collect();
        // 只有多显示器时才显示"所有显示器"选项（与 C# 一致）
        if has_all {
            names.insert(0, "所有显示器".into());
        }
        names
    };
    window.set_monitor_names(slint::ModelRc::from(monitor_names.as_slice()));

    // 设置最大高度上限 = 主任务栏高度（物理像素，与 overlay 窗口的 DIB Section 尺寸一致）
    {
        let tb_height = taskbars.first().map(|tb| tb.height).unwrap_or(40) as f32;
        window.set_spectrum_max_height_max(tb_height);
    }

    // 设置预设名称列表
    let preset_names: Vec<slint::SharedString> = PRESETS
        .iter()
        .map(|p| p.name.into())
        .chain(std::iter::once("自定义".into()))
        .collect();
    window.set_preset_names(slint::ModelRc::from(preset_names.as_slice()));

    // 窗口图标通过 EXE 资源段（panon.rc）嵌入，Explorer/任务栏自动显示。
    // Slint 1.17 的 Window::icon 在 Windows 上不支持（已知限制）。

    // 居中窗口到屏幕工作区（对齐 C# DisplayArea.WorkArea）
    {
        let (wx, wy, ww, wh) = get_primary_work_area();
        let win_w = 720i32;
        let win_h = 600i32;
        let x = wx + (ww - win_w) / 2;
        let y = wy + (wh - win_h) / 2;
        window.window().set_position(slint::PhysicalPosition::new(x, y));
    }

    // === 回调设置 ===

    // 转换 Box<dyn Fn> 为 Arc<dyn Fn> 以便克隆
    let on_startup: Arc<dyn Fn(bool)> = Arc::from(on_startup);
    let on_transparency: Arc<dyn Fn(bool)> = Arc::from(on_transparency);
    let on_recreate_overlays: Arc<dyn Fn(String) + Send + Sync> = Arc::from(on_recreate_overlays);

    // 保存回调（通用：读取所有属性并保存到 JSON）
    {
        let weak = window.as_weak();
        let settings_clone = settings.clone();
        window.on_save_requested(move || {
            let w = weak.upgrade().unwrap();
            let mut s = settings_clone.lock().unwrap();
            s.reduce_bass = w.get_reduce_bass();
            s.bass_resolution_level = w.get_bass_resolution_level().round() as u8;
            s.fps = w.get_fps().round() as u8;
            s.visual_effect_name = idx_to_visual_effect(w.get_visual_effect_idx());
            s.gravity = w.get_gravity() as u8;
            s.inversion = w.get_inversion();
            s.bar_width = w.get_bar_width().round() as i32;
            s.gap_width = w.get_gap_width().round() as i32;
            s.fill_mode = w.get_fill_mode() as u8;
            s.overlay_mode = (w.get_overlay_mode() + 1) as u8;
            s.color_space_hsluv = w.get_color_space_idx() == 1;
            s.hsl_hue_from = w.get_hsl_hue_from().round() as i32;
            s.hsl_hue_to = w.get_hsl_hue_to().round() as i32;
            s.hsl_saturation = w.get_hsl_saturation().round() as i32;
            s.hsl_lightness = w.get_hsl_lightness().round() as i32;
            s.hsluv_hue_from = w.get_hsluv_hue_from().round() as i32;
            s.hsluv_hue_to = w.get_hsluv_hue_to().round() as i32;
            s.hsluv_saturation = w.get_hsluv_saturation().round() as i32;
            s.hsluv_lightness = w.get_hsluv_lightness().round() as i32;
            s.max_height = w.get_spectrum_max_height().round() as i32;
            s.save();
        });
    }

    // 目标显示器变更回调
    {
        let weak = window.as_weak();
        let settings_clone = settings.clone();
        let on_recreate = on_recreate_overlays.clone();
        let has_all_cb = has_all;
        window.on_target_monitor_changed(move || {
            let w = weak.upgrade().unwrap();
            let idx = w.get_target_monitor_idx();
            let target = idx_to_target_monitor(idx, has_all_cb);
            let mut s = settings_clone.lock().unwrap();
            s.target_monitor = target.clone();
            s.save();
            drop(s);
            // 延迟到下一帧执行，避免 CreateWindowEx 在 Slint 事件处理中重入
            let rec = on_recreate.clone();
            let _ = slint::invoke_from_event_loop(move || {
                rec(target);
            });
        });
    }

    // 最大高度变更回调：仅写入 AtomicI32，零阻塞
    // 渲染线程每帧检查并应用 resize，避免 GDI 操作在 Slint 事件循环中执行
    {
        let pending = pending_max_height.clone();
        let settings_clone = settings.clone();
        window.on_max_height_changed(move || {
            let s = settings_clone.lock().unwrap();
            pending.store(s.max_height, Ordering::Relaxed);
        });
    }

    // 透明效果变更回调
    {
        let weak = window.as_weak();
        let settings_clone = settings.clone();
        let on_trans = on_transparency.clone();
        window.on_transparency_changed(move || {
            let w = weak.upgrade().unwrap();
            let et = w.get_enable_transparency();
            let mut s = settings_clone.lock().unwrap();
            s.enable_transparency = et;
            s.save();
            drop(s);
            on_trans(et);
        });
    }

    // 开机自启变更回调
    {
        let settings_clone = settings.clone();
        let on_start = on_startup.clone();
        window.on_startup_changed(move |startup| {
            let mut s = settings_clone.lock().unwrap();
            s.start_with_windows = startup;
            s.save();
            drop(s);
            on_start(startup);
        });
    }

    // 随机颜色回调
    // 注意：颜色变化不需要 on_recreate 重建 overlay —— 渲染线程每帧
    // 都会通过 apply_settings_to_renderer 读取最新设置自动应用。
    {
        let weak = window.as_weak();
        let settings_clone = settings.clone();
        window.on_random_color_clicked(move || {
            let w = weak.upgrade().unwrap();
            let (hf, ht, sat, light) = random_color();

            // 更新 slint 属性
            w.set_color_space_idx(1); // HSLuv
            w.set_hsluv_hue_from(hf as f32);
            w.set_hsluv_hue_to(ht as f32);
            w.set_hsluv_saturation(sat as f32);
            w.set_hsluv_lightness(light as f32);
            w.set_preset_idx(PRESETS.len() as i32); // 自定义

            // 更新设置并保存（渲染线程下一帧自动应用新颜色）
            let mut s = settings_clone.lock().unwrap();
            s.color_space_hsluv = true;
            s.hsluv_hue_from = hf;
            s.hsluv_hue_to = ht;
            s.hsluv_saturation = sat;
            s.hsluv_lightness = light;
            s.save();
        });
    }

    // 预设配色选择回调
    {
        let weak = window.as_weak();
        let settings_clone = settings.clone();
        window.on_preset_selected(move |idx| {
            if idx < 0 || idx as usize >= PRESETS.len() {
                return;
            }
            let preset = &PRESETS[idx as usize];
            let w = weak.upgrade().unwrap();

            // 更新 slint 属性
            w.set_color_space_idx(if preset.use_hsluv { 1 } else { 0 });
            w.set_hsl_hue_from(preset.hsl_hue_from as f32);
            w.set_hsl_hue_to(preset.hsl_hue_to as f32);
            w.set_hsl_saturation(preset.hsl_saturation as f32);
            w.set_hsl_lightness(preset.hsl_lightness as f32);
            w.set_hsluv_hue_from(preset.hsluv_hue_from as f32);
            w.set_hsluv_hue_to(preset.hsluv_hue_to as f32);
            w.set_hsluv_saturation(preset.hsluv_saturation as f32);
            w.set_hsluv_lightness(preset.hsluv_lightness as f32);

            // 更新设置并保存
            let mut s = settings_clone.lock().unwrap();
            s.color_space_hsluv = preset.use_hsluv;
            s.hsl_hue_from = preset.hsl_hue_from;
            s.hsl_hue_to = preset.hsl_hue_to;
            s.hsl_saturation = preset.hsl_saturation;
            s.hsl_lightness = preset.hsl_lightness;
            s.hsluv_hue_from = preset.hsluv_hue_from;
            s.hsluv_hue_to = preset.hsluv_hue_to;
            s.hsluv_saturation = preset.hsluv_saturation;
            s.hsluv_lightness = preset.hsluv_lightness;
            s.save();
        });
    }

    // 定时检查托盘退出请求
    window.on_check_actions(move || {
        if crate::tray::icon::EXIT_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = slint::quit_event_loop();
        }
    });

    // 运行事件循环（阻塞直到窗口关闭）
    if let Err(e) = window.run() {
        eprintln!("[settings] slint 事件循环错误: {}", e);
    }

    // 标记窗口已关闭
    *open_flag.lock().unwrap() = false;
}
