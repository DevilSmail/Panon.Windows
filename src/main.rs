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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

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
use taskbar::detect::get_all_taskbars;
use tray::TrayAction;
use tray::icon::TrayIcon;
use ui::settings_window::run_settings_window;

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
    single_instance_check();
    setup_panic_hook();

    unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }

    println!("=== Panon.Windows (Rust) ===");

    let initial_settings = AppSettings::load();
    let transparency = TransparencyManager::new();
    transparency.apply(
        initial_settings.enable_transparency,
        initial_settings.use_oled_taskbar_transparency,
    );
    set_startup(initial_settings.startup);

    let taskbars = get_all_taskbars();
    if taskbars.is_empty() {
        eprintln!("No taskbar detected, exiting");
        std::process::exit(1);
    }
    let monitor_count = taskbars.len();

    let mut overlays: Vec<OverlayWindow> = Vec::new();
    for tb in &taskbars {
        match OverlayWindow::create(tb) {
            Ok(o) => overlays.push(o),
            Err(e) => eprintln!("Failed to create overlay: {}", e),
        }
    }
    if overlays.is_empty() {
        eprintln!("No overlay window created, exiting");
        std::process::exit(1);
    }

    {
        let s = &initial_settings;
        for o in &mut overlays {
            apply_settings_to_renderer(&mut o.renderer, s);
        }
    }

    for o in &overlays {
        let thwnd = HWND(o.taskbar().hwnd as *mut _);
        unsafe { o.ensure_z_order(thwnd, 2); }
    }

    let settings = Arc::new(Mutex::new(initial_settings));
    let settings_window_open = Arc::new(Mutex::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let exiting = Arc::new(AtomicBool::new(false));

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
    let tray = match TrayIcon::create(action_tx) {
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
    let render_paused = paused.clone();
    let render_exiting = exiting.clone();
    let idle_timeout = Duration::from_millis(200);
    let z_order_interval = Duration::from_secs(2);

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
                    for o in &mut overlays {
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
                    for o in &mut overlays {
                        apply_settings_to_renderer(&mut o.renderer, &s);
                    }
                    s.fps.max(1) as u64
                };

                if !render_paused.load(Ordering::SeqCst) {
                    while let Ok(samples) = sample_rx.try_recv() {
                        if !samples.is_empty() {
                            last_spectrum = fft.process(&samples, channels, sample_rate);
                            last_spectrum_time = Instant::now();
                        }
                    }
                }

                let spectrum = if render_paused.load(Ordering::SeqCst) {
                    SpectrumData::default()
                } else {
                    let is_idle = last_spectrum_time.elapsed() > idle_timeout;
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
                for o in &mut overlays {
                    if o.renderer.fill_mode == 1 {
                        let min_bw = o.renderer.bar_width + o.renderer.gap_width;
                        o.update_free_regions(min_bw);
                    }
                    unsafe { o.render(&decayed.left_channel, &decayed.right_channel); }
                }
                frame_count += 1;

                if last_z_order.elapsed() >= z_order_interval {
                    for o in &overlays {
                        let thwnd = HWND(o.taskbar().hwnd as *mut _);
                        unsafe { o.ensure_z_order(thwnd, 2); }
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
                    exiting.store(true, Ordering::SeqCst);
                    capture.stop();
                    let _ = render_thread.join();
                    settings.lock().unwrap().save();
                    transparency.restore();
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(action) = action_rx.try_recv() {
            match action {
                TrayAction::TogglePause => {
                    let was = paused.load(Ordering::SeqCst);
                    paused.store(!was, Ordering::SeqCst);
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
                        run_settings_window(
                            settings.clone(),
                            monitor_count,
                            settings_window_open.clone(),
                        );
                    }
                }
                TrayAction::Exit => {
                    exiting.store(true, Ordering::SeqCst);
                    capture.stop();
                    let _ = render_thread.join();
                    settings.lock().unwrap().save();
                    transparency.restore();
                    return;
                }
                TrayAction::TaskbarRestart => {
                    tray.re_add();
                }
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
