# Panon.Windows → Rust 迁移计划

> 备份: `d:\VisualStudioProject\Panon.Windows_backup`
> 目标: `d:\VisualStudioProject\panon-windows`（同级的 Rust 项目目录）

## 阶段划分

共 9 个阶段，按依赖关系排序。每阶段独立验证，不依赖后续阶段。

---

### 阶段 0：搭建项目骨架

**目标**：创建 Rust 项目，配置依赖，嵌入图标

**操作**：
1. `cargo init panon-windows`（在 `VisualStudioProject` 下）
2. 编写 `Cargo.toml`（所有依赖一次性声明）
3. 编写 `build.rs`（嵌入 `panon.ico`）
4. 复制 `assets/panon.ico` 到项目
5. `cargo build --release` → 确认编译通过

**验证**：`target/release/panon-windows.exe` 生成成功，文件带上图标

**依赖**：无

---

### 阶段 1：音频捕获 + FFT

**目标**：从系统音频输出捕获 PCM，做完 FFT，输出频谱数据

**操作**：
1. `audio/spectrum.rs` — `SpectrumData` 结构体
2. `audio/capture.rs` — WASAPI Loopback，`mpsc::Sender` 发送 PCM
3. `audio/fft.rs` — 汉宁窗 + Cooley-Tukey 2048pt，接收 PCM → 输出 `SpectrumData`
4. `main.rs` — 临时测试：启动捕获 → 接收频谱 → 打印 max 值到控制台

**验证**：播放音乐时控制台输出实时频谱数据（20 段 log-scale bar chart + peak bin + magnitude，参考 POC 已验证的输出格式）。POC 已通过：48000Hz 2ch PCM 环回 + 2048pt FFT，发布版 1.0 MB

**依赖**：阶段 0

---

### 阶段 2：基础渲染管线

**目标**：创建分层窗口，把频谱渲染到任务栏（先只实现柱状图）

**操作**：
1. `render/color.rs` — HSL/HSLuv → RGB
2. `render/effects.rs` — `render_bar1ch`（一种效果）
3. `render/renderer.rs` — `SpectrumRenderer`，封装像素缓冲区写入
4. `overlay/window.rs` — `OverlayWindow`：注册窗口类 → `CreateWindowEx(WS_EX_LAYERED)` → DIB Section → 30 FPS 定时器 → `UpdateLayeredWindow`
5. `taskbar/detect.rs` — `get_taskbar_info()` 获取主显示器任务栏位置
6. `main.rs` — 串联：捕获 → FFT → OverlayWindow（全屏填充模式）

**验证**：任务栏上出现柱状图频谱，随音乐跳动

**依赖**：阶段 1

**设计备注 — MaxBarHeight：** 此阶段实现 `effective_height()` 辅助函数（`max_bar_height > 0 ? min(max_bar_height, window_height) : window_height`），`height` 参数在所有频谱幅值→像素缩放的场景中统一替换为 `effective_height`。后续阶段 4 新增的其余 6 种效果也遵循同一规则。**所有 7 种效果共享此限制，不是仅柱状图。**

---

### 阶段 3：衰减处理 + 暂停/静默

**目标**：音乐停止后频谱平滑回落，暂停和静默行为正确

**操作**：
1. `audio/decay.rs` — `DecayProcessor`（NormalFactor / SilenceFactor / ExitFactor）
2. `overlay/window.rs` — 集成衰减：渲染前调用 `decay.process()`
3. `overlay/window.rs` — 静默检测：200ms 无数据 → 零值 → 衰减回落
4. `overlay/window.rs` — `force_decay()` 跳过静默等待

**验证**：
- 播放 → 停止音乐 → 频谱平滑回落到 2px 基线（无突变）
- 暂停/恢复在后续阶段验证（需托盘）

**依赖**：阶段 2

---

### 阶段 4：全部 7 种视觉效果 + 颜色预设

**目标**：补全其余 6 种效果 + 8 套预设配色 + 随机颜色 + HSL/HSLuv 切换

**操作**：
1. `render/effects.rs` — 补全 `render_wave` / `render_solid1ch` / `render_solid` / `render_beam` / `render_spectrogram` / `render_oie1ch`
2. `render/renderer.rs` — 所有颜色参数（HueFrom/To、Saturation、Lightness、HSLuv 切换）
3. `render/renderer.rs` — 重采样 + 缓存
4. `render/renderer.rs` — 峰值线（`_peakHeights`）
5. `render/renderer.rs` — 频谱瀑布历史缓冲区
6. 预设配色常量数组（8 套，与 C# 数值一致）

