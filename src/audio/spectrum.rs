// spectrum.rs — SpectrumData 数据模型（← SpectrumData.cs）
// 存储 FFT 计算后的频谱数据

/// FFT 计算后的频谱数据
/// left_channel / right_channel: 归一化到 0.0~1.0
#[derive(Clone, Debug, Default)]
pub struct SpectrumData {
    /// 左声道频谱数据（归一化 0.0~1.0）
    pub left_channel: Vec<f32>,
    /// 右声道频谱数据（归一化 0.0~1.0）
    pub right_channel: Vec<f32>,
    /// 是否检测到节拍
    pub beat_detected: bool,
    /// 音频 RMS 音量 (0.0~1.0)
    pub volume: f32,
}

impl SpectrumData {
    /// 频谱条数
    pub fn bar_count(&self) -> usize {
        self.left_channel.len()
    }

    /// 是否静音
    pub fn is_silent(&self) -> bool {
        self.volume < 0.001
    }
}
