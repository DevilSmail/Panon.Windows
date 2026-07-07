// panon.windows — Rust 原生版入口
// 阶段 1：音频捕获 + FFT 验证

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

use audio::capture::AudioCapture;
use audio::fft::FftProcessor;

const DISPLAY_BARS: usize = 20;

fn main() {
    println!("=== Panon.Windows (Rust) — Phase 1: Audio + FFT ===");
    println!();

    let (tx, rx) = mpsc::channel();

    // 启动 WASAPI Loopback 捕获
    print!("Starting WASAPI loopback capture... ");
    let (mut capture, sample_rate, channels) = match AudioCapture::start(tx) {
        Ok(result) => {
            println!("OK");
            result
        }
        Err(e) => {
            println!("FAILED");
            eprintln!("Error: {}", e);
            eprintln!("Hint: 确保系统有音频输出设备且正在播放声音");
            std::process::exit(1);
        }
    };

    println!("Format: {}Hz {}ch 32-bit float (typical WASAPI shared mode)", sample_rate, channels);
    println!("Press Ctrl+C to exit");
    println!();

    let mut fft = FftProcessor::new();
    fft.set_bass_resolution_level(4);
    fft.set_reduce_bass(true);

    let mut frame_count = 0u64;
    let mut last_print = Instant::now();
    let mut last_spectrum: Option<audio::spectrum::SpectrumData> = None;

    // 接收并处理音频数据
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(samples) => {
                if samples.is_empty() {
                    continue;
                }

                let spectrum = fft.process(&samples, channels, sample_rate);
                frame_count += 1;
                last_spectrum = Some(spectrum);

                // 每 100ms 刷新一次显示
                if last_print.elapsed() >= Duration::from_millis(100) {
                    if let Some(ref spec) = last_spectrum {
                        print_bar_chart(spec, sample_rate, frame_count);
                    }
                    last_print = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 无数据，显示静默状态
                if last_print.elapsed() >= Duration::from_millis(500) {
                    print_silent(frame_count);
                    last_print = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("\nCapture thread disconnected");
                break;
            }
        }
    }

    println!("\nStopping...");
    capture.stop();
    println!("Total frames processed: {}", frame_count);
}

/// 打印静默状态
fn print_silent(frame_count: u64) {
    print!("\x1b[H\x1b[2J"); // 清屏
    println!("=== Panon.Windows Phase 1 === [frames: {}]", frame_count);
    println!();
    println!("  [静默 / 无音频]");
    println!();
    println!("  播放音乐以查看频谱输出...");
}

/// 打印频谱 bar chart
fn print_bar_chart(spectrum: &audio::spectrum::SpectrumData, _sample_rate: u32, frame_count: u64) {
    print!("\x1b[H\x1b[2J"); // 清屏 + 光标到左上

    println!("=== Panon.Windows Phase 1 === [frames: {}]", frame_count);
    println!(
        "RMS: {:.4}  {}  bars: {}",
        spectrum.volume,
        if spectrum.is_silent() { "[SILENT]" } else { "[ACTIVE]" },
        spectrum.bar_count()
    );
    println!();

    // 左声道 20 段 bar chart
    let bars = resample_to_bars(&spectrum.left_channel, DISPLAY_BARS);
    let peak_idx = bars
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, v)| (i, *v))
        .unwrap_or((0, 0.0));

    // 频率范围（bassResolutionLevel=4: 0~1800Hz）
    let (low_freq, high_freq) = (0.0f64, 1800.0f64);

    println!("Left Channel ({} bars, log-scale):", DISPLAY_BARS);
    for (i, &v) in bars.iter().enumerate() {
        let freq = low_freq + (high_freq - low_freq) * (i as f64 + 0.5) / DISPLAY_BARS as f64;
        let bar_width = (v * 40.0) as usize;
        let bar: String = "█".repeat(bar_width);
        let marker = if i == peak_idx.0 { " ◄ PEAK" } else { "" };
        println!(
            "  {:2} {:>6.0}Hz [{:>5.2}] {}{}",
            i + 1,
            freq,
            v,
            bar,
            marker
        );
    }

    println!();
    println!(
        "Peak: bin #{}  magnitude: {:.4}  (~{:.0}Hz)",
        peak_idx.0 + 1,
        peak_idx.1,
        low_freq + (high_freq - low_freq) * (peak_idx.0 as f64 + 0.5) / DISPLAY_BARS as f64
    );

    // 右声道峰值对比
    if !spectrum.right_channel.is_empty() {
        let right_max = spectrum.right_channel.iter().cloned().fold(0.0f32, f32::max);
        println!(
            "Right channel max: {:.4}  (stereo balance: {:.1}%)",
            right_max,
            if spectrum.left_channel.iter().cloned().fold(0.0f32, f32::max) + right_max > 0.0 {
                right_max / (spectrum.left_channel.iter().cloned().fold(0.0f32, f32::max) + right_max) * 100.0
            } else {
                50.0
            }
        );
    }
}

/// 将任意长度的频谱重采样为固定数量的 bar（max-pooling）
fn resample_to_bars(spectrum: &[f32], num_bars: usize) -> Vec<f32> {
    if spectrum.is_empty() || num_bars == 0 {
        return vec![0.0; num_bars];
    }

    let n = spectrum.len();
    let mut result = vec![0.0; num_bars];

    for i in 0..num_bars {
        // 对数间距：低频密集，高频稀疏（模拟标准频谱分析仪）
        let t_start = i as f64 / num_bars as f64;
        let t_end = (i + 1) as f64 / num_bars as f64;

        // 使用 power 1.5 曲线获得视觉上接近 log 的分布
        let start = (t_start.powf(1.5) * n as f64) as usize;
        let end = ((t_end.powf(1.5) * n as f64) as usize).min(n).max(start + 1);

        let mut max = 0.0f32;
        for j in start..end {
            if spectrum[j] > max {
                max = spectrum[j];
            }
        }
        result[i] = max;
    }

    result
}
