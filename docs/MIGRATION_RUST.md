# Panon.Windows → Rust 迁移文档

> 目标：将当前 WinUI 3 / .NET 8 C# 项目迁移为 Rust 原生 Windows 应用
> 单 exe ~5MB，内存基线 30~50MB（无 UIA COM 泄漏，所有权模型保证确定性释放），复制即运行，无需任何运行时

---

## 〇、为什么要迁移

### 当前项目的三个硬伤

| # | 问题 | 原因 | 能否不迁移修复 |
|---|------|------|:---:|
| 1 | **便携部署失败** | WinUI 3 的 Windows App SDK 框架包必须系统级安装（MSIX 部署），无法 xcopy。拷贝到裸机弹"缺少 Windows App Runtime"错误 | ❌ 框架硬限制 |
| 2 | **内存占用 500+MB** | WinUI 3 强制加载 WebView2（~150MB）、AI/ML/ONNX 投影 DLL（~80MB）、.NET GC 堆（~120MB），框架占总内存 90%+ | ❌ 框架硬限制 |
| 3 | **包体积 80MB** | 55 个 DLL（WinAppSDK + NAudio + .NET runtime），而应用代码仅占 ~2MB | ❌ 框架硬限制 |

这三个问题的共同根源：**一个小工具背了一整套重型 UI 框架**。覆盖窗口、托盘、音频、渲染全是纯 Win32 API，框架只服务于一个设置页面。

### 为什么是 Rust

| 候选方案 | 包体积 | 内存 | 真便携 | 设置 UI 还原 | 结论 |
|---------|:---:|:---:|:---:|:---:|------|
| C++ / Win32 | 1-3 MB | 30-50 MB | ✅ | ❌ 需手绘 | 设置窗口成本太高 |
| C# NativeAOT + P/Invoke | 10-15 MB | 80-100 MB | ✅ | ❌ 无可用 UI | 仍带 GC 负担 |
| Go + walk | 10-15 MB | 80-120 MB | ✅ | ❌ 简陋 | Windows GUI 生态弱 |
| WPF (.NET 8) | 40-50 MB | 200-300 MB | ⚠️ 需 .NET | ✅ 完美 | 解决了便携但仍大 |
| **Rust + egui** | **3-5 MB** | **30-50 MB** ✅ | **✅** | **✅ 90%** | **最优** |

Rust 是唯一同时满足四个维度（体积、内存、便携、UI）的方案。核心原因：

1. **`windows-rs`** crate 由微软官方维护，Win32 API 自动生成绑定，WASAPI/UIA/Shell COM 无需手写 FFI
2. **`egui`** 即时模式 GUI 能还原卡片布局、滑块、下拉框、开关等全部控件，且由 wgpu 渲染
3. **零运行时**：编译产物是纯原生 exe，静态链接 CRT，链接系统 DLL（user32.dll/gdi32.dll，任何 Windows 都有）
4. **所有权模型**：编译期排除 use-after-free，音频实时处理不需要 GC，确定性内存释放。COM 接口 wrapper 的 `Drop` 实现保证 `IUIAutomationElement` 等引用在作用域结束时立即 `Release()`，**从根源消除 UIA COM 泄漏**（C# 托管 `AutomationElement` 依赖 GC Finalizer，遍历产生的 50~200 个 COM 引用在 GC 触发前持续堆积）

---

## 一、技术选型

| 项目 | 选择 | 说明 |
|------|------|------|
| 语言 | Rust 1.85+ (stable) | 微软官方 `windows-rs` crate 支撑 |
| UI 框架（设置窗口） | `egui` + `egui-winit` + `egui-wgpu` | 即时模式 GUI，GPU 渲染 |
| Win32 API | `windows` crate (0.58+) | 微软官方，自动生成所有 COM 绑定 |
| 音频捕获 | 直接 WASAPI COM (`windows::Media::Audio`) | 与 NAudio 等价 |
| JSON 序列化 | `serde` + `serde_json` | 替代 `System.Text.Json` |
| FFT | 手写 Cooley-Tukey（不改现有逻辑） | 2048 点，无需外部依赖 |
| 构建产物 | `x86_64-pc-windows-msvc` | 单 exe，静态链接 CRT |

### 构建环境要求

| 组件 | 必需 | 说明 |
|------|:---:|------|
| Rust toolchain (stable) | ✅ | `rustup default stable-msvc` |
| MSVC Build Tools | ✅ | Visual Studio 2022 Build Tools，含 Windows 11 SDK |
| Windows SDK 10.0.22621+ | ✅ | WASAPI/UIA/Shell COM 头文件（Windows 10/11 SDK 自带） |
| Git | 推荐 | 克隆仓库 |

无需额外安装 .NET、WinAppSDK、NAudio 等任何第三方运行时。MSVC Build Tools 是编译任何 Windows 原生程序的基础依赖。

---

## 二、项目结构

