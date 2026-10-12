//! 窗口置顶助手 —— 装配层。
//!
//! 沿用了本机既有 Tauri 项目的骨架约定（见 menstruation/src/lib.rs）：
//! 配置文件里主窗口 `create: false`，在这里手动创建，以便注入 WebView2 的
//! 数据目录（便携版零残留）。

// 模块对 crate 外可见，是为了让 tests/ 下的集成测试能直接对真实窗口
// 验证 Win32 层（见 tests/win32_e2e.rs）。
pub mod autostart;
pub mod commands;
pub mod error;
pub mod events;
pub mod hotkey;
pub mod log;
pub mod pin;
pub mod portable;
pub mod state;
pub mod tray;
pub mod winapi;

use tauri::{Manager, WebviewWindowBuilder};

use crate::state::AppState;

pub fn run() {
    let app = tauri::Builder::default()
        // 单实例：第二次启动时不新开进程，而是把已有窗口唤到前台
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main(app);
        }))
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::toggle_pin,
            commands::unpin_all,
            commands::get_snapshot,
            commands::set_autostart,
            commands::open_data_dir,
            commands::hide_to_tray,
            commands::quit_app,
        ])
        .setup(|app| {
            // ── 1) 便携数据目录（必须在创建窗口之前完成）──────────────────
            let paths = portable::resolve()?;
            std::fs::create_dir_all(&paths.data_dir)?;

            // 日志要尽早可用：后面每一步失败都靠它留下线索
            log::init(&paths.data_dir);
            log::write(
                "app",
                format!(
                    "启动 v{} portable={} data_dir={}",
                    env!("CARGO_PKG_VERSION"),
                    paths.portable,
                    paths.data_dir.display()
                ),
            );

            app.manage(AppState::new(paths));

            // ── 2) 手动创建主窗口，并把 WebView2 缓存也放进 data\ ────────
            let cache_dir = app
                .state::<AppState>()
                .webview_cache_dir();
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

            // ── 3) 托盘 ─────────────────────────────────────────────────
            tray::setup(app)?;

            // ── 4) 热键线程 + 重断言定时器 ───────────────────────────────
            let handle = hotkey::spawn(app.handle().clone());
            app.state::<AppState>().set_hotkey(handle);

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口 = 收进托盘。本程序的价值就在于后台持续守护置顶，
            // 直接退出会让这个工具失去意义；真正退出走托盘菜单或设置页。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("启动窗口置顶助手失败");

    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app.try_state::<AppState>() {
                // 退出后没人再维护置顶，留着只会让那些窗口一直赖在最上层。
                // 正常退出时恢复现场；异常崩溃时留下的残留，重开点「全部取消」即可清掉。
                state.pins.unpin_all();
                state.stop_hotkey();
            }
        }
    });
}
