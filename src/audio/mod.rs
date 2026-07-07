// audio 模块：WASAPI 捕获 + FFT + 衰减 + 频谱数据
// 阶段 1 实现 capture / fft / spectrum，阶段 3 实现 decay

pub mod capture;
pub mod decay;
pub mod fft;
pub mod spectrum;