```
panon.windows/
├── Cargo.toml
├── build.rs                          # 嵌入图标 (embed icon)
├── assets/
│   └── panon.ico
├── src/
│   ├── main.rs                       # 入口：WinMain + 消息循环
│   ├── app.rs                        # App 生命周期（托盘、覆盖窗口协调）
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── capture.rs                # WASAPI Loopback 捕获（← AudioCaptureService.cs）
│   │   ├── fft.rs                    # Cooley-Tukey FFT（← FftProcessor.cs）
│   │   ├── decay.rs                  # 指数衰减（← DecayProcessor.cs）
│   │   └── spectrum.rs              # SpectrumData 数据模型
│   ├── overlay/
│   │   ├── mod.rs
│   │   └── window.rs                 # 分层覆盖窗口（← LayeredOverlayWindow.cs）
│   ├── render/
│   │   ├── mod.rs
│   │   ├── renderer.rs               # CPU 频谱渲染（← SpectrumRenderer.cs）
│   │   ├── effects.rs               # 7 种视觉效果（← RenderBar1ch/Wave/...）
│   │   └── color.rs                  # HSL/HSLuv 渐变（← ColorProcessor.cs）
│   ├── tray/
│   │   ├── mod.rs
│   │   ├── icon.rs                   # Shell_NotifyIcon（← NativeTrayIcon.cs）
│   │   └── menu.rs                   # 右键菜单（← MessageWindow.ShowContextMenu）
│   ├── taskbar/
│   │   ├── mod.rs
│   │   ├── detect.rs                 # 任务栏位置/范围（← TaskbarHelper.cs）
│   │   └── uia.rs                    # UI Automation 按钮探测（← UiaInterop.cs）
│   ├── settings/
│   │   ├── mod.rs
│   │   ├── config.rs                 # AppSettings 模型 + JSON 读写（← AppSettings.cs + SettingsManager.cs）
│   │   └── transparency.rs           # 注册表透明效果（← TransparencyChecker.cs）
│   └── ui/
│       ├── mod.rs
│       └── settings_window.rs        # egui 设置窗口（← SettingsPage.xaml + .xaml.cs）
└── Cargo.lock
```

---

## 三、Cargo.toml 依赖

```toml
[package]
name = "panon-windows"
version = "1.0.0"
edition = "2021"

[profile.release]
opt-level = "z"       # 体积优先
lto = true            # 链接时优化
codegen-units = 1     # 单一编译单元，最大化优化
strip = true          # 去掉符号
panic = "abort"       # 去掉 unwind 信息

[dependencies]
# Win32 API（微软官方）— WASAPI 音频捕获已验证可用
windows = { version = "0.58", features = [
    # WASAPI 音频
    "Media_Audio",
    # 窗口/消息
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_System_Com",
    # 系统托盘
    "Win32_UI_Shell",
    # UI Automation
    "Win32_UI_Automation",
    # 注册表
    "Win32_System_Registry",
    # 基础
    "Win32_Foundation",
    "Win32_System_Threading",
]}

# 设置窗口 GUI
egui = "0.29"
egui-wgpu = "0.29"
egui-winit = "0.29"
winit = "0.30"
wgpu = "22"

# JSON 配置
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 日志
log = "0.4"
log4rs = "1.3"

# 时间（panic hook 时间戳）
chrono = "0.4"

[build-dependencies]
# 编译时嵌入图标资源
embed-resource = "2.5"
```

---

## 四、模块迁移对照

### 4.1 入口 — `main.rs`

```
C# 等价: App.xaml.cs (OnLaunched / ExitApp)
```

```rust
// main.rs
fn main() {
    // 1. 单实例检查 (Global\Panon.Windows.SingleInstance mutex)
    // 2. 加载设置 (%APPDATA%/Panon/settings.json)
    // 3. 初始化托盘图标
    // 4. 启动 WASAPI 捕获
    // 5. 创建分层覆盖窗口
    // 6. 进入 winit 事件循环（处理托盘消息 + 设置窗口开关）
}
```

**关键差异 vs C#：** 没有 `_hiddenMainWindow` 这个 trick。winit 事件循环本身就是应用主循环，托盘消息线程通过 `PostMessage` 发送到主窗口。

---

### 4.2 音频捕获 — `audio/capture.rs`

```
C# 等价: AudioCaptureService.cs (~160 行)
Rust 预估: ~100 行
```

```rust
use windows::Media::Audio::*;
use windows::Media::Devices::*;

pub struct AudioCapture {
    client: AudioClient,
    capture: AudioCaptureClient,
    event_handle: HANDLE,
    running: AtomicBool,
    wave_format: WaveFormat,      // 48000Hz 2ch 32bit float
    samples_buffer: Vec<f32>,     // 复用池
}

impl AudioCapture {
    pub fn start(&mut self) {
        // 1. ActivateAudioInterfaceAsync → IAudioClient
        // 2. Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, ...)
        // 3. GetBufferSize → buffer_frame_count
        // 4. 创建事件句柄 SetEventHandle
        // 5. Start()
        // 6. 启动捕获线程: WaitForSingleObject → GetBuffer → 拷贝 PCM → ReleaseBuffer
    }

    fn capture_loop(&mut self) {
        // while running {
        //     WaitForSingleObject(event, INFINITE)
        //     GetBuffer(&mut data, &mut frames, &mut flags)
        //     转换 PCM → f32
        //     tx.send(samples)  // 发送到处理通道
        //     ReleaseBuffer(frames)
        // }
    }
}
```

**关键差异：** Rust 没有托管事件 `event Action<float[], WaveFormat>`，改用 `std::sync::mpsc::Sender<SpectrumData>` 管道从捕获线程发送到主线程。

---

### 4.3 FFT 频谱 — `audio/fft.rs`

```
C# 等价: FftProcessor.cs (~240 行)
Rust 预估: ~120 行
```

直接翻译即可，算法完全相同：