**验证**：逐一切换 7 种效果和 8 套预设，截屏对比 C# 版本

**依赖**：阶段 2（可与阶段 3 并行）

---

### 阶段 5：任务栏检测 + 空白区域填充

**目标**：多显示器识别、UIA 探测空白区域、FillMode 两种模式

**操作**：
1. `taskbar/detect.rs` — `get_all_taskbars()`（Shell_TrayWnd + Shell_SecondaryTrayWnd，按 X 排序，主显示器索引 0）
2. `taskbar/uia.rs` — `get_free_regions()`（IUIAutomation 遍历，500ms 缓存，**含 Y 坐标过滤修复**；COM 引用由 `windows` crate 的 `Drop` 确定性释放，无 C# 的 GC Finalizer 泄漏，详见 MIGRATION_RUST.md §八）
3. `overlay/window.rs` — 集成 UIA：FillMode=1 时每帧更新 `free_regions`
4. `overlay/window.rs` — 铺满模式 (FillMode=0) vs 空白区域模式 (FillMode=1)
5. `overlay/window.rs` — 多显示器：每个任务栏创建独立 OverlayWindow

**验证**：
- 铺满：整个任务栏有频谱
- 空白区域：只在图标间隙显示
- 点击网络/声音图标弹窗 → 频谱不消失（Y 过滤生效）
- 多显示器：每个屏幕任务栏独立显示

**依赖**：阶段 2

---

### 阶段 6：系统托盘 + 菜单 + Explorer 重启恢复

**目标**：托盘图标、右键菜单（设置/暂停/退出）、TaskbarCreated 恢复

**操作**：
1. `tray/icon.rs` — `TrayIcon`：隐藏消息窗口 + `Shell_NotifyIcon(NIM_ADD)` + `TaskbarCreated` 注册
2. `tray/menu.rs` — 右键菜单：`CreatePopupMenu` → `AppendMenu`（⚙ 设置 / ⏸ 暂停 / ✕ 退出）→ `TrackPopupMenu`
3. `main.rs` — 托盘回调：左键/双击 → 设置窗口、右键 → 菜单、暂停/退出逻辑
4. `main.rs` — TaskbarCreated 处理：重建托盘图标 + 重建所有 overlay
5. `main.rs` — 退出流程：停止捕获 → force_decay(exit) → 等待衰减完成 (≤800ms) → 清理 → process::exit

**验证**：
- 托盘图标显示，悬停显示 tooltip
- 左键 → 打开设置窗口（先弹出占位窗口即可，阶段 7 才实现内容）
- 右键 → 菜单 → 暂停/退出正常
- kill explorer.exe → 重启 → 托盘图标和频谱自动恢复

**依赖**：阶段 3

---

### 阶段 7：设置窗口 UI (egui)

**目标**：完整的设置窗口，所有控件功能与 C# 版一致

**操作**：
0. 在 `Cargo.toml` 中为 `wgpu` 开启 `angle` feature（`wgpu = { version = "22", features = ["angle"] }`），在无 GPU 驱动时回退到 ANGLE/D3D11 软件渲染；若宿主环境不支持 wgpu，提供 `egui_glow` 作为备选后端
1. `ui/settings_window.rs` — eframe 窗口（820×720，居中，带图标）
2. 四个卡片分组：音频、显示、颜色、Windows 设置
3. 控件：
   - ToggleSwitch → `ui.checkbox()`（降低低音、反转频谱、开机自启）
   - Slider → `egui::Slider`（频率分辨率、帧率、柱宽、间隙、色相×2、饱和度、亮度）
   - ComboBox → `egui::ComboBox`（预设配色、图形效果、填充模式、频谱方向、覆盖模式、目标显示器）
   - RadioButtons → `ui.radio_value()`（色彩空间 HSL/HSLuv）
   - Button → `ui.button()`（随机颜色）
   - 系统透明效果 Toggle → 读/写注册表
4. 所有值显示：滑块右侧 `当前: X`
5. 预设配色：下拉选择 → 即时更新滑块 → 自动匹配预设名称
6. 色彩空间切换：HSL ↔ HSLuv 滑块值切换
7. 柱宽/间隙仅在柱状图时启用
8. 目标显示器：运行时枚举 + "所有显示器"

