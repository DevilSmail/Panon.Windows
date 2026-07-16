// menu.rs — 右键菜单（← NativeTrayIcon.ShowContextMenu）
// 阶段 6 实现：CreatePopupMenu → AppendMenu → TrackPopupMenu
// 修复：添加 AttachThreadInput，对齐 C# 的线程附加逻辑，确保菜单点击正常响应

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetForegroundWindow,
    GetWindowThreadProcessId, MF_SEPARATOR, MF_STRING, TrackPopupMenu, TPM_RETURNCMD,
    TPM_RIGHTBUTTON,
};

pub const MENU_ID_SETTINGS: u32 = 1001;
pub const MENU_ID_PAUSE: u32 = 1002;
pub const MENU_ID_EXIT: u32 = 1003;

/// 显示右键菜单，返回选中的菜单项 ID（0 表示未选中/取消）
/// is_paused: 当前是否暂停，用于切换"暂停"/"恢复"文字
pub unsafe fn show_context_menu(hwnd: HWND, is_paused: bool) -> u32 {
    let hmenu = match CreatePopupMenu() {
        Ok(h) => h,
        Err(_) => return 0,
    };

    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_SETTINGS as usize, w!("设置"));
    let pause_text = if is_paused { w!("恢复") } else { w!("暂停") };
    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_PAUSE as usize, pause_text);
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_EXIT as usize, w!("退出"));

    let mut pt = POINT { x: 0, y: 0 };
    let _ = GetCursorPos(&mut pt);

    // 对齐 C# NativeTrayIcon.ShowContextMenu：
    // 将当前线程附加到前台窗口线程，确保 TrackPopupMenu 能正确接收鼠标输入
    let foreground = GetForegroundWindow();
    let foreground_thread = GetWindowThreadProcessId(foreground, None);
    let current_thread = GetCurrentThreadId();
    let attached = foreground_thread != 0 && foreground_thread != current_thread;
    if attached {
        let _ = AttachThreadInput(current_thread, foreground_thread, true);
    }

    // TPM_RETURNCMD: 返回选中菜单项 ID；TPM_RIGHTBUTTON: 左右键均可选择
    let result = TrackPopupMenu(hmenu, TPM_RIGHTBUTTON | TPM_RETURNCMD, pt.x, pt.y, 0, hwnd, None);

    if attached {
        let _ = AttachThreadInput(current_thread, foreground_thread, false);
    }

    let _ = DestroyMenu(hmenu);

    result.0 as u32
}