```rust
pub struct FftProcessor {
    level: u8,                    // 0-6
    reduce_bass: bool,
    sample_rate: u32,
    // 预分配缓冲区（复用，避免分配）
    left_samples: Vec<f32>,
    right_samples: Vec<f32>,
    windowed: Vec<f32>,           // 2048
    real: Vec<f32>,               // 2048
    imag: Vec<f32>,               // 2048
    magnitudes: Vec<f32>,         // 1024
    spectrum: Vec<f32>,           // 动态长度
}

impl FftProcessor {
    pub fn process(&mut self, samples: &[f32], channels: u16) -> SpectrumData {
        // 左右声道分离（复用 left_samples / right_samples）
        // ComputeSpectrum(left) → 汉宁窗 → Cooley-Tukey FFT → 幅度谱 → 截取 → 归⼀化
        // ComputeSpectrum(right)
        // ComputeRms(samples)
        // 返回 SpectrumData
    }

    fn fft(real: &mut [f32], imag: &mut [f32]) {
        // 位反转 + 蝶形运算（直接从 C# 翻译）
    }
}
```

**关键差异：** `Vec<f32>` 替代 `float[]`，API 不同但逻辑完全对齐。C# 的 `SpectrumUpdated` 事件改为返回 `SpectrumData` 值。

---

### 4.4 衰减处理器 — `audio/decay.rs`

```
C# 等价: DecayProcessor.cs (~125 行)
Rust 预估: ~60 行
```

```rust
pub struct DecayProcessor {
    normal_factor: f32,           // 0.96
    silence_factor: f32,          // 0.75
    exit_factor: f32,             // 0.80
    use_exit: bool,
    silence_threshold: f32,
    min_value: f32,
    prev_left: Vec<f32>,
    prev_right: Vec<f32>,
    result_left: Vec<f32>,        // 复用池
    result_right: Vec<f32>,       // 复用池
}

impl DecayProcessor {
    pub fn process(&mut self, input: &SpectrumData) -> SpectrumData {
        // self.apply_decay(input.left, &mut self.prev_left, &mut self.result_left, input.volume)
        // self.apply_decay(input.right, &mut self.prev_right, &mut self.result_right, input.volume)
        // SpectrumData { left: self.result_left.clone(), ... }
        // 逻辑完全对齐 C# 版
    }
}
```

---

### 4.5 频谱渲染 — `render/renderer.rs` + `render/effects.rs`

```
C# 等价: SpectrumRenderer.cs (~440 行)
Rust 预估: ~350 行
```

```rust
pub struct SpectrumRenderer {
    pub visual_effect: VisualEffect,  // 枚举：Bar1ch, Wave, Solid1ch, ...
    pub gravity: u8,                // 0-4
    pub inversion: bool,
    pub color_space_hsluv: bool,
    pub hsl_hue_from: i32,          // -4000..4000
    pub hsl_hue_to: i32,
    // ... 其他颜色参数
    pub bar_width: u8,
    pub gap_width: u8,
    pub fill_mode: u8,              // 0=铺满, 1=空白区域
    pub free_regions: Vec<(i32, i32)>, // (x, width)

    peak_heights: Vec<f32>,
    resample_idx: Vec<usize>,       // 缓存
    resample_frac: Vec<f32>,
    buffer_l: Vec<f32>,             // 复用池
    buffer_r: Vec<f32>,
    spectrogram_buf: Vec<u32>,      // 频谱瀑布历史
}

impl SpectrumRenderer {
    /// 写入 BGRA 像素缓冲区（DIB Section）
    pub unsafe fn render(&mut self, left: &[f32], right: &[f32],
                          pixels: *mut u32, width: i32, height: i32) {
        // 1. pixels[0..width*height] 清零
        // 2. match visual_effect:
        //    Bar1ch → render_bar1ch(...)
        //    Wave   → render_wave(...)
        //    ...共 7 种
        //    对齐 C# 版每个效果的逻辑
    }
}
```

**7 种效果逐个翻译：**

| C# 方法 | Rust 方法 | 行数 | 关键差异 |
|---------|-----------|------|---------|
| `RenderBar1ch` | `render_bar1ch` | ~80 | `Unsafe.InitBlock` → `ptr::write_bytes` |
| `RenderWave` | `render_wave` | ~20 | 直接翻译 |
| `RenderSolid1ch` | `render_solid1ch` | ~20 | 直接翻译 |
| `RenderSolid` | `render_solid` | ~25 | 直接翻译 |
| `RenderBeam` | `render_beam` | ~20 | 直接翻译 |
| `RenderSpectrogram` | `render_spectrogram` | ~30 | `Array.Copy` → `ptr::copy` |
| `RenderOie1ch` | `render_oie1ch` | ~30 | 直接翻译 |

**像素缓冲区操作：** 全部用 `unsafe { *pixels.add(y * width + x) = color }`，与 C# 的 `uint* pixels` 零抽象层对齐。

---

### 4.6 颜色系统 — `render/color.rs`

```
C# 等价: ColorProcessor.cs (~70 行)
Rust 预估: ~60 行
```

```rust
/// HSL → RGB
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) { ... }

/// HSLuv → RGB（简化实现，对齐 C#）
pub fn hsluv_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) { ... }

/// 渐变颜色（position 0..1）
pub fn gradient_color(pos: f32, use_hsluv: bool,
                      hue_from: i32, hue_to: i32,
                      sat: i32, light: i32) -> (u8, u8, u8) { ... }
```

逻辑完全对齐 C#，纯数学运算，无平台依赖。

---

### 4.7 分层覆盖窗口 — `overlay/window.rs`

```
C# 等价: LayeredOverlayWindow.cs (~700 行)
Rust 预估: ~250 行
```

