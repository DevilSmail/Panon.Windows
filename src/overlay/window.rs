// window.rs — 分层覆盖窗口（← LayeredOverlayWindow.cs）
// CreateWindowEx(WS_EX_LAYERED) + DIB Section + UpdateLayeredWindow
// 30 FPS 渲染，per-pixel alpha

use std::mem::size_of;
use std::ptr;

use windows::core::{w, Error};
use windows::Win32::Foundation::{
    COLORREF, E_INVALIDARG, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HDC, HBITMAP, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DeferWindowPos, DefWindowProcW, DestroyWindow,
    EndDeferWindowPos, HWND_TOPMOST, HMENU, PostQuitMessage, RegisterClassExW, ShowWindow,
    UpdateLayeredWindow, ULW_ALPHA, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE,
};

use crate::render::renderer::SpectrumRenderer;
use crate::taskbar::detect::TaskbarInfo;

pub struct OverlayWindow {
    hwnd: HWND,
    hdc_screen: HDC,
    hdc_mem: HDC,
    h_bitmap: HBITMAP,
    p_bits: *mut u32,
    width: i32,
    height: i32,
    pub renderer: SpectrumRenderer,
}

impl OverlayWindow {
    /// 创建分层覆盖窗口
    pub fn create(taskbar: &TaskbarInfo) -> windows::core::Result<Self> {
        let width = taskbar.width;
        let height = taskbar.height;
        if width <= 0 || height <= 0 {
            return Err(Error::new(E_INVALIDARG, "taskbar has invalid dimensions"));
        }

        unsafe {
            let h_module = GetModuleHandleW(windows::core::PCWSTR::null())?;

            // 注册窗口类
            let wc: WNDCLASSEXW = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(h_module.0),
                lpszClassName: w!("Panon_Overlay_2024"),
                ..Default::default()
            };
            let atom = RegisterClassExW(&wc);
            if atom == 0 {
                return Err(Error::from_win32());
            }

            // 创建分层窗口
            let ex_style = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE;
            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                ex_style,
                w!("Panon_Overlay_2024"),
                w!("Panon Overlay"),
                style,
                taskbar.x,
                taskbar.y,
                width,
                height,
                HWND::default(),
                HMENU::default(),
                HINSTANCE(h_module.0),
                None,
            )?;

            // 创建 DIB Section (32bpp BGRA, top-down)
            // GetDC(HWND::default()) 获取屏幕 DC（null HWND = 整个屏幕）
            let hdc_screen = GetDC(HWND::default());
            if hdc_screen.is_invalid() {
                let _ = DestroyWindow(hwnd);
                return Err(Error::from_win32());
            }
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            if hdc_mem.is_invalid() {
                ReleaseDC(HWND::default(), hdc_screen);
                let _ = DestroyWindow(hwnd);
                return Err(Error::from_win32());
            }

            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = width;
            bmi.bmiHeader.biHeight = -height; // top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = 0; // BI_RGB

            let mut p_bits: *mut std::ffi::c_void = ptr::null_mut();
            let h_bitmap = CreateDIBSection(
                hdc_screen,
                &bmi,
                DIB_RGB_COLORS,
                &mut p_bits,
                HANDLE::default(),
                0,
            )?;
            if p_bits.is_null() {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(HWND::default(), hdc_screen);
                let _ = DestroyWindow(hwnd);
                return Err(Error::from_win32());
            }
            SelectObject(hdc_mem, h_bitmap);

            // 显示窗口
            ShowWindow(hwnd, SW_SHOW);

            Ok(Self {
                hwnd,
                hdc_screen,
                hdc_mem,
                h_bitmap,
                p_bits: p_bits as *mut u32,
                width,
                height,
                renderer: SpectrumRenderer::new(),
            })
        }
    }

    /// 渲染一帧到 DIB 并更新分层窗口
    pub unsafe fn render(&mut self, left: &[f32], right: &[f32]) {
        if self.p_bits.is_null() {
            return;
        }
        self.renderer
            .render_to_pixels(left, right, self.p_bits, self.width, self.height);
        let _ = self.update_layered_window();
    }

    unsafe fn update_layered_window(&self) -> windows::core::Result<()> {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pt_size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let pt_src = POINT { x: 0, y: 0 };

        UpdateLayeredWindow(
            self.hwnd,
            HDC::default(),
            None,
            Some(&pt_size),
            self.hdc_mem,
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }

    /// 维护 Z-order（overlay 与 taskbar 的层叠关系）
    /// overlay_mode: 1=Under (taskbar 覆盖频谱), 2=Above (频谱覆盖 taskbar)
    pub unsafe fn ensure_z_order(&self, taskbar_hwnd: HWND, overlay_mode: u8) {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        let dwp = match BeginDeferWindowPos(2) {
            Ok(d) => d,
            Err(_) => return,
        };

        // Under 模式: overlay 先入栈（底层），taskbar 后入栈（上层）
        // Above 模式: taskbar 先入栈（底层），overlay 后入栈（上层）
        let (first_hwnd, second_hwnd) = if overlay_mode == 2 {
            (taskbar_hwnd, self.hwnd)
        } else {
            (self.hwnd, taskbar_hwnd)
        };

        let dwp = match DeferWindowPos(dwp, first_hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags) {
            Ok(d) => d,
            Err(_) => return,
        };
        let dwp = match DeferWindowPos(dwp, second_hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags) {
            Ok(d) => d,
            Err(_) => return,
        };

        let _ = EndDeferWindowPos(dwp);
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub fn width(&self) -> i32 {
        self.width
    }
    pub fn height(&self) -> i32 {
        self.height
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.h_bitmap.is_invalid() {
                let _ = DeleteObject(self.h_bitmap);
            }
            if !self.hdc_mem.is_invalid() {
                let _ = DeleteDC(self.hdc_mem);
            }
            if !self.hdc_screen.is_invalid() {
                ReleaseDC(HWND::default(), self.hdc_screen);
            }
            if !self.hwnd.is_invalid() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

/// 默认窗口过程
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
