// uia.rs — UI Automation 按钮探测（← UiaInterop.cs + TaskbarHelper.cs flyout 防御）
// IUIAutomation 遍历 + HWND 子窗口 + 顶层窗口 回退 + Flyout 防御稳定系统

use std::sync::Mutex;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, RECT, BOOL, LPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetWindowRect,
};

use crate::taskbar::detect::TaskbarInfo;

/// UIA 容差：严格对齐 C#（零容差），flyout 防御依赖此值
const UIA_TOLERANCE_PX: i32 = 0;
/// HWND 容差：Win11 DPI 缩放下系统托盘元素可能有微小偏差
const HWND_TOLERANCE_PX: i32 = 6;

/// 缓存有效期 500ms（正常）/ 3s（空闲模式）
const REFRESH_INTERVAL_ACTIVE_MS: u128 = 500;
const REFRESH_INTERVAL_IDLE_MS: u128 = 3000;
/// UIA 树递归最大深度
const MAX_DEPTH: u32 = 10;
const MAX_HWND_DEPTH: u32 = 8;
/// Flyout 防御：连续确认阈值
const STABLE_CONFIRM_COUNT: u32 = 3;
const EMPTY_STABLE_COUNT: u32 = 8;

struct UiaState {
    hwnd: isize,
    taskbar_width: i32,
    taskbar_top: i32,
    last_refresh: Instant,

    cached_uia_rects: Vec<(i32, i32)>,
    cached_hwnd_rects: Vec<(i32, i32)>,
    cached_merged: Option<Vec<(i32, i32)>>,
    cached_regions: Vec<(i32, i32)>,
    cached_min_bar_width: i32,

    // Flyout 防御
    stable_regions: Option<Vec<(i32, i32)>>,
    stable_candidate: Option<Vec<(i32, i32)>>,
    last_good_regions: Option<Vec<(i32, i32)>>,
    good_confirm_count: u32,
    empty_confirm_count: u32,

    idle_mode: bool,
}

impl UiaState {
    fn new() -> Self {
        Self {
            hwnd: 0, taskbar_width: 0, taskbar_top: 0,
            last_refresh: Instant::now(),
            cached_uia_rects: Vec::new(), cached_hwnd_rects: Vec::new(),
            cached_merged: None, cached_regions: Vec::new(), cached_min_bar_width: 0,
            stable_regions: None, stable_candidate: None, last_good_regions: None,
            good_confirm_count: 0, empty_confirm_count: 0,
            idle_mode: false,
        }
    }
}

static UIA_STATE: Mutex<Option<UiaState>> = Mutex::new(None);

pub fn set_idle_mode(idle: bool) {
    let mut state = UIA_STATE.lock().unwrap();
    if let Some(ref mut s) = *state {
        if s.idle_mode != idle {
            s.idle_mode = idle;
            s.last_refresh = Instant::now() - std::time::Duration::from_millis(3001);
        }
    }
}

