// fft.rs — 手写 Cooley-Tukey 2048 点 FFT（← FftProcessor.cs）
// PCM → 汉宁窗 → FFT → 幅度谱 → 截取 → 低音衰减 → 归一化

use super::spectrum::SpectrumData;

/// 频率范围定义，对应 panon 的 7 级 bassResolutionLevel
/// (low_freq, high_freq) in Hz
const FREQUENCY_RANGES: [(f64, f64); 7] = [
    (0.0, 22050.0),  // Level 0: 全频段
    (0.0, 9000.0),   // Level 1
    (0.0, 3000.0),   // Level 2: F7
    (0.0, 1800.0),   // Level 3: A6
    (0.0, 1800.0),   // Level 4: A6（默认，低延迟）
    (300.0, 1800.0), // Level 5: 过滤低音
    (0.0, 600.0),    // Level 6: D5
];

const FFT_SIZE: usize = 2048;
const HALF_SIZE: usize = FFT_SIZE / 2;

/// FFT 频谱处理器
/// 将 PCM 音频数据转换为频域频谱数据
pub struct FftProcessor {
    bass_resolution_level: u8,
    reduce_bass: bool,
    sample_rate: u32,

    // 预分配缓冲区（复用，避免每帧分配）
    left_samples: Vec<f32>,
    right_samples: Vec<f32>,
    windowed: Vec<f32>,    // FFT_SIZE
    real: Vec<f32>,        // FFT_SIZE
    imag: Vec<f32>,        // FFT_SIZE
    magnitudes: Vec<f32>,  // HALF_SIZE
    spectrum: Vec<f32>,    // 动态长度
}

impl FftProcessor {
    pub fn new() -> Self {
        Self {
            bass_resolution_level: 4,
            reduce_bass: true,
            sample_rate: 44100,
            left_samples: Vec::new(),
            right_samples: Vec::new(),
            windowed: vec![0.0; FFT_SIZE],
            real: vec![0.0; FFT_SIZE],
            imag: vec![0.0; FFT_SIZE],
            magnitudes: vec![0.0; HALF_SIZE],
            spectrum: Vec::new(),
        }
    }

    pub fn set_bass_resolution_level(&mut self, level: u8) {
        self.bass_resolution_level = level.min(6);
    }

    pub fn set_reduce_bass(&mut self, reduce: bool) {
        self.reduce_bass = reduce;
    }

    /// 处理音频采样数据，返回频谱数据
    /// samples: 交错 PCM (L R L R ...), channels: 声道数
    pub fn process(&mut self, samples: &[f32], channels: u16, sample_rate: u32) -> SpectrumData {
        self.sample_rate = sample_rate;

        // 分离左右声道（复用缓冲区）
        let frame_count = samples.len() / channels as usize;
        if self.left_samples.len() != frame_count {
            self.left_samples.resize(frame_count, 0.0);
            self.right_samples.resize(frame_count, 0.0);
        }

        for i in 0..frame_count {
            self.left_samples[i] = samples[i * channels as usize];
            self.right_samples[i] = if channels > 1 {
                samples[i * channels as usize + 1]
            } else {
                samples[i * channels as usize]
            };
        }

        // 计算频谱
        let left_spectrum = self.compute_spectrum(&self.left_samples.clone());
        let right_spectrum = self.compute_spectrum(&self.right_samples.clone());

        // 计算 RMS 音量
        let rms = compute_rms(samples);

        SpectrumData {
            left_channel: left_spectrum,
            right_channel: right_spectrum,
            volume: rms,
            beat_detected: false,
        }
    }

