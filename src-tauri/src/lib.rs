//! 窗口置顶助手 —— 装配层。
//!
//! 启动顺序有讲究，每一步的次序都是踩出来的：
//!
//!   1. 便携数据目录 + 日志   —— 后面任何一步失败都要能留下线索
//!   2. 数据库                —— 权限设置存在里面，第 3 步要用
//!   3. 权限切换              —— 必须排在单实例之前：重启中的新实例要能等旧实例
//!                               释放互斥体，否则会被自己判成「重复启动」而自杀
//!   4. 单实例                —— 拿不到就唤醒已有实例并退出
//!   5. Tauri 装配            —— 启动动画窗口 + 隐藏的主窗口 + 托盘 + 热键线程

// 模块对 crate 外可见，是为了让 tests/ 下的集成测试能直接对真实窗口
// 验证 Win32 层（见 tests/win32_e2e.rs）。
pub mod autostart;
pub mod commands;
pub mod db;
pub mod elevation;
pub mod error;
pub mod events;
pub mod hotkey;
pub mod log;
pub mod pin;
pub mod portable;
pub mod single_instance;
pub mod splash;
pub mod state;
pub mod tray;
pub mod winapi;

use tauri::{Manager, WebviewWindowBuilder};

use crate::db::Db;
use crate::state::AppState;

/// 权限切换的防抖窗口。重启失败后（比如用户取消了 UAC）不要在每次启动都再弹一次。
const SWITCH_COOLDOWN_MS: i64 = 60_000;

pub fn run() {
    // ── 1) 便携数据目录 + 日志 ────────────────────────────────────────────
    let paths = match portable::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("无法定位数据目录：{e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&paths.data_dir) {
        eprintln!("无法创建数据目录：{e}");
        std::process::exit(1);
    }
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

    // ── 2) 数据库 ─────────────────────────────────────────────────────────
    let db_path = paths.data_dir.join(db::DB_FILE_NAME);
    let db = match Db::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            log::write("db", format!("打开数据库失败：{e}"));
            std::process::exit(1);
        }
    };
    if let Err(e) = db.migrate() {
        log::write("db", format!("建表失败：{e}"));
        std::process::exit(1);
    }
    log::write("db", format!("数据文件 {} 就绪", db_path.display()));

    // ── 3) 权限切换（必须在单实例之前）────────────────────────────────────
    let marker = elevation::marker_path(&paths.data_dir);
    let relaunching = marker.exists();
    if relaunching {
        let _ = std::fs::remove_file(&marker);
    }

    let is_admin = elevation::is_elevated();
    let want_admin = db.get_bool(db::keys::RUN_AS_ADMIN, false);
    log::write(
        "app",
        format!("权限：设置={want_admin} 实际={is_admin} 本次是重启={relaunching}"),
    );

    if !relaunching && want_admin != is_admin {
        let now = db::now_ms();
        let last = db.get_i64(db::keys::LAST_SWITCH_AT, 0);
        if now - last > SWITCH_COOLDOWN_MS {
            let _ = db.set_i64(db::keys::LAST_SWITCH_AT, now);
            // 标记必须先落盘再启动新实例：新实例靠它判断「我是一次权限切换」
            let _ = std::fs::write(&marker, if want_admin { "admin" } else { "user" });
            match elevation::relaunch(want_admin) {
                Ok(()) => {
                    log::write(
                        "app",
                        format!(
                            "已发起重启（目标：{}），本进程退出以释放单实例互斥体",
                            if want_admin { "管理员" } else { "普通权限" }
                        ),
                    );
                    std::process::exit(0);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&marker);
                    log::write("app", format!("重启失败，继续以当前权限运行：{e}"));
                }
            }
        } else {
            log::write("app", "冷却期内，跳过本次权限切换以免反复弹 UAC");
        }
    }

    // ── 4) 单实例 ─────────────────────────────────────────────────────────
    let Some(_guard) = single_instance::acquire(relaunching) else {
        log::write("app", "已有实例在运行，唤醒它之后退出");
        single_instance::signal_existing();
        std::process::exit(0);
    };

    // ── 5) Tauri 装配 ─────────────────────────────────────────────────────
    let state = AppState::new(paths, db, is_admin);

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::toggle_pin,
            commands::unpin_all,
            commands::get_snapshot,
            commands::set_autostart,
            commands::set_run_as_admin,
            commands::finish_splash,
            commands::open_data_dir,
            commands::hide_to_tray,
            commands::quit_app,
        ])
        .setup(move |app| {
            let cache_dir = state.webview_cache_dir();
            std::fs::create_dir_all(&cache_dir)?;
            let handle = app.handle().clone();

            // ★ 启动动画窗口必须**第一个**创建。
            // 主窗口的 WebView2 初始化是同步的，实测要 2 秒上下；先建主窗口的话，
            // 这 2 秒里屏幕上是空的，动画等于白做。先把动画弹出来，主窗口在它背后慢慢初始化。
            let splash_ok = match splash::create(&handle, &cache_dir) {
                Ok(()) => {
                    log::write("splash", "启动动画窗口已创建");
                    true
                }
                Err(e) => {
                    // 动画窗建不起来不该阻断启动 —— 下面直接把主窗口显示出来
                    log::write("splash", format!("创建启动动画窗口失败：{e}，将直接显示主窗口"));
                    false
                }
            };

            // 主窗口先隐藏（tauri.conf.json 里 visible=false），
            // 等启动动画跑完再放出来，避免动画期间后面闪出主界面。
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == splash::MAIN_LABEL)
                .ok_or_else(|| portable::err("tauri.conf.json 缺少 label=main 的窗口配置"))?
                .clone();

            WebviewWindowBuilder::from_config(&handle, &window_config)?
                .data_directory(cache_dir.clone())
                .visible(false)
                .build()?;
            log::write("app", "主窗口已创建（隐藏）");
            app.manage(state);

            if splash_ok {
                splash::spawn_fallback(handle.clone());
            } else if let Some(m) = handle.get_webview_window(splash::MAIN_LABEL) {
                let _ = m.show();
            }

            // 托盘与热键
            tray::setup(app)?;
            let hk = hotkey::spawn(handle.clone());
            app.state::<AppState>().set_hotkey(hk);

            // 重复启动时唤醒本实例的窗口
            let h = handle.clone();
            single_instance::listen(move || tray::show_main(&h));

            Ok(())
        })
        .on_window_event(|window, event| {
            // ★ 只拦主窗口：启动动画窗是程序自己关的，若一并 prevent_close 会关不掉。
            if window.label() != splash::MAIN_LABEL {
                return;
            }
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
                // 正常退出时恢复现场；异常崩溃留下的残留，重开点「全部取消」即可清掉。
                state.unpin_all();
                state.stop_hotkey();
                log::write("app", "已退出");
            }
        }
    });
}
