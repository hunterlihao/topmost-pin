//! 最小骨架 —— 仅用于验证离线构建链路可用。
//! 后续步骤会逐步替换为完整实现。

mod portable;

use tauri::{Manager, WebviewWindowBuilder};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let paths = portable::resolve()?;
            std::fs::create_dir_all(&paths.data_dir)?;

            let cache_dir = paths.data_dir.join("webview");
            std::fs::create_dir_all(&cache_dir)?;

            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .ok_or_else(|| portable::err("tauri.conf.json 缺少 label=main 的窗口配置"))?
                .clone();

            let builder = WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .data_directory(cache_dir);
            builder.build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动窗口置顶助手失败");
}
