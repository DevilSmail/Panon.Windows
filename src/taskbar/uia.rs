// uia.rs — UI Automation 按钮探测（← UiaInterop.cs + TaskbarHelper.cs flyout 防御）
// IUIAutomation 遍历 + HWND 回退 + Flyout 防御稳定系统
// COM 引用由 windows crate 的 Drop 确定性释放，无 C# GC Finalizer 泄漏问题

use std::sync::Mutex;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, RECT, BOOL, LPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetWindowRect};

use crate::taskbar::detect::TaskbarInfo;

/// 缓存有效期 500ms（正常）/ 3s（空闲模式）
const REFRESH_INTERVAL_ACTIVE_MS: u128 = 500;
const REFRESH_INTERVAL_IDLE_MS: u128 = 3000;
/// UIA 树递归最大深度（任务栏通常 3-5 层）
const MAX_DEPTH: u32 = 10;
/// HWND 树递归最大深度
const MAX_HWND_DEPTH: u32 = 8;
/// Y / 高度 容差（Win11 DPI 缩放下系统托盘元素可能有微小偏差）
const TOLERANCE_PX: i32 = 6;
/// Flyout 防御：连续确认阈值
const STABLE_CONFIRM_COUNT: u32 = 3;
const EMPTY_STABLE_COUNT: u32 = 8;

struct UiaState {
    hwnd: isize,
    taskbar_width: i32,
    taskbar_top: i32,
    last_refresh: Instant,

    cached_uia_rects: Vec<(i32, i32)>,
    cached_merged: Option<Vec<(i32, i32)>>,
    cached_regions: Vec<(i32, i32)>,
    cached_min_bar_width: i32,

    // Flyout 防御（对齐 C# TaskbarHelper）
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
            hwnd: 0,
            taskbar_width: 0,
            taskbar_top: 0,
            last_refresh: Instant::now(),
            cached_uia_rects: Vec::new(),
            cached_merged: None,
            cached_regions: Vec::new(),
            cached_min_bar_width: 0,
            stable_regions: None,
            stable_candidate: None,
            last_good_regions: None,
            good_confirm_count: 0,
            empty_confirm_count: 0,
            idle_mode: false,
        }
    }
}

static UIA_STATE: Mutex<Option<UiaState>> = Mutex::new(None);

/// 设置空闲模式（静音/暂停时拉长到 3s）
pub fn set_idle_mode(idle: bool) {
    let mut state = UIA_STATE.lock().unwrap();
    if let Some(ref mut s) = *state {
        if s.idle_mode != idle {
            s.idle_mode = idle;
            s.last_refresh = Instant::now() - std::time::Duration::from_millis(3001);
        }
    }
}

/// 获取任务栏空白区域（含 500ms/3s 缓存 + flyout 防御）
pub fn get_free_regions(taskbar: &TaskbarInfo, min_bar_width: i32) -> Vec<(i32, i32)> {
    let taskbar_hwnd = HWND(taskbar.hwnd as *mut _);
    let tw = taskbar.width;
    let taskbar_top = taskbar.y;

    let mut state_guard = UIA_STATE.lock().unwrap();
    let state = state_guard.get_or_insert_with(UiaState::new);

    let refresh_interval = if state.idle_mode {
        REFRESH_INTERVAL_IDLE_MS
    } else {
        REFRESH_INTERVAL_ACTIVE_MS
    };

    // ── UIA + HWND 缓存刷新 ──
    let uia_stale = state.hwnd != taskbar.hwnd
        || state.taskbar_width != tw
        || state.taskbar_top != taskbar_top
        || state.last_refresh.elapsed().as_millis() >= refresh_interval;

    if uia_stale {
        let taskbar_rect = RECT {
            left: taskbar.x,
            top: taskbar.y,
            right: taskbar.x + taskbar.width,
            bottom: taskbar.y + taskbar.height,
        };
        let mut uia_rects = collect_uia_rects(taskbar_hwnd, taskbar_rect)
            .unwrap_or_else(|_| Vec::new());

        // HWND 回退：Win11 XAML 任务栏按钮不暴露 UIA BoundingRectangle，
        // 但第三方 DeskBand（如 TrafficMonitor）有真实 HWND
        let hwnd_rects = collect_child_hwnd_rects(taskbar_hwnd, taskbar_rect);
        uia_rects.extend(hwnd_rects);

        state.cached_uia_rects = uia_rects;
        state.hwnd = taskbar.hwnd;
        state.taskbar_width = tw;
        state.taskbar_top = taskbar_top;
        state.last_refresh = Instant::now();
        state.cached_merged = None;
        state.cached_regions = Vec::new();
    }

    // ── 合并重叠区域 + 计算空白区域 ──
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
                if gap_width >= min_bar_width {
                    regions.push((pos, gap_width));
                }
                pos = pos.max(x + w);
            }
            let last_gap = tw - pos;
            if last_gap >= min_bar_width {
                regions.push((pos, last_gap));
            }
            state.cached_regions = regions;
            state.cached_min_bar_width = min_bar_width;
        }
    }

    // ── Flyout 防御回退（双计数器稳定窗口，对齐 C#）──
    let current_result = state.cached_regions.clone();

    if !current_result.is_empty() {
        if state.empty_confirm_count > 0 {
            state.empty_confirm_count = 0;
        }
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

        drop(state_guard);
        current_result
    } else {
        state.good_confirm_count = 0;
        state.stable_candidate = None;
        state.empty_confirm_count += 1;

        if state.empty_confirm_count >= EMPTY_STABLE_COUNT && state.stable_regions.is_some() {
            state.stable_regions = None;
        }

        let fallback = state
            .stable_regions
            .clone()
            .or_else(|| state.last_good_regions.clone())
            .unwrap_or_else(|| current_result.clone());

        drop(state_guard);
        fallback
    }
}

