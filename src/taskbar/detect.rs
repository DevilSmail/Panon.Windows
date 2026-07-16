// detect.rs — 任务栏位置/范围检测（← TaskbarHelper.cs）
// 阶段 2 实现主任务栏检测，阶段 5 补全多显示器

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_GETTASKBARPOS};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowRect};

/// 任务栏位置
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskbarPosition {
    #[allow(dead_code)]
    Unknown,
    Left,
    Top,
    Right,
    Bottom,
}

impl Default for TaskbarPosition {
    fn default() -> Self {
        Self::Bottom
    }
}

/// 任务栏信息
#[derive(Clone, Debug, Default)]
pub struct TaskbarInfo {
    pub position: TaskbarPosition,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// 任务栏窗口句柄（用于 Z-order 维护）
    pub hwnd: isize,
    /// 是否为主显示器任务栏
    pub is_primary: bool,
    /// 所属显示器句柄（HMONITOR），用于去重（对齐 C# MonitorFromWindow）
    pub monitor: isize,
}

impl TaskbarInfo {
    #[allow(dead_code)]
    pub fn is_horizontal(&self) -> bool {
        matches!(self.position, TaskbarPosition::Top | TaskbarPosition::Bottom)
    }
}

/// 获取主任务栏信息
/// 使用 SHAppBarMessage 获取位置和范围，回退到 GetWindowRect
pub fn get_taskbar_info() -> TaskbarInfo {
    let mut info = TaskbarInfo::default();

    unsafe {
        let taskbar_hwnd = match FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            Ok(h) => h,
            Err(_) => return info,
        };
        if taskbar_hwnd.is_invalid() {
            return info;
        }
        info = get_taskbar_info_from_hwnd(taskbar_hwnd);
        info.is_primary = true;
        info.monitor = MonitorFromWindow(taskbar_hwnd, MONITOR_DEFAULTTONEAREST).0 as isize;
    }

    info
}

/// 获取所有任务栏（主 + 副显示器）
/// 返回按 X 坐标排序的列表，主显示器索引始终为 0
/// 使用 MonitorFromWindow 确保每个任务栏属于不同显示器（去重）
pub fn get_all_taskbars() -> Vec<TaskbarInfo> {
    let mut taskbars = Vec::new();

    // 主任务栏
    let main = get_taskbar_info();
    if main.width > 0 && main.height > 0 {
        taskbars.push(main);
    }

    // 副任务栏（多显示器，Shell_SecondaryTrayWnd）
    unsafe {
        let mut prev: HWND = HWND::default();
        loop {
            let hwnd = match FindWindowExW(
                HWND::default(),
                prev,
                w!("Shell_SecondaryTrayWnd"),
                PCWSTR::null(),
            ) {
                Ok(h) if !h.is_invalid() => h,
                _ => break,
            };
            let mut info = get_taskbar_info_from_hwnd(hwnd);
            if info.width > 0 && info.height > 0 {
                // 使用 MonitorFromWindow 确定所属显示器（与 C# 对齐）
                info.monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST).0 as isize;
                taskbars.push(info);
            }
            prev = hwnd;
        }
    }

    // 按 X 坐标排序
    taskbars.sort_by_key(|t| t.x);

    // 标记主显示器（Shell_TrayWnd 始终是索引 0）
    // 先确保主任务栏在索引 0
    if let Some(primary_idx) = taskbars.iter().position(|t| t.is_primary) {
        if primary_idx > 0 {
            let primary = taskbars.remove(primary_idx);
            taskbars.insert(0, primary);
        }
    }

    // 按显示器句柄去重：同一 HMONITOR 上只保留一个任务栏窗口
    // 这解决了 Win11 在某些配置下为内部段创建 Shell_SecondaryTrayWnd 导致重复检测的问题
    let mut seen_monitors = std::collections::HashSet::new();
    taskbars.retain(|tb| {
        if tb.monitor == 0 {
            true // 保留 monitor=0 的条目（没有通过 MonitorFromWindow 的，通常是有效的）
        } else {
            seen_monitors.insert(tb.monitor) // 首次出现的 monitor 保留，后续重复的丢弃
        }
    });

    // 标记显示器索引：0=主显示器, 1,2...=其他（从左到右）
    for (i, tb) in taskbars.iter_mut().enumerate() {
        tb.is_primary = i == 0;
    }

    taskbars
}

/// 从窗口句柄获取任务栏信息
unsafe fn get_taskbar_info_from_hwnd(taskbar_hwnd: HWND) -> TaskbarInfo {
    let mut info = TaskbarInfo::default();
    info.hwnd = taskbar_hwnd.0 as isize;

    // 尝试 SHAppBarMessage 获取精确位置
    let mut data: APPBARDATA = std::mem::zeroed();
    data.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
    data.hWnd = taskbar_hwnd;

    let result = SHAppBarMessage(ABM_GETTASKBARPOS, &mut data);
    if result != 0 {
        info.position = match data.uEdge {
            0 => TaskbarPosition::Left,
            1 => TaskbarPosition::Top,
            2 => TaskbarPosition::Right,
            3 => TaskbarPosition::Bottom,
            _ => TaskbarPosition::Bottom,
        };
        info.x = data.rc.left;
        info.y = data.rc.top;
        info.width = data.rc.right - data.rc.left;
        info.height = data.rc.bottom - data.rc.top;
    } else {
        // 回退：GetWindowRect
        let mut rect = RECT::default();
        if GetWindowRect(taskbar_hwnd, &mut rect).is_ok() {
            info.x = rect.left;
            info.y = rect.top;
            info.width = rect.right - rect.left;
            info.height = rect.bottom - rect.top;
            if rect.top < 10 {
                info.position = TaskbarPosition::Top;
            } else if rect.left < 10 && rect.right - rect.left < 200 {
                info.position = TaskbarPosition::Left;
            } else if rect.left > 500 {
                info.position = TaskbarPosition::Right;
            } else {
                info.position = TaskbarPosition::Bottom;
            }
        }
    }

    info
}
