// panon.windows — Rust 原生版入口
// 阶段 3：音频捕获 + FFT + 衰减处理 + 任务栏频谱渲染

mod app;
mod audio;
mod overlay;
mod render;
mod settings;
mod taskbar;
mod tray;
mod ui;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
};

use audio::capture::AudioCapture;
use audio::decay::DecayProcessor;
use audio::fft::FftProcessor;
use audio::spectrum::SpectrumData;
use overlay::window::OverlayWindow;
use taskbar::detect::get_taskbar_info;

fn main() {
    // DPI 感知：防御性设置（清单未生效时的兜底），确保 overlay 坐标不被 Windows 虚拟化
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE,
        };
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }

    println!("=== Panon.Windows (Rust) — Phase 2: Taskbar Spectrum ===");

    // 1. 获取任务栏信息
    let taskbar = get_taskbar_info();
    if taskbar.width <= 0 || taskbar.height <= 0 {
        eprintln!("Failed to detect taskbar dimensions");
        std::process::exit(1);
    }
    println!(
        "Taskbar: {}x{} at ({},{}) pos={:?}",
        taskbar.width, taskbar.height, taskbar.x, taskbar.y, taskbar.position
    );

    // 2. 创建覆盖窗口
    let mut overlay = match OverlayWindow::create(&taskbar) {
        Ok(o) => {
            println!(
                "Overlay window created: {}x{}",
                o.width(),
                o.height()
            );
            o
        }
        Err(e) => {
            eprintln!("Failed to create overlay window: {}", e);
            std::process::exit(1);
        }
    };

    // 维护 Z-order：Above 模式（频谱覆盖 taskbar，柱子间透明可看到 taskbar）
    let taskbar_hwnd = windows::Win32::Foundation::HWND(taskbar.hwnd as *mut _);
    unsafe {
        overlay.ensure_z_order(taskbar_hwnd, 2);
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

    // 4. FFT 处理器 + 衰减处理器
    let mut fft = FftProcessor::new();
    fft.set_bass_resolution_level(4);
    fft.set_reduce_bass(true);
    let mut decay = DecayProcessor::new();

    // 5. 主循环
    let mut last_spectrum = SpectrumData::default();
    let mut last_spectrum_time = Instant::now();
    let mut last_render = Instant::now();
    let mut last_z_order = Instant::now();
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    let render_interval = Duration::from_millis(33); // ~30 FPS
    let idle_timeout = Duration::from_millis(200);
    let z_order_interval = Duration::from_secs(2);
    let exit_timeout = Duration::from_millis(800);
    let mut frame_count = 0u64;
    let mut last_debug = Instant::now();
    let mut exiting = false;
    let mut exit_start = Instant::now();

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

        // 退出衰减流程：继续渲染直到频谱归零或超时
        if exiting {
            let silent = SpectrumData::default();
            let decayed = decay.process(&silent);
            unsafe {
                overlay.render(&decayed.left_channel, &decayed.right_channel);
            }

            if decay.is_exit_complete() || exit_start.elapsed() > exit_timeout {
                println!("[exit] decay complete, exiting");
                return;
            }

            std::thread::sleep(Duration::from_millis(16));
            continue;
        }

        // 接收音频数据 → FFT
        while let Ok(samples) = rx.try_recv() {
            if !samples.is_empty() {
                last_spectrum = fft.process(&samples, channels, sample_rate);
                last_spectrum_time = Instant::now();
            }
        }

        // 30 FPS 渲染
        if last_render.elapsed() >= render_interval {
            let is_idle = last_spectrum_time.elapsed() > idle_timeout;
            let spectrum = if is_idle {
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
            };

            // 应用衰减后渲染
            let decayed = decay.process(&spectrum);
            unsafe {
                overlay.render(&decayed.left_channel, &decayed.right_channel);
            }
            last_render = Instant::now();
            frame_count += 1;
        }

        // 定期维护 Z-order（每 2 秒）
        if last_z_order.elapsed() >= z_order_interval {
            unsafe {
                overlay.ensure_z_order(taskbar_hwnd, 2);
            }
            last_z_order = Instant::now();
        }

        // 调试输出（每 3 秒）
        if last_debug.elapsed() >= Duration::from_secs(3) {
            let bars = last_spectrum.left_channel.len();
            let vol = last_spectrum.volume;
            let idle = last_spectrum_time.elapsed() > idle_timeout;
            println!(
                "[debug] frames={} bars={} vol={:.4} idle={} {:?}",
                frame_count, bars, vol, idle, if idle { "silent" } else { "active" }
            );
            last_debug = Instant::now();
        }

        // 避免忙等
        std::thread::sleep(Duration::from_millis(1));
    }
}
