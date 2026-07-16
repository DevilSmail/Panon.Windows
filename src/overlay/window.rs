// window.rs — 分层覆盖窗口（← LayeredOverlayWindow.cs）
// CreateWindowEx(WS_EX_LAYERED) + DIB Section + UpdateLayeredWindow
// 30 FPS 渲染，per-pixel alpha

use std::mem::size_of;
use std::ptr;

use windows::core::{w, Error};
use windows::Win32::Foundation::{
    COLORREF, E_INVALIDARG, ERROR_CLASS_ALREADY_EXISTS, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, SIZE, WPARAM, GetLastError,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HDC, HBITMAP, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DeferWindowPos, DefWindowProcW, DestroyWindow,
    EndDeferWindowPos, HWND_TOPMOST, HMENU, RegisterClassExW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, ULW_ALPHA, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
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
    /// 关联的任务栏信息（用于 UIA 探测和 Z-order 维护）
    taskbar: TaskbarInfo,
}

impl OverlayWindow {
    /// 创建分层覆盖窗口
    pub fn create(taskbar: &TaskbarInfo, max_height: i32) -> windows::core::Result<Self> {
        let width = taskbar.width;
        let mut height = taskbar.height;
        if width <= 0 || height <= 0 {
            return Err(Error::new(E_INVALIDARG, "taskbar has invalid dimensions"));
        }
        if max_height > 0 {
            let desired = max_height;
            if desired < height {
                height = desired;
            }
        }
        if height <= 0 {
            return Err(Error::new(E_INVALIDARG, "overlay has invalid height"));
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
                let err = GetLastError();
                if err != ERROR_CLASS_ALREADY_EXISTS {
                    return Err(Error::from_win32());
                }
            }

            // 创建分层窗口
            let ex_style = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE;
            let style = WS_POPUP;

            // 与 C# 对齐：overlayY = taskbarInfo.Y + (taskbarInfo.Height - _height)
            // 适用于所有任务栏位置（底部/顶部/左侧/右侧）
            let y = if max_height > 0 {
                taskbar.y + (taskbar.height - height)
            } else {
                taskbar.y
            };

            let hwnd = CreateWindowExW(
                ex_style,
                w!("Panon_Overlay_2024"),
                w!("Panon Overlay"),
                style,
                taskbar.x,
                y,
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
            let _ = ShowWindow(hwnd, SW_SHOW);

            Ok(Self {
                hwnd,
                hdc_screen,
                hdc_mem,
                h_bitmap,
                p_bits: p_bits as *mut u32,
                width,
                height,
                renderer: SpectrumRenderer::new(),
                taskbar: taskbar.clone(),
            })
        }
    }

    /// 更新空白区域（FillMode=1 时由主循环定期调用）
    /// min_bar_width: 最小可用间隙宽度（通常 = bar_width + gap_width）
    pub fn update_free_regions(&mut self, min_bar_width: i32) {
        let regions = crate::taskbar::uia::get_free_regions(&self.taskbar, min_bar_width);
        self.renderer.free_regions = Some(regions);
    }

    /// 调整覆盖窗口高度（重建 DIB Section + 重新定位，不销毁 HWND）
    /// 由渲染线程调用（持有 overlay 锁），GDI 操作在渲染线程安全执行
    pub unsafe fn set_max_height(&mut self, max_height: i32) {
        let new_height = if max_height > 0 && max_height < self.taskbar.height {
            max_height
        } else {
            self.taskbar.height
        };

        if new_height == self.height {
            return;
        }

        // 1. 先创建新 DIB Section
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = self.width;
        bmi.bmiHeader.biHeight = -new_height;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0;

        let mut p_bits: *mut std::ffi::c_void = ptr::null_mut();
        let new_bitmap = match CreateDIBSection(
            self.hdc_screen, &bmi, DIB_RGB_COLORS, &mut p_bits, HANDLE::default(), 0,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        // 2. SelectObject 新 bitmap → 自动 deselect 旧 bitmap
        let old_bitmap = SelectObject(self.hdc_mem, new_bitmap);

        // 3. 安全删除旧 bitmap（已不在 DC 中）
        if !old_bitmap.is_invalid() {
            let _ = DeleteObject(old_bitmap);
        }

        self.h_bitmap = new_bitmap;
        self.p_bits = p_bits as *mut u32;
        self.height = new_height;

        // 4. 重新定位窗口
        let new_y = self.taskbar.y + (self.taskbar.height - new_height);
        let _ = SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            self.taskbar.x,
            new_y,
            self.width,
            new_height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }

    /// 获取关联的任务栏信息
    pub fn taskbar(&self) -> &TaskbarInfo {
        &self.taskbar
    }

    /// 诊断：填充整个 overlay 为纯色（确认窗口位置和 UpdateLayeredWindow 正常）
    #[allow(dead_code)]
    pub unsafe fn fill_solid(&mut self, r: u8, g: u8, b: u8) {
        if self.p_bits.is_null() {
            return;
        }
        let total = (self.width * self.height) as usize;
        let color = ((255u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        std::slice::from_raw_parts_mut(self.p_bits, total).fill(color);
        let _ = self.update_layered_window();
    }

    /// 渲染一帧到 DIB 并更新分层窗口
    pub unsafe fn render(&mut self, left: &[f32], right: &[f32]) {
        if self.p_bits.is_null() {
            return;
        }
        self.renderer
            .render_to_pixels(left, right, self.p_bits, self.width, self.height);
        if let Err(e) = self.update_layered_window() {
            eprintln!("[error] UpdateLayeredWindow failed: {}", e);
        }
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

    #[allow(dead_code)]
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

// 安全：OverlayWindow 持有的原始指针（DIB、HDC、HWND）在创建后不变且不移动，
// UpdateLayeredWindow 可以从任意线程调用（MSDN 明确声明）。
// 渲染线程独占访问这些资源，主线程仅在创建/销毁时访问。
unsafe impl Send for OverlayWindow {}
unsafe impl Sync for OverlayWindow {}

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
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
