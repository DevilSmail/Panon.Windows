# Panon Windows - 项目状态文档

> 更新时间: 2026-07-02 | 目的: 新会话快速恢复上下文 | 状态: 项目完成

---

## 一、项目概述

| 属性 | 值 |
|------|-----|
| **项目名称** | Panon.Windows |
| **目标平台** | Windows 10/11 (x64) |
| **框架** | .NET 8.0 + WinUI 3 (Windows App SDK 2.2.0) |
| **项目路径** | `D:\Python Project\panon\src\Panon.Windows` |
| **运行命令** | `cd src\Panon.Windows && dotnet run --configuration Debug` |
| **调试日志** | `%TEMP%\panon_debug.txt` |

### 原始项目
Panon 是 KDE Plasma 的任务栏音频频谱可视化器（Python + GLSL 着色器）。本项目将其**移植到 Windows**，实现：
- 任务栏区域频谱覆盖显示（频谱与任务栏重叠，任务栏覆盖在频谱上方）
- 7 种视觉效果（CPU 模拟对齐原版 GLSL）
- 仅空白区域填充（UIA 探测任务栏图标位置）
- 系统托盘控制（设置/暂停/退出）
- Explorer 重启自动恢复（托盘 + 覆盖窗口重建）
- 多显示器支持 / 预设配色 / 开机自启 / 系统透明效果

---

## 二、关键架构决策

### 决策 1: 渲染方案 — Win32 分层窗口 + 纯软件渲染（直接写像素）

**为什么不用 WinUI3 Window？**
- WinUI3 Window **无法实现真透明**（DWM 合成层始终有不透明背景）
- WinUI3 Window **无法嵌入任务栏**

**为什么从 Win2D 软件渲染改为纯 GDI+ 软件渲染？**
- Win2D `CanvasDevice` 与 WinUI3 DirectComposition 存在合成层冲突
- 打开设置窗口时 WinUI3 激活会导致 Win2D 设备状态异常
- 改为直接操作 `System.Drawing.Bitmap` 的 `LockBits` 像素内存，完全脱离 D3D/CanvasDevice

**最终方案:**
```
Win32 Layered Window (WS_EX_LAYERED | WS_EX_TOPMOST)
  └─ DIB Section (32bpp BGRA 像素缓冲区)
     └─ SpectrumRenderer.RenderToPixels() (纯软件, 直接写 uint* 像素)
        └─ UpdateLayeredWindow() 每帧更新 → per-pixel alpha 透明
```

**核心文件:** [LayeredOverlayWindow.cs](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs), [SpectrumRenderer.cs](src/Panon.Windows/Shader/SpectrumRenderer.cs)

### 决策 2: 音频捕获 — NAudio WASAPI Loopback

```
系统音频输出 → WASAPI Loopback Capture → PCM float[] → FFT → 频谱数据 → 衰减处理 → 渲染
```

**核心文件:** [AudioCaptureService.cs](src/Panon.Windows/Audio/AudioCaptureService.cs), [FftProcessor.cs](src/Panon.Windows/Audio/FftProcessor.cs), [DecayProcessor.cs](src/Panon.Windows/Audio/DecayProcessor.cs)

### 决策 3: 托盘图标 — Win32 原生 Shell_NotifyIcon

**为什么放弃 H.NotifyIcon？**
- H.NotifyIcon.WinUI 2.1.3 的 `MenuFlyoutItem.Click` 事件**不触发**
- 与 WinUI 3 存在 XAML 编译器兼容性问题（WMC9999 错误）

**最终方案:**
```
Shell_NotifyIcon API (原生)
  └─ MessageWindow (隐藏窗口, 独立线程消息循环)
     └─ WM_TRAY_CALLBACK 处理左键/右键/双击
        └─ TrackPopupMenu 显示右键菜单
```

**核心文件:** [NativeTrayIcon.cs](src/Panon.Windows/Tray/NativeTrayIcon.cs), [MessageWindow.cs](src/Panon.Windows/Tray/MessageWindow.cs), [TrayIconController.cs](src/Panon.Windows/Tray/TrayIconController.cs)

### 决策 4: 设置窗口 — WinUI3 MainWindow

设置窗口使用 WinUI3 (`MainWindow.xaml`)，通过 `DispatcherQueue.TryEnqueue` 在 UI 线程创建。

**关键修复:** 打开设置窗口时调用 `_overlayWindow?.SetSettingsHwnd(settingsHwnd)` 将设置窗口句柄传给 overlay，overlay 在 `EnsureZOrder` 中将自身和 taskbar 重新原子排序，避免设置窗口激活导致频谱被遮挡。

**已尝试但失败的替代方案:**
- WPF 设置窗口 — 与 WinUI3 SDK XAML 编译器冲突
- WinForms 设置窗口 — 与 WinUI3 SDK 编译冲突（MC6000 PresentationFramework 引用错误）
- Microsoft.Windows.Compatibility 包 — 同样有编译冲突

### 决策 5: 渲染定时器 — System.Timers.Timer

**为什么不用 DispatcherTimer？**
- `DispatcherTimer` (WinUI3) 依赖 UI 消息循环
- 在 Win32 分层窗口环境下 **Tick 事件不触发**

**最终方案:** 使用 `System.Timers.Timer`（不依赖任何 UI 线程消息循环）

### 决策 6: 频谱衰减 — 柱身指数衰减 + 峰值线固定减法衰减（双轨）

```
柱身衰减（指数）:
  有音频: NormalFactor=0.96 (每帧保留96%，平滑跟随)
  静音:   SilenceFactor=0.75 (每帧保留75%，约500ms降到3%)
  退出:   ExitFactor=0.80   (每帧保留80%，约300ms降到13%)

峰值线衰减（固定减法，对齐 Linux buffer.frag）:
  PeakDecayValue=0.02 (每帧减0.02，约1.7秒降完)
  算法: peak = max(value, peak - 0.02)
```

**与 Linux 差异:**
| 项 | Linux | Windows | 理由 |
|----|-------|---------|------|
| 柱身衰减 | 无（FFT=0立即消失）| 指数衰减 0.75 | Windows 避免突变，更平滑 |
| 峰值线衰减 | 0.003（11秒）| 0.02（1.7秒）| Windows 调快6.5倍，适配柱身节奏 |
| 衰减算法 | 固定减法 | 固定减法（对齐）| 算法一致，仅参数不同 |

### 决策 7: 无音频状态 — 峰值线在底部2px（对齐 Linux）

**核心思路:** Linux 没有"底部2px待机细线"，待机细线就是 **peak=0 时的峰值线**。

```
柱身: value * height (无最小值，value=0 时柱身消失)
峰值线: 始终绘制，peak=0 时在底部2px = 待机细线
```

**状态变化:**
| 状态 | 柱身 | 峰值线 | 视觉 |
|------|------|--------|------|
| 启动（无音乐）| 不绘制 | 底部2px | 一条彩色细线 |
| 播放音乐 | 生长 | 被顶上去（覆盖柱顶2px）| 柱子+顶部细线 |
| 音乐下降 | 衰减下落 | 缓慢下落（0.02/帧）| 峰值线"顶出"在柱身上方 |
| 最终 | 消失 | 底部2px | 一条彩色细线 |

**关键:** 始终只有一条细线（峰值线），不会出现两条。绘制顺序为**先柱身后峰值线**，确保峰值线覆盖柱顶始终可见。

### 决策 8: 频谱窗口定位 — 与任务栏完全重叠

```
窗口位置 = 任务栏位置 (taskbarInfo.X, taskbarInfo.Y)
窗口大小 = 任务栏大小 (taskbarInfo.Width × taskbarInfo.Height)
Z-order: overlay(TOPMOST) < taskbar(TOPMOST) → 任务栏覆盖在频谱上方
```

频谱默认从任务栏下边缘向上生长（Gravity=South），可通过设置调整为从任务栏上边缘向下生长（Gravity=North），最大高度为任务栏高度。

### 决策 9: Z-order 策略 — TOPMOST + 原子操作 + 定时器 + 设置窗口感知

**当前方案（最新）:**
```
1. overlay 窗口使用 WS_EX_TOPMOST 样式（高于所有普通窗口）
2. taskbar 天生 TOPMOST，始终在 overlay 上面
3. 使用 BeginDeferWindowPos/EndDeferWindowPos 原子操作：
   - 步骤1: overlay → HWND_TOPMOST
   - 步骤2: taskbar → HWND_TOPMOST（推到 overlay 上面）
   - 两步原子提交，无中间状态，不闪烁
4. 500ms 定时器周期性维护 Z-order
5. 打开设置窗口时调用 SetSettingsHwnd() 触发 EnsureZOrder()，恢复 overlay 层级
6. 不使用 WinEvent 钩子（会干扰设置窗口激活）
```

**已尝试但失败的 Z-order 方案:**
| 方案 | 问题 |
|------|------|
| WinEvent 钩子 + SetWindowPos | 钩子回调干扰 WinUI3 窗口激活，设置窗口打不开 |
| HWND_TOP（非 TOPMOST）| 有焦点窗口自动被 Windows 推到最顶层，overlay 被盖住 |
| 两个独立 SetWindowPos | 非原子操作，中间状态导致频谱闪烁 |
| WinEvent 钩子 + 节流 | 仍然干扰设置窗口，且节流不够及时 |

### 决策 10: 柱间间隙 — 整数分配算法（Bresenham 风格）

**问题:** 浮点累加计算柱子位置 `x = i * barWidth` 再四舍五入，会导致柱子宽度和间隙宽度在 N 和 N+1 像素之间交替波动，视觉上间隙宽窄不一。

**解决方案:**
```
1. cellWidth = width / barCount (整数除法)
2. widthRemainder = width % barCount (余数)
3. 前 widthRemainder 个单元每个多 1 像素（最优像素离散化分配）
4. 每个单元内: barW = round(cellWidth × (1 - BarGap))，间隙 = cellWidth - barW
5. 单元位置用整数累加 cellX += currentCellWidth，无浮点误差累积
```

**核心文件:** [SpectrumRenderer.cs:ComputeBarSize()](src/Panon.Windows/Shader/SpectrumRenderer.cs)

### 决策 11: 线程安全 — DebugLog + _spectrumLock

