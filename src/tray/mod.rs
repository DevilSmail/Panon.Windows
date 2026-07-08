// tray 模块：系统托盘图标 + 右键菜单
// 阶段 6 实现

pub mod icon;
pub mod menu;

#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    TogglePause,
    ShowSettings,
    Exit,
    TaskbarRestart,
}
