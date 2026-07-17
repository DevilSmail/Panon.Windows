// icon.rs — Shell_NotifyIcon 托盘图标（← NativeTrayIcon.cs）
// 阶段 6 实现：隐藏消息窗口 + NIM_ADD + TaskbarCreated 注册

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use windows::core::{w, Error, PCWSTR};
use windows::Win32::Foundation::{HWND, HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadIconW, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER,
    WNDCLASSEXW, HICON, HMENU, WINDOW_EX_STYLE, WINDOW_STYLE,
};

use crate::tray::TrayAction;
use crate::tray::menu::{show_context_menu, MENU_ID_EXIT, MENU_ID_PAUSE, MENU_ID_SETTINGS};

/// 从 WindowProc 回调向主循环通信的 channel
static ACTION_TX: Mutex<Option<Sender<TrayAction>>> = Mutex::new(None);
/// TaskbarCreated 消息 ID（运行时通过 RegisterWindowMessageW 注册）
static TASKBAR_CREATED_MSG: Mutex<Option<u32>> = Mutex::new(None);

/// 全局退出标志 — 托盘菜单点击 Exit 时设置，供 slint Timer 轮询后调用 quit_event_loop()
pub static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 全局暂停标志 — 供 WindowProc 读取当前暂停状态以切换菜单文字
pub static IS_PAUSED: AtomicBool = AtomicBool::new(false);
/// 暂停切换请求 — 设置窗口打开时主循环阻塞，wndproc 直接设置此标志
/// 由 slint Timer (on_check_actions) 和主循环轮询处理
pub static PENDING_PAUSE_TOGGLE: AtomicBool = AtomicBool::new(false);

/// 托盘回调消息（WM_USER + 1）
const WM_TRAY: u32 = WM_USER + 1;

pub struct TrayIcon {
    hwnd: HWND,
    h_icon: HICON,
}

impl TrayIcon {
    /// 创建托盘图标并注册隐藏消息窗口
    pub fn create(tx: Sender<TrayAction>) -> windows::core::Result<Self> {
        // 存储 channel sender 到 static，供 WindowProc 回调使用
        *ACTION_TX.lock().unwrap() = Some(tx);

        unsafe {
            let h_module = GetModuleHandleW(PCWSTR::null())?;

            // 注册 TaskbarCreated 消息（Explorer 重启后广播）
            let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
            if taskbar_created != 0 {
                *TASKBAR_CREATED_MSG.lock().unwrap() = Some(taskbar_created);
            }

            // 注册窗口类
            let wc: WNDCLASSEXW = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(tray_wnd_proc),
                hInstance: HINSTANCE(h_module.0),
                lpszClassName: w!("Panon_Tray_2024"),
                ..Default::default()
            };
            let atom = RegisterClassExW(&wc);
            if atom == 0 {
                return Err(Error::from_win32());
            }

            // 创建隐藏消息窗口
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("Panon_Tray_2024"),
                w!("Panon Tray"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                HWND::default(),
                HMENU::default(),
                HINSTANCE(h_module.0),
                None,
            )?;

            // 加载嵌入资源图标 ID=1
            // MAKEINTRESOURCEW(1) 等价物：低位 = 资源 ID，高位 = 0
            let icon_name = PCWSTR(1 as *const u16);
            let h_icon = LoadIconW(HINSTANCE(h_module.0), icon_name)?;

            // 添加托盘图标
            add_icon(hwnd, h_icon);

            Ok(Self { hwnd, h_icon })
        }
    }

    /// Explorer 重启后重新添加托盘图标
    pub fn re_add(&self) {
        unsafe { add_icon(self.hwnd, self.h_icon) };
    }

    #[allow(dead_code)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            // 删除托盘图标
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = self.hwnd;
            nid.uID = 1;
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);

            // 销毁窗口
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// 填充 NOTIFYICONDATAW 并调用 Shell_NotifyIconW(NIM_ADD)
unsafe fn add_icon(hwnd: HWND, h_icon: HICON) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = h_icon;

    // 填充 tooltip "Panon"（UTF-16）
    let tip: Vec<u16> = "Panon".encode_utf16().collect();
    for (i, &c) in tip.iter().enumerate() {
        if i < nid.szTip.len() {
            nid.szTip[i] = c;
        }
    }

    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

/// 从 WindowProc 回调向主循环发送 TrayAction
unsafe fn send_action(action: TrayAction) {
    if let Ok(guard) = ACTION_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(action);
        }
    }
}

/// 托盘消息窗口过程
unsafe extern "system" fn tray_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // 检查 TaskbarCreated 消息（Explorer 重启）
    if let Ok(guard) = TASKBAR_CREATED_MSG.lock() {
        if let Some(tb_msg) = *guard {
            if msg == tb_msg {
                send_action(TrayAction::TaskbarRestart);
                return LRESULT(0);
            }
        }
    }

    // 托盘图标回调
    if msg == WM_TRAY {
        // lParam 包含鼠标消息
        match lp.0 as u32 {
            WM_LBUTTONUP => {
                send_action(TrayAction::ShowSettings);
                return LRESULT(0);
            }
            WM_RBUTTONUP => {
                let id = show_context_menu(hwnd, IS_PAUSED.load(Ordering::SeqCst));
                match id {
                    MENU_ID_SETTINGS => send_action(TrayAction::ShowSettings),
                    MENU_ID_PAUSE => {
                        // 只设标志：设置窗口打开时 channel 主循环被阻塞，无法投递
                        PENDING_PAUSE_TOGGLE.store(true, Ordering::SeqCst);
                    }
                    MENU_ID_EXIT => {
                        EXIT_REQUESTED.store(true, Ordering::SeqCst);
                        send_action(TrayAction::Exit);
                    }
                    _ => {}
                };
                return LRESULT(0);
            }
            _ => {}
        }
        return LRESULT(0);
    }

    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