fn regions_equal(a: Option<&Vec<(i32, i32)>>, b: &[(i32, i32)]) -> bool {
    match a {
        None => false,
        Some(a) => {
            if a.len() != b.len() { return false; }
            for i in 0..a.len() {
                if a[i].0 != b[i].0 || a[i].1 != b[i].1 { return false; }
            }
            true
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// UIA 遍历
// ═══════════════════════════════════════════════════════════════════

fn collect_uia_rects(
    taskbar_hwnd: HWND,
    taskbar_rect: RECT,
) -> windows::core::Result<Vec<(i32, i32)>> {
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

/// 递归收集 UIA 子元素的 BoundingRectangle
/// 三重过滤（与 C# UiaInterop.CollectElementRects 一致）：
/// 1. 宽度 > 0 且 < 80% 任务栏宽
/// 2. Y 坐标在任务栏范围内（TOLERANCE_PX 容差，处理 DPI 舍入）
/// 3. 高度 ≤ 任务栏高度 + 容差（排除 flyout 弹窗）
///
/// Win11 特判：XAML 任务栏按钮返回 (0,0,0,0) 空矩形
/// → 跳过当前元素但始终递归子元素（子元素可能有有效坐标）
fn collect_element_rects(
    walker: &IUIAutomationTreeWalker,
    el: &IUIAutomationElement,
    taskbar_rect: RECT,
    result: &mut Vec<(i32, i32)>,
    depth: u32,
) {
    if depth >= MAX_DEPTH { return; }

    unsafe {
        let mut child = match walker.GetFirstChildElement(el) {
            Ok(c) => c,
            Err(_) => return,
        };

        loop {
            if let Ok(rect) = child.CurrentBoundingRectangle() {
                let tw = taskbar_rect.right - taskbar_rect.left;
                let cw = rect.right - rect.left;
                let ch = rect.bottom - rect.top;
                let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;

                // Win11 XAML 元素可能返回 (0,0,0,0) 空矩形
                let is_empty_rect = rect.left == 0 && rect.top == 0
                    && rect.right == 0 && rect.bottom == 0;

                let within_taskbar_y = is_empty_rect || (
                    rect.top >= taskbar_rect.top - TOLERANCE_PX
                    && rect.top < taskbar_rect.bottom
                );
                let height_reasonable = is_empty_rect || (
                    ch <= taskbar_height + TOLERANCE_PX
                );

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

                // 递归：空矩形始终递归（子元素可能有有效坐标）
                if is_empty_rect || (within_taskbar_y && height_reasonable) {
                    collect_element_rects(walker, &child, taskbar_rect, result, depth + 1);
                }
            } else {
                // BoundingRectangle 失败时仍递归（与 C# 行为一致）
                collect_element_rects(walker, &child, taskbar_rect, result, depth + 1);
            }

            match walker.GetNextSiblingElement(&child) {
                Ok(next) => child = next,
                Err(_) => break,
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// HWND 回退方案
// ═══════════════════════════════════════════════════════════════════

/// 递归枚举 Shell_TrayWnd 的子窗口
/// 用于检测 Win11 XAML 渲染中 UIA 无法获取位置的控件
/// 以及第三方 DeskBand（如 TrafficMonitor）——它们有真实 HWND
fn collect_child_hwnd_rects(
    taskbar_hwnd: HWND,
    taskbar_rect: RECT,
) -> Vec<(i32, i32)> {
    unsafe {
        let tw = taskbar_rect.right - taskbar_rect.left;
        let mut rects: Vec<(i32, i32)> = Vec::new();

        enumerate_hwnd_children(taskbar_hwnd, taskbar_rect, &mut rects, 0);

        // 过滤宽度（与 UIA 一致）
        rects.into_iter()
            .filter_map(|(abs_x, abs_w)| {
                if abs_w > 0 && abs_w < tw * 4 / 5 {
                    let mut rel_x = abs_x - taskbar_rect.left;
                    let mut cw = abs_w;
                    if rel_x < 0 { cw += rel_x; rel_x = 0; }
                    if rel_x + cw > tw { cw = tw - rel_x; }
                    if cw > 0 { Some((rel_x, cw)) } else { None }
                } else {
                    None
                }
            })
            .collect()
    }
}

unsafe fn enumerate_hwnd_children(
    hwnd: HWND,
    taskbar_rect: RECT,
    result: &mut Vec<(i32, i32)>,
    depth: u32,
) {
    if depth > MAX_HWND_DEPTH { return; }

    // 收集 hwnd 本身的 rect（跳过 depth=0 的任务栏自身）
    if depth > 0 {
        let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_ok() {
            let w = wr.right - wr.left;
            let h = wr.bottom - wr.top;
            let within_y = wr.top >= taskbar_rect.top - TOLERANCE_PX
                && wr.top < taskbar_rect.bottom;
            let height_ok = h <= taskbar_height + TOLERANCE_PX;
            if w > 0 && within_y && height_ok {
                result.push((wr.left, w));
            }
        }
    }

    // 递归枚举子窗口
    let result_ptr = result as *mut Vec<(i32, i32)>;
    let enum_ctx = HwndEnumCtx {
        result: result_ptr,
        taskbar_rect,
        depth: depth + 1,
    };

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

struct HwndEnumCtx {
    result: *mut Vec<(i32, i32)>,
    taskbar_rect: RECT,
    depth: u32,
}
