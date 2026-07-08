// panon.windows — Rust 原生版入口
// 阶段 7：设置窗口 UI (egui) + 共享状态

mod app;
mod audio;
mod overlay;
mod render;
mod settings;
mod taskbar;
mod tray;
mod ui;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
};

use audio::capture::AudioCapture;
use audio::decay::DecayProcessor;
use audio::fft::FftProcessor;
use audio::spectrum::SpectrumData;
use overlay::window::OverlayWindow;
use render::renderer::{SpectrumRenderer, VisualEffect};
use settings::config::AppSettings;
use taskbar::detect::get_all_taskbars;
use tray::TrayAction;
use tray::icon::TrayIcon;
use ui::settings_window::run_settings_window;

/// 将 AppSettings 应用到渲染器（每帧调用，开销可忽略）
fn apply_settings_to_renderer(r: &mut SpectrumRenderer, s: &AppSettings) {
    r.visual_effect = VisualEffect::from_name(&s.visual_effect);
    r.gravity = s.gravity;
    r.inversion = s.inversion;
    r.color_space_hsluv = s.color_space_hsluv;
    r.hsl_hue_from = s.hsl_hue_from;
    r.hsl_hue_to = s.hsl_hue_to;
    r.hsl_saturation = s.hsl_saturation;
    r.hsl_lightness = s.hsl_lightness;
    r.hsluv_hue_from = s.hsluv_hue_from;
    r.hsluv_hue_to = s.hsluv_hue_to;
    r.hsluv_saturation = s.hsluv_saturation;
    r.hsluv_lightness = s.hsluv_lightness;
    r.bar_width = s.bar_width;
    r.gap_width = s.gap_width;
    r.fill_mode = s.fill_mode;
}

