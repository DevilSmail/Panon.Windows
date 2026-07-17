// panon.windows — Rust 原生版入口
// 线程架构：MAIN（消息+托盘+设置窗口）| RENDER（FFT+衰减+渲染）| CAPTURE（WASAPI）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod overlay;
mod render;
mod settings;
mod taskbar;
mod tray;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use png::{BitDepth, ColorType, Encoder};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_SZ,
};
use windows::Win32::System::Threading::CreateMutexW;
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
use settings::transparency::TransparencyManager;
use taskbar::detect::{get_all_taskbars, TaskbarInfo};
use tray::TrayAction;
use tray::icon::TrayIcon;
use ui::settings_window::run_settings_window;

fn apply_settings_to_renderer(r: &mut SpectrumRenderer, s: &AppSettings) {
    r.visual_effect = VisualEffect::from_name(&s.visual_effect_name);
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

fn create_overlays(taskbars: &[TaskbarInfo], target_monitor: &str, max_height: i32) -> Vec<OverlayWindow> {
    let mut overlays = Vec::new();
    if taskbars.is_empty() {
        return overlays;
    }

    let indices: Vec<usize> = if target_monitor == "-1" {
        (0..taskbars.len()).collect()
    } else {
        let idx: usize = target_monitor.parse().unwrap_or(0);
        if idx < taskbars.len() {
            vec![idx]
        } else {
            vec![0]
        }
    };

    for idx in indices {
        let tb = &taskbars[idx];
        match OverlayWindow::create(tb, max_height) {
            Ok(o) => {
                println!("[overlay] created monitor={} pos={}x{} size={}x{} max_height={}", idx, tb.x, tb.y, o.width(), o.height(), max_height);
                overlays.push(o);
            }
            Err(e) => eprintln!("[overlay] failed to create monitor {}: {}", idx, e),
        }
    }

    overlays
}

fn export_renderer_images(output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;
    let width = 800;
    let height = 120;
    let left = make_test_audio(2048);
    let right = left.clone();
    let mut renderer = SpectrumRenderer::new();

    let effect_names = [
        "bar1ch",
        "wave",
        "solid1ch",
        "solid",
        "beam",
        "spectrogram",
        "oie1ch",
    ];

    for name in effect_names {
        renderer.visual_effect = VisualEffect::from_name(name);
        renderer.free_regions = None;
        let mut pixels = vec![0u32; (width * height) as usize];
        unsafe {
            renderer.render_to_pixels(&left, &right, pixels.as_mut_ptr(), width, height);
        }
        save_png(output_dir.join(format!("render_{}.png", name)), &pixels, width, height)?;
    }

    println!("Rendered {} frames to {}", effect_names.len(), output_dir.display());
    Ok(())
}

fn make_test_audio(samples: usize) -> Vec<f32> {
    let mut result = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f32 / samples as f32;
        let value = (t * std::f32::consts::PI * 4.0).sin() * 0.35
            + (t * std::f32::consts::PI * 12.0).sin() * 0.2
            + (t * std::f32::consts::PI * 30.0).sin() * 0.1;
        result.push((value + 0.8).clamp(0.0, 1.0));
    }
    result
}

fn save_png(path: PathBuf, pixels: &[u32], width: i32, height: i32) -> Result<(), Box<dyn std::error::Error>> {
    let mut buffer = Vec::with_capacity(pixels.len() * 4);
    for &pixel in pixels {
        let b = (pixel & 0xff) as u8;
        let g = ((pixel >> 8) & 0xff) as u8;
        let r = ((pixel >> 16) & 0xff) as u8;
        let a = ((pixel >> 24) & 0xff) as u8;
        buffer.push(r);
        buffer.push(g);
        buffer.push(b);
        buffer.push(a);
    }

    let file = std::fs::File::create(path)?;
    let w = std::io::BufWriter::new(file);
    let mut encoder = Encoder::new(w, width as u32, height as u32);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buffer)?;
    Ok(())
}

fn single_instance_check() {
    unsafe {
        match CreateMutexW(None, true, w!("Global\\Panon.Windows.SingleInstance")) {
            Ok(_) => {
                if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                    eprintln!("Another instance is already running, exiting");
                    std::process::exit(0);
                }
            }
            Err(e) => eprintln!("[warn] CreateMutex failed: {}, continuing", e),
        }
    }
}

