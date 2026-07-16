// capture.rs — WASAPI Loopback 捕获（← AudioCaptureService.cs）
// 在独立 MTA 线程上运行，通过 mpsc::Sender 发送 PCM 采样

use std::ptr;
use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

/// AUDCLNT_BUFFERFLAGS_SILENT 标志位
const BUFFERFLAGS_SILENT: u32 = 0x2;

/// WASAPI Loopback 音频捕获
pub struct AudioCapture {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioCapture {
    /// 启动系统音频捕获
    /// 返回 (AudioCapture, sample_rate, channels)
    /// 在专用 MTA 后台线程上创建 WASAPI 捕获，与 UI 线程的 COM 环境隔离
    pub fn start(sample_tx: Sender<Vec<f32>>) -> windows::core::Result<(Self, u32, u16)> {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let (format_tx, format_rx) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("WASAPI Capture".into())
            .spawn(move || {
                unsafe {
                    capture_thread_proc(running_clone, sample_tx, format_tx);
                }
            })
            .expect("failed to spawn capture thread");

        // 等待格式信息（最多 5 秒）
        let (sample_rate, channels) = format_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| {
                windows::core::Error::new(
                    HRESULT(-1),
                    "WASAPI capture thread did not report format within 5s",
                )
            })?
            .map_err(|e| e)?;

        Ok((Self { running, thread: Some(thread) }, sample_rate, channels))
    }