    /// 计算单声道频谱
    fn compute_spectrum(&mut self, samples: &[f32]) -> Vec<f32> {
        let use_samples = samples.len().min(FFT_SIZE);

        // 应用汉宁窗（复用 windowed，用后清零尾部）
        for i in use_samples..FFT_SIZE {
            self.windowed[i] = 0.0;
        }
        for i in 0..use_samples {
            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (use_samples - 1) as f32).cos());
            self.windowed[i] = samples[i] * window;
        }

        // 就地 FFT（复用 real / imag）
        self.real[..FFT_SIZE].copy_from_slice(&self.windowed[..FFT_SIZE]);
        for i in 0..FFT_SIZE {
            self.imag[i] = 0.0;
        }
        fft(&mut self.real, &mut self.imag, FFT_SIZE);

        // 计算幅度谱（复用 magnitudes）
        for i in 0..HALF_SIZE {
            self.magnitudes[i] = (self.real[i] * self.real[i] + self.imag[i] * self.imag[i]).sqrt() / FFT_SIZE as f32;
        }

        // 根据频率范围截取
        let (low_freq, high_freq) = FREQUENCY_RANGES[self.bass_resolution_level as usize];
        let low_bin = (low_freq * FFT_SIZE as f64 / self.sample_rate as f64) as usize;
        let high_bin = ((high_freq * FFT_SIZE as f64 / self.sample_rate as f64) as usize).min(HALF_SIZE - 1);

        let bar_count = (high_bin - low_bin).max(1);

        // 复用 spectrum 当长度匹配时
        if self.spectrum.len() != bar_count {
            self.spectrum.resize(bar_count, 0.0);
        }
        for i in 0..bar_count {
            self.spectrum[i] = self.magnitudes[low_bin + i];
        }

        // 低音衰减
        if self.reduce_bass {
            apply_bass_reduction(&mut self.spectrum, low_bin, self.sample_rate, FFT_SIZE);
        }

        // 归一化到 0~1
        let max = self.spectrum[..bar_count].iter().cloned().fold(0.0f32, f32::max);
        let mut result = vec![0.0; bar_count];
        if max > 0.001 {
            for i in 0..bar_count {
                result[i] = (self.spectrum[i] / max).clamp(0.0, 1.0);
            }
        }

        result
    }
}

impl Default for FftProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// 低音衰减：频率 < 300Hz 时按 freq/300 缩放
fn apply_bass_reduction(spectrum: &mut [f32], low_bin: usize, sample_rate: u32, fft_size: usize) {
    for i in 0..spectrum.len() {
        let freq = (low_bin + i) as f64 * sample_rate as f64 / fft_size as f64;
        if freq < 300.0 {
            let factor = freq / 300.0;
            spectrum[i] *= factor as f32;
        }
    }
}

/// 计算 RMS 音量
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// 就地 Cooley-Tukey FFT（radix-2）
/// real / imag 长度必须为 2 的幂
fn fft(real: &mut [f32], imag: &mut [f32], n: usize) {
    // 位反转排列
    let mut j = 0usize;
    for i in 0..n - 1 {
        if i < j {
            real.swap(i, j);
            imag.swap(i, j);
        }
        let mut k = n >> 1;
        while k <= j {
            j -= k;
            k >>= 1;
        }
        j += k;
    }

    // 蝶形运算
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let w_real = angle.cos() as f32;
        let w_imag = angle.sin() as f32;

        let mut i = 0;
        while i < n {
            let mut cur_real = 1.0f32;
            let mut cur_imag = 0.0f32;
            for m in 0..len / 2 {
                let even_idx = i + m;
                let odd_idx = i + m + len / 2;

                let t_real = cur_real * real[odd_idx] - cur_imag * imag[odd_idx];
                let t_imag = cur_real * imag[odd_idx] + cur_imag * real[odd_idx];

                real[odd_idx] = real[even_idx] - t_real;
                imag[odd_idx] = imag[even_idx] - t_imag;
                real[even_idx] += t_real;
                imag[even_idx] += t_imag;

                let new_cur_real = cur_real * w_real - cur_imag * w_imag;
                cur_imag = cur_real * w_imag + cur_imag * w_real;
                cur_real = new_cur_real;
            }
            i += len;
        }
        len <<= 1;
    }
}