**问题:** 多线程并发使用 `File.AppendAllText` 写入日志文件导致 `IOException`，NAudio 录音回调捕获异常后停止录音，频谱消失且无法恢复。

**解决方案:**
- 创建线程安全的 `DebugLog` 类，使用 `lock` 确保日志写入原子性
- 所有日志调用替换为 `DebugLog.Write`
- 添加 `_spectrumLock` 确保频谱数据线程安全访问

**核心文件:** [DebugLog.cs](src/Panon.Windows/Helpers/DebugLog.cs)

### 决策 12: 柱宽控制方案 — 独立 BarWidth + GapWidth（像素值，铺满任务栏）

**问题:** 之前用 `BarGap`（比例值 0~0.9）控制柱子粗细，无法独立调整柱宽和间隙；且柱子数量由 FFT 分辨率等级固定，用户无法直接控制柱子粗细。

**最终方案:**
```
用户设定:
  BarWidth (像素, 1~30)  — 单个柱子的宽度
  GapWidth  (像素, 0~20) — 相邻柱子之间的间隙

自动计算:
  柱子数 N = (任务栏宽度 + GapWidth) / (BarWidth + GapWidth)
  间隙数 = N - 1（最后一个柱子后无间隙）
  余数 = width - (N * BarWidth + (N-1) * GapWidth)
  前 remainder 个柱子宽度 +1px（精确填满任务栏）

FFT 重采样:
  FFT 输出固定 bar 数量 → 线性插值重采样到 N 个柱子
```

**示例** (1920px 任务栏, BarWidth=7, GapWidth=4):
- 柱子数 N = (1920+4)/11 = 174
- 余数 = 10，前 10 个柱子 8px，后 164 个柱子 7px
- 间隙全部 4px，总宽 = 10×8 + 164×7 + 173×4 = 1920px 精确填满

**核心文件:** [SpectrumRenderer.cs](src/Panon.Windows/Shader/SpectrumRenderer.cs), [AppSettings.cs](src/Panon.Windows/Settings/AppSettings.cs)

### 决策 13: 色彩空间切换 — 独立事件 + UpdateColorSliders

**问题:** 之前所有设置控件共用 `OnSettingChanged`，切换 HSL/HSLuv 后滑块仍显示旧色彩空间的值，导致颜色参数错乱。

**解决方案:**
- 拆分事件：`OnColorSpaceChanged`（色彩空间切换）+ `OnColorSliderChanged`（颜色滑块变化）
- 切换色彩空间时调用 `UpdateColorSliders()` 更新滑块显示对应色彩空间的值
- 颜色滑块变化时根据当前色彩空间写入对应字段（HslXxx 或 HsluvXxx）

**核心文件:** [SettingsPage.xaml.cs](src/Panon.Windows/SettingsPage.xaml.cs)

### 决策 14: 设置页面 UI — 分组 + 说明文字 + 实时数值显示

**问题:** 设置项缺少说明，用户不知道调整后会发生什么变化。

**解决方案:**
- 重新分组：音频 / 显示 / 颜色 / Windows 设置
- 每个控件下方添加灰色小字说明（作用、范围、建议值、适用场景）
- 每个滑块右侧显示当前值（蓝色加粗，如"当前: 7px"），拖动时实时更新
- 下拉选项添加括号说明（如"居中 (从任务栏中线向上下扩展)"）

**核心文件:** [SettingsPage.xaml](src/Panon.Windows/SettingsPage.xaml), [SettingsPage.xaml.cs](src/Panon.Windows/SettingsPage.xaml.cs)

### 决策 15: 配置文件路径 — %APPDATA%\Panon\settings.json（Windows 标准）

**路径:** `%APPDATA%\Panon\settings.json`，展开为 `C:\Users\<用户>\AppData\Roaming\Panon\settings.json`

**为什么用 `%APPDATA%`（Roaming）而非 `%LOCALAPPDATA%`（Local）？**
- `%APPDATA%` 指向用户漫游应用数据目录，是 Windows 存储应用配置的标准位置
- 漫游数据跟随用户账号跨设备同步（域环境），符合 Microsoft 推荐做法
- 与多数 Windows 应用一致（如 VS Code、Discord 等都在此目录存配置）

**代码位置:** [SettingsManager.cs](src/Panon.Windows/Settings/SettingsManager.cs)
```csharp
private static readonly string SettingsPath = Path.Combine(
    Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
    "Panon", "settings.json");
```

**沙箱环境限制（重要）:**
- Trae IDE 沙箱限制文件系统写入权限，`dotnet run` 启动的程序**无法写入** `%APPDATA%\Panon\` 目录
- 现象：修改设置后配置文件修改时间不更新（停留在旧时间），但程序运行时内存中的配置仍然正确
- 解决方案：**直接双击 `Panon.Windows.exe` 运行**以绕过沙箱限制，配置文件才会真正持久化
- 代码逻辑正确，无需修改，这是环境限制而非代码 bug

**防御性修复（已实现）:**
- `SettingsManager.Load` 检测无效字段（如旧 bug 写入的污染值），仅重置无效字段而非整个配置
- 修复后立即调用 `Save()` 写回干净配置（dirty 标记驱动）
- `SettingsPage.OnSettingChanged` 添加 `SelectedIndex >= 0` 防御，避免 ComboBox 未初始化时保存 -1

### 决策 16: 设置项对齐 Linux 版本

**对齐结果:**

| 处理方式 | 设置项 |
|---------|--------|
| **保留并生效** | ReduceBass, BassResolutionLevel, Fps, Gravity, Inversion, BarWidth, GapWidth, 颜色全套 (HSL/HSLuv), OverlayMode |
| **新增** | 随机颜色按钮（对齐 Linux 的 random color 逻辑） |
| **移除（不适用 Windows）** | AutoHide, AutoExtend, PreferredWidth, AnimateAutoHiding, Effect/VisualEffect, hideTooltip |
| **丢弃（Linux 专属）** | pulseaudioDevice, backendIndex, fifoPath, glDFT |

**颜色滑块范围对齐:** 色相范围从 `-720~720` 扩大到 `-4000~4000`（对齐 Linux 版本）

---

## 三、文件结构总览

```
src/Panon.Windows/
├── App.xaml / App.xaml.cs          # 应用入口, 全局生命周期管理
├── MainWindow.xaml / .cs           # WinUI3 主窗口(用于设置页面)
├── SettingsPage.xaml / .cs          # WinUI3 设置页面内容
├── Panon.Windows.csproj            # 项目文件 (net8.0-windows10.0.26100.0)
│
├── Audio/                          # 音频引擎
│   ├── AudioCaptureService.cs      # WASAPI Loopback 捕获
│   ├── FftProcessor.cs             # Cooley-Tukey FFT 频谱分析
│   ├── DecayProcessor.cs           # 指数衰减平滑
│   ├── SpectrumData.cs             # 频谱数据模型
│   └── SpectrumEncoder.cs          # 频谱编码
│
├── Overlay/                        # 任务栏覆盖窗口
│   ├── LayeredOverlayWindow.cs     # ★ 核心: Win32 分层窗口 + 纯软件渲染
│   └── TaskbarOverlayWindow.cs     # (备用) 任务栏覆盖窗口
│
├── Shader/                         # 渲染器 & 着色器
│   ├── SpectrumRenderer.cs         # ★ 核心: 纯软件渲染器 (直接写像素)
│   ├── ColorProcessor.cs           # HSL/HSLuv 颜色渐变计算
│   ├── EffectLoader.cs             # GLSL 着色器加载器
│   ├── EffectInfo.cs               # 效果元数据
│   ├── EffectParameter.cs          # 效果参数
│   └── Shaders/                    # GLSL 着色器文件 (从原项目移植)
│
├── Tray/                           # 系统托盘
│   ├── TrayIconController.cs       # 托盘控制器 (事件分发)
│   ├── NativeTrayIcon.cs           # ★ 核心: Win32 Shell_NotifyIcon 实现
│   └── MessageWindow.cs            # 隐藏消息窗口 (接收托盘回调)
│
├── Helpers/
│   ├── TaskbarHelper.cs            # 任务栏位置/尺寸检测 (SHAppBarMessage)
│   ├── SingleInstance.cs           # 单实例检查 (Mutex)
│   ├── TransparencyChecker.cs     # 透明度检测
│   └── DebugLog.cs                 # ★ 线程安全日志工具 (lock 保护)
│
├── Settings/
│   ├── AppSettings.cs              # ★ 设置数据模型 (所有可配置参数)
│   ├── SettingsManager.cs          # 设置持久化 (JSON) + 污染值检测
│   └── TransparencyChecker.cs     # 系统透明度检测
│
└── Assets/                         # 图标资源
    └── AppIcon.ico                 # 托盘图标
```

---

## 四、核心数据流

```
┌─────────────────────────────────────────────────────────────┐
│                      启动流程 (App.OnLaunched)                │
├─────────────────────────────────────────────────────────────┤
│ 1. 保存 DispatcherQueue (UI线程调度器)                       │
│ 2. 创建隐藏主窗口 (防止设置关闭时退出)                        │
│ 3. 单实例检查 (Mutex)                                       │
│ 4. 初始化 SettingsManager                                    │
│ 5. 初始化 AudioCapture → FftProcessor → DecayProcessor       │
│ 6. 初始化 EffectLoader                                       │
│ 7. 初始化 TrayIconController (独立线程)                     │
│ 8. 创建 LayeredOverlayWindow (分层窗口 + 渲染定时器)          │
│ 9. ApplySettings(settings) → _overlayWindow?.ApplySettings   │
│ 10. 启动音频捕获                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                      运行时数据流                              │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  WASAPI Loopback                                            │
│       ↓                                                     │
│  AudioCaptureService.DataAvailable  (PCM float[])           │
│       ↓                                                     │
│  FftProcessor.Process()            (FFT → 频谱数据)          │
│       ↓                                                     │
│  SpectrumUpdated event                                        │
│       ↓                                                     │
│  DecayProcessor.Process()          (指数衰减平滑)            │
│       ↓                                                     │
│  LayeredOverlayWindow._lastSpectrum (lock _spectrumLock)    │
│       ↓                                                     │
│  System.Timers.Timer (30fps)                                 │
│       ↓                                                     │
│  检查 _lastSpectrumUpdateTime (>200ms 则用零值)              │
│       ↓                                                     │
│  SpectrumRenderer.RenderToPixels() (纯软件 → BGRA像素)       │
│  → 整数分配算法计算柱宽/间隙                                  │
│  → Math.Max(value * height, 2) 保证最小2px细线               │
│       ↓                                                     │
│  UpdateLayeredWindow()           (GDI 分层窗口更新)          │
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                      托盘交互                                  │
├─────────────────────────────────────────────────────────────┤
│  右键托盘 → TrackPopupMenu                                   │
│    ├─ "设置" → OpenSettingsWindow() → DispatcherQueue.TryEnqueue → new MainWindow()
│    │           → _overlayWindow?.SetSettingsHwnd(hwnd) → EnsureZOrder()
│    ├─ "暂停" → TogglePause() → 停止音频 + 隐藏覆盖窗口        │
│    └─ "退出" → ExitApp() → Dispose all + Exit()              │
│                                                             │
│  左键托盘 → OpenSettingsWindow()                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 五、已完成功能

