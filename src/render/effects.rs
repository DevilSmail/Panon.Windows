// effects.rs — 7 种视觉效果（← SpectrumRenderer.cs 的 Render* 方法）
// 实现 impl SpectrumRenderer 的 7 种渲染方法 + 公共辅助

use crate::render::color;
use crate::render::renderer::{EXIT_PEAK_DECAY_VALUE, PEAK_DECAY_VALUE, SpectrumRenderer};

impl SpectrumRenderer {
    // ---- 颜色辅助 ----

    #[inline]
    fn get_color(&self, pos: f32) -> (u8, u8, u8) {
        let (hue_from, hue_to, sat, light) = if self.color_space_hsluv {
            (
                self.hsluv_hue_from,
                self.hsluv_hue_to,
                self.hsluv_saturation,
                self.hsluv_lightness,
            )
        } else {
            (self.hsl_hue_from, self.hsl_hue_to, self.hsl_saturation, self.hsl_lightness)
        };
        color::gradient_color(pos, self.color_space_hsluv, hue_from, hue_to, sat, light)
    }

    #[inline]
    fn make_pixel(b: u8, g: u8, r: u8, a: u8) -> u32 {
        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    #[inline]
    fn make_pixel_c(c: (u8, u8, u8)) -> u32 {
        Self::make_pixel(c.0, c.1, c.2, 255)
    }

    // ---- 采样辅助 ----

    fn sample_avg(left: &[f32], right: &[f32], t: f32) -> f32 {
        let len = left.len();
        if len == 0 {
            return 0.0;
        }
        let pos = t * (len - 1) as f32;
        let idx = pos as usize;
        let frac = pos - idx as f32;
        if idx >= len - 1 {
            return (left[len - 1] + right[len - 1]) / 2.0;
        }
        ((left[idx] * (1.0 - frac) + left[idx + 1] * frac)
            + (right[idx] * (1.0 - frac) + right[idx + 1] * frac))
            / 2.0
    }

    fn sample_left(left: &[f32], t: f32) -> f32 {
        let len = left.len();
        if len == 0 {
            return 0.0;
        }
        let pos = t * (len - 1) as f32;
        let idx = pos as usize;
        let frac = pos - idx as f32;
        if idx >= len - 1 {
            return left[len - 1];
        }
        left[idx] * (1.0 - frac) + left[idx + 1] * frac
    }

    fn sample_lr(left: &[f32], right: &[f32], t: f32) -> (f32, f32) {
        let len = left.len();
        if len == 0 {
            return (0.0, 0.0);
        }
        let pos = t * (len - 1) as f32;
        let idx = pos as usize;
        let frac = pos - idx as f32;
        if idx >= len - 1 {
            return (left[len - 1], right[len - 1]);
        }
        let l = left[idx] * (1.0 - frac) + left[idx + 1] * frac;
        let r = right[idx] * (1.0 - frac) + right[idx + 1] * frac;
        (l, r)
    }

    // ---- 区域辅助 ----

    fn effective_regions(&self, width: i32) -> Option<Vec<(i32, i32)>> {
        if self.fill_mode != 1 || self.free_regions.is_none() {
            return None;
        }
        let regions = self.free_regions.as_ref().unwrap();
        if regions.is_empty() {
            return Some(Vec::new());
        }
        Some(regions.clone())
    }

    #[inline]
    fn is_column_visible(&self, x: i32) -> bool {
        if self.fill_mode != 1 || self.free_regions.is_none() {
            return true;
        }
        for &(rx, rw) in self.free_regions.as_ref().unwrap() {
            if x >= rx && x < rx + rw {
                return true;
            }
        }
        false
    }

    // ---- 重采样 ----

    fn resample(&mut self, source: &[f32], src_count: usize, target_count: usize, buffer: &mut Vec<f32>) {
        if src_count == 0 {
            buffer.clear();
            buffer.resize(target_count, 0.0);
            return;
        }
        if src_count == target_count {
            buffer.clear();
            buffer.extend_from_slice(source);
            return;
        }

        if self.cached_src_count != src_count
            || self.cached_target_count != target_count
            || self.resample_indices.len() != target_count
        {
            self.cached_src_count = src_count;
            self.cached_target_count = target_count;
            self.resample_indices.resize(target_count, 0);
            self.resample_fracs.resize(target_count, 0.0);
            for i in 0..target_count {
                let sp = i as f32 / target_count as f32 * src_count as f32;
                self.resample_indices[i] = sp as usize;
                self.resample_fracs[i] = sp - self.resample_indices[i] as f32;
            }
        }

        if buffer.len() != target_count {
            buffer.resize(target_count, 0.0);
        }

        let last_src = src_count - 1;
        for i in 0..target_count {
            let si = self.resample_indices[i];
            buffer[i] = if si >= last_src {
                source[last_src]
            } else {
                source[si] * (1.0 - self.resample_fracs[i])
                    + source[si + 1] * self.resample_fracs[i]
            };
        }
    }

    // ---- 填充行辅助 ----

    #[inline]
    unsafe fn fill_row(pixels: *mut u32, offset: usize, count: usize, value: u32) {
        if count == 0 {
            return;
        }
        let slice = std::slice::from_raw_parts_mut(pixels.add(offset), count);
        slice.fill(value);
    }

    // ============ bar1ch ============

    pub unsafe fn render_bar1ch(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        let regions = self.effective_regions(width);
        if let Some(ref r) = regions {
            if r.is_empty() {
                return;
            }
        }
        let regions = regions.unwrap_or_else(|| vec![(0, width)]);

        let total_free_w: i32 = regions.iter().map(|(_, w)| *w).sum();
        let cell_size = (self.bar_width + self.gap_width).max(1);
        let target_bar_count = (((total_free_w + self.gap_width) / cell_size) as usize).max(1);

        let src_count = left.len();
        // 用 std::mem::take 避免 &mut self 与 &mut self.buffer 的双重借用
        let mut buf_l = std::mem::take(&mut self.buffer_l);
        let mut buf_r = std::mem::take(&mut self.buffer_r);
        self.resample(left, src_count, target_bar_count, &mut buf_l);
        self.resample(right, right.len(), target_bar_count, &mut buf_r);
        self.buffer_l = buf_l;
        self.buffer_r = buf_r;

        let r_l = self.buffer_l.clone();
        let r_r = self.buffer_r.clone();

        if self.peak_heights.len() != target_bar_count {
            self.peak_heights.resize(target_bar_count, 0.0);
        }

        let height_f = height as f32;
        let mut bar_index = 0usize;

        for (reg_x, reg_w) in regions {
            let bar_count_in_region = (((reg_w + self.gap_width) / cell_size) as usize).max(0);
            if bar_count_in_region < 1 {
                continue;
            }
            let rem = reg_w
                - (bar_count_in_region as i32 * self.bar_width
                    + (bar_count_in_region as i32 - 1) * self.gap_width);

            let mut cx = reg_x;
            for k in 0..bar_count_in_region {
                if bar_index >= target_bar_count {
                    break;
                }
                let cw = self.bar_width + if k < rem as usize { 1 } else { 0 };
                let v = (r_l[bar_index] + r_r[bar_index]) / 2.0;
                let v = if self.inversion { 1.0 - v } else { v };
                let c = self.get_color(bar_index as f32 / target_bar_count as f32);
                let px = Self::make_pixel_c(c);

                let bh = if self.gravity == 0 {
                    (v * height_f).max(2.0)
                } else {
                    v * height_f
                };
                let (y_s, y_e) = match self.gravity {
                    0 => (
                        (height_f / 2.0 - bh / 2.0) as i32,
                        (height_f / 2.0 + bh / 2.0) as i32,
                    ),
                    1 => (0, bh as i32),
                    _ => ((height_f - bh) as i32, height),
                };
                let x_s = cx.max(0);
                let x_e = (cx + cw).min(width);
                let y_s = y_s.max(0);
                let y_e = y_e.min(height);

                if bh >= 0.5 && y_e > y_s && x_e > x_s {
                    let w = (x_e - x_s) as usize;
                    for py in y_s..y_e {
                        let offset = (py * width + x_s) as usize;
                        Self::fill_row(pixels, offset, w, px);
                    }
                }

                // 峰值线
                if self.gravity == 1 || self.gravity == 2 {
                    let pd = if self.use_exit_factor {
                        EXIT_PEAK_DECAY_VALUE
                    } else {
                        PEAK_DECAY_VALUE
                    };
                    self.peak_heights[bar_index] = if v > self.peak_heights[bar_index] {
                        v
                    } else {
                        (self.peak_heights[bar_index] - pd).max(0.0)
                    };
                    let ph = self.peak_heights[bar_index] * height_f;
                    let (py_s, py_e) = match self.gravity {
                        1 => {
                            let ps = (ph as i32).max(0);
                            (ps, (ps + 2).min(height))
                        }
                        2 => {
                            let pe = (height - ph as i32).min(height);
                            ((pe - 2).max(0), pe)
                        }
                        _ => (0, 0),
                    };
                    if py_e > py_s && x_e > x_s {
                        let w = (x_e - x_s) as usize;
                        for py in py_s..py_e {
                            let offset = (py * width + x_s) as usize;
                            Self::fill_row(pixels, offset, w, px);
                        }
                    }
                }

                cx += cw + self.gap_width;
                bar_index += 1;
            }
        }
    }

    // ============ wave ============

    pub unsafe fn render_wave(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        for px in 0..width {
            if !self.is_column_visible(px) {
                continue;
            }
            let t = px as f32 / width as f32;
            let val = Self::sample_left(left, t * 0.5);
            let max_y = ((val * height as f32) as i32 + 1).min(height - 1);
            let min_y = ((val * height as f32) as i32 - 1).max(0);
            let px_v = Self::make_pixel_c(self.get_color(t));
            for py in min_y..=max_y {
                *pixels.add((py * width + px) as usize) = px_v;
            }
        }
    }

    // ============ solid1ch ============

    pub unsafe fn render_solid1ch(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        for py in 0..height {
            let hy = py as f32 / height as f32;
            let row = pixels.add((py * width) as usize);
            for px in 0..width {
                if !self.is_column_visible(px) {
                    continue;
                }
                let t = px as f32 / width as f32;
                if Self::sample_avg(left, right, t) > hy {
                    *row.add(px as usize) = Self::make_pixel_c(self.get_color(t));
                }
            }
        }
    }

    // ============ solid ============

    pub unsafe fn render_solid(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        for py in 0..height {
            let hy = py as f32 / height as f32;
            let row = pixels.add((py * width) as usize);
            for px in 0..width {
                if !self.is_column_visible(px) {
                    continue;
                }
                let t = px as f32 / width as f32;
                let (l, r) = Self::sample_lr(left, right, t);
                if 0.5 - l * 0.5 <= hy && hy <= 0.5 + r * 0.5 {
                    *row.add(px as usize) = Self::make_pixel_c(self.get_color(t));
                }
            }
        }
    }

    // ============ beam ============

    pub unsafe fn render_beam(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        for px in 0..width {
            if !self.is_column_visible(px) {
                continue;
            }
            let t = px as f32 / width as f32;
            let val = Self::sample_avg(left, right, t);
            let (b, g, r) = self.get_color(t);
            let a = (val * 255.0) as u8;
            let px_v = ((a as u32) << 24)
                | (((r as f32 * val) as u8) as u32) << 16
                | (((g as f32 * val) as u8) as u32) << 8
                | ((b as f32 * val) as u8) as u32;
            for py in 0..height {
                *pixels.add((py * width + px) as usize) = px_v;
            }
        }
    }

    // ============ spectrogram ============

    pub unsafe fn render_spectrogram(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        let total = (width * height) as usize;
        if self.spectrogram_buf.len() != total {
            self.spectrogram_buf.resize(total, 0);
        }

        // 向下滚动一行
        let last_row = (height - 1) * width;
        self.spectrogram_buf.copy_within((width as usize)..(last_row as usize + width as usize), 0);

        // 写新行到底部
        for px in 0..width {
            if !self.is_column_visible(px) {
                self.spectrogram_buf[(last_row + px) as usize] = 0;
                continue;
            }
            let t = px as f32 / width as f32;
            let val = Self::sample_avg(left, right, t);
            let (b, g, r) = self.get_color(t);
            let br = (val * 255.0) as u8;
            self.spectrogram_buf[(last_row + px) as usize] = Self::make_pixel(
                ((b as u32 * br as u32) / 255) as u8,
                ((g as u32 * br as u32) / 255) as u8,
                ((r as u32 * br as u32) / 255) as u8,
                br,
            );
        }

        // 复制到像素缓冲区
        std::ptr::copy_nonoverlapping(self.spectrogram_buf.as_ptr(), pixels, total);
    }

    // ============ oie1ch ============

    pub unsafe fn render_oie1ch(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        for px in 0..width {
            if !self.is_column_visible(px) {
                continue;
            }
            let t = px as f32 / width as f32;
            let t_prev = ((px - 1) as f32 / width as f32).max(0.0);
            let t_next = ((px + 1) as f32 / width as f32).min(1.0);

            let vl = Self::sample_left(left, t);
            let vl_prev = Self::sample_left(left, t_prev);
            let vl_next = Self::sample_left(left, t_next);

            let p1 = (0.5 * (vl + vl_prev) * height as f32) as i32;
            let p2 = (0.5 * (vl + vl_next) * height as f32) as i32;
            let px_v = Self::make_pixel_c(self.get_color(t));

            let min_y = p1.min(p2);
            let max_y = p2.max(p1) + 2;
            for py in (min_y.max(0))..(max_y.min(height - 1) + 1) {
                *pixels.add((py * width + px) as usize) = px_v;
            }
        }
    }
}