```rust
pub struct OverlayWindow {
    hwnd: HWND,
    hdc_screen: HDC,
    hdc_mem: HDC,
    h_bitmap: HBITMAP,
    p_bits: *mut u32,            // DIB 像素指针
    width: i32,
    height: i32,

    renderer: SpectrumRenderer,
    decay: DecayProcessor,
    last_spectrum: SpectrumData,
    last_update: Instant,
    taskbar_hwnd: HWND,
    settings_hwnd: HWND,
    overlay_mode: u8,            // 1=Under, 2=Above
    z_order_interval: Duration,
}

impl OverlayWindow {
    pub fn create(taskbar: &TaskbarInfo) -> Self {
        // 1. RegisterClassEx → "Panon_Overlay_2024"
        // 2. CreateWindowEx(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
        // 3. CreateDIBSection (BGRA 32bpp, top-down)
        // 4. 初始化 SpectrumRenderer + DecayProcessor
        // 5. SetLayeredWindowAttributes (per-pixel alpha)
        // 6. 启动 winit 定时器 (30 FPS 渲染)
        // 7. 启动 Z-order 维护定时器 (200ms)
    }

    fn render_tick(&mut self) {
        // 1. 检测静默 (200ms 无音频 → 零值)
        // 2. 空白区域更新 (FillMode=1 → UIA 探测)
        // 3. decay.process(spectrum) → 衰减
        // 4. renderer.render(pixels, width, height) → 写入 BGRA
        // 5. UpdateLayeredWindow(hwnd, ...) → 屏幕显示
    }

    fn ensure_z_order(&self) {
        // BeginDeferWindowPos → DeferWindowPos → EndDeferWindowPos
        // 原子 Z-order 维护，与 C# 版逻辑完全一致
    }
}
```

**关键差异：** `System.Timers.Timer` 替换为 winit 事件循环定时器或 `std::thread::spawn` + `WaitableTimer`。

---

### 4.8 系统托盘 — `tray/icon.rs` + `tray/menu.rs`

```
C# 等价: NativeTrayIcon.cs + MessageWindow.cs + TrayIconController.cs (~580 行)
Rust 预估: ~200 行
```

```rust
pub struct TrayIcon {
    hwnd: HWND,                   // 隐藏消息窗口
    notify_id: u32,
    icon: HICON,
    taskbar_restarted_msg: u32,   // RegisterWindowMessage("TaskbarCreated")
}

impl TrayIcon {
    pub fn create(hinstance: HINSTANCE) -> Self {
        // 1. RegisterClass → "Panon_TrayMsgWnd"
        // 2. CreateWindowEx (隐藏消息窗口)
        // 3. Shell_NotifyIcon(NIM_ADD, ...)
        //     → hIcon = 加载 assets/panon.ico
        //     → uCallbackMessage = WM_APP + 1
        //     → szTip = "Panon"
        // 4. 注册 TaskbarCreated 消息
    }

    fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // match msg {
        //     WM_TRAY_CALLBACK → HandleMessage(wparam, lparam)
        //     TaskbarCreated  → 重建托盘图标 + 重建覆盖窗口
        //     WM_DESTROY      → PostQuitMessage(0)
        // }
    }

    fn show_menu(&self, x: i32, y: i32) {
        // CreatePopupMenu → AppendMenu("⚙ 设置"/"⏸ 暂停"/"✕ 退出")
        // → TrackPopupMenu → 根据返回值触发回调
    }
}
```

**关键差异：** 不需要独立的消息循环线程，所有消息统一由主窗口的 winit 事件循环 + Win32 消息泵处理。`TaskbarCreated` 由隐藏消息窗口的 WindowProc 捕获。

---

### 4.9 任务栏检测 — `taskbar/detect.rs` + `taskbar/uia.rs`

```
C# 等价: TaskbarHelper.cs (~275 行) + UiaInterop.cs (~70 行)
Rust 预估: ~200 行
```

```rust
pub struct TaskbarInfo {
    pub hwnd: HWND,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub monitor_index: u8,
}

pub fn get_taskbar_info() -> TaskbarInfo {
    // FindWindow("Shell_TrayWnd") → SHAppBarMessage(ABM_GETTASKBARPOS)
}

pub fn get_all_taskbars() -> Vec<TaskbarInfo> {
    // 收集 Shell_TrayWnd + Shell_SecondaryTrayWnd
    // 按 X 坐标排序，主显示器移到索引 0
}

pub fn get_free_regions(taskbar_hwnd: HWND, min_bar_width: i32) -> Vec<(i32, i32)> {
    // IUIAutomation::ElementFromHandle → TreeWalker → 遍历所有子元素
    // 收集 BoundingRectangle → 合并重叠 → 计算间隙
    // 逻辑完全对齐 UiaInterop.cs
}
```

**关键差异：** `System.Windows.Automation` 替换为 COM `IUIAutomation` 接口，`windows` crate 提供完整绑定。

---

### 4.10 设置管理 — `settings/config.rs`

```
C# 等价: AppSettings.cs + SettingsManager.cs + TransparencyChecker.cs (~300 行)
Rust 预估: ~120 行
```

```rust
#[derive(Serialize, Deserialize)]
pub struct AppSettings {
    pub reduce_bass: bool,
    pub bass_resolution_level: u8,
    pub fps: u8,
    pub gravity: u8,
    pub inversion: bool,
    pub color_space_hsluv: bool,
    pub hsl_hue_from: i32,
    pub hsl_hue_to: i32,
    pub hsl_saturation: i32,
    pub hsl_lightness: i32,
    pub hsluv_hue_from: i32,
    pub hsluv_hue_to: i32,
    pub hsluv_saturation: i32,
    pub hsluv_lightness: i32,
    pub bar_width: u8,
    pub gap_width: u8,
    pub visual_effect_name: String,
    pub fill_mode: u8,
    pub start_with_windows: bool,
    pub target_monitor: String,
    pub overlay_mode: u8,
}

impl AppSettings {
    pub fn load() -> Self {
        let path = dirs::config_dir().unwrap().join("Panon/settings.json");
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    }
    pub fn save(&self) { ... }
    pub fn update(&mut self, f: impl FnOnce(&mut Self)) { f(self); self.save(); }
}
```