| 功能 | 状态 | 说明 |
|------|------|------|
| 音频捕获 (WASAPI Loopback) | ✅ 完成 | NAudio, 48000Hz 2ch 32bit float |
| FFT 频谱分析 | ✅ 完成 | Cooley-Tukey, 2048点, 汉宁窗 |
| 指数衰减平滑 | ✅ 完成 | NormalFactor=0.96, SilenceFactor=0.75, ExitFactor=0.80 |
| 纯软件渲染 (直接写像素) | ✅ 完成 | 直接操作 DIB Section 像素内存, per-pixel alpha |
| Win32 分层窗口 | ✅ 完成 | WS_EX_LAYERED + WS_EX_TOPMOST + UpdateLayeredWindow |
| 系统托盘 (原生 API) | ✅ 完成 | Shell_NotifyIcon + 右键菜单 |
| 无音频彩色细线 | ✅ 完成 | 峰值线 peak=0 时在底部2px（对齐 Linux），非 Math.Max 方案 |
| 频谱与任务栏重叠 | ✅ 完成 | 窗口位置=任务栏位置，Z-order: taskbar > overlay |
| 频谱方向可调 | ✅ 完成 | South(默认)/North/Center/East/West |
| 设置页面 (WinUI3) | ✅ 完成 | 可打开, 参数修改即时生效 |
| 单实例保护 | ✅ 完成 | Mutex |
| 应用生命周期管理 | ✅ 完成 | 托盘控制, 关闭设置不退出 |
| Z-order 原子操作 | ✅ 完成 | BeginDeferWindowPos/EndDeferWindowPos 防闪烁 |
| **设置窗口打开频谱不消失** | ✅ 完成 | SetSettingsHwnd + EnsureZOrder 恢复层级 |
| **柱宽/间隙独立可配置** | ✅ 完成 | BarWidth (1~30px) + GapWidth (0~20px) 独立滑块, 实时生效 |
| **频谱精确填满任务栏** | ✅ 完成 | 间隙数=N-1 + 余数分配, 无右侧空白 |
| **FFT 重采样** | ✅ 完成 | 线性插值, FFT 固定 bar 数 → 用户设定柱子数 |
| **柱间间隙均匀** | ✅ 完成 | 整数分配算法 (Bresenham 风格) |
| **线程安全日志** | ✅ 完成 | DebugLog 类 (lock 保护) |
| **设置文件污染检测** | ✅ 完成 | SettingsManager.Load 检测无效值并重置 |
| **频谱保持彩色** | ✅ 完成 | 修复设置文件污染 + DispatcherTimer 延迟 _isLoading |
| **色彩空间切换正确** | ✅ 完成 | 独立事件 OnColorSpaceChanged + UpdateColorSliders |
| **随机颜色按钮** | ✅ 完成 | 对齐 Linux 版本, 自动切换 HSLuv |
| **设置项说明文字** | ✅ 完成 | 每个控件下方灰色小字说明 + 实时数值显示 |
| **设置项对齐 Linux** | ✅ 完成 | 移除不适用项, 保留并生效项, 新增随机颜色 |
| **暂停时频谱恢复细线** | ✅ 完成 | 任务12: 移除 Hide()/Show()，依赖衰减自然回落 |
| **退出时频谱平滑衰减** | ✅ 完成 | 任务13: 轮询检测 GetMaxDecayedValue()<0.05 后退出，1200ms 超时兜底 |
| **柱顶峰值细线** | ✅ 完成 | 任务11: 对齐 Linux bar1ch，固定减法衰减0.02，始终绘制，peak=0 时为待机细线 |
| **设置窗口屏幕居中** | ✅ 完成 | 任务19: DisplayArea.GetFromWindowId 获取工作区，AppWindow.Move 居中 |
| **应用图标替换** | ✅ 完成 | 任务5: 三处图标统一替换为 PanonWindows.ico（csproj/标题栏/托盘/任务栏 WM_SETICON） |
| **预设配色方案** | ✅ 完成 | 任务2: 8 套内置预设（彩虹/霓虹/极光/日落/海洋/火焰/森林/紫罗兰）+ 智能匹配 |
| **Center 方向无峰值线** | ✅ 完成 | Center 是 Windows 独有方向，不绘制峰值线，柱身恢复 Math.Max 2px 待机细线 |
| **设置窗口 UI 优化** | ✅ 完成 | 移除重复标题、滚动条贴右边缘、托盘菜单去勾选框列 |
| **颜色设置逻辑修复** | ✅ 完成 | 预设切换屏蔽滑块事件、色彩空间切换直接切自定义（避免匹配到其他预设） |
| **配置保存防御性修复** | ✅ 完成 | SelectedIndex>=0 防御、Load 仅重置无效字段而非整个配置 |
| **设置窗口布局现代化** | ✅ 完成 | 任务7: 卡片化分组（圆角+边框+主题背景）、分组图标（音频/显示/颜色/Windows）、统一间距规范、Windows 11 设置风格 |
| **设置窗口尺寸 & 音频图标优化** | ✅ 完成 | 窗口尺寸 600×600 → 820×720（适配 MaxWidth=720 内容区，垂直更舒展）；"音频"分组图标 Glyph 从 `&#xE799;`（喇叭）改为 `&#xE9E9;`（Equalizer 频谱柱体，更契合可视化主题） |
| **退出卡顿修复（直接运行 .exe）** | ✅ 完成 | 直接运行 .exe 退出慢约10秒的根因：`Environment.Exit(0)` 触发 WinUI3 ProcessExit 缓慢清理 + `_trayIcon.Dispose()` 自死锁1秒。改为 `TerminateProcess` 硬终止 + `RemoveIcon()` 仅移除图标不 Join 线程 |
| **设置页描述文字换行** | ✅ 完成 | 8 个滑块描述+当前值从 `StackPanel Orientation="Horizontal"`（无限宽度，Wrap 不生效）改为 `Grid` 两列（`*`+`Auto`），描述文字受宽度约束自动换行 |
| **退出时峰值线未回落到细线** | ✅ 完成 | 任务13补完：退出检测漏了峰值线（只检测柱身），且峰值线衰减(0.02/帧,1.57s)比柱身(指数0.80,470ms)慢3.5倍。新增 `SpectrumRenderer.UseExitFactor` + `ExitPeakDecayValue=0.08`（400ms落完），`ExitApp` 同时检测柱身和峰值线 |
| **覆盖模式实现** | ✅ 完成 | `LayeredOverlayWindow.ApplySettings` 接入 `OverlayMode` 控制 Z-order。2 种模式：Under（任务栏覆盖在频谱上面，默认）、Above（频谱覆盖在任务栏上面）。设置页切换即时生效。 |

---

## 六、待办事项

> 任务来源：`docs/PanonWindows项目记录.txt`（辅助文件，不加载到会话）
> 任务编号对应 `PanonWindows项目记录.txt` 中的序号，完成后同步标记该文件
> 已完成：任务1（柱体最小宽度）、任务4（设置对齐 Linux）、任务11（柱顶峰值线）、任务12（暂停恢复细线）、任务13（退出卡顿修复）

### P0: Bug 修复（影响体验，最高优先级）✅ 全部完成

#### 任务12: 暂停时频谱恢复细线（而非消失） ✅ 已完成
- **现状:** 托盘点击"暂停"→ `TogglePause()` 停止音频捕获 + 隐藏覆盖窗口，频谱直接消失
- **期望:** 暂停后频谱窗口保留，通过衰减机制自然回落到 2px 彩色细线（待机状态）
- **方案:** 修改 `TogglePause()`，不再隐藏窗口，仅停止音频捕获；依赖现有 `_lastSpectrumUpdateTime` 过期检测（>200ms 用零值）+ 指数衰减，让频谱平滑降落至细线
- **风险:** 低。复用现有衰减逻辑，不引入新代码路径
- **涉及文件:** [App.xaml.cs](src/Panon.Windows/App.xaml.cs) `TogglePause()`、[LayeredOverlayWindow.cs](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs)
- **实现:** 移除 `TogglePause()` 中的 `_overlayWindow?.Hide()`/`Show()` 调用，仅控制音频捕获。窗口保持显示，渲染定时器继续运行，衰减处理器自然把频谱降到 2px 细线。编译通过。

