// build.rs — 编译时将 assets/panon.ico 嵌入 exe 资源段
// 这样托盘图标和设置窗口图标无需外部文件
fn main() {
    embed_resource::compile("assets/panon.rc", embed_resource::NONE);
    println!("cargo:rerun-if-changed=assets/panon.rc");
    println!("cargo:rerun-if-changed=assets/panon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
}