fn set_startup(enable: bool) {
    const RUN_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run");
    const VAL_NAME: PCWSTR = w!("Panon");
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let exe_wide: Vec<u16> = exe_path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_WRITE, &mut hkey).is_err() {
            return;
        }
        if enable {
            let data: &[u8] =
                std::slice::from_raw_parts(exe_wide.as_ptr() as *const u8, exe_wide.len() * 2);
            let _ = RegSetValueExW(hkey, VAL_NAME, 0, REG_SZ, Some(data));
        } else {
            let _ = RegDeleteValueW(hkey, VAL_NAME);
        }
        let _ = RegCloseKey(hkey);
    }
}

fn setup_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let msg = format!("[{}] Panon crash: {}\n", timestamp, info);
        eprintln!("{}", msg);
        let crash_path = std::env::temp_dir().join("panon_crash.txt");
        let _ = std::fs::write(crash_path, &msg);
    }));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut export_dir: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--export-renders" => {
                if let Some(dir) = args.next() {
                    export_dir = Some(PathBuf::from(dir));
                }
            }
            _ => {}
        }
    }

    if let Some(dir) = export_dir {
        if let Err(err) = export_renderer_images(&dir) {
            eprintln!("Failed to export renders: {}", err);
            std::process::exit(1);
        }
        return;
    }

    single_instance_check();
    setup_panic_hook();

    unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }

    println!("=== Panon.Windows (Rust) ===");

    let initial_settings = AppSettings::load();
    let transparency = Arc::new(TransparencyManager::new());
    transparency.apply(initial_settings.enable_transparency);
    set_startup(initial_settings.start_with_windows);

    let mut taskbars = Arc::new(get_all_taskbars());
    if taskbars.is_empty() {
        eprintln!("No taskbar detected, exiting");
        std::process::exit(1);
    }

    let overlays = std::sync::Arc::new(std::sync::Mutex::new(Vec::<OverlayWindow>::new()));
    {
        let created = create_overlays(&taskbars, &initial_settings.target_monitor, initial_settings.max_height);
        let mut ov = overlays.lock().unwrap();
        *ov = created;
    }
    if overlays.lock().unwrap().is_empty() {
        eprintln!("No overlay window created, exiting");
        std::process::exit(1);
    }

    {
        let s = &initial_settings;
        let mut ov = overlays.lock().unwrap();
        for o in &mut *ov {
            apply_settings_to_renderer(&mut o.renderer, s);
        }
    }

    {
        let ov = overlays.lock().unwrap();
        for o in &*ov {
            let thwnd = HWND(o.taskbar().hwnd as *mut _);
            unsafe { o.ensure_z_order(thwnd, initial_settings.overlay_mode); }
        }
    }

    let settings = Arc::new(Mutex::new(initial_settings));
    let settings_window_open = Arc::new(Mutex::new(false));
    // 暂停状态使用 tray::icon::IS_PAUSED 统一管理（设置窗口阻塞时不经过 channel）
    let exiting = Arc::new(AtomicBool::new(false));
    // 最大高度变更通道：设置窗口写入 → 渲染线程读取并应用
    let pending_max_height: Arc<AtomicI32> = Arc::new(AtomicI32::new(-1));

    let (sample_tx, sample_rx) = mpsc::channel();
    let (mut capture, sample_rate, channels) = match AudioCapture::start(sample_tx) {
        Ok((c, sr, ch)) => {
            println!("Audio: {}Hz {}ch", sr, ch);
            (c, sr, ch)
        }
        Err(e) => {
            eprintln!("Audio capture failed: {}", e);
            std::process::exit(1);
        }
    };

    let (action_tx, action_rx) = mpsc::channel();

    let tray = match TrayIcon::create(action_tx.clone()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tray icon failed: {}", e);
            std::process::exit(1);
        }
    };

    // ═══════════════════════════════════════════════════════════════
    // RENDER THREAD: FFT → Decay → Render → UpdateLayeredWindow
    // 独立运行，设置窗口打开时频谱不中断，设置更改即时生效
    // ═══════════════════════════════════════════════════════════════
    let render_settings = settings.clone();
    let render_exiting = exiting.clone();
    let idle_timeout = Duration::from_millis(200);
    let z_order_interval = Duration::from_secs(2);

    let overlays_for_render = overlays.clone();
    let render_pending_max_height = pending_max_height.clone();
    let render_thread = std::thread::Builder::new()
        .name("Panon Render".into())
        .spawn(move || {
            let mut fft = FftProcessor::new();
            {
                let s = render_settings.lock().unwrap();
                fft.set_bass_resolution_level(s.bass_resolution_level);
                fft.set_reduce_bass(s.reduce_bass);
            }
            let mut decay = DecayProcessor::new();
            let mut last_spectrum = SpectrumData::default();
            let mut last_spectrum_time = Instant::now();
            let mut last_z_order = Instant::now();
            let mut frame_count = 0u64;
            let mut last_debug = Instant::now();

            loop {
                if render_exiting.load(Ordering::SeqCst) {
                    decay.force_exit();
                    let silent = SpectrumData::default();
                    let decayed = decay.process(&silent);
                    let mut ov = overlays_for_render.lock().unwrap();
                    for o in &mut *ov {
                        unsafe { o.render(&decayed.left_channel, &decayed.right_channel); }
                    }
                    if decay.is_exit_complete() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(16));
                    continue;
                }

                let fps = {
                    let s = render_settings.lock().unwrap();
                    fft.set_bass_resolution_level(s.bass_resolution_level);
                    fft.set_reduce_bass(s.reduce_bass);
                    {
                        let mut ov = overlays_for_render.lock().unwrap();
                        for o in &mut *ov {
                            apply_settings_to_renderer(&mut o.renderer, &s);
                        }
                    }
                    s.fps.max(1) as u64
                };

                // 检查是否有待处理的 max_height 变更（设置窗口写入，渲染线程应用）
                let requested = render_pending_max_height.load(Ordering::Relaxed);
                if requested >= 0 {
                    render_pending_max_height.store(-1, Ordering::Relaxed);
                    let mut ov = overlays_for_render.lock().unwrap();
                    for o in ov.iter_mut() {
                        unsafe { o.set_max_height(requested); }
                    }
                }

                if !tray::icon::IS_PAUSED.load(Ordering::SeqCst) {
                    while let Ok(samples) = sample_rx.try_recv() {
                        if !samples.is_empty() {
                            last_spectrum = fft.process(&samples, channels, sample_rate);
                            last_spectrum_time = Instant::now();
                        }
                    }
                }

                let spectrum = if tray::icon::IS_PAUSED.load(Ordering::SeqCst) {
                    // 暂停：切换到空闲模式，保持上次频谱数据让 decay 自然衰减（对齐 C# 行为）
                    taskbar::uia::set_idle_mode(true);
                    // Feed silence: 清零通道数据 + volume，使 decay 使用 silence_factor (0.75)
                    let mut s = last_spectrum.clone();
                    for v in &mut s.left_channel { *v = 0.0; }
                    for v in &mut s.right_channel { *v = 0.0; }
                    s.volume = 0.0;
                    s
                } else {
                    let is_idle = last_spectrum_time.elapsed() > idle_timeout;
                    // 空闲状态切换 UIA 刷新间隔（与 C# TaskbarHelper.SetIdleMode 对齐）
                    taskbar::uia::set_idle_mode(is_idle);
                    if is_idle {
                        let mut s = last_spectrum.clone();
                        for v in &mut s.left_channel { *v = 0.0; }
                        for v in &mut s.right_channel { *v = 0.0; }
                        s.volume = 0.0;
                        s
                    } else {
                        last_spectrum.clone()
                    }
                };
                let decayed = decay.process(&spectrum);
                {
                    let mut ov = overlays_for_render.lock().unwrap();
                    for o in &mut *ov {
                        if o.renderer.fill_mode == 1 {
                            let min_bw = o.renderer.bar_width + o.renderer.gap_width;
                            o.update_free_regions(min_bw);
                        }
                        unsafe { o.render(&decayed.left_channel, &decayed.right_channel); }
                    }
                }
                frame_count += 1;

                if last_z_order.elapsed() >= z_order_interval {
                    let overlay_mode = {
                        let s = render_settings.lock().unwrap();
                        s.overlay_mode
                    };
                    let ov = overlays_for_render.lock().unwrap();
                    for o in &*ov {
                        let thwnd = HWND(o.taskbar().hwnd as *mut _);
                        unsafe { o.ensure_z_order(thwnd, overlay_mode); }
                    }
                    last_z_order = Instant::now();
                }

                if last_debug.elapsed() >= Duration::from_secs(3) {
                    println!(
                        "[debug] frames={} bars={} vol={:.4}",
                        frame_count,
                        last_spectrum.left_channel.len(),
                        last_spectrum.volume
                    );
                    last_debug = Instant::now();
                }

                std::thread::sleep(Duration::from_millis(1000 / fps));
            }
        })
        .expect("failed to spawn render thread");

    // ═══════════════════════════════════════════════════════════════
    // MAIN THREAD: 消息循环 + 托盘 + 设置窗口
    // ═══════════════════════════════════════════════════════════════
    let mut msg: MSG = unsafe { std::mem::zeroed() };

    loop {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    println!("[main] ignored WM_QUIT from unrelated window message queue");
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // 检查托盘暂停请求（设置窗口打开时主循环阻塞，通过标志直达）
        if tray::icon::PENDING_PAUSE_TOGGLE.load(Ordering::SeqCst) {
            tray::icon::PENDING_PAUSE_TOGGLE.store(false, Ordering::SeqCst);
            let was = tray::icon::IS_PAUSED.load(Ordering::SeqCst);
            tray::icon::IS_PAUSED.store(!was, Ordering::SeqCst);
            println!("[tray] {}", if was { "Resumed" } else { "Paused" });
        }

        while let Ok(action) = action_rx.try_recv() {
            match action {
                TrayAction::TogglePause => {
                    let was = tray::icon::IS_PAUSED.load(Ordering::SeqCst);
                    tray::icon::IS_PAUSED.store(!was, Ordering::SeqCst);
                    println!("[tray] {}", if was { "Resumed" } else { "Paused" });
                }
                TrayAction::ShowSettings => {
                    let already_open = {
                        let mut open = settings_window_open.lock().unwrap();
                        if !*open {
                            *open = true;
                            false
                        } else {
                            true
                        }
                    };
                    if !already_open {
                        let settings_ref = settings.clone();
                        let open_flag = settings_window_open.clone();
                        let tbs = taskbars.clone();

                        // 重建 overlay 回调
                        let on_recreate = {
                            let overlays = overlays.clone();
                            let settings = settings.clone();
                            let tb = taskbars.clone();
                            move |target: String| {
                                println!("[settings] RecreateOverlays: target={}", target);
                                let current_max_height = settings.lock().unwrap().max_height;
                                let created = create_overlays(&tb, &target, current_max_height);
                                if created.is_empty() {
                                    eprintln!("[settings] recreate overlays failed; keeping existing overlays");
                                } else {
                                    let mut ov = overlays.lock().unwrap();
                                    *ov = created;
                                    let s = settings.lock().unwrap().clone();
                                    for o in &mut *ov {
                                        apply_settings_to_renderer(&mut o.renderer, &s);
                                        let thwnd = HWND(o.taskbar().hwnd as *mut _);
                                        unsafe { o.ensure_z_order(thwnd, s.overlay_mode); }
                                    }
                                    println!("[settings] {} overlays recreated", overlays.lock().unwrap().len());
                                }
                            }
                        };

                        // 开机自启回调
                        let on_startup = |enable: bool| set_startup(enable);

                        // 透明效果回调（对齐 C#：单开关同时控制两个注册表键）
                        let t = transparency.clone();
                        let on_transparency = move |enable: bool| {
                            t.apply(enable);
                        };

                        run_settings_window(
                            settings_ref,
                            (*tbs).clone(),
                            open_flag,
                            Box::new(on_startup),
                            Box::new(on_transparency),
                            Box::new(on_recreate),
                            pending_max_height.clone(),
                        );
                    }
                }
                TrayAction::Exit => {
                    exiting.store(true, Ordering::SeqCst);
                    capture.stop();
                    let _ = render_thread.join();
                    // 保存设置但不恢复透明效果（与 C# 对齐：退出时保持用户设置）
                    settings.lock().unwrap().save();
                    // transparency.restore() 仅在卸载时显式调用
                    return;
                }
                TrayAction::TaskbarRestart => {
                    println!("[main] TaskbarCreated — re-detecting taskbars and recreating overlays");
                    tray.re_add();
                    let new_taskbars = get_all_taskbars();
                    if !new_taskbars.is_empty() {
                        taskbars = Arc::new(new_taskbars);
                        let s = settings.lock().unwrap().clone();
                        let current_target = s.target_monitor.clone();
                        let current_max_height = s.max_height;
                        let created = create_overlays(&taskbars, &current_target, current_max_height);
                        if !created.is_empty() {
                            let mut ov = overlays.lock().unwrap();
                            *ov = created;
                            for o in &mut *ov {
                                apply_settings_to_renderer(&mut o.renderer, &s);
                                let thwnd = HWND(o.taskbar().hwnd as *mut _);
                                unsafe { o.ensure_z_order(thwnd, s.overlay_mode); }
                            }
                            println!("[main] TaskbarCreated — {} overlays recreated", overlays.lock().unwrap().len());
                        }
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