**`serde` + `#[derive(Serialize, Deserialize)]`** 自动处理 JSON 序列化，与 C# `System.Text.Json` 等效，字段名用 `#[serde(rename = "camelCase")]` 保持一致。

---

### 4.11 设置窗口 UI — `ui/settings_window.rs`

```
C# 等价: SettingsPage.xaml + SettingsPage.xaml.cs (~940 行)
Rust 预估: ~500 行
```

这是**变化最大的模块**。XAML 静态布局替换为 egui 即时模式代码布局：

```
XAML                              → egui 代码
─────────────────────────────────────────────────────────────────────
<Page>                            → egui::CentralPanel
<ScrollViewer>                    → egui::ScrollArea
<Border Style="SettingsCard">     → egui::Frame::group() + rounding
<StackPanel>                      → ui.vertical(|ui| { ... })
<TextBlock Style="SectionTitle">  → ui.heading("音频")
<FontIcon Glyph="&#xE9E9;">       → ui.label("🎵") 或 Segoe Fluent Icons
<ToggleSwitch>                    → ui.checkbox() 或自定义开关 widget
<Slider>                          → ui.add(egui::Slider::new(...))
<ComboBox>                        → egui::ComboBox::new(...)
<RadioButtons>                    → ui.radio_value(...)
<Button>                          → ui.button(...)
<TextBlock Style="HintText">      → ui.label(RichText::new(...).small().weak())
```

**卡片样式还原：**

```rust
fn settings_card(ui: &mut Ui, title: &str, icon: &str, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::group(ui.style())
        .rounding(8.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(icon);
                ui.heading(title);
            });
            ui.add_space(4.0);
            add_contents(ui);
        });
}

// 使用
settings_card(ui, "音频", "🎵", |ui| {
    ui.checkbox(&mut settings.reduce_bass, "降低低音权重");
    // ...
});
```

**预设配色下拉：**

```rust
egui::ComboBox::from_label("预设配色")
    .selected_text(PRESET_NAMES[current_preset])
    .show_ui(ui, |ui| {
        for (i, name) in PRESET_NAMES.iter().enumerate() {
            if ui.selectable_value(&mut current_preset, i, *name).clicked() {
                apply_preset(&mut settings, i);
            }
        }
    });
```

**设置窗口独立线程：**

```rust
// winit 事件循环中
if settings_requested {
    let settings_clone = settings.clone();
    std::thread::spawn(move || {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([820.0, 720.0])
                .with_icon(icon_data),
            ..Default::default()
        };
        eframe::run_native("Panon 设置", options, Box::new(|_cc| {
            Ok(Box::new(SettingsApp { settings: settings_clone }))
        })).unwrap();
    });
}
```

---

## 五、线程模型与通信架构

### 5.1 线程划分（4 线程）

```
┌─────────────────────────────────────────────────────────┐
│ Thread 1: MAIN (winit 事件循环 + Win32 消息泵)           │
│   → 托盘消息处理 (WM_TRAY_CALLBACK / TaskbarCreated)      │
│   → 设置窗口生命周期管理 (eframe 子线程)                   │
│   → 退出协调 (通知捕获线程停止 → 等待衰减完成 → 清理)       │
│   → 单实例 Mutex 持有者                                  │
├─────────────────────────────────────────────────────────┤
│ Thread 2: AUDIO CAPTURE (独立 MTA COM 线程)              │
│   → WASAPI Loopback Client → GetBuffer → PCM → f32[]     │
│   → FftProcessor.process() → SpectrumData                │
│   → tx.send(spectrum_data)  // mpsc 发送到渲染线程        │
│   → 循环直到 running=false                               │
├─────────────────────────────────────────────────────────┤
│ Thread 3: RENDER TIMER (WaitableTimer, 30 FPS)          │
│   → 检查 mpsc channel 有无新频谱数据                       │
│   → 超过 200ms 无数据 → 使用 SilentSpectrum               │
│   → DecayProcessor.process() → 衰减                      │
│   → FillMode=1 → get_free_regions()（UIA 探测）           │
│   → SpectrumRenderer.render() → 写入 DIB 像素             │
│   → UpdateLayeredWindow() → 屏幕显示                      │
│   → 每 200ms: EnsureZOrder() 原子层级维护                  │
│   → WaitableTimer 等待下一帧                              │
├─────────────────────────────────────────────────────────┤
│ Thread 4: SETTINGS UI (eframe 独立线程，按需创建)          │
│   → 用户点击托盘"设置" → 创建 eframe 窗口                   │
│   → egui 即时模式渲染设置页面                              │
│   → 关闭 → hide 而非销毁，复用单实例                       │
│   → settings: Arc<RwLock<AppSettings>> 共享状态            │
└─────────────────────────────────────────────────────────┘
```

### 5.2 通信通道

```
AUDIO THREAD                            RENDER THREAD
────────────                            ─────────────
capture loop                            render loop
  │                                       │
  ├─ FftProcessor.process()               ├─ rx.try_recv()  // 非阻塞读取
  ├─ tx.send(spectrum) ──── mpsc ────→    ├─ 有新数据 → 更新 last_spectrum + timestamp
  │                                       ├─ 超 200ms → SilentSpectrum
  │                                       ├─ DecayProcessor.process()
SettingsApp (独立 eframe 线程)             ├─ SpectrumRenderer.render()
  │                                       └─ UpdateLayeredWindow()
  ├─ settings.lock().unwrap()
  ├─ 用户修改                                 ↑
  ├─ settings.save()                   MAIN THREAD
  └─ 写 JSON 到磁盘                       │
                                       ├─ 检测 settings.json 变化
                                       ├─ 重新加载 → 应用到 renderer
                                       └─ 重建 overlay（若显示器切换）
```