#### 任务13: 退出时频谱卡顿修复 ✅ 已完成
- **现状:** 点击"退出"时频谱冻结在律动高度，直到进程结束才消失
- **期望:** 退出前频谱先平滑回落到细线状态后再退出
- **方案:** `ExitApp()` 中先停止音频捕获，触发 `ForceDecay(useExitFactor: true)`，轮询检测柱身 `GetMaxDecayedValue() < 0.05` **和峰值线** `GetMaxPeakHeight() < 0.05`（对应2px细线），两者都达标后立即退出，800ms 超时兜底
- **风险:** 中。需平衡"退出响应速度"与"视觉过渡"
- **涉及文件:** [App.xaml.cs](src/Panon.Windows/App.xaml.cs) `ExitApp()`、[LayeredOverlayWindow.cs](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs) `ForceDecay()`/`GetMaxPeakHeight()`、[SpectrumRenderer.cs](src/Panon.Windows/Shader/SpectrumRenderer.cs) `UseExitFactor`/`GetMaxPeakHeight()`
- **实现（2026-06-26 完整版）:**
  - **第一阶段（衰减回落）:** `_audioCapture?.Stop()` → `_overlayWindow?.ForceDecay(useExitFactor: true)` 同时启用柱身指数衰减（ExitFactor=0.80，~470ms）和峰值线减法衰减（ExitPeakDecayValue=0.08，~400ms）
  - **第二阶段（双轨检测）:** 轮询 `barMax < 0.05f && peakMax < 0.05f`，两者都达标才退出。之前只检测柱身导致峰值线悬在半空，视觉上没回到细线
  - **第三阶段（硬终止）:** `_trayIcon?.RemoveIcon()` 仅移除图标（不 Join 消息线程，避免自死锁）→ `TerminateProcess(GetCurrentProcess(), 0)` 强制终止进程
  - **退出慢10秒根因:** `Environment.Exit(0)` 会触发 WinUI3 ProcessExit 处理器缓慢清理（~10秒），`dotnet run`/`trae-sandbox` 会更强制终止所以感觉正常。改用 `TerminateProcess` 绕过
  - **自死锁1秒根因:** `ExitApp` 运行在托盘消息线程，`_trayIcon.Dispose()` 内的 `_msgThread.Join(1000)` 是自己 Join 自己，空等1秒。改用 `RemoveIcon()` 仅移除图标

### P1: 核心新功能

#### 任务11: 柱顶峰值细线（对齐 Linux） ✅ 已完成
- **现状:** 与 Linux 版差异 — Linux 有声音时柱顶有"峰值细线"被顶起后缓慢下落，无声音时与底部细线合一
- **期望:** 柱顶叠加独立衰减的峰值线（下落比柱身慢），始终绘制，peak=0 时为待机细线
- **方案:**
  1. `SpectrumRenderer` 维护每柱峰值数组 `_peakHeights[]`，每帧 `peak = max(value, peak - 0.02)`（固定减法衰减，对齐 Linux buffer.frag）
  2. `RenderToPixels` 先绘制柱身，后绘制峰值线（覆盖柱顶2px，确保始终可见）
  3. 柱身无最小值（`value * height`），value=0 时柱身消失，待机细线由峰值线提供
  4. peak=0 时峰值线在底部2px（South）/顶部2px（North）/中心2px（Center）
- **关键决策:** 移除 `PeakLineEnabled` 开关，峰值线始终绘制（对齐 Linux 设计）
- **涉及文件:** [SpectrumRenderer.cs](src/Panon.Windows/Shader/SpectrumRenderer.cs)
- **实现:** 完成。峰值线衰减参数从 0.003（Linux默认，11秒）调快到 0.02（约1.7秒），适配 Windows 柱身衰减节奏（500ms）。

#### 任务19: 设置窗口屏幕居中 ✅ 已完成
- **现状:** 打开设置窗口时未居中屏幕，位置可能偏移
- **期望:** 设置窗口在屏幕居中显示
- **方案:** 使用 WinUI3 原生 `DisplayArea` API 获取屏幕工作区，计算居中位置
- **涉及文件:** [MainWindow.xaml.cs](src/Panon.Windows/MainWindow.xaml.cs)
- **实现:** 完成。使用 `DisplayArea.GetFromWindowId` 获取工作区（自动排除任务栏），`AppWindow.Move` 居中。同时修复 Center 方向峰值线问题（Center 是 Windows 独有方向，不绘制峰值线，柱身恢复 Math.Max 2px 待机细线）。

#### 任务3: 任务栏空白区域填充（P1 核心功能）✅ 已完成
- **状态:** 已完成（2026-07-01）。使用 UI Automation (System.Windows.Automation) 探测 Win11 任务栏按钮位置（XAML 渲染，无独立 HWND），500ms 缓存刷新，TreeWalker.RawViewWalker 递归遍历。12 种视觉效果全部接入 FillMode 裁剪。
- **实现方案:**
  - `UiaInterop.GetTaskbarButtonRects()` — UIA 遍历任务栏子树，收集所有非容器元素 BoundingRectangle
  - `TaskbarHelper` — 500ms 缓存 + 合并重叠 → 计算空白区域
  - `SpectrumRenderer.GetEffectiveRegions()` / `IsColumnVisible()` — 统一 FillMode 入口
  - FillMode 默认值改为 1（仅空白区域）
  - 新增 `FrameworkReference Include="Microsoft.WindowsDesktop.App"` 提供 UIAutomation 托管 API

### P2: 功能增强

#### 任务2: 预设配色方案 + 自定义增加 ✅ 已完成
- **期望:** 内置几套现代化配色（如霓虹/极光/日落/海洋等），用户可保存自定义配色
- **方案:**
  1. `AppSettings` 新增 `Presets` 列表（内置 + 自定义）
  2. 设置页新增预设下拉/卡片选择，点击应用预设颜色
  3. "保存当前为预设"按钮，命名后存入自定义列表
- **涉及文件:** [AppSettings.cs](src/Panon.Windows/Settings/AppSettings.cs)、[SettingsPage.xaml](src/Panon.Windows/SettingsPage.xaml)

#### 任务5: 应用图标替换 ✅ 已完成
- **期望:** 使用项目中已有的图片/图标替换当前 `Assets/AppIcon.ico`
- **实现:** 完成。三处图标引用统一替换为 `PanonWindows.ico`：
  - csproj: `<ApplicationIcon>Assets\PanonWindows.ico</ApplicationIcon>` + `<Content Include>`
  - MainWindow.xaml: `TitleBar.IconSource`（后移除 TitleBar 控件，改用系统标题栏）
  - NativeTrayIcon.cs: `LoadImage` 加载 ico 文件，失败回退 `IDI_APPLICATION`
  - MainWindow.xaml.cs: `WM_SETICON` 设置窗口任务栏图标（大/小图标）
- **涉及文件:** [Panon.Windows.csproj](src/Panon.Windows/Panon.Windows.csproj)、[MainWindow.xaml](src/Panon.Windows/MainWindow.xaml)、[MainWindow.xaml.cs](src/Panon.Windows/MainWindow.xaml.cs)、[NativeTrayIcon.cs](src/Panon.Windows/Tray/NativeTrayIcon.cs)

