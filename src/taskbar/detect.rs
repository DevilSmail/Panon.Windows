// detect.rs — 任务栏位置/范围检测（← TaskbarHelper.cs）
// 阶段 2 实现主任务栏检测，阶段 5 补全多显示器

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_GETTASKBARPOS};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowRect};

/// 任务栏位置
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskbarPosition {
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
}

impl TaskbarInfo {
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
    }

    info
}

/// 获取所有任务栏（主 + 副显示器）
/// 返回按 X 坐标排序的列表，主显示器索引始终为 0
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
            let info = get_taskbar_info_from_hwnd(hwnd);
            if info.width > 0 && info.height > 0 {
                taskbars.push(info);
            }
            prev = hwnd;
        }
    }

    // 按 X 坐标排序（主显示器通常在最左，但排序确保一致性）
    taskbars.sort_by_key(|t| t.x);
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
