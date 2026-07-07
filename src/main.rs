// panon.windows — Rust 原生版入口
// 阶段 0：仅验证编译链路与图标嵌入

mod app;
mod audio;
mod overlay;
mod render;
mod settings;
mod taskbar;
mod tray;
mod ui;

fn main() {
    // 阶段 0 占位：后续阶段将依次接入
    // 1. 单实例检查 (Global\Panon.Windows.SingleInstance mutex)
    // 2. 加载设置 (%APPDATA%/Panon/settings.json)
    // 3. 初始化托盘图标
    // 4. 启动 WASAPI 捕获
    // 5. 创建分层覆盖窗口
    // 6. 进入事件循环
    println!("Panon.Windows (Rust) — phase 0 skeleton");
}
