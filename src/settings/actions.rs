use serde::{Deserialize, Serialize};

/// 动作：设置 UI 向主线程发送的请求，需要在主线程执行的操作
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettingsAction {
    /// 写入/移除开机自启 (HKCU\\...\\Run)
    ApplyStartup(bool),
    /// 应用透明度设置 (EnableTransparency, UseOLEDTaskbarTransparency)
    ApplyTransparency { enable: bool, use_oled: bool },
    /// 请求重建 overlay（参数为 target_monitor: i32）
    RecreateOverlays(i32),
}