### 5.3 设置热生效机制

C# 版通过静态方法 `App.ApplySettingsToAllOverlays()` 即时推送。Rust 版用 `Arc<RwLock<AppSettings>>`：

```
egui 设置窗口                     OverlayWindow
     │                                  │
     ├─ settings.write().unwrap()        │
     ├─ 修改字段                         │
     ├─ 保存 JSON                        │
     └─ drop(write_guard)  ─────────→   │ 下一帧 read().unwrap()
                                         ├─ 比对字段变化
                                         ├─ BarWidth/GapWidth/FillMode 变化 → 重算重采样缓存
                                         ├─ Fps 变化 → 调整定时器间隔
                                         ├─ TargetMonitor 变化 → MAIN 线程重建 overlay
                                         └─ 其他字段 → 直接应用到 renderer
```

### 5.4 应用生命周期

#### 启动

```
main()
  ├─ 1. 单实例检查 Mutex::new("Global\\Panon.Windows.SingleInstance")
  │      → 已被持有 → 激活已有实例窗口 → exit(0)
  ├─ 2. env_logger::init()  // 日志 → %TEMP%/panon_debug.txt
  ├─ 3. AppSettings::load()  // %APPDATA%/Panon/settings.json
  ├─ 4. TransparencyChecker::capture_original()  // 注册表快照
  ├─ 5. 启动 AUDIO THREAD (WASAPI 捕获)
  ├─ 6. 创建 OverlayWindow (分层窗口 + 渲染定时器)
  ├─ 7. 创建 TrayIcon (隐藏消息窗口 + Shell_NotifyIcon)
  ├─ 8. 应用设置到 renderer + decay
  └─ 9. winit 事件循环 ← 阻塞直到退出
```

#### 暂停

```
用户点击托盘"暂停"
  ├─ audio_thread.running = false  // 停止 WASAPI
  ├─ overlay.force_decay()         // 跳过 200ms 静默等待
  ├─ tray.update_tooltip("Panon - 已暂停")
  └─ 频谱平滑衰减到 2px 基线（SilenceFactor=0.75）

恢复
  ├─ audio_thread.running = true   // 重新启动捕获
  └─ tray.update_tooltip("Panon - 运行中")
```

#### 退出

```
用户点击托盘"退出"
  ├─ audio_thread.running = false  // 停止捕获
  ├─ overlay.force_decay(exit=true) // 启用 ExitFactor=0.80 快速衰减
  ├─ 轮询等待 (最多 800ms):
  │    while elapsed < 800ms:
  │      if overlay.max_bar_height < 2px AND overlay.max_peak_height < 2px:
  │        break
  │      sleep(16ms)
  ├─ overlay.dispose()  // 释放 DIB/GDI 资源
  ├─ tray.remove_icon() // NIM_DELETE
  └─ std::process::exit(0)
```

> **对比 C# 版**：C# 用 `TerminateProcess` 硬杀；Rust 版正常退出，无需硬杀，DIB/GDI/COM 资源由 Drop 自动清理。

### 5.5 数据流总览（简化）

```
WASAPI → PCM f32[] → FFT(汉宁窗+Cooley-Tukey) → SpectrumData
                                                      │
                                              mpsc::channel
                                                      ↓
                                              OverlayWindow (30 FPS)
                                                → DecayProcessor
                                                → SpectrumRenderer (7 effects)
                                                → DIB Section (BGRA pixels)
                                                → UpdateLayeredWindow()
                                                      ↓
                                                   屏幕
```

---

## 六、build.rs（嵌入图标）

```rust
// build.rs
fn main() {
    // 将 panon.ico 嵌入 exe 资源段
    // 这样托盘图标和设置窗口图标无需外部文件
    embed_resource::compile("assets/panon.rc", embed_resource::NONE);
    // panon.rc:
    //   1 ICON "panon.ico"
}
```

---

## 七、文件行数预估对比

| 模块 | C# 行数 | Rust 预估 | 变化 |
|------|--------|----------|------|
| 入口/App 协调 | 370 | 80 | 更少（无 `_hiddenMainWindow`、`DispatcherQueue`） |
| 音频捕获 | 160 | 100 | 更少（直接调 COM） |
| FFT 处理 | 240 | 120 | 更少（Vec 比 float[] 更简洁） |
| 衰减 | 125 | 60 | 更少（无 class/event 套壳） |
| 频谱数据 | 38 | 15 | 更少（纯 struct） |
| 渲染器 | 440 | 350 | 略少 |
| 颜色处理 | 70 | 60 | 近似 |
| 分层窗口 | 700 | 250 | 大幅减少 |
| 托盘 | 580 | 200 | 大幅减少 |
| 任务栏检测 | 345 | 200 | 更少 |
| 设置模型 | 300 | 120 | 更少（serde 自动序列化） |
| 设置窗口 | 940 | 500 | 大幅减少 |
| **总计** | **4,308** | **~2,055** | **52% 减少** |

---

## 八、UIA 弹窗误判问题修复

### 问题

`UiaInterop.CollectElementRects` 遍历任务栏所有 UIA 后代元素时，会把展开的控制面板弹窗（网络/声音/输入法）的 BoundingRectangle 当作占用区域，导致空白区域被压缩、频谱消失。

### 修复

