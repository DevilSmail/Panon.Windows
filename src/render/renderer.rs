// renderer.rs — CPU 频谱渲染器（← SpectrumRenderer.cs）
// 封装像素缓冲区写入，分发到 7 种效果

/// 视觉效果枚举
#[derive(Clone, Debug)]
pub enum VisualEffect {
    Bar1ch,
    Wave,
    Solid1ch,
    Solid,
    Beam,
    Spectrogram,
    Oie1ch,
}

impl Default for VisualEffect {
    fn default() -> Self {
        Self::Bar1ch
    }
}

impl VisualEffect {
    pub fn from_name(name: &str) -> Self {
        match name {
            "wave" => Self::Wave,
            "solid1ch" => Self::Solid1ch,
            "solid" => Self::Solid,
            "beam" => Self::Beam,
            "spectrogram" => Self::Spectrogram,
            "oie1ch" => Self::Oie1ch,
            _ => Self::Bar1ch,
        }
    }
}

/// 频谱渲染器
/// 写入 BGRA 32bpp 像素缓冲区（DIB Section）
pub struct SpectrumRenderer {
    pub visual_effect: VisualEffect,
    pub gravity: u8,
    pub inversion: bool,
    pub color_space_hsluv: bool,
    pub hsl_hue_from: i32,
    pub hsl_hue_to: i32,
    pub hsl_saturation: i32,
    pub hsl_lightness: i32,
    pub hsluv_hue_from: i32,
    pub hsluv_hue_to: i32,
    pub hsluv_saturation: i32,
    pub hsluv_lightness: i32,
    pub bar_width: i32,
    pub gap_width: i32,
    /// 填充模式: 0=铺满, 1=仅空白区域
    pub fill_mode: u8,
    /// 空白区域列表 (x, width)，FillMode=1 时使用
    pub free_regions: Option<Vec<(i32, i32)>>,
    pub use_exit_factor: bool,

    pub peak_heights: Vec<f32>,
    pub cached_src_count: usize,
    pub cached_target_count: usize,
    pub resample_indices: Vec<usize>,
    pub resample_fracs: Vec<f32>,
    pub buffer_l: Vec<f32>,
    pub buffer_r: Vec<f32>,
    pub spectrogram_buf: Vec<u32>,
}

pub const PEAK_DECAY_VALUE: f32 = 0.02;
pub const EXIT_PEAK_DECAY_VALUE: f32 = 0.08;

impl SpectrumRenderer {
    pub fn new() -> Self {
        Self {
            visual_effect: VisualEffect::Bar1ch,
            gravity: 2,
            inversion: false,
            color_space_hsluv: false,
            hsl_hue_from: 180,
            hsl_hue_to: 720,
            hsl_saturation: 80,
            hsl_lightness: 50,
            hsluv_hue_from: 270,
            hsluv_hue_to: -270,
            hsluv_saturation: 100,
            hsluv_lightness: 50,
            bar_width: 6,
            gap_width: 3,
            fill_mode: 0,
            free_regions: None,
            use_exit_factor: false,
            peak_heights: Vec::new(),
            cached_src_count: 0,
            cached_target_count: 0,
            resample_indices: Vec::new(),
            resample_fracs: Vec::new(),
            buffer_l: Vec::new(),
            buffer_r: Vec::new(),
            spectrogram_buf: Vec::new(),
        }
    }

    /// 渲染到像素缓冲区
    /// pixels: BGRA 32bpp, top-down DIB
    pub unsafe fn render_to_pixels(
        &mut self,
        left: &[f32],
        right: &[f32],
        pixels: *mut u32,
        width: i32,
        height: i32,
    ) {
        if pixels.is_null() || left.is_empty() || width <= 0 || height <= 0 {
            return;
        }

        let total = (width * height) as usize;
        std::ptr::write_bytes(pixels as *mut u8, 0, total * 4);

        match self.visual_effect {
            VisualEffect::Bar1ch => self.render_bar1ch(left, right, pixels, width, height),
            VisualEffect::Wave => self.render_wave(left, right, pixels, width, height),
            VisualEffect::Solid1ch => self.render_solid1ch(left, right, pixels, width, height),
            VisualEffect::Solid => self.render_solid(left, right, pixels, width, height),
            VisualEffect::Beam => self.render_beam(left, right, pixels, width, height),
            VisualEffect::Spectrogram => self.render_spectrogram(left, right, pixels, width, height),
            VisualEffect::Oie1ch => self.render_oie1ch(left, right, pixels, width, height),
        }
    }

    pub fn max_peak_height(&self) -> f32 {
        self.peak_heights.iter().cloned().fold(0.0f32, f32::max)
    }

    pub fn cleanup(&mut self) {
        self.spectrogram_buf.clear();
    }
}

impl Default for SpectrumRenderer {
    fn default() -> Self {
        Self::new()
    }
}