pub fn get_free_regions(taskbar: &TaskbarInfo, min_bar_width: i32) -> Vec<(i32, i32)> {
    let taskbar_hwnd = HWND(taskbar.hwnd as *mut _);
    let tw = taskbar.width;
    let taskbar_top = taskbar.y;

    let mut state_guard = UIA_STATE.lock().unwrap();
    let state = state_guard.get_or_insert_with(UiaState::new);

    let refresh_interval = if state.idle_mode { REFRESH_INTERVAL_IDLE_MS } else { REFRESH_INTERVAL_ACTIVE_MS };

    let uia_stale = state.hwnd != taskbar.hwnd
        || state.taskbar_width != tw
        || state.taskbar_top != taskbar_top
        || state.last_refresh.elapsed().as_millis() >= refresh_interval;

    let taskbar_rect = RECT {
        left: taskbar.x, top: taskbar.y,
        right: taskbar.x + taskbar.width,
        bottom: taskbar.y + taskbar.height,
    };

    if uia_stale {
        // 1. UIA（对齐 C#，零容差）
        let uia_rects = collect_uia_rects(taskbar_hwnd, taskbar_rect).unwrap_or_default();
        state.cached_uia_rects = uia_rects;

        state.hwnd = taskbar.hwnd;
        state.taskbar_width = tw;
        state.taskbar_top = taskbar_top;
        state.last_refresh = Instant::now();
        state.cached_merged = None;
        state.cached_regions = Vec::new();
    }

    // HWND 回退：只在任务栏切换时刷新，不受 UIA 刷新周期影响
    // flyout 期间 TrafficMonitor 窗口可能被隐藏，刷新会丢失缓存
    let hwnd_stale = state.cached_hwnd_rects.is_empty() || state.hwnd != taskbar.hwnd
        || state.taskbar_width != tw;
    if hwnd_stale {
        let hwnd_rects = collect_child_hwnd_rects(taskbar_hwnd, taskbar_rect);
        state.cached_hwnd_rects = hwnd_rects;
    }

    // ── 计算 UIA-only 空白区域 ──
    if state.cached_uia_rects.is_empty() {
        state.cached_merged = None;
        state.cached_regions = Vec::new();
    } else {
        if state.cached_merged.is_none() {
            let mut sorted = state.cached_uia_rects.clone();
            sorted.sort_by_key(|r| r.0);
            let mut merged: Vec<(i32, i32)> = Vec::new();
            for &(x, w) in &sorted {
                if let Some(last) = merged.last_mut() {
                    if x <= last.0 + last.1 {
                        let end = (last.0 + last.1).max(x + w);
                        last.1 = end - last.0;
                        continue;
                    }
                }
                merged.push((x, w));
            }
            state.cached_merged = Some(merged);
            state.cached_regions = Vec::new();
        }

        if state.cached_regions.is_empty() || state.cached_min_bar_width != min_bar_width {
            let merged = state.cached_merged.as_ref().unwrap();
            let mut regions = Vec::new();
            let mut pos = 0i32;
            for &(x, w) in merged {
                let gap_width = x - pos;
                if gap_width >= min_bar_width { regions.push((pos, gap_width)); }
                pos = pos.max(x + w);
            }
            let last_gap = tw - pos;
            if last_gap >= min_bar_width { regions.push((pos, last_gap)); }
            state.cached_regions = regions;
            state.cached_min_bar_width = min_bar_width;
        }
    }

    // ── Flyout 防御（UIA-only，完全对齐 C#）──
    let uia_result = {
        let current_result = state.cached_regions.clone();

        if !current_result.is_empty() {
            if state.empty_confirm_count > 0 { state.empty_confirm_count = 0; }
            state.last_good_regions = Some(current_result.clone());

            if regions_equal(state.stable_candidate.as_ref(), &current_result) {
                state.good_confirm_count += 1;
            } else {
                state.stable_candidate = Some(current_result.clone());
                state.good_confirm_count = 1;
            }

            if state.good_confirm_count >= STABLE_CONFIRM_COUNT && state.stable_regions.is_none() {
                state.stable_regions = Some(current_result.clone());
            }

            current_result
        } else {
            state.good_confirm_count = 0;
            state.stable_candidate = None;
            state.empty_confirm_count += 1;

            if state.empty_confirm_count >= EMPTY_STABLE_COUNT && state.stable_regions.is_some() {
                state.stable_regions = None;
            }

            state.stable_regions.clone()
                .or_else(|| state.last_good_regions.clone())
                .unwrap_or_else(|| current_result.clone())
        }
    };
    drop(state_guard);

    // ── 叠加 HWND 回退 rects（从 UIA 空白区域中减去）──
    let hwnd = { UIA_STATE.lock().unwrap().as_ref().map(|s| s.cached_hwnd_rects.clone()).unwrap_or_default() };
    subtract_hwnd_from_regions(&uia_result, &hwnd, min_bar_width)
}

fn regions_equal(a: Option<&Vec<(i32, i32)>>, b: &[(i32, i32)]) -> bool {
    match a {
        None => false,
        Some(a) => {
            if a.len() != b.len() { return false; }
            for i in 0..a.len() { if a[i].0 != b[i].0 || a[i].1 != b[i].1 { return false; } }
            true
        }
    }
}

fn subtract_hwnd_from_regions(regions: &[(i32, i32)], hwnd_rects: &[(i32, i32)], min_bar_width: i32) -> Vec<(i32, i32)> {
    if hwnd_rects.is_empty() { return regions.to_vec(); }
    let mut result = regions.to_vec();
    for &(hx, hw) in hwnd_rects {
        let h_end = hx + hw;
        let mut i = 0;
        while i < result.len() {
            let (rx, rw) = result[i];
            let r_end = rx + rw;
            if h_end <= rx || hx >= r_end { i += 1; continue; }
            if hx <= rx && h_end >= r_end { result.remove(i); }
            else if hx <= rx && h_end < r_end {
                let new_w = r_end - h_end;
                if new_w >= min_bar_width { result[i] = (h_end, new_w); i += 1; }
                else { result.remove(i); }
            } else if hx > rx && h_end >= r_end {
                let new_w = hx - rx;
                if new_w >= min_bar_width { result[i] = (rx, new_w); i += 1; }
                else { result.remove(i); }
            } else {
                let left_w = hx - rx;
                let right_w = r_end - h_end;
                result.remove(i);
                if right_w >= min_bar_width { result.insert(i, (h_end, right_w)); }
                if left_w >= min_bar_width { result.insert(i, (rx, left_w)); i += 1; }
                if right_w >= min_bar_width { i += 1; }
            }
        }
    }
    result
}

// ═══════════════════════════════════════════════════════════════════
// UIA 遍历
// ═══════════════════════════════════════════════════════════════════

