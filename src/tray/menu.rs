// menu.rs — 右键菜单（← MessageWindow.ShowContextMenu）
// 阶段 6 实现：CreatePopupMenu → AppendMenu → TrackPopupMenu

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_SEPARATOR, MF_STRING,
    TrackPopupMenu, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TRACK_POPUP_MENU_FLAGS,
};

pub const MENU_ID_SETTINGS: u32 = 1001;
pub const MENU_ID_PAUSE: u32 = 1002;
pub const MENU_ID_EXIT: u32 = 1003;

/// 显示右键菜单，返回选中的菜单项 ID（0 表示未选中/取消）
pub unsafe fn show_context_menu(hwnd: HWND) -> u32 {
    let hmenu = match CreatePopupMenu() {
        Ok(h) => h,
        Err(_) => return 0,
    };

    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_SETTINGS as usize, w!("Settings"));
    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_PAUSE as usize, w!("Pause"));
    let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(hmenu, MF_STRING, MENU_ID_EXIT as usize, w!("Exit"));

    let mut pt = POINT { x: 0, y: 0 };
    let _ = GetCursorPos(&mut pt);

    // 托盘消息窗口是隐藏窗口，不能 SetForegroundWindow（会失败导致菜单行为异常）。
    // 用户右键点击托盘图标时，线程已处于前台状态，直接 TrackPopupMenu 即可。
    let flags: TRACK_POPUP_MENU_FLAGS = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY;
    let result = TrackPopupMenu(hmenu, flags, pt.x, pt.y, 0, hwnd, None);

    let _ = DestroyMenu(hmenu);

    result.0 as u32
}