#### 任务14: 开机自启选项 ✅ 已完成
- **现状:** `AppSettings.StartWithWindows` 字段已预留，设置页 ToggleSwitch 已就位，未实现注册表写入逻辑
- **实现:**
  1. `SyncStartWithWindowsFromRegistry()` 启动时从注册表读取实际状态（不受 settings.json 影响，因为用户可能通过任务管理器删除启动项）
  2. `UpdateStartWithWindows(bool)` 开启时写入 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Panon` = `"exe路径"`，关闭时删除该键
  3. 设置页 ToggleSwitch 切换时即时写注册表
- **涉及文件:** [SettingsPage.xaml.cs](src/Panon.Windows/SettingsPage.xaml.cs)

#### 启动透明度检查 ⏭️ 已改为设置页 ToggleSwitch 配置项 ✅ 已完成
- **交互逻辑（2026-06-26 最终版）:** 
  - 启动时调用 `CaptureOriginalState()` 记录原始注册表快照（首次快照，后续启动不覆盖）
  - 不再拦截启动流程，频谱正常显示
  - 设置页「Windows 设置」卡片新增「开启系统透明效果」ToggleSwitch：
    - 列出两项透明度的实时开启/关闭状态（✅/❌）
    - **开启时:** 写入注册表 `EnableTransparency=1` + `UseOLEDTaskbarTransparency=1`
    - **关闭时:** 恢复原始注册表值（键值恢复原样，键不存在则删除）
    - 说明文字提示当前效果和开关影响
  - 卸载/安装友好：`Disable()` 还原原始注册表，不留痕迹
- **涉及文件:** [App.xaml.cs](src/Panon.Windows/App.xaml.cs)、[TransparencyChecker.cs](src/Panon.Windows/Settings/TransparencyChecker.cs)、[SettingsPage.xaml](src/Panon.Windows/SettingsPage.xaml)、[SettingsPage.xaml.cs](src/Panon.Windows/SettingsPage.xaml.cs)

#### 任务15: 多显示器支持（P2 功能增强） ✅ 已完成
- **实现时间:** 2026-06-30
- **实现概要:**
  - `TaskbarInfo` 新增 `TaskbarHwnd`、`MonitorIndex` 字段
  - `TaskbarHelper.GetAllTaskbarInfos()` — 通过 `FindWindow(Shell_TrayWnd)` + `FindWindowEx(Shell_SecondaryTrayWnd)` 枚举所有任务栏窗口，`MonitorFromWindow` 映射显示器
  - `TaskbarHelper.GetTaskbarInfoByIndex(int)` — 按索引获取指定显示器任务栏
  - `LayeredOverlayWindow.Create(TaskbarInfo?)` — 接受任务栏信息参数，定位到指定显示器
  - 每个 overlay 拥有独立的 `DecayProcessor`（避免多 overlay 共享衰减状态冲突）
  - `App.xaml.cs._overlayWindow` → `_overlayWindows: List<LayeredOverlayWindow>`
  - `App.CreateOverlays(targetMonitor)` / `DestroyOverlays()` / `RecreateOverlays()` — 管理多 overlay 生命周期
  - 设置页 `OnSettingChanged` 检测 `TargetMonitor` 变化时调用 `RecreateOverlays()` 即时重建
  - 所有 overlay 引用点（ApplySettings/TogglePause/ExitApp/OnSettingsClosing/OpenSettingsWindow）改为遍历列表
  - `ExitApp` 检测所有 overlay 衰减完成后退出
- **涉及文件:** [TaskbarHelper.cs](src/Panon.Windows/Helpers/TaskbarHelper.cs)、[App.xaml.cs](src/Panon.Windows/App.xaml.cs)、[LayeredOverlayWindow.cs](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs)、[SettingsPage.xaml.cs](src/Panon.Windows/SettingsPage.xaml.cs)

#### 任务16: GLSL 着色器集成（CPU 软件模拟） ✅ 已完成
- **已实现 12 种效果（对齐 Linux 全部 .frag）：**
  - `bar1ch` 柱状图（含峰值线，已有实现）
  - `hill1ch` 山丘（相邻频率高斯衰减，连绵山峰）
  - `wave` 波浪（单条采样曲线）
  - `solid1ch` 实心单声道（连续填充）
  - `solid` 实心立体声（左右声道对称填充）
  - `dune1ch` 沙丘（随机粒子 + 山丘高度）
  - `beam` 光束（alpha 混合柱状）
  - `blur1ch` 模糊柱状（10 邻域平均）
  - `chain` 链条（随机粒子连接）
  - `spectrogram` 频谱瀑布（帧缓冲向下滚动）
  - `oie1ch` Oie 连线（相邻采样点折线）
  - `default` 多层（5 层缩放叠加）
- **涉及文件:** [SpectrumRenderer.cs](src/Panon.Windows/Shader/SpectrumRenderer.cs)、[LayeredOverlayWindow.cs](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs)、[SettingsPage.xaml](src/Panon.Windows/SettingsPage.xaml)

#### 任务18: 配置文件路径调整 ⏭️ 无需调整
- **现状:** 配置文件位于 `C:\Users\<用户>\AppData\Roaming\Panon\settings.json`（`Environment.SpecialFolder.ApplicationData`）
- **分析结论:** 当前路径 `%APPDATA%\Panon\` 是 **Windows 推荐的标准做法**，无需调整：
  - ✅ 用户有完全读写权限（不需要管理员权限）
  - ✅ 每个用户独立配置（多用户系统互不干扰）
  - ✅ 符合 Windows 应用规范（Roaming 目录支持域用户漫游）
  - ✅ 打包分发后，任何用户安装运行都能正常首次创建，无权限问题
  - ✅ 程序首次启动时自动创建（`Save()` 调用 `Directory.CreateDirectory`）
- **调整反而有风险:**
  - 程序目录下：安装到 `C:\Program Files\` 时普通用户无写入权限，Save 会失败
  - 用户根目录：可行但与 Windows 规范不符
- **涉及文件:** [SettingsManager.cs](src/Panon.Windows/Settings/SettingsManager.cs) `SettingsPath` 属性（不修改）

### P3: 代码质量与维护（需先分析再实施）

#### 任务6: 代码冗余优化分析 ✅ 已完成
- **期望:** 扫描全项目，识别冗余/不合理写法，输出优化清单（不直接改，不影响功能）
- **方法:** 静态审查 + 输出报告
- **实现:** 已完成（2026-07-01）。双 agent 并行扫描（未使用代码 + 重复代码），清理 32 项冗余（4 个删除文件、4 个 NuGet 包、6 个字段/P/Invoke、4 个方法、4 个 using、10 个废弃属性）

#### 任务7: 设置窗口 UI 现代化 ✅ 已完成
- **期望:** 优化布局、图标、说明文字，参考 Windows 11 设置页面风格，让说明易懂
- **依赖:** 可与任务2、任务11、任务14 的设置页改动合并
- **实现:** 完成。纯 XAML 重构，未改动 `SettingsPage.xaml.cs`，所有 `x:Name` 控件和事件绑定完整保留，功能零影响：
  - **卡片化分组:** 4 个分组（音频/显示/颜色/Windows）各用 `Border` 包裹，`CornerRadius=8` + `CardBackgroundFillColorDefaultBrush` 主题背景 + `CardStrokeColorDefaultBrush` 细边框
  - **分组图标:** Segoe Fluent Icons 字体图标（音频 `&#xE9E9;` Equalizer 频谱柱体（原 `&#xE799;` 喇叭，2026-06-26 替换为更契合可视化主题）/ 显示 `&#xE7F4;` / 颜色 `&#xE790;` / Windows `&#xE783;`），强调色（AccentTextFillColorPrimaryBrush）
  - **间距规范:** 外层 `Padding=24,20` + `Spacing=12`，卡片内 `Padding=16,14` + `Spacing=14`，设置项 `Spacing=2`
  - **样式资源:** 新增 `SettingsCard` / `SectionTitle` / `SectionIcon` / `SettingItem` 样式，统一视觉规范
  - **MaxWidth:** 从 700 调整为 720，留更多呼吸空间
  - **明暗主题适配:** 全部使用 `ThemeResource`，自动跟随系统主题

#### 任务8: 可删除文件排查 ✅ 已完成
- **实现:** 2026-07-01。删除 panon/、plasmoid/、translations/、venv/、third_party/（子模块）、*.sh 脚本、Resources/(空)、UI/(空)、DEVELOPMENT.md。清理 .gitmodules、.gitignore、csproj Shaders CopyToOutput

#### 任务9: 打包体积分析 ⏸️ 已分析，暂缓
- **分析结论:** 144 MB = WinUI3 强制自包含 (30 MB) + WPF/WinForms (35 MB，UIAutomation 依赖链) + AI/ML (39 MB) + 运行时 (40 MB)
- **唯一可行优化:** 改用原生 UIA COM Interop 替换 `System.Windows.Automation` 托管 API → 省 ~35 MB。需要完整声明 58 个 vtable 方法（工作量大，暂缓）
- **已尝试失败的方案:** 框架依赖发布（WinUI3 非打包不支持）、排除 DLL（WPF 是 UIA 运行时依赖）

#### 任务10: 完善 README ✅ 已完成
- **期望:** 注明基于原 Panon 项目修改，参考链接、Windows 适配说明
- **实现:** 2026-07-01。删除 README.org，新建 README.md（中文，含平台支持/功能/技术栈/安装/设置/差异对比/致谢/许可证）+ 更新 LICENSE 文件添加版权头

### P4: 性能优化

#### 任务17: 性能优化
- 像素填充循环可考虑 SIMD 优化（System.Numerics.Vectors）
- 当前 30fps，可测试 60fps 的性能表现（CPU 开销约 10-15%，仍在安全范围）
- FFT 重采样可考虑缓存（柱子数不变时跳过 Resample）
- 当前每帧 ~2ms @30fps = ~6% 单核 CPU，优化空间有限但可做

---

### 任务执行顺序建议
```
第一批（P0 Bug修复）: 任务12 ✅ → 任务13 ✅
第二批（P0 启动强制检测 → 设置页配置）: 启动透明度检查 ✅
第三批（P1 核心功能）: 任务11 ✅ → 任务19 ✅ → 任务3 ✅
第四批（P2 功能增强）: 任务2 ✅ → 任务5 ✅ → 任务14 ✅ → 任务16 ✅ → 任务15 ✅ → 任务17 → 任务18 ⏭️
第五批（P3 代码质量）: 任务6 ✅ → 任务7 ✅ → 任务8 ✅ → 任务10 ✅ → 任务9

**最终状态（2026-07-02）:**
- 全部 19 项任务，18 项完成，1 项暂缓（任务9 打包体积——WinUI3 技术限制）
- Explorer 重启自动恢复（TaskbarCreated 消息 → 托盘重注册 + overlay 重建）
- P3/P4 代码质量优化（删 5 隐藏效果、去重 Log、死分支修复、TaskbarHelper 缓存、空 catch 加日志）
- 已删除 Linux 遗留代码（panon/plasmoid/translations/venv/third_party）、死代码文件、无用 NuGet 包
- csproj 清理（去 MSIX 死块、去 Shader 拷贝、去无用资产）

### 新增配置项定义

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `VisualEffectName` | `string` | `"bar1ch"` | 图形效果：bar1ch/wave/solid1ch/solid/beam/spectrogram/oie1ch |
| `FillMode` | `int` | `1` | 填充模式: 0=铺满任务栏, 1=仅空白区域(默认) |
| `TargetMonitor` | `string` | `"0"` | 目标显示器: "0"=主显示器, "-1"=所有 |
| `StartWithWindows` | `bool` | `false` | 开机自启 |
- Windows 卡片 → 开机自启 ToggleSwitch

---

## 七、重要文件修改记录

### SpectrumRenderer.cs — 频谱渲染器（核心，修改最多）

**修改历史:**

1. **从 Win2D 改为纯软件渲染** — 移除 CanvasRenderTarget/CreateDrawingSession，直接操作 `uint*` 像素内存，避免 D3D/CanvasDevice 与 WinUI3 合成层冲突
2. **移除 hasAudio 参数** — 不再用二值开关控制细线/频谱
3. **平滑过渡** — `Math.Max(value * height, 2)` 始终用衰减值渲染，最小 2px（已废弃，改为峰值线方案）
4. **添加 BarGap 配置** — 柱间缝隙比例 (0.0~1.0)，0=无缝隙（已废弃，改为 BarWidth+GapWidth）
5. **整数分配算法** — 解决柱间间隙不均匀问题，使用 Bresenham 风格的整数分配
6. **改为 BarWidth + GapWidth 像素值控制** — 用户独立设定柱宽和间隙，柱子数自动计算填满任务栏
7. **添加 Resample 线性插值** — FFT 固定 bar 数 → 用户设定柱子数
8. **间隙数 = N-1 + 余数分配** — 修复任务栏右侧空白问题，精确填满任务栏宽度
9. **柱顶峰值细线（对齐 Linux）** — 新增 `_peakHeights[]` 数组，固定减法衰减 `peak = max(value, peak - 0.02)`，始终绘制（peak=0 时为底部2px待机细线）
10. **绘制顺序调整** — 先柱身后峰值线，确保峰值线覆盖柱顶始终可见
11. **柱身最小值移除** — 从 `Math.Max(value * height, 2)` 改为 `value * height`，待机细线由峰值线提供（对齐 Linux 设计）
12. **退出专用峰值衰减（2026-06-26）** — 新增 `UseExitFactor` 标志 + `ExitPeakDecayValue = 0.08f`（正常 `PeakDecayValue=0.02` 的 4 倍速度），退出时峰值线 ~400ms 落完，与柱身指数衰减（ExitFactor=0.80, ~470ms）同步到达细线。之前峰值线衰减(1.57s)比柱身(470ms)慢3.5倍，柱身落到底时峰值线还悬在半空
13. **添加 GetMaxPeakHeight()（2026-06-26）** — 读取 `_peakHeights[]` 最大值，供 `ExitApp` 双轨检测（柱身+峰值线）使用

**当前关键代码:**
```csharp
public int BarWidth { get; set; } = 7;   // 柱子像素宽度
public int GapWidth { get; set; } = 4;   // 间隙像素宽度