```rust
fn collect_element_rects(el: &IUIAutomationElement, taskbar_rect: RECT,
                          tw: i32, result: &mut Vec<(i32, i32)>) {
    let walker = automation.raw_view_walker().unwrap();
    let mut child = walker.get_first_child_element(el).ok();

    while let Some(c) = child {
        if let Ok(rect) = c.current_bounding_rectangle() {
            let cw = rect.width() as i32;
            let cy = rect.top() as i32;

            // 过滤：宽度在合理范围 (0 < w < 80% 任务栏宽)
            // 过滤：Y 坐标在任务栏范围内（排除弹窗）
            if cw > 0
                && cw < tw * 4 / 5                          // < 80% 任务栏宽
                && cy >= taskbar_rect.top                     // 不低于任务栏上缘
                && cy < taskbar_rect.top + taskbar_rect.height() // 不高于任务栏下缘
            {
                let cx = rect.left() as i32 - taskbar_rect.left;
                result.push((cx.max(0), cw.min(tw - cx.max(0))));
            }
        }
        // 递归子元素
        collect_element_rects(&c, taskbar_rect, tw, result);
        child = walker.get_next_sibling_element(&c).ok();
    }
}
```

**改动只需 +2 行**：`cy >= taskbar_rect.top` 和 `cy < taskbar_rect.top + height`。

### COM 引用生命周期：为什么 Rust 没有 UIA 泄漏

**C# 版的泄漏根源：**

`System.Windows.Automation.AutomationElement` 内部持有 `IUIAutomationElement*` COM 指针。释放链为：

```
AutomationElement 托管对象 → GC 触发 → Finalizer → Marshal.ReleaseComObject → COM Release()
```

单次 `CollectElementRects` 递归遍历产生 50~200 个 `AutomationElement` 对象，其 COM 引用 **在遍历结束后不立即释放**，而是等 GC。GC 触发间隔不确定，在这期间 COM 引用累积在堆上，导致工作集膨胀。500ms 缓存降低了调用频率，但没有解决单次遍历的 COM 对象堆积问题。

**Rust 版的确定性释放：**

`windows` crate 为每个 COM 接口类型自动生成 `Drop` 实现：

```rust
// windows-rs 生成的 Drop（简化示意）
impl Drop for IUIAutomationElement {
    fn drop(&mut self) {
        unsafe { (*(*self.ptr).lpVtbl).Release(self.ptr) };
    }
}
```

递归遍历时，wrapper 离开作用域即触发 `Release()`：

```rust
fn collect_element_rects(el: &IUIAutomationElement, ...) {
    let mut child = walker.get_first_child_element(el).ok();   // COM AddRef

    while let Some(c) = child {
        // ... 处理 c（只读，不 clone） ...

        // 递归：传递 &c 引用，不增加引用计数
        collect_element_rects(&c, taskbar_rect, tw, result);

        // 取下一个兄弟 → 旧 child wrapper 被覆盖 → Drop → Release()
        child = walker.get_next_sibling_element(&c).ok();
    }
    // 最后一个 child 在此 Drop → Release()
    // 函数返回时，walker 局部变量 Drop → Release()
}
```

**关键差异：**

| | C# 托管 UIA | Rust `windows` crate |
|---|---|---|
| 释放触发 | GC Finalizer（非确定性） | `Drop` trait（作用域结束立即） |
| 单次遍历 COM 峰值 | 50~200 个引用同时在堆上 | O(深度) ≈ 5~10 个引用在栈上 |
| 遍历结束后 | COM 引用残留，等 GC | **零残留** ✅ |
| 两次遍历之间 | 残留引用持续占用内存 | 全部释放，内存回到基线 |
| 仍需 500ms 缓存？ | ✅ 必须（降低 GC 压力） | ✅ 建议（COM 调用本身有开销） |

**为什么 Rust 遍历中的 COM 峰值也更低：**

C# 的 `while (child != null)` 循环中，每次 `GetNextSibling` 创建**新** `AutomationElement` 赋给 `child`，旧对象变为不可达但不会立即回收，导致：
- 遍历到第 N 个兄弟时，前 N-1 个兄弟的 COM 引用**全部存活**在 GC 堆上
- GC 可能在遍历中途触发（Gen0），但只回收部分

Rust 的 `while let Some(c) = child` 中，`child = walker.get_next_sibling_element(&c)` 覆盖 `child` 绑定时，旧 wrapper **原地 Drop**，COM `Release()` 立即调用。同一时刻只有一个兄弟元素持有 COM 引用。

**结论：** C# 的 UIA 泄漏不是"逻辑 bug"而是"GC 延迟释放"的架构问题。Rust 的所有权模型从根源消除了这个问题——不是"修了泄漏"，而是**泄漏机制根本不存在**。

---

## 九、错误处理与调试日志

### Crash 日志

C# 的 `AppDomain.CurrentDomain.UnhandledException` → Rust 用 panic hook：

```rust
// main.rs 启动时注册
std::panic::set_hook(Box::new(|info| {
    let crash_path = std::env::temp_dir().join("panon_crash.txt");
    let _ = std::fs::write(&crash_path, format!(
        "[{}] PANIC: {}\n{:?}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        info,
        std::backtrace::Backtrace::capture()
    ));
}));
```

### 调试日志

`log4rs` 写文件（循环日志，1MB 上限，对齐 C# `DebugLog`）：

```rust
// main.rs 启动时配置
log4rs::init_file("log4rs.yaml", Default::default()).ok();
// log4rs.yaml:
//   appenders:
//     file:
//       kind: rolling_file
//       path: "%TEMP%/panon_debug.txt"
//       policy:
//         trigger:
//           kind: size
//           limit: 1mb
//       encoder:
//         pattern: "[{d(%Y-%m-%d %H:%M:%S)}] {m}{n}"

// audio/capture.rs 示例
log::debug!("Audio Start OK: {}Hz {}ch", sample_rate, channels);
```

