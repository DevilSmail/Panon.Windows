# Panon.Windows

Windows 任务栏音频频谱可视化器 —— 基于 [Panon](https://github.com/rbn42/panon) (KDE Plasma) 的 Windows 移植版。

在 Windows 10/11 任务栏上显示实时音频频谱动画，支持 7 种视觉效果。

> **当前状态：Rust 原生重构进行中**（从 C# / WinUI 3 迁移至 Rust + Win32 + egui）
> 目标：单 exe ~5MB，内存基线 30~50MB，复制即运行，无需任何运行时。

---

## 支持平台

| Windows 版本           | 状态                                     |
| ---------------------- | ---------------------------------------- |
| Windows 11 (21H2~24H2) | ✅ 完整支持（主力开发平台）              |
| Windows 10 (1809~22H2) | ✅ 支持                                  |
| Windows 10 (1607~1803) | ⚠️ 理论可用                              |
| Windows 7 / 8 / 8.1    | ❌ 不支持                                |

---

## 功能特性

- **实时音频频谱** — WASAPI Loopback 捕获系统音频输出
- **7 种视觉效果** — 柱状图 / 波浪 / 实心单声道 / 实心立体声 / 光束 / 频谱瀑布 / 连线
- **任务栏集成** — 频谱与任务栏重叠显示，支持"铺满任务栏"和"仅空白区域填充"两种模式
- **多显示器** — 支持主显示器、指定显示器或所有显示器
- **8 套预设配色** — 彩虹 / 霓虹 / 极光 / 日落 / 海洋 / 火焰 / 森林 / 紫罗兰 + 随机颜色
- **HSL / HSLuv 色彩空间** — 可精细调整色相、饱和度、亮度
- **系统托盘控制** — 右键菜单：设置 / 暂停 / 退出
- **开机自启** — 可选注册表 Run 启动项
- **系统透明效果** — 一键开启/关闭 Windows 任务栏透明效果
- **平滑过渡** — 音乐停止后频谱平滑衰减回落，无突变

---

## 技术栈

| 组件       | 技术                                       |
| ---------- | ------------------------------------------ |
| 语言       | Rust 1.85+ (stable)                        |
| Win32 API  | `windows` crate（微软官方绑定）            |
| 音频捕获   | WASAPI Loopback COM                        |
| FFT        | 手写 Cooley-Tukey 2048 点                  |
| 渲染       | 纯软件 DIB Section 像素缓冲区 (BGRA 32bpp) |
| 设置窗口   | egui (即时模式 GUI)                        |
| 任务栏检测 | Win32 SHAppBarMessage + UI Automation      |
| 设置存储   | JSON (`%APPDATA%/Panon/settings.json`)     |

---

## 安装与运行

### 便携版（普通用户）

从 [Releases](https://github.com/DevilSmail/Panon.Windows/releases) 下载 `Panon.Windows_vX.X_portable.zip`，解压后双击 `panon.windows.exe` 即可运行。

**系统要求：**
- Windows 10 version 1809+ 或 Windows 11
- 无需管理员权限
- 无需任何运行时（单 exe，静态链接 CRT）

### 开发编译（开发者）

**环境要求：** Rust toolchain (stable-msvc) + MSVC Build Tools (含 Windows 11 SDK)

```powershell
git clone https://github.com/DevilSmail/Panon.Windows.git
cd Panon.Windows
cargo run --release
```

**发布便携版：**
```powershell
cargo build --release
# 产物 target/release/panon-windows.exe（约 5 MB）
# 发布时重命名为 panon.windows.exe（对齐 C# 版命名）
```

---

## 使用说明

### 系统托盘

| 操作         | 效果                           |
| ------------ | ------------------------------ |
| 左键单击图标 | 打开设置窗口                   |
| 右键单击图标 | 弹出菜单（设置 / 暂停 / 退出） |

### 配置文件

配置存储在 `%APPDATA%\Panon\settings.json`，支持手动编辑（需重启程序生效）。

---

## 与 Linux 原版的差异

| 项目     | Linux 原版                  | Windows 移植版                       |
| -------- | --------------------------- | ------------------------------------ |
| 渲染引擎 | OpenGL + GLSL               | 纯软件 CPU 渲染（直接写像素）        |
| 音频捕获 | PyAudio / PulseAudio        | WASAPI Loopback                      |
| 窗口系统 | KDE Plasmoid                | Win32 分层窗口 + egui 设置窗口       |
| 着色器   | 原生 GLSL                   | CPU 模拟（已实现 7 种，原版共 12 种）|
| 配置文件 | KConfig (~/.config/panonrc) | JSON (%APPDATA%/Panon/settings.json) |

---

## 致谢

本项目基于 [rbn42/panon](https://github.com/rbn42/panon) 移植，感谢原作者及 KDE 社区的贡献。

---

## 许可证

[GNU General Public License v3.0](LICENSE)

Copyright (C) 2024-2026 Panon.Windows Contributors
原项目 Copyright (C) 2018-2024 rbn42 and Panon contributors