/// <summary>峰值固定减法衰减值（对齐 Linux bar1ch/buffer.frag 的固定减法算法）</summary>
/// <remarks>Linux 默认 0.003（11秒落完），Windows 调快到 0.02（约1.7秒），适配 Windows 柱身衰减节奏</remarks>
private const float PeakDecayValue = 0.02f;

/// <summary>退出专用峰值衰减值（极快衰减，与柱身 ExitFactor 节奏匹配，约 400ms 落完）</summary>
private const float ExitPeakDecayValue = 0.08f;

/// <summary>是否启用退出专用峰值衰减（退出时设为 true，加速峰值线回落）</summary>
public bool UseExitFactor { get; set; } = false;

private float[] _peakHeights = Array.Empty<float>();  // 每柱峰值高度（0~1）

public unsafe void RenderToPixels(float[] left, float[] right, IntPtr pBits, int width, int height)
{
    // ... 整数分配算法计算柱宽/间隙
    // ... FFT 重采样到 targetBarCount

    // 确保峰值数组长度匹配
    if (_peakHeights.Length != targetBarCount)
        _peakHeights = new float[targetBarCount];

    for (int i = 0; i < targetBarCount; i++)
    {
        float value = resampled[i];
        float barHeight = value * height;

        // 1. 先绘制柱身（value=0 时柱身消失，不绘制）
        if (barHeight >= 0.5f)
        {
            // ... 填充柱身像素
        }

        // 2. 后绘制峰值线（始终绘制，覆盖柱顶2px）
        // 更新峰值：声音大时立即跟上，声音小时固定减法衰减（对齐 Linux buffer.frag）
        if (value > _peakHeights[i])
            _peakHeights[i] = value;
        else
            _peakHeights[i] = Math.Max(0, _peakHeights[i] - PeakDecayValue);

        float peakHeight = _peakHeights[i] * height;
        int peakLineThickness = 2;
        // 根据 Gravity 计算 peakYStart/peakYEnd（peak=0 时在底部2px）
        // ... 填充峰值线像素
    }
}
```

### LayeredOverlayWindow.cs — 频谱窗口核心

**修改历史:**

1. **窗口定位** — 与任务栏完全重叠 (`taskbarInfo.X, taskbarInfo.Y, taskbarInfo.Width, taskbarInfo.Height`)
2. **Z-order 策略演进:**
   - v1: `SetWindowPos(hwnd, taskbarHwnd, ...)` → overlay 低于普通窗口
   - v2: WinEvent 钩子 + `SetWindowPos(HWND_TOPMOST)` → 干扰设置窗口
   - v3: WinEvent 钩子 + 500ms 节流 → 仍干扰设置窗口
   - v4: `HWND_TOP` + 定时器 → 有焦点窗口覆盖 overlay
   - v5: `WS_EX_TOPMOST` + `BeginDeferWindowPos` 原子操作 + 500ms 定时器（当前）
3. **移除 WinEvent 钩子** — 避免干扰设置窗口激活
4. **添加 _spectrumLock** — 确保频谱数据线程安全访问
5. **添加 _lastSpectrumUpdateTime** — 检测频谱数据过期（>200ms 用零值）
6. **添加 SetSettingsHwnd** — 打开设置窗口时传入句柄，触发 EnsureZOrder 恢复层级
7. **从 Win2D 渲染改为纯软件渲染** — 直接调用 `SpectrumRenderer.RenderToPixels`
8. **添加 ForceDecay()** — 退出时强制触发衰减，让频谱平滑回落到细线状态
9. **添加 GetMaxDecayedValue()** — 获取当前频谱最大衰减值，用于退出时检测是否已回落到细线
10. **ForceDecay 传播峰值线退出因子** — `ForceDecay(useExitFactor: true)` 同时设置 `_renderer.UseExitFactor = true`，让峰值线与柱身同步快速回落
11. **添加 GetMaxPeakHeight()** — 委托 `_renderer.GetMaxPeakHeight()`，用于退出时检测峰值线是否已回落到 2px 细线

**当前关键代码:**
```csharp
// 窗口样式
WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE

// Z-order 原子操作
private void EnsureZOrder()
{
    var dwp = BeginDeferWindowPos(2);
    dwp = DeferWindowPos(dwp, _hwnd, HWND_TOPMOST, ...);      // overlay → TOPMOST
    dwp = DeferWindowPos(dwp, _taskbarHwnd, HWND_TOPMOST, ...); // taskbar → TOPMOST（更高）
    EndDeferWindowPos(dwp);
}

// 频谱数据线程安全 + 过期检测
private void OnUpdateTick(object? sender, ElapsedEventArgs e)
{
    SpectrumData currentSpectrum;
    lock (_spectrumLock)
    {
        if (_lastSpectrumUpdateTime != DateTime.MinValue &&
            (DateTime.Now - _lastSpectrumUpdateTime).TotalMilliseconds > 200)
        {
            currentSpectrum = new SpectrumData { /* 零值 */ };
        }
        else
        {
            currentSpectrum = _lastSpectrum;
        }
    }
    // ...
}

public void SetSettingsHwnd(IntPtr hwnd)
{
    _settingsHwnd = hwnd;
    if (_hwnd != IntPtr.Zero) EnsureZOrder();
}
```

### App.xaml.cs — 应用入口

**修改历史:**

1. **OpenSettingsWindow** — 打开设置后调用 `_overlayWindow?.SetSettingsHwnd(settingsHwnd)` 恢复 overlay 层级
2. **ApplySettings** — 启动时调用 `_overlayWindow?.ApplySettings(settings)` 确保渲染器应用正确设置值
3. **日志替换** — 所有 `File.AppendAllText` 替换为 `DebugLog.Write`
4. **TogglePause 修改** — 移除 `_overlayWindow?.Hide()`/`Show()`，仅控制音频捕获，依赖衰减自然回落到细线
5. **ExitApp 修改** — 从固定延迟改为轮询检测衰减状态：先停止音频 → ForceDecay(useExitFactor: true) → 轮询柱身 `GetMaxDecayedValue()<0.05` **和峰值线** `GetMaxPeakHeight()<0.05` → RemoveIcon + TerminateProcess 硬终止，800ms 超时兜底（2026-06-26 完整版：双轨检测 + 硬终止绕过 WinUI3 ProcessExit 10秒卡顿）

**当前关键代码:**
```csharp
private void OpenSettingsWindow()
{
    _uiDispatcher?.TryEnqueue(() =>
    {
        if (_settingsWindow == null)
        {
            _settingsWindow = new MainWindow();
            _settingsWindow.AppWindow.Closing += OnSettingsClosing;
        }
        _settingsWindow?.Activate();

        // 关键：将设置窗口句柄传给 overlay，触发 Z-order 恢复
        if (_settingsWindow != null)
        {
            var settingsHwnd = WinRT.Interop.WindowNative.GetWindowHandle(_settingsWindow);
            _overlayWindow?.SetSettingsHwnd(settingsHwnd);
        }
    });
}

private void ApplySettings(AppSettings settings)
{
    if (_fftProcessor != null)
    {
        _fftProcessor.BassResolutionLevel = settings.BassResolutionLevel;
        _fftProcessor.ReduceBass = settings.ReduceBass;
    }
    _overlayWindow?.ApplySettings(settings); // 关键：启动时应用设置
}

// 退出时频谱平滑衰减到细线后再退出（柱身 + 峰值线双轨检测）
private void ExitApp()
{
    _audioCapture?.Stop();
    // 同时启用柱身指数衰减（ExitFactor=0.80）和峰值线减法衰减（ExitPeakDecayValue=0.08）
    _overlayWindow?.ForceDecay(useExitFactor: true);

    // 轮询检测柱身和峰值线是否都已回落到细线状态（最大值 < 0.05 ≈ 2px）
    // 之前只检测柱身，峰值线还悬在半空就退出了，视觉上没回到细线
    var maxWait = DateTime.Now.AddMilliseconds(800);
    while (DateTime.Now < maxWait)
    {
        float barMax = App.Decay?.GetMaxDecayedValue() ?? 0f;
        float peakMax = _overlayWindow?.GetMaxPeakHeight() ?? 0f;
        if (barMax < 0.05f && peakMax < 0.05f) break;
        System.Threading.Thread.Sleep(16);
    }

    // 仅移除托盘图标（不 Join 消息线程，ExitApp 运行在托盘线程上，自 Join 会空等1秒）
    _trayIcon?.RemoveIcon();
    // TerminateProcess 硬终止，绕过 WinUI3 ProcessExit 的 10 秒缓慢清理
    TerminateProcess(GetCurrentProcess(), 0);
}
```

### SettingsManager.cs — 设置管理器

**修改历史:**

1. **移除 Validate 方法** — 改为在 Load 方法中检测污染值
2. **添加污染值检测** — 检测 `Gravity < 0`、`OverlayMode < 0`、`HslLightness > 95` 等无效值，重置为默认值

**当前关键代码:**
```csharp
public void Load()
{
    try
    {
        if (File.Exists(SettingsPath))
        {
            var json = File.ReadAllText(SettingsPath);
            Current = JsonSerializer.Deserialize<AppSettings>(json, JsonOptions) ?? new AppSettings();

            // 检测被污染的设置值（之前 bug 保存的无效值），重置为默认值
            if (Current.Gravity < 0 || Current.Gravity > 4 ||
                Current.OverlayMode < 0 || Current.OverlayMode > 2 ||
                Current.HslLightness < 0 || Current.HslLightness > 95)
            {
                Current = new AppSettings();
                Save();
            }
        }
    }
    catch { Current = new AppSettings(); }
}
```

### SettingsPage.xaml.cs — 设置页面

**修改历史:**

1. **添加柱间缝隙滑块** — BarGap (0.0~1.0)，实时生效（已废弃，改为 BarWidth+GapWidth）
2. **修复颜色变白问题** — 使用 `DispatcherTimer` 延迟设置 `_isLoading = false`，拦截 WinUI3 延迟事件触发
3. **改为 BarWidth + GapWidth 独立滑块** — 两个独立滑块控制柱宽和间隙
4. **修复色彩空间切换 bug** — 拆分 `OnColorSpaceChanged` 和 `OnColorSliderChanged`，切换时调用 `UpdateColorSliders()` 更新滑块值
5. **添加随机颜色按钮** — 对齐 Linux 版本，点击随机生成配色方案
6. **添加实时数值显示** — `UpdateValueDisplays()` 方法，每个滑块右侧显示当前值
7. **色相范围扩大** — 从 `-720~720` 扩大到 `-4000~4000`（对齐 Linux 版本）

**当前关键代码:**
```csharp
// 色彩空间切换：更新滑块显示对应色彩空间的值
private void OnColorSpaceChanged(object sender, SelectionChangedEventArgs e)
{
    if (_isLoading) return;
    _settings.ColorSpaceHSLuv = ColorSpaceRadio.SelectedIndex == 1;
    UpdateColorSliders();      // 关键：切换后更新滑块
    UpdateValueDisplays();
    SaveSettings();
    ApplySettingsToEngine();
}

