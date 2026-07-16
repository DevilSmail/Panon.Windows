// build.rs — 编译时嵌入图标资源 + 编译 slint UI
fn main() {
    // 嵌入图标资源到 exe 资源段
    embed_resource::compile("assets/panon.rc", embed_resource::NONE);
    println!("cargo:rerun-if-changed=assets/panon.rc");
    println!("cargo:rerun-if-changed=assets/panon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    // 编译 slint UI 文件（不使用 fluent-dark 样式，改为自定义控件）
    slint_build::compile("ui/settings.slint")
        .expect("Slint UI compilation failed");
    println!("cargo:rerun-if-changed=ui/settings.slint");
}
