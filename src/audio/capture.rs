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
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;

    // 激活 IAudioClient（泛型由左值类型推断）
    let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    // 获取混合格式（WASAPI 共享模式的输出格式）
    let format_ptr = audio_client.GetMixFormat()?;
    let format = &*format_ptr;
    let sample_rate = format.nSamplesPerSec;
    let channels = format.nChannels;
    let bits_per_sample = format.wBitsPerSample;

    // 通知主线程格式信息
    format_tx.send(Ok((sample_rate, channels))).ok();

    // 初始化（loopback 模式：捕获系统输出）
    audio_client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        0,
        0,
        format_ptr,
        None,
    )?;

    // 释放格式内存（GetMixFormat 用 CoTaskMemAlloc 分配）
    CoTaskMemFree(Some(format_ptr as *const _));

    // 获取捕获客户端（泛型由左值类型推断）
    let capture_client: IAudioCaptureClient = audio_client.GetService()?;

    // 开始捕获
    audio_client.Start()?;

    // 捕获循环
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(5));

        let mut packet_size = capture_client.GetNextPacketSize().unwrap_or(0);
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

                    // 检查静音标志
                    if flags & BUFFERFLAGS_SILENT != 0 {
                        let _ = sample_tx.send(vec![0.0; sample_count]);
                    } else if sample_count > 0 {
                        let samples = convert_to_f32(
                            data_ptr,
                            sample_count,
                            bits_per_sample,
                        );
                        let _ = sample_tx.send(samples);
                    }

                    let _ = capture_client.ReleaseBuffer(frames);
                }
                Err(_) => break,
            }

            packet_size = capture_client.GetNextPacketSize().unwrap_or(0);
        }
    }

    let _ = audio_client.Stop();
    Ok(())
}

/// 将原始音频数据转换为 f32 采样
unsafe fn convert_to_f32(data_ptr: *mut u8, sample_count: usize, bits_per_sample: u16) -> Vec<f32> {
    if bits_per_sample == 32 {
        // IEEE float（WASAPI 共享模式最常见）
        std::slice::from_raw_parts(data_ptr as *const f32, sample_count).to_vec()
    } else if bits_per_sample == 16 {
        // 16-bit PCM
        let raw = std::slice::from_raw_parts(data_ptr as *const i16, sample_count);
        raw.iter().map(|&s| s as f32 / 32768.0).collect()
    } else {
        // 不支持的格式，返回零
        vec![0.0; sample_count]
    }
}