// 颜色滑块变化：根据当前色彩空间写入对应字段
private void OnColorSliderChanged(object sender, RangeBaseValueChangedEventArgs e)
{
    if (_isLoading) return;
    if (_settings.ColorSpaceHSLuv)
    {
        _settings.HsluvHueFrom = (int)HueFromSlider.Value;
        _settings.HsluvHueTo = (int)HueToSlider.Value;
        _settings.HsluvSaturation = (int)SaturationSlider.Value;
        _settings.HsluvLightness = (int)LightnessSlider.Value;
    }
    else
    {
        _settings.HslHueFrom = (int)HueFromSlider.Value;
        // ... HslXxx 字段
    }
    UpdateValueDisplays();
    SaveSettings();
    ApplySettingsToEngine();
}

// 随机颜色按钮
private void OnRandomColorClick(object sender, RoutedEventArgs e)
{
    _settings.ColorSpaceHSLuv = true;
    _settings.HsluvHueFrom = _random.Next(-4000, 0);
    _settings.HsluvHueTo = _random.Next(0, 4000);
    _settings.HsluvSaturation = _random.Next(60, 100);
    _settings.HsluvLightness = _random.Next(40, 70);
    UpdateColorSliders();
    UpdateValueDisplays();
    SaveSettings();
    ApplySettingsToEngine();
}

// 实时数值显示
private void UpdateValueDisplays()
{
    BassResolutionValue.Text = $"当前: {(int)BassResolutionSlider.Value}";
    FpsValue.Text = $"当前: {(int)FpsSlider.Value}";
    BarWidthValue.Text = $"当前: {(int)BarWidthSlider.Value}px";
    GapWidthValue.Text = $"当前: {(int)GapWidthSlider.Value}px";
    HueFromValue.Text = $"当前: {(int)HueFromSlider.Value}";
    HueToValue.Text = $"当前: {(int)HueToSlider.Value}";
    SaturationValue.Text = $"当前: {(int)SaturationSlider.Value}";
    LightnessValue.Text = $"当前: {(int)LightnessSlider.Value}";
}
```

### DebugLog.cs — 线程安全日志（新增）

**用途:** 解决多线程并发写入日志文件导致 IOException，进而导致 NAudio 录音回调异常停止的问题。

```csharp
public static class DebugLog
{
    private static readonly object _lock = new();
    private static readonly string _logPath = Path.Combine(Path.GetTempPath(), "panon_debug.txt");

    public static void Write(string message)
    {
        try
        {
            lock (_lock)
            {
                File.AppendAllText(_logPath, $"[{DateTime.Now:HH:mm:ss.fff}] {message}\n");
            }
        }
        catch { }
    }
}
```

### AppSettings.cs — 设置数据模型

**字段变更:**
```csharp
// === Bar 设置（新版，替代 BarGap）===
/// <summary>柱子宽度（像素），1~30</summary>
public int BarWidth { get; set; } = 7;

/// <summary>柱间间隙（像素），0~20</summary>
public int GapWidth { get; set; } = 4;

// 已废弃: public float BarGap { get; set; } = 0.15f;
```

**色相范围调整:** HslHueFrom/HslHueTo/HsluvHueFrom/HsluvHueTo 滑块范围从 `-720~720` 扩大到 `-4000~4000`（对齐 Linux 版本）

### MainWindow.xaml.cs — 设置主窗口（2026-06-26 新增记录）

**修改历史:**

1. **窗口尺寸调大** — 从 `600 x 600` 调整为 `820 x 720`，适配 `SettingsPage.xaml` 的 `MaxWidth=720` 内容区（左右留出对称边距），垂直方向更舒展，避免内容拥挤
2. **居中逻辑无需改动** — `AppWindow.Move` 居中计算已基于 `size.Width/Height` 动态读取，新尺寸自动生效

**当前关键代码:**
```csharp
public MainWindow()
{
    InitializeComponent();

    // 设置窗口大小（2026-06-26 调大：600x600 → 820x720）
    var size = new SizeInt32(820, 720);
    AppWindow.Resize(size);

    // 居中到屏幕工作区（排除任务栏），按 size 动态计算，无需额外改动
    var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
    var windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
    var displayArea = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
    if (displayArea != null)
    {
        int x = displayArea.WorkArea.X + (displayArea.WorkArea.Width - size.Width) / 2;
        int y = displayArea.WorkArea.Y + (displayArea.WorkArea.Height - size.Height) / 2;
        AppWindow.Move(new PointInt32(x, y));
    }
    // ...
}
```

### SettingsPage.xaml — 设置页面 UI（2026-06-26 新增记录）

**修改历史:**

1. **音频分组图标替换** — Glyph 从 `&#xE799;`（喇叭/Speaker）改为 `&#xE9E9;`（Equalizer，长短不一的柱体），更契合"音频频谱可视化"主题，与显示/颜色/Windows 分组图标视觉协调
2. **滑块描述文字换行修复** — 8 个滑块的"描述文字 + 当前值"从 `StackPanel Orientation="Horizontal"` 改为 `Grid` 两列布局（`ColumnDefinition Width="*"` + `Width="Auto"`）。根因：水平 StackPanel 给子元素无限宽度，导致 `TextWrapping=Wrap` 永远不生效，描述文字被压缩成一行或溢出

**当前关键代码:**
```xml
<!-- 音频分组图标（2026-06-26: E799 喇叭 → E9E9 Equalizer 频谱柱体） -->
<FontIcon Glyph="&#xE9E9;" Style="{StaticResource SectionIcon}" />

<!-- 滑块描述 + 当前值（2026-06-26: StackPanel Horizontal → Grid 两列，描述文字可换行） -->
<Grid ColumnSpacing="8">
    <Grid.ColumnDefinitions>
        <ColumnDefinition Width="*" />
        <ColumnDefinition Width="Auto" />
    </Grid.ColumnDefinitions>
    <TextBlock Grid.Column="0" Style="{StaticResource HintText}"
               Text="控制 FFT 分析的频率范围。值越小范围越宽..." />
    <TextBlock Grid.Column="1" x:Name="BassResolutionValue" Style="{StaticResource ValueText}" />
</Grid>
```

---

## 八、技术债务与注意事项

### 编译注意事项
1. **不要添加 `<UseWPF>` 或 `<UseWindowsForms>`** 到 csproj — 会与 WinUI3 SDK 的 XAML 编译器冲突
2. **不要引入 `Microsoft.Windows.Compatibility`** — 同样会导致 MC6000 编译错误
3. **Win32 delegate 必须保持引用** — 否则 GC 回收后回调崩溃（如 `_wndProcDelegate`）
4. **`GetDC`/`ReleaseDC` 从 `user32.dll` 导入** — 不是 `gdi32.dll`
5. **纯软件渲染无并发限制** — 不需要 _renderLock（已移除 Win2D CreateDrawingSession 并发限制）

### 已知限制
- WinUI3 `DispatcherTimer` 在分层窗口环境下 Tick 不触发 → 用 `System.Timers.Timer`
- WinUI3 Window 不能在非 UI 线程创建 → 用 `DispatcherQueue.TryEnqueue`
- 关闭最后一个 WinUI3 Window 默认会退出应用 → 创建隐藏主窗口 + 托盘控制生命周期
- 设置窗口打开时需主动调用 `SetSettingsHwnd` 恢复 overlay Z-order（已解决）

### NuGet 依赖
| 包名 | 版本 | 用途 |
|------|------|------|
| Microsoft.WindowsAppSDK | 2.2.0 | WinUI 3 运行时 |
| NAudio | 2.3.0 | WASAPI Loopback 音频捕获 |
| CommunityToolkit.Mvvm | 8.4.2 | MVVM 工具包 |
| Microsoft.Windows.SDK.BuildTools | 10.0.28000.1839 | Windows SDK 构建工具 |
| Silk.NET.OpenGL / Windowing | 2.23.0 | (预留, 当前未使用) |

> 注: Microsoft.Graphics.Win2D 已移除，改为纯软件渲染

---

## 九、关键代码片段索引

### 频谱渲染逻辑（纯软件）
[SpectrumRenderer.cs:RenderToPixels()](src/Panon.Windows/Shader/SpectrumRenderer.cs) — 核心渲染方法:
- 整数分配算法计算柱宽/间隙（Bresenham 风格）
- `Math.Max(value * height, 2)` — 始终用衰减值，最小 2px，平滑过渡
- 根据 Gravity 方向绘制柱状图（South=从底部向上，North=从顶部向下，East/West=水平）
- 颜色: HSL/HSLuv 渐变 (`ColorProcessor.GetGradientColor`)
- 直接写入 `uint*` 像素缓冲区（BGRA 格式）

### Z-order 原子操作
[LayeredOverlayWindow.cs:EnsureZOrder()](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs) — 原子 Z-order 维护:
```csharp
var dwp = BeginDeferWindowPos(2);
dwp = DeferWindowPos(dwp, _hwnd, HWND_TOPMOST, ...);       // overlay → TOPMOST
dwp = DeferWindowPos(dwp, _taskbarHwnd, HWND_TOPMOST, ...); // taskbar → TOPMOST
EndDeferWindowPos(dwp); // 原子提交，无中间状态
```