fn main() {
    // DPI 感知：防御性设置（清单未生效时的兜底），确保 overlay 坐标不被 Windows 虚拟化
    unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }

    println!("=== Panon.Windows (Rust) — Phase 7: Settings Window (egui) ===");

    // 1. 获取所有任务栏（主 + 副显示器）
    let taskbars = get_all_taskbars();
    if taskbars.is_empty() {
        eprintln!("Failed to detect any taskbar");
        std::process::exit(1);
    }
    println!("Detected {} taskbar(s):", taskbars.len());
    for (i, tb) in taskbars.iter().enumerate() {
        println!(
            "  [{}] {}x{} at ({},{}) pos={:?} primary={}",
            i, tb.width, tb.height, tb.x, tb.y, tb.position, tb.is_primary
        );
    }
    let monitor_count = taskbars.len();

    // 2. 为每个任务栏创建独立覆盖窗口
    let mut overlays: Vec<OverlayWindow> = Vec::new();
    for tb in &taskbars {
        match OverlayWindow::create(tb) {
            Ok(o) => {
                println!(
                    "Overlay [{}] created: {}x{} at ({},{})",
                    overlays.len(),
                    o.width(),
                    o.height(),
                    tb.x,
                    tb.y
                );
                overlays.push(o);
            }
            Err(e) => {
                eprintln!("Failed to create overlay for taskbar {:?}: {}", tb, e);
            }
        }
    }
    if overlays.is_empty() {
        eprintln!("No overlay window created, exiting");
        std::process::exit(1);
    }

    // 共享设置状态（设置窗口写入，主循环读取）
    // 阶段 8 起从 %APPDATA%/Panon/settings.json 加载
    let settings = Arc::new(Mutex::new(AppSettings::default()));
    let settings_window_open = Arc::new(Mutex::new(false));

    // 应用初始设置到所有 overlay
    {
        let s = settings.lock().unwrap();
        for overlay in &mut overlays {
            apply_settings_to_renderer(&mut overlay.renderer, &s);
        }
    }

    // 维护 Z-order：Above 模式（频谱覆盖 taskbar，柱子间透明可看到 taskbar）
    for overlay in &overlays {
        let taskbar_hwnd = HWND(overlay.taskbar().hwnd as *mut _);
        unsafe { overlay.ensure_z_order(taskbar_hwnd, 2); }
    }

    // 3. 启动音频捕获
    let (tx, rx) = mpsc::channel();
    let (mut capture, sample_rate, channels) = match AudioCapture::start(tx) {
        Ok((capture, sr, ch)) => {
            println!("Audio: {}Hz {}ch", sr, ch);
            (capture, sr, ch)
        }
        Err(e) => {
            eprintln!("Audio capture failed: {}", e);
            eprintln!("Hint: 确保系统有音频输出设备且正在播放声音");
            std::process::exit(1);
        }
    };
    println!("Press Ctrl+C to exit");
    println!();

    // 4. FFT 处理器 + 衰减处理器（应用初始设置）
    let mut fft = FftProcessor::new();
    {
        let s = settings.lock().unwrap();
        fft.set_bass_resolution_level(s.bass_resolution_level);
        fft.set_reduce_bass(s.reduce_bass);
    }
    let mut decay = DecayProcessor::new();

    // 4.5. 创建系统托盘图标（接收托盘动作通过 channel）
    let (action_tx, action_rx) = mpsc::channel();
    let tray = match TrayIcon::create(action_tx) {
        Ok(t) => {
            println!("Tray icon created");
            t
        }
        Err(e) => {
            eprintln!("Tray icon creation failed: {}", e);
            std::process::exit(1);
        }
    };

    // 5. 主循环
    let mut last_spectrum = SpectrumData::default();
    let mut last_spectrum_time = Instant::now();
    let mut last_render = Instant::now();
    let mut last_z_order = Instant::now();
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    let mut render_interval = Duration::from_millis(33); // 由 settings.fps 动态更新
    let idle_timeout = Duration::from_millis(200);
    let z_order_interval = Duration::from_secs(2);
    let exit_timeout = Duration::from_millis(800);
    let mut frame_count = 0u64;
    let mut last_debug = Instant::now();
    let mut exiting = false;
    let mut exit_start = Instant::now();
    let mut paused = false;

    loop {
        // 处理窗口消息
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    exiting = true;
                    exit_start = Instant::now();
                    capture.stop();
                    decay.force_exit();
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // 处理托盘动作
        while let Ok(action) = action_rx.try_recv() {
            match action {
                TrayAction::TogglePause => {
                    paused = !paused;
                    println!("[tray] {}", if paused { "Paused" } else { "Resumed" });
                }
                TrayAction::ShowSettings => {
                    let mut open = settings_window_open.lock().unwrap();
                    if !*open {
                        *open = true;
                        run_settings_window(
                            settings.clone(),
                            monitor_count,
                            settings_window_open.clone(),
                        );
                        println!("[tray] Opening settings window");
                    } else {
                        println!("[tray] Settings window already open");
                    }
                }
                TrayAction::Exit => {
                    exiting = true;
                    exit_start = Instant::now();
                    capture.stop();
                    decay.force_exit();
                }
                TrayAction::TaskbarRestart => {
                    println!("[tray] TaskbarCreated: re-adding icon and recreating overlays");
                    tray.re_add();
                    overlays.clear();
                    // 排空因销毁旧 overlay 窗口产生的 WM_QUIT 消息
                    unsafe {
                        while PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool() {}
                    }
                    let new_taskbars = get_all_taskbars();
                    let s = settings.lock().unwrap();
                    for tb in &new_taskbars {
                        if let Ok(mut o) = OverlayWindow::create(tb) {
                            apply_settings_to_renderer(&mut o.renderer, &s);
                            let taskbar_hwnd = HWND(o.taskbar().hwnd as *mut _);
                            unsafe { o.ensure_z_order(taskbar_hwnd, 2); }
                            overlays.push(o);
                        }
                    }
                }
            }
        }

        // 退出衰减流程：继续渲染直到频谱归零或超时
        if exiting {
            let silent = SpectrumData::default();
            let decayed = decay.process(&silent);
            for overlay in &mut overlays {
                unsafe { overlay.render(&decayed.left_channel, &decayed.right_channel); }
            }

            if decay.is_exit_complete() || exit_start.elapsed() > exit_timeout {
                println!("[exit] decay complete, exiting");
                return;
            }

            std::thread::sleep(Duration::from_millis(16));
            continue;
        }

        // 接收音频数据 → FFT（暂停时跳过，使用零值频谱）
        if !paused {
            while let Ok(samples) = rx.try_recv() {
                if !samples.is_empty() {
                    last_spectrum = fft.process(&samples, channels, sample_rate);
                    last_spectrum_time = Instant::now();
                }
            }
        }

        // 应用设置到 FFT + 所有 overlay（每帧同步，设置窗口修改即时生效）
        {
            let s = settings.lock().unwrap();
            fft.set_bass_resolution_level(s.bass_resolution_level);
            fft.set_reduce_bass(s.reduce_bass);
            let fps = s.fps.max(1) as u64;
            render_interval = Duration::from_millis(1000 / fps);
            for overlay in &mut overlays {
                apply_settings_to_renderer(&mut overlay.renderer, &s);
            }
        }

        // 渲染（由 settings.fps 控制帧率）
        if last_render.elapsed() >= render_interval {
            let spectrum = if paused {
                // 暂停：零值频谱（衰减自然回落到零）
                SpectrumData::default()
            } else {
                let is_idle = last_spectrum_time.elapsed() > idle_timeout;
                if is_idle {
                    // 静默：零值频谱（volume=0 触发 silence_factor 快速回落）
                    let mut s = last_spectrum.clone();
                    for v in &mut s.left_channel {
                        *v = 0.0;
                    }
                    for v in &mut s.right_channel {
                        *v = 0.0;
                    }
                    s.volume = 0.0;
                    s
                } else {
                    last_spectrum.clone()
                }
            };

            // 应用衰减后渲染到所有 overlay
            let decayed = decay.process(&spectrum);
            for overlay in &mut overlays {
                // FillMode=1: 更新空白区域（UIA 探测，含 500ms 缓存）
                if overlay.renderer.fill_mode == 1 {
                    let min_bw = overlay.renderer.bar_width + overlay.renderer.gap_width;
                    overlay.update_free_regions(min_bw);
                }
                unsafe { overlay.render(&decayed.left_channel, &decayed.right_channel); }
            }
            last_render = Instant::now();
            frame_count += 1;
        }

        // 定期维护 Z-order（每 2 秒）
        if last_z_order.elapsed() >= z_order_interval {
            for overlay in &overlays {
                let taskbar_hwnd = HWND(overlay.taskbar().hwnd as *mut _);
                unsafe { overlay.ensure_z_order(taskbar_hwnd, 2); }
            }
            last_z_order = Instant::now();
        }

        // 调试输出（每 3 秒）
        if last_debug.elapsed() >= Duration::from_secs(3) {
            let bars = last_spectrum.left_channel.len();
            let vol = last_spectrum.volume;
            let idle = last_spectrum_time.elapsed() > idle_timeout;
            println!(
                "[debug] frames={} overlays={} bars={} vol={:.4} idle={} paused={} {:?}",
                frame_count,
                overlays.len(),
                bars,
                vol,
                idle,
                paused,
                if paused { "paused" } else if idle { "silent" } else { "active" }
            );
            last_debug = Instant::now();
        }

        // 避免忙等
        std::thread::sleep(Duration::from_millis(1));
    }
}
