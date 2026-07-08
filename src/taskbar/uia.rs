// uia.rs — UI Automation 按钮探测（← UiaInterop.cs）
// 阶段 5 实现：IUIAutomation 遍历 + Y 坐标过滤修复 + 500ms 缓存
// COM 引用由 windows crate 的 Drop 确定性释放，无 C# GC Finalizer 泄漏问题

use std::sync::Mutex;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
};

use crate::taskbar::detect::TaskbarInfo;

/// 缓存有效期 500ms（降低 COM 调用开销）
const CACHE_TTL_MS: u128 = 500;
/// UIA 树递归最大深度（任务栏通常 3-5 层）
const MAX_DEPTH: u32 = 10;

struct UiaCache {
    hwnd: isize,
    regions: Vec<(i32, i32)>,
    timestamp: Instant,
}

static UIA_CACHE: Mutex<Option<UiaCache>> = Mutex::new(None);

/// 获取任务栏空白区域（含 500ms 缓存）
/// 返回相对于任务栏左上角的 (x, width) 列表
/// FillMode=1 时由渲染器使用，仅在按钮间隙显示频谱
pub fn get_free_regions(taskbar: &TaskbarInfo, min_bar_width: i32) -> Vec<(i32, i32)> {
    // 检查缓存
    {
        let cache = UIA_CACHE.lock().unwrap();
        if let Some(ref c) = *cache {
            if c.hwnd == taskbar.hwnd && c.timestamp.elapsed().as_millis() < CACHE_TTL_MS {
                return c.regions.clone();
            }
        }
    }

    // 重新探测
    let taskbar_hwnd = HWND(taskbar.hwnd as *mut _);
    let taskbar_rect = RECT {
        left: taskbar.x,
        top: taskbar.y,
        right: taskbar.x + taskbar.width,
        bottom: taskbar.y + taskbar.height,
    };

    // UIA 探测失败时回退到铺满模式（整个任务栏都是空白）
    let regions = collect_free_regions(taskbar_hwnd, taskbar_rect, min_bar_width)
        .unwrap_or_else(|_| vec![(0, taskbar.width)]);

    // 更新缓存
    {
        let mut cache = UIA_CACHE.lock().unwrap();
        *cache = Some(UiaCache {
            hwnd: taskbar.hwnd,
            regions: regions.clone(),
            timestamp: Instant::now(),
        });
    }

    regions
}

/// 通过 IUIAutomation 遍历任务栏子元素，收集控件矩形并计算空白区域
fn collect_free_regions(
    taskbar_hwnd: HWND,
    taskbar_rect: RECT,
    min_bar_width: i32,
) -> windows::core::Result<Vec<(i32, i32)>> {
    unsafe {
        // COM 初始化（主线程 STA，已初始化时返回 S_FALSE，忽略）
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        // 创建 IUIAutomation 实例（CUIAutomation CLSID = ff48dba4-60ef-4201-aa87-54103eef594e）
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)?;

        // 从任务栏句柄获取根 UIA 元素
        let root = automation.ElementFromHandle(taskbar_hwnd)?;

        // 获取 RawView TreeWalker（包含所有元素类型）
        let walker = automation.RawViewWalker()?;

        // 递归收集所有子元素的 BoundingRectangle
        let mut element_rects: Vec<(i32, i32)> = Vec::new();
        collect_element_rects(&walker, &root, taskbar_rect, &mut element_rects, 0);

        // 合并重叠矩形 + 计算空白区域
        Ok(compute_free_regions(&element_rects, taskbar_rect, min_bar_width))
    }
}

/// 递归收集 UIA 子元素的 BoundingRectangle
/// 含 Y 坐标过滤：排除不在任务栏高度范围内的弹窗元素（如 Quick Settings / Volume flyout）
fn collect_element_rects(
    walker: &IUIAutomationTreeWalker,
    el: &IUIAutomationElement,
    taskbar_rect: RECT,
    result: &mut Vec<(i32, i32)>,
    depth: u32,
) {
    if depth >= MAX_DEPTH {
        return;
    }

    unsafe {
        let mut child = match walker.GetFirstChildElement(el) {
            Ok(c) => c,
            Err(_) => return,
        };

        loop {
            // 收集当前子元素的矩形
            if let Ok(rect) = child.CurrentBoundingRectangle() {
                let cw = rect.right - rect.left;
                let tw = taskbar_rect.right - taskbar_rect.left;

                // 过滤条件：
                // 1. 宽度 > 0（跳过零宽元素）
                // 2. 宽度 < 80% 任务栏宽（排除全宽容器）
                // 3. Y 坐标在任务栏范围内（排除弹出的 flyout — §八 修复）
                if cw > 0
                    && cw < tw * 4 / 5
                    && rect.top >= taskbar_rect.top
                    && rect.top < taskbar_rect.bottom
                {
                    let cx = (rect.left - taskbar_rect.left).max(0);
                    let cw = cw.min(tw - cx);
                    if cw > 0 {
                        result.push((cx, cw));
                    }
                }
            }

            // 递归遍历子元素
            collect_element_rects(walker, &child, taskbar_rect, result, depth + 1);

            // 下一个兄弟元素（旧 child 在赋值时 Drop → COM Release 立即调用）
            match walker.GetNextSiblingElement(&child) {
                Ok(next) => child = next,
                Err(_) => break,
            }
        }
    }
}

/// 合并重叠矩形 + 计算空白区域（任务栏矩形减去所有控件矩形）
fn compute_free_regions(
    element_rects: &[(i32, i32)],
    taskbar_rect: RECT,
    min_bar_width: i32,
) -> Vec<(i32, i32)> {
    let tw = taskbar_rect.right - taskbar_rect.left;

    if element_rects.is_empty() {
        return vec![(0, tw)];
    }

    // 按起始 X 坐标排序
    let mut rects: Vec<(i32, i32)> = element_rects.to_vec();
    rects.sort_by_key(|r| r.0);

    // 合并重叠矩形
    let mut merged: Vec<(i32, i32)> = Vec::new();
    for &(x, w) in &rects {
        if let Some(last) = merged.last_mut() {
            if x <= last.0 + last.1 {
                // 重叠 → 扩展最后一个矩形
                let end = (last.0 + last.1).max(x + w);
                last.1 = end - last.0;
                continue;
            }
        }
        merged.push((x, w));
    }

    // 计算空白区域（控件之间的间隙）
    let mut free: Vec<(i32, i32)> = Vec::new();
    let mut prev_end = 0i32;

    for &(x, w) in &merged {
        if x > prev_end && x - prev_end >= min_bar_width {
            free.push((prev_end, x - prev_end));
        }
        prev_end = (x + w).max(prev_end);
    }

    // 最后一段
    if prev_end < tw && tw - prev_end >= min_bar_width {
        free.push((prev_end, tw - prev_end));
    }

    free
}
