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

use std::time::Duration;

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

    // ── 3) 单实例 ─────────────────────────────────────────────────────────
    //
    // ★ 这一步必须在权限切换**之前**。
    //
    // 反过来（先切权限、再查单实例）在「默认以管理员运行」下有个很烦人的后果：
    // 普通权限启动的进程必然发现「设置=管理员、实际=普通」，于是先弹一次 UAC；
    // 等用户点完，新实例才发现旧实例还在，唤醒它然后退出 —— 为了把窗口调到前台，
    // 白弹一次 UAC。而「双击图标把窗口叫出来」恰恰是最高频的操作。
    //
    // 先抢互斥体就完全没有这个问题：抢不到 = 已经有实例在跑，
    // 直接唤醒它走人，权限碰都不碰。
    let marker = elevation::marker_path(&paths.data_dir);
    let relaunching = marker.exists();
    if relaunching {
        let _ = std::fs::remove_file(&marker);
    }

    let Some(_guard) = single_instance::acquire(relaunching) else {
        log::write("app", "已有实例在运行，唤醒它之后退出");
        single_instance::signal_existing();
        std::process::exit(0);
    };

    // ── 4) 提权 ───────────────────────────────────────────────────────────
    //
    // 程序**固定**以管理员身份运行，不做成可选项。
    //
    // 置顶受 Windows 的 UIPI 限制：非提权进程对提权窗口调用 SetWindowPos 只会拿到
    // ERROR_ACCESS_DENIED(5)。而浏览器 / IDE / 资源管理器这些最常想置顶的对象，
    // 往往正是提权跑起来的（本机实测 chrome / msedge / idea64 / explorer / QQ 都是）。
    // 做成选项的话，默认那一侧在最需要它的场景下恰好用不了 —— 那就不该是选项。
    //
    // 提权失败（UAC 被拒 / 账户本身无管理员权限）不阻断启动：继续以当前权限运行，
    // 设置页会说明为什么有些窗口置顶不了。`--no-elevate` 可显式跳过本次提权。
    let is_admin = elevation::is_elevated();
    log::write("app", format!("权限：已提权={is_admin} 本次是重启={relaunching}"));

    let no_elevate = std::env::args().any(|a| a == "--no-elevate");
    if no_elevate {
        log::write("app", "命令行指定 --no-elevate，本次不尝试提权");
    }

    if !relaunching && !no_elevate && !is_admin {
        let now = db::now_ms();
        let last = db.get_i64(db::keys::LAST_SWITCH_AT, 0);
        if now - last > SWITCH_COOLDOWN_MS {
            let _ = db.set_i64(db::keys::LAST_SWITCH_AT, now);
            // 标记必须先落盘再启动新实例：新实例靠它判断「我是一次提权重启」
            let _ = std::fs::write(&marker, "admin");
            match elevation::relaunch_elevated() {
                Ok(()) => {
                    log::write("app", "已发起提权重启，本进程退出以释放单实例互斥体");
                    std::process::exit(0);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&marker);
                    log::write("app", format!("提权失败，继续以普通权限运行：{e}"));
                }
            }
        } else {
            log::write("app", "冷却期内，跳过本次提权以免反复弹 UAC");
        }
    }

    // ── 5) 启动动画 ───────────────────────────────────────────────────────
    // ★ 必须在 tauri::Builder 之前。这正是把动画写成原生 GDI 窗口的原因：
    //   WebView2 初始化要 1～1.5 秒，用 WebView 画启动画面只会先给用户一个白屏。
    //   原生窗口在进程启动后几十毫秒就能显示第一帧动画。
    splash::start();

    // ── 6) Tauri 装配 ─────────────────────────────────────────────────────
    let state = AppState::new(paths, db, is_admin);

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::toggle_pin,
            commands::unpin_all,
            commands::get_snapshot,
            commands::set_autostart,
            commands::set_hotkey,
            commands::main_ready,
            commands::open_data_dir,
            commands::hide_to_tray,
            commands::quit_app,
        ])
        .setup(move |app| {
            let cache_dir = state.webview_cache_dir();
            std::fs::create_dir_all(&cache_dir)?;
            let handle = app.handle().clone();

            // 主窗口先隐藏：等前端报告「第一帧已画好」再显示。
            // 由 Rust 定时显示是不行的 —— 它无从知道 WebView 什么时候才真的画出了内容，
            // 那样用户会看到一个还没画好的界面。
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == splash::MAIN_LABEL)
                .ok_or_else(|| portable::err("tauri.conf.json 缺少 label=main 的窗口配置"))?
                .clone();

            WebviewWindowBuilder::from_config(&handle, &window_config)?
                .data_directory(cache_dir)
                .visible(false)
                .build()?;
            log::write("app", "主窗口已创建（隐藏），等待前端就绪");
            app.manage(state);

            spawn_ready_watchdog(handle.clone());

            // --set-hotkey=Ctrl+Alt+K：启动时套用指定热键。
            // 给脚本化部署用（装完直接配好，不用点界面），同时它也是这条
            // 「命令 -> 热键线程 -> 注册」链路唯一能被自动验证的入口。
            if let Some(combo) = requested_hotkey() {
                let h = handle.clone();
                std::thread::spawn(move || apply_requested_hotkey(h, combo));
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

/// 解析 `--set-hotkey=Ctrl+Alt+K`
fn requested_hotkey() -> Option<String> {
    std::env::args().find_map(|a| {
        a.strip_prefix("--set-hotkey=")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

/// 等热键线程就绪，然后套用命令行指定的热键。
///
/// 走的是和界面「修改」按钮**完全相同**的那条路径（`AppState::rebind_hotkey`
/// → `HotkeyHandle::rebind` → 投线程消息 → 热键线程注册）。
/// 所以它能验证的正是界面那条链路，包括跨线程投消息与回执。
fn apply_requested_hotkey(app: tauri::AppHandle, combo: String) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        // 换键请求要等线程把消息队列建好，否则 PostThreadMessage 会失败
        if app
            .try_state::<AppState>()
            .map(|s| s.hotkey_ready())
            .unwrap_or(false)
        {
            break;
        }
        if std::time::Instant::now() >= deadline {
            log::write("app", "--set-hotkey：热键线程迟迟未就绪，放弃");
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match state.rebind_hotkey(&combo) {
        Ok(label) => log::write("app", format!("--set-hotkey 已生效：{label}")),
        Err(e) => log::write("app", format!("--set-hotkey 失败：{e}")),
    }
}

/// 兜底：前端万一始终没有报告就绪（脚本出错、资源没加载上……），
/// 也必须把主窗口放出来，否则用户看到的是「启动动画播完后什么都没有」。
fn spawn_ready_watchdog(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("topmost-pin-ready-watchdog".to_string())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(splash::HOLD_MAX_MS + 800));
            let inner = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(m) = inner.get_webview_window(splash::MAIN_LABEL) {
                    if !m.is_visible().unwrap_or(false) {
                        log::write("app", "前端始终未报告就绪，兜底放出主窗口");
                        splash::destroy();
                        let _ = m.show();
                        let _ = m.set_focus();
                    }
                }
            });
        })
        .ok();
}