Release 默认只输出 `WARN` 及以上级别，通过环境变量控制：
```
set RUST_LOG=panon=debug
panon-windows.exe
```

---

## 十、C# 版修复记录（迁移前已完成）

以下问题已在 C# 当前版本中修复，Rust 迁移时直接继承正确逻辑，无需再次处理。

| 修复 | 说明 | Rust 对应 |
|------|------|---------|
| **UIA Flyout 弹窗误判** | 点击网络/声音图标 → flyout 弹出 → 频谱消失。根因：flyout 元素 BoundingRectangle 被 UIA 收录。修复：Y+高度双层过滤 + 递归截断 + 三阶段回退（`_stableRegions` / `_lastGoodRegions`） | MIGRATION_RUST.md §八 已分析，Rust `Drop` 确定性释放进一步杜绝 COM 泄漏 |
| **频谱窗口高度控制** | 新增 `MaxHeight` 设置项（0=自动跟随，>0=限制高度），解决某些电脑任务栏偏高导致频谱效果不好的问题。设置页滑块上限动态适配任务栏高度 | 直接翻译 `maxHeightOverride` 参数 + `ui.add(egui::Slider::new(...))` |
| **GC 压力优化** | 音频/FFT/渲染管线的热路径 `new float[]` 全部替换为预分配缓冲区复用；`SpectrumData` 对象复用；`SilentSpectrum` 静态单例 | **Rust 不需要** — `Vec<f32>` 栈上预分配 + 所有权模型天然零分配热路径 |
| **Server GC** | `.csproj` 启用 `<ServerGarbageCollection>true`，降低 GC 对实时音频的停顿影响 | **Rust 不需要** — 无 GC |
| **Release 便携部署** | `WindowsAppSdkSelfContained=true`（Release 构建时 WinAppSDK DLL 打入包内），目标机器无需安装任何运行时 | Rust 天然支持 — 单 exe，静态链接 CRT |

## 十一、当前已知缺陷（迁移时处理）

| 缺陷 | 说明 | Rust 处理 |
|------|------|---------|
| Gravity 3/4（横向）未实现 | 设置提供了"从右到左""从左到右"选项，但 `SpectrumRenderer` 只处理 0/1/2 | 迁移时补全，或从设置下拉中移除；对齐 Linux 原版 |
| `VisualEffectCombo` 不恢复保存值 | 始终默认柱状图 | 迁移时修复：加载后匹配 `visual_effect_name` |
| `DebugLog` 无限增长 | 当前 Release 也会写诊断日志（已用 `#if DEBUG` 包裹大部分诊断输出，但仍有少量） | Rust 版用 `log4rs` level 过滤，Release 默认 `WARN` |

---

## 十二、风险点

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| egui 默认外观不如 WinUI 3 精致 | 中 | 自定义 Style + rounding + spacing 可接近当前效果 |
| 设置窗口独立线程通信 | 低 | `Arc<RwLock<AppSettings>>` + 共享状态 |
| WASAPI COM 初始化 | 低 | `windows-rs` 官方示例已验证 |
| 多显示器一致性 | 低 | 逻辑移植自已验证的 C# 代码 |
| 7 种渲染效果正确性 | 低 | 逐行翻译 + 截屏对比 C# 版 |
| 托盘右键菜单输入焦点 | 低 | `AttachThreadInput` + `SetForegroundWindow`，与 C# 逻辑一致 |
| 设置窗口 eframe 独立线程生命周期 | 低 | 隐藏 (hide) 而非销毁，复用一个实例，避免重复初始化 GPU |

---

## 十三、构建与发布

**开发构建：**
```powershell
cargo build --release
# → target/release/panon-windows.exe (~5 MB)
```

**发布便携版：**
```powershell
copy target\release\panon-windows.exe Panon.Windows_v1.0\
# 单文件，无需其他任何东西
```

**对比：**
```
当前: Panon.Windows_portable/ (80 MB, 57 个文件)
后续: panon-windows.exe        (~5 MB, 1 个文件)
```

---

## 十四、迁移步骤建议

| 阶段 | 内容 | 验证方式 | 状态 |
|------|------|---------|:---:|
| 1 | 搭建项目骨架（Cargo.toml、main.rs、build.rs） | `cargo build` | — |
| 2 | 音频捕获 + FFT → mpsc 管通 | WASAPI 环回 → FFT 频谱控制台打印 bar chart | ✅ POC 已验证 |
| 3 | 分层窗口 + 渲染器（柱状图一种效果） | 任务栏上看到柱子 | — |
| 4 | 全部 7 种效果 + 颜色系统 | 视觉对比 C# 版 | — |
| 5 | 衰减处理器 | 停止音频验证平滑回落 | — |
| 6 | 设置窗口（egui） | 所有控件功能 | — |
| 7 | 系统托盘 + 菜单 + Explorer 重启恢复 | 托盘操作验证 | — |
| 8 | 任务栏检测 + UIA + 空白区域填充 | 多显示器测试 | — |
| 9 | 设置持久化 + 多显示器 + 开机自启 + 透明效果 | 完整功能测试 | — |
| 10 | Release 优化（LTO + strip + abort） | 体积 ≤ 5 MB / 内存基线 30~50 MB | — |

> **POC 结果**（2026-07-03）：阶段 1+2 已验证通过。`windows` 0.58 + `rustfft` 6.4，发布版 1.0 MB，48000Hz 2ch PCM 环回捕获 + 2048pt FFT 正常运行。
