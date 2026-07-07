// detect.rs — 任务栏位置/范围检测（← TaskbarHelper.cs）
// 阶段 2 实现主任务栏检测，阶段 5 补全多显示器

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_GETTASKBARPOS};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};

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
        // FindWindowW 在 windows-rs 0.58 返回 Result<HWND>
        let taskbar_hwnd = match FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
            Ok(h) => h,
            Err(_) => return info,
        };
        if taskbar_hwnd.is_invalid() {
            return info;
        }
        info.hwnd = taskbar_hwnd.0 as isize;

        let mut data: APPBARDATA = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        data.hWnd = taskbar_hwnd;

        // SHAppBarMessage 返回 usize（非零表示成功）
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
            // 回退：通过窗口句柄获取矩形
            let mut rect = RECT::default();
            // GetWindowRect 在 windows-rs 0.58 返回 Result<()>
            if GetWindowRect(taskbar_hwnd, &mut rect).is_ok() {
                info.x = rect.left;
                info.y = rect.top;
                info.width = rect.right - rect.left;
                info.height = rect.bottom - rect.top;
                // 推断位置
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
    }

    info
}