    /// 停止捕获
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// WASAPI 捕获线程主逻辑
unsafe fn capture_thread_proc(
    running: Arc<AtomicBool>,
    sample_tx: Sender<Vec<f32>>,
    format_tx: mpsc::Sender<windows::core::Result<(u32, u16)>>,
) {
    // COM 初始化（MTA，与 UI 线程隔离）
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

    let result = capture_inner(&running, &sample_tx, &format_tx);

    if let Err(e) = result {
        eprintln!("[capture] FATAL ERROR: {}", e);
        let _ = format_tx.send(Err(e));
    }

    CoUninitialize();
}

unsafe fn capture_inner(
    running: &Arc<AtomicBool>,
    sample_tx: &Sender<Vec<f32>>,
    format_tx: &mpsc::Sender<windows::core::Result<(u32, u16)>>,
) -> windows::core::Result<()> {
    // 创建设备枚举器
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

    // 获取默认渲染端点（系统输出）
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

    // 日志：设备 ID
    if let Ok(id) = device.GetId() {
        println!("[capture] device id: {}", id.display());
    }

    // 激活第一个 IAudioClient（用于 loopback 捕获）
    let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    // 获取混合格式
    let format_ptr = audio_client.GetMixFormat()?;
    let format = &*format_ptr;
    let sample_rate = format.nSamplesPerSec;
    let channels = format.nChannels;
    let bits_per_sample = format.wBitsPerSample;
    let format_tag = format.wFormatTag;

    // 检测是否为 IEEE float 格式
    const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: windows::core::GUID =
        windows::core::GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
    const KSDATAFORMAT_SUBTYPE_PCM: windows::core::GUID =
        windows::core::GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

    let is_float = if format_tag == 65534 {
        let ext_ptr = format_ptr as *const WAVEFORMATEXTENSIBLE;
        let sub_format = ptr::addr_of!((*ext_ptr).SubFormat).read_unaligned();
        if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            true
        } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
            false
        } else {
            bits_per_sample == 32
        }
    } else {
        format_tag == 3
    };

    println!(
        "[capture] format: tag={} {}Hz {}ch {}bits float={}",
        format_tag, sample_rate, channels, bits_per_sample, is_float
    );

    // 通知主线程格式信息
    format_tx.send(Ok((sample_rate, channels))).ok();

    // 初始化第一个 client：loopback 模式
    audio_client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        0,
        0,
        format_ptr,
        None,
    )?;

    // 创建第二个 IAudioClient（辅助渲染流，确保音频引擎处于运行状态）
    // MSDN: 音频引擎只有在运行状态时才会填充 loopback buffer。
    // 创建一个静默的渲染流来激活引擎。
    let render_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
    render_client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        0,
        0,
        0,
        format_ptr,
        None,
    )?;

    // 释放格式内存
    CoTaskMemFree(Some(format_ptr as *const _));

    // 获取捕获客户端
    let capture_client: IAudioCaptureClient = audio_client.GetService()?;

    // 获取渲染客户端（用于写入静默数据保持引擎活跃）
    let render_buffer: IAudioRenderClient = render_client.GetService()?;

    // 日志：buffer 大小
    let buf_size = audio_client.GetBufferSize().unwrap_or(0);
    let render_buf_size = render_client.GetBufferSize().unwrap_or(0);
    println!(
        "[capture] buffer size: capture={}, render={}",
        buf_size, render_buf_size
    );

    // 启动辅助渲染流（写入静默数据，激活音频引擎）
    let render_frames = render_buf_size;
    if let Ok(buf) = render_buffer.GetBuffer(render_frames) {
        // 写入静默（零）
        std::ptr::write_bytes(buf, 0, (render_frames as usize) * (channels as usize) * 4);
        let _ = render_buffer.ReleaseBuffer(render_frames, 1); // AUDCLNT_BUFFERFLAGS_SILENT=1
    }
    render_client.Start()?;

    // 开始捕获
    audio_client.Start()?;

    // 等待音频引擎填充 loopback buffer
    thread::sleep(Duration::from_millis(300));

    // 日志：初始 padding
    if let Ok(padding) = audio_client.GetCurrentPadding() {
        println!("[capture] initial padding: {} frames", padding);
    }
    println!("[capture] started, polling for audio data...");

    let mut loop_count = 0u64;
    let mut total_packets = 0u64;

    // 捕获循环
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(5));
        loop_count += 1;

        let mut packet_size = match capture_client.GetNextPacketSize() {
            Ok(n) => n,
            Err(e) => {
                if loop_count % 600 == 0 {
                    println!("[capture] GetNextPacketSize error: {}", e);
                }
                0
            }
        };

        // 调试：每 3 秒打印状态
        if loop_count % 600 == 0 {
            // 检查 buffer 状态
            let padding = audio_client.GetCurrentPadding().unwrap_or(0);
            let buf_size = audio_client.GetBufferSize().unwrap_or(0);
            println!(
                "[capture] loop={} packets={} packet_size={} buf={} padding={}",
                loop_count, total_packets, packet_size, buf_size, padding
            );
        }

        while packet_size > 0 {
            let mut data_ptr: *mut u8 = ptr::null_mut();
            let mut frames = 0u32;
            let mut flags: u32 = 0;

            match capture_client.GetBuffer(
                &mut data_ptr,
                &mut frames,
                &mut flags,
                None,
                None,
            ) {
                Ok(()) => {
                    let sample_count = frames as usize * channels as usize;
                    total_packets += 1;

                    // 检查静音标志
                    if flags & BUFFERFLAGS_SILENT != 0 {
                        let _ = sample_tx.send(vec![0.0; sample_count]);
                    } else if sample_count > 0 {
                        let samples = convert_to_f32(
                            data_ptr,
                            sample_count,
                            bits_per_sample,
                            is_float,
                        );

                        // 诊断：每 600 次循环打印数据最大绝对值
                        if loop_count % 600 == 0 {
                            let max_abs = samples
                                .iter()
                                .map(|s| s.abs())
                                .fold(0.0f32, f32::max);
                            println!(
                                "[capture] data diag: frames={} samples={} max_abs={:.6} flags={}",
                                frames, sample_count, max_abs, flags
                            );
                        }

                        let _ = sample_tx.send(samples);
                    }

                    let _ = capture_client.ReleaseBuffer(frames);
                }
                Err(e) => {
                    if loop_count % 600 == 0 {
                        println!("[capture] GetBuffer error: {}", e);
                    }
                    break;
                }
            }

            packet_size = match capture_client.GetNextPacketSize() {
                Ok(n) => n,
                Err(_) => 0,
            };
        }
    }

    let _ = render_client.Stop();
    let _ = audio_client.Stop();
    Ok(())
}

/// 将原始音频数据转换为 f32 采样
unsafe fn convert_to_f32(
    data_ptr: *mut u8,
    sample_count: usize,
    bits_per_sample: u16,
    is_float: bool,
) -> Vec<f32> {
    if bits_per_sample == 32 && is_float {
        // 32-bit IEEE float（WASAPI 共享模式最常见）
        std::slice::from_raw_parts(data_ptr as *const f32, sample_count).to_vec()
    } else if bits_per_sample == 32 {
        // 32-bit 整数 PCM
        let raw = std::slice::from_raw_parts(data_ptr as *const i32, sample_count);
        raw.iter().map(|&s| s as f32 / 2147483648.0).collect()
    } else if bits_per_sample == 16 {
        // 16-bit PCM
        let raw = std::slice::from_raw_parts(data_ptr as *const i16, sample_count);
        raw.iter().map(|&s| s as f32 / 32768.0).collect()
    } else {
        // 不支持的格式，返回零
        vec![0.0; sample_count]
    }
}
