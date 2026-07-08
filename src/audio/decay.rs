// decay.rs — 指数衰减处理器（← DecayProcessor.cs）
// 峰值保持衰减：新值 = max(当前输入, 上一帧 × 衰减因子)
// 三种因子：正常播放(0.96) / 静默(0.75) / 退出(0.80)

use crate::audio::spectrum::SpectrumData;

/// 指数衰减处理器
/// 音乐停止后频谱平滑回落，避免突变
pub struct DecayProcessor {
    /// 正常衰减因子（播放中，缓慢下降）
    normal_factor: f32,
    /// 静默衰减因子（无音频时，快速下降）
    silence_factor: f32,
    /// 退出衰减因子（程序退出时，中速下降）
    exit_factor: f32,
    /// 是否使用退出因子（退出流程触发）
    use_exit: bool,
    /// 静默阈值（volume 低于此值时使用 silence_factor）
    silence_threshold: f32,
    /// 最小值（低于此值归零，避免无限微小值残留）
    min_value: f32,
    /// 上一帧左声道（用于衰减计算）
    prev_left: Vec<f32>,
    /// 上一帧右声道
    prev_right: Vec<f32>,
    /// 结果复用池（避免每帧分配）
    result_left: Vec<f32>,
    /// 结果复用池
    result_right: Vec<f32>,
}

impl DecayProcessor {
    pub fn new() -> Self {
        Self {
            normal_factor: 0.96,
            silence_factor: 0.75,
            exit_factor: 0.80,
            use_exit: false,
            silence_threshold: 0.001,
            min_value: 0.0001,
            prev_left: Vec::new(),
            prev_right: Vec::new(),
            result_left: Vec::new(),
            result_right: Vec::new(),
        }
    }

    /// 处理频谱数据，应用衰减
    pub fn process(&mut self, input: &SpectrumData) -> SpectrumData {
        // 先读取需要的参数（Copy 类型，读取后不再借用 self）
        let factor = if self.use_exit {
            self.exit_factor
        } else if input.volume < self.silence_threshold {
            self.silence_factor
        } else {
            self.normal_factor
        };
        let min_value = self.min_value;

        Self::apply_decay(
            &input.left_channel,
            &mut self.prev_left,
            &mut self.result_left,
            factor,
            min_value,
        );
        Self::apply_decay(
            &input.right_channel,
            &mut self.prev_right,
            &mut self.result_right,
            factor,
            min_value,
        );

        SpectrumData {
            left_channel: self.result_left.clone(),
            right_channel: self.result_right.clone(),
            beat_detected: input.beat_detected,
            volume: input.volume,
        }
    }

    /// 触发退出衰减（跳过静默等待，使用 exit_factor）
    pub fn force_exit(&mut self) {
        self.use_exit = true;
    }

    /// 退出衰减是否完成（所有频谱值已归零）
    pub fn is_exit_complete(&self) -> bool {
        self.prev_left.iter().all(|&v| v < self.min_value)
            && self.prev_right.iter().all(|&v| v < self.min_value)
    }

    /// 重置衰减状态（清空历史缓冲区）
    pub fn reset(&mut self) {
        self.prev_left.clear();
        self.prev_right.clear();
        self.result_left.clear();
        self.result_right.clear();
        self.use_exit = false;
    }

    /// 对单个声道应用衰减
    /// 峰值保持：result[i] = max(input[i], prev[i] * factor)
    fn apply_decay(
        input: &[f32],
        prev: &mut Vec<f32>,
        result: &mut Vec<f32>,
        factor: f32,
        min_value: f32,
    ) {
        let len = input.len();
        if prev.len() != len {
            prev.resize(len, 0.0);
        }
        if result.len() != len {
            result.resize(len, 0.0);
        }

        for i in 0..len {
            let decayed = prev[i] * factor;
            let val = if input[i] > decayed {
                input[i]
            } else {
                decayed
            };
            // 低于最小值时归零
            let val = if val < min_value { 0.0 } else { val };
            result[i] = val;
            prev[i] = val;
        }
    }
}

impl Default for DecayProcessor {
    fn default() -> Self {
        Self::new()
    }
}