**验证**：
- 修改任意设置 → 频谱即时生效
- 关闭设置 → 重新打开 → 值保留
- 退出程序 → 重新启动 → 设置持久化 (JSON)
- 预设配色选中 → 重启 → 仍显示预设名称（不跳回自定义）

**依赖**：阶段 1-6

---

### 阶段 8：设置持久化 + 全功能集成

**目标**：JSON 读写、注册表操作、开机自启、透明效果、单实例

**操作**：
1. `settings/config.rs` — `AppSettings` + `serde_json` 读写（`%APPDATA%/Panon/settings.json`，camelCase 对齐 C#）
2. 字段验证（Gravity 0-4、FillMode 0-1、Fps 10-60 等，不合法时自动修复）
3. `settings/transparency.rs` — 注册表 `EnableTransparency` + `UseOLEDTaskbarTransparency` 读写 + 原始值快照持久化
4. `main.rs` — 开机自启：注册表 `HKCU\...\Run\Panon` 写/删
5. `main.rs` — 单实例检查：`CreateMutex("Global\\Panon.Windows.SingleInstance")`
6. `main.rs` — 启动时应用保存的设置到所有组件
7. 错误处理：panic hook → `%TEMP%/panon_crash.txt` + 日志（`env_logger`）
8. 清理临时测试代码，移除控制台子系统（`#![windows_subsystem = "windows"]`）

**验证**：
- 修改设置 → 退出 → 重新打开 → 所有设置正确恢复
- 开机自启开关 → 检查注册表 Run 键
- 透明效果开关 → 检查注册表透明效果键
- 启动第二个实例 → 拒绝启动
- 删除 settings.json → 启动 → 生成默认配置

**依赖**：阶段 7

---

### 阶段 9：Release 优化 + 测试

**目标**：体积/内存优化，功能完整性测试，打包发布

**操作**：
1. `Cargo.toml` — `opt-level = "z"`、`lto = true`、`codegen-units = 1`、`strip = true`、`panic = "abort"`
2. `cargo build --release` → 检查 exe 体积（预期 ~5 MB）
3. 在干净虚拟机/其他电脑测试：复制 exe → 直接运行 → 无任何缺少组件提示
4. 功能回归测试：参考 C# 版逐项检查
5. 打包：`Panon.Windows_v1.0_portable.zip`（仅一个 exe）

**验证**：
- exe ≤ 5 MB（对齐 MIGRATION_RUST.md 目标；POC 阶段仅 WASAPI+FFT 已 1.0 MB）
- 复制到另一台电脑直接运行（无需任何运行时）
- 所有 C# 版功能正常工作

**依赖**：阶段 8

---

## 阶段依赖图

```
阶段 0 (骨架)
  │
阶段 1 (音频+FFT)
  │
阶段 2 (渲染管线) ──→ 阶段 4 (7种效果) ──┐
  │                                        │
阶段 3 (衰减) ─────────────────────────────┤
  │                                        │
阶段 5 (任务栏+UIA) ───────────────────────┤
  │                                        │
阶段 6 (托盘) ─────────────────────────────┤
                                           │
                                    ┌──────┘
                                    ↓
                              阶段 7 (设置UI)
                                    │
                              阶段 8 (持久化集成)
                                    │
                              阶段 9 (Release)
```

阶段 2-6 之间耦合低，可调整顺序。

---

## 注意事项

| 事项 | 说明 |
|------|------|
| C# 源码作为参考 | 每个阶段对比 C# 实现，确保逻辑一致 |
| 编译中间检查 | 每完成一个文件就 `cargo build`，不要累积到阶段末尾 |
| git 小步提交 | 每阶段完成后 commit，方便回退 |
| 不跨阶段修改 | 当前阶段不修改已完成阶段的代码（除非是 bug） |
| UIA Flyout 防御 | C# 版已修复（Y+高度过滤 + 三阶段回退），阶段 5 直接继承逻辑；Rust COM 确定性释放进一步杜绝泄漏，详见 MIGRATION_RUST.md §八 |
| MaxHeight 功能 | C# 版已新增，阶段 2 创建 overlay 时支持 `maxHeightOverride` 参数 |
| GC 优化 | C# 版已完成热路径缓冲区复用，Rust 版天然零分配，无需额外处理 |
