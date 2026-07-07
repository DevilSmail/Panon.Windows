// panon.windows — Rust 原生版入口
// 阶段 2：音频捕获 + FFT + 任务栏频谱渲染

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
use audio::fft::FftProcessor;
use audio::spectrum::SpectrumData;
use overlay::window::OverlayWindow;
use taskbar::detect::get_taskbar_info;

fn main() {
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

    // 维护 Z-order：Under 模式（taskbar 覆盖频谱，柱子上半部分可见）
    unsafe {
        let taskbar_hwnd = windows::Win32::Foundation::HWND(taskbar.hwnd as *mut _);
        overlay.ensure_z_order(taskbar_hwnd, 1);
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

    // 4. FFT 处理器
    let mut fft = FftProcessor::new();
    fft.set_bass_resolution_level(4);
    fft.set_reduce_bass(true);

    // 5. 主循环
    let mut last_spectrum = SpectrumData::default();
    let mut last_spectrum_time = Instant::now();
    let mut last_render = Instant::now();
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    let render_interval = Duration::from_millis(33); // ~30 FPS
    let idle_timeout = Duration::from_millis(200);

    loop {
        // 处理窗口消息
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    capture.stop();
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
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
                // 静默：零值频谱，让像素清零
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

            unsafe {
                overlay.render(&spectrum.left_channel, &spectrum.right_channel);
            }
            last_render = Instant::now();
        }

        // 避免忙等
        std::thread::sleep(Duration::from_millis(1));
    }
}