### 设置窗口创建 + Z-order 恢复
[App.xaml.cs:OpenSettingsWindow()](src/Panon.Windows/App.xaml.cs) — 通过 DispatcherQueue 在 UI 线程创建:
```csharp
_uiDispatcher.TryEnqueue(() =>
{
    _settingsWindow = new MainWindow();
    _settingsWindow.AppWindow.Closing += OnSettingsClosing;
    _settingsWindow?.Activate();

    // 关键：传入设置窗口句柄，触发 overlay Z-order 恢复
    var settingsHwnd = WinRT.Interop.WindowNative.GetWindowHandle(_settingsWindow);
    _overlayWindow?.SetSettingsHwnd(settingsHwnd);
});
```

### 频谱数据线程安全 + 过期检测
[LayeredOverlayWindow.cs:OnUpdateTick()](src/Panon.Windows/Overlay/LayeredOverlayWindow.cs):
```csharp
lock (_spectrumLock)
{
    if (_lastSpectrumUpdateTime != DateTime.MinValue &&
        (DateTime.Now - _lastSpectrumUpdateTime).TotalMilliseconds > 200)
    {
        currentSpectrum = new SpectrumData { /* 零值 */ };
    }
    else
    {
        currentSpectrum = _lastSpectrum;
    }
}
```

### 任务栏位置获取
[TaskbarHelper.cs:GetTaskbarInfo()](src/Panon.Windows/Helpers/TaskbarHelper.cs) — 使用 SHAppBarMessage:
```csharp
SHAppBarMessage(ABM_GETTASKBARPOS, ref data);
// 返回: Position (Top/Bottom/Left/Right), Bounds (X,Y,Width,Height)
```

---

## 十、快速启动新会话指南

### 第一步: 了解现状
1. 阅读本文档（当前文件）
2. 查看 `%TEMP%\panon_debug.txt` 了解上次运行的详细日志

### 第二步: 运行项目
```powershell
cd "D:\Python Project\panon\src\Panon.Windows"
dotnet run --configuration Debug
```

### 第三步: 验证基本功能
1. 任务栏底部应有彩色细线（无音频时的静止状态 = 峰值线 peak=0 时在底部2px）
2. 播放音乐后频谱应动态响应，柱顶有峰值细线被顶起，停止后柱身快速回落（500ms），峰值线缓慢下落（1.7秒）
3. 右键托盘图标应有菜单（设置/暂停/退出）
4. 暂停时频谱应平滑回落到细线（不直接消失）
5. 退出时频谱应先回落到细线状态后再退出（不卡顿）
6. 打开设置窗口，频谱应保持彩色不消失
7. 调整"柱子宽度"和"柱间间隙"滑块，柱子粗细和间隙应实时变化，且精确填满任务栏（无右侧空白）
8. 切换色彩空间 HSL/HSLuv，滑块值应跟着变化
9. 点击"随机颜色"按钮，颜色应随机变化
10. 每个设置项下方应有灰色说明文字，滑块右侧应有实时数值显示
11. 关闭设置窗口，频谱应继续正常显示

### 第四步: 后续任务
任务清单与优先级详见**第六节「待办事项」**，按 P0→P4 顺序执行：
- **P0（Bug修复）:** ✅ 全部完成（任务12、任务13）
- **P1（核心功能）:** 任务11 ✅、任务19 ✅ → 任务3（任务栏空白填充，需先出详细计划）
- **P2（功能增强）:** 任务2 ✅、任务5 ✅ → 任务14（开机自启）、任务15（多显示器）、任务16（GLSL 着色器）、任务18（配置文件路径）✅
- **P3（代码质量）:** 任务7 ✅ → **任务6（冗余优化分析，下一项推荐）** → 任务8（可删除文件）、任务9（体积分析）、任务10（README）
- **P4（性能）:** 任务17（SIMD/60fps/缓存）

### 下一项任务推荐: 任务6 — 代码冗余优化分析

**为什么选任务6：**
- P3 代码质量任务中风险最低的一项（仅分析输出报告，不直接改代码，明确"不影响功能"）
- 紧接任务7（设置窗口 UI 现代化）完成后，趁热对全项目做一次静态审查
- 输出的优化清单可为后续任务8（清理可删除文件）、任务9（体积分析）提供输入
- 与任务8/9 有协同效应：任务6 识别冗余代码 → 任务8 识别冗余文件 → 任务9 分析体积

**任务要求（来自 PROJECT_STATUS）：**
- 扫描全项目，识别冗余/不合理写法，输出优化清单
- **不直接改代码，不影响功能**
- 方法：静态审查 + 输出报告

**审查方向：**
1. 未使用的字段、方法、using 引用（如 `FftProcessor._hasLogged` 警告）
2. 重复代码逻辑（可抽取为公共方法）
3. 不合理的写法（如可简化的条件判断、可合并的事件处理器）
4. 已废弃但未删除的代码（如旧的 `BarGap` 相关残留）
5. 未使用的资源（XAML 中的样式、资源）
6. 可删除的备用文件（如 `TaskbarOverlayWindow.cs` 是否仍需要）

> 任务编号对应 `docs/PanonWindows项目记录.txt`，完成后同步标记该文件

---

## 十一、附录: 重要历史错误记录

| 错误 | 原因 | 修复方式 |
|------|------|----------|
| 频谱卡顿突变 | 固定减法衰减 DecayRate=0.003 | 改为指数衰减 (NormalFactor=0.96) |
| 白色背景 | WinUI3 Window 无法真透明 | 改用 Win32 WS_EX_LAYERED 分层窗口 |
| 托盘无响应 | H.NotifyIcon Click 事件不触发 | 重写为 Win32 Shell_NotifyIcon 原生 API |
| WMC9999 编译错误 | H.NotifyIcon 与 WinUI3 XAML 冲突 | 移除 H.NotifyIcon |
| GetDC 入口点错误 | 错误从 gdi32.dll 导入 | 改为 user32.dll |
| 设置窗口跨线程崩溃 | WinUI3 Window 在非 UI 线程创建 | DispatcherQueue.TryEnqueue 调度 |
| 无音频时无显示 | 频谱数据全0时不渲染 | Math.Max(value*height, 2) 保证最小2px |
| DispatcherTimer 不触发 | 依赖 UI 消息循环 | 改用 System.Timers.Timer |
| 关闭设置退出应用 | WinUI3 最后一个窗口关闭=Exit | 创建隐藏主窗口 |
| WinEvent 回调崩溃 | delegate 被 GC 回收 | 保存字段引用（后移除 WinEvent 钩子） |
| WPF/WinForms 编译失败 | UseWPF/UseWindowsForms 与 WinUI3 冲突 | 保持纯 WinUI3 设置窗口 |
| 频谱闪烁 | 两个独立 SetWindowPos 非原子 | BeginDeferWindowPos/EndDeferWindowPos 原子操作 |
| 设置窗口打不开 | WinEvent 钩子回调干扰 WinUI3 激活 | 移除 WinEvent 钩子，改用定时器 |
| 音乐停止后频谱不回细线 | hasAudio 二值开关导致跳变 | 移除 hasAudio，用 Math.Max 平滑过渡 |
| HWND_TOP 被焦点窗口覆盖 | HWND_TOP 只对无焦点窗口有效 | 改回 WS_EX_TOPMOST |
| **打开设置频谱消失** | 多线程日志写入 IOException 导致 NAudio 停止 | 线程安全 DebugLog 类 (lock 保护) |
| **频谱在任务栏上方显示** | EnsureZOrder 误将 overlay 置于 taskbar 之上 | 还原 EnsureZOrder 逻辑，overlay 在 taskbar 之下 |
| **音乐停止后频谱未恢复细线** | _lastSpectrum 线程不安全 + 数据未清零 | _spectrumLock + _lastSpectrumUpdateTime 过期检测 |
| **调整柱间缝隙后频谱变白** | 设置文件污染 hslLightness:100 | SettingsManager.Load 检测污染值并重置 |
| **点击设置后频谱变白** | WinUI3 延迟事件触发错误颜色参数 | DispatcherTimer 延迟 _isLoading=false |
| **启动时频谱变白** | 启动未调用 ApplySettings | App.LaunchInternal 添加 _overlayWindow?.ApplySettings |
| **柱间间隙不均匀** | 浮点累加 + 四舍五入导致宽度波动 | 整数分配算法 (Bresenham 风格) |
| **Win2D 与 WinUI3 合成层冲突** | CanvasDevice 与 DirectComposition 冲突 | 改为纯软件渲染 (直接写像素内存) |
| **柱子太宽无法调细** | BarGap 比例值无法独立控制柱宽和间隙 | 改为 BarWidth + GapWidth 像素值独立控制 |
| **任务栏右侧空白** | 整除丢弃余数 + 最后柱子后留间隙 | 间隙数=N-1 + 余数分配给前几个柱子 |
| **柱子数量固定** | FFT 分辨率等级决定柱子数，用户无法控制 | 添加 Resample 线性插值，柱子数由 BarWidth+GapWidth 自动计算 |
| **色彩空间切换滑块错乱** | 所有控件共用 OnSettingChanged，切换后滑块未更新 | 拆分 OnColorSpaceChanged + UpdateColorSliders |
| **设置项缺少说明** | 用户不知道调整后发生什么变化 | 每个控件添加灰色说明文字 + 实时数值显示 |
| **XAML 中文引号解析错误** | 说明文字中的中文引号被当成属性结束 | 移除中文引号或改用其他标点 |
| **退出时频谱卡顿** | 固定延迟800ms无法适应不同律动高度 | 改为轮询检测 GetMaxDecayedValue()<0.05，达标后立即退出，1200ms超时兜底 |
| **峰值线不可见** | 先绘制峰值线后绘制柱身，柱身覆盖了峰值线 | 调整绘制顺序：先柱身后峰值线 |
| **峰值线与柱身紧贴** | 绘制条件 `peakHeight > barHeight + 1` 导致峰值线被隐藏 | 移除绘制条件，峰值线始终绘制（对齐 Linux） |
| **峰值线下降太慢** | 使用 Linux 默认 0.003（11秒），不符合 Windows 节奏 | 调快到 0.02（1.7秒），适配 Windows 柱身衰减（500ms） |
| **启动无细线/回落出现两条线** | 柱身 Math.Max 2px + 峰值线底部2px = 两条线 | 移除柱身 Math.Max，待机细线由峰值线提供（对齐 Linux 设计） |