fn collect_uia_rects(taskbar_hwnd: HWND, taskbar_rect: RECT) -> windows::core::Result<Vec<(i32, i32)>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)?;
        let root = automation.ElementFromHandle(taskbar_hwnd)?;
        let walker = automation.RawViewWalker()?;
        let mut element_rects: Vec<(i32, i32)> = Vec::new();
        collect_element_rects(&walker, &root, taskbar_rect, &mut element_rects, 0);
        Ok(element_rects)
    }
}

fn collect_element_rects(
    walker: &IUIAutomationTreeWalker, el: &IUIAutomationElement,
    taskbar_rect: RECT, result: &mut Vec<(i32, i32)>, depth: u32,
) {
    if depth >= MAX_DEPTH { return; }
    unsafe {
        let mut child = match walker.GetFirstChildElement(el) { Ok(c) => c, Err(_) => return };
        loop {
            // 获取 BoundingRectangle
            let rect = match child.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => {
                    // 获取失败时直接递归子元素
                    collect_element_rects(walker, &child, taskbar_rect, result, depth + 1);
                    match walker.GetNextSiblingElement(&child) { Ok(next) => child = next, Err(_) => break }
                    continue;
                }
            };

            let tw = taskbar_rect.right - taskbar_rect.left;
            let cw = rect.right - rect.left;
            let ch = rect.bottom - rect.top;
            let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;

            let is_empty_rect = rect.left == 0 && rect.top == 0 && rect.right == 0 && rect.bottom == 0;

            let within_taskbar_y = is_empty_rect || (rect.top >= taskbar_rect.top - UIA_TOLERANCE_PX && rect.top < taskbar_rect.bottom);
            let height_reasonable = is_empty_rect || (ch <= taskbar_height + UIA_TOLERANCE_PX);

            let element_x = rect.left - taskbar_rect.left;
            let passes_width = cw > 0 && cw < tw * 4 / 5;
            let all_pass = passes_width && within_taskbar_y && height_reasonable && !is_empty_rect;

            if all_pass {
                let mut cx = element_x;
                let mut cw_clipped = cw;
                if cx < 0 { cw_clipped += cx; cx = 0; }
                if cx + cw_clipped > tw { cw_clipped = tw - cx; }
                if cw_clipped > 0 { result.push((cx, cw_clipped)); }
            }

            if is_empty_rect || (within_taskbar_y && height_reasonable) {
                collect_element_rects(walker, &child, taskbar_rect, result, depth + 1);
            }

            match walker.GetNextSiblingElement(&child) { Ok(next) => child = next, Err(_) => break }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// HWND 回退方案
// ═══════════════════════════════════════════════════════════════════

fn collect_child_hwnd_rects(taskbar_hwnd: HWND, taskbar_rect: RECT) -> Vec<(i32, i32)> {
    unsafe {
        let tw = taskbar_rect.right - taskbar_rect.left;
        let mut rects: Vec<(i32, i32)> = Vec::new();
        enumerate_hwnd_children(taskbar_hwnd, taskbar_rect, &mut rects, 0);
        rects.into_iter().filter_map(|(abs_x, abs_w)| {
            if abs_w > 0 && abs_w < tw * 4 / 5 {
                let mut rel_x = abs_x - taskbar_rect.left;
                let mut cw = abs_w;
                if rel_x < 0 { cw += rel_x; rel_x = 0; }
                if rel_x + cw > tw { cw = tw - rel_x; }
                if cw > 0 { Some((rel_x, cw)) } else { None }
            } else { None }
        }).collect()
    }
}

unsafe fn enumerate_hwnd_children(hwnd: HWND, taskbar_rect: RECT, result: &mut Vec<(i32, i32)>, depth: u32) {
    if depth > MAX_HWND_DEPTH { return; }
    if depth > 0 {
        let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_ok() {
            let w = wr.right - wr.left;
            let h = wr.bottom - wr.top;
            if w > 0 && wr.top >= taskbar_rect.top - HWND_TOLERANCE_PX
                && wr.top < taskbar_rect.bottom && h <= taskbar_height + HWND_TOLERANCE_PX {
                result.push((wr.left, w));
            }
        }
    }
    let result_ptr = result as *mut Vec<(i32, i32)>;
    let enum_ctx = HwndEnumCtx { result: result_ptr, taskbar_rect, depth: depth + 1 };
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut HwndEnumCtx);
        enumerate_hwnd_children(hwnd, ctx.taskbar_rect, &mut *ctx.result, ctx.depth);
        BOOL(1)
    }
    let ctx_box = Box::new(enum_ctx);
    let ctx_ptr = Box::into_raw(ctx_box);
    let _ = EnumChildWindows(hwnd, Some(enum_proc), LPARAM(ctx_ptr as isize));
    let _ = Box::from_raw(ctx_ptr);
}

struct HwndEnumCtx { result: *mut Vec<(i32, i32)>, taskbar_rect: RECT, depth: u32 }
