//! Tauri 命令层：前端唯一能调到的入口。每个命令都只做转发与数据整形，
//! 业务规则在 pin.rs / db.rs / elevation.rs 里。

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::db;
use crate::elevation;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::winapi;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// 窗口句柄。isize 序列化成 JSON number，JS 侧按 number 使用。
    pub hwnd: isize,
    pub title: String,
    pub class_name: String,
    pub process_name: String,
    /// 是否由本程序置顶（依据是 pin 集合，不是 WS_EX_TOPMOST）
    pub pinned: bool,
}

/// 枚举当前所有可见的顶层窗口
#[tauri::command]
pub fn list_windows(state: State<'_, AppState>) -> Vec<WindowInfo> {
    let mut items: Vec<WindowInfo> = winapi::enum_top_level_windows()
        .into_iter()
        .map(|w| WindowInfo {
            pinned: state.pins.is_pinned(w.hwnd),
            process_name: winapi::process_name(w.pid),
            hwnd: w.hwnd,
            title: w.title,
            class_name: w.class_name,
        })
        .collect();

    // 枚举顺序由系统决定，不稳定；按标题排一下，避免每次刷新列表乱跳
    items.sort_by_key(|w| w.title.to_lowercase());
    items
}

/// 切换某个窗口的置顶状态，返回切换后的状态
#[tauri::command]
pub fn toggle_pin(state: State<'_, AppState>, hwnd: isize) -> AppResult<bool> {
    if !winapi::is_candidate(hwnd) {
        crate::log::write("cmd", format!("toggle_pin 拒绝 hwnd=0x{hwnd:X}：不是候选窗口"));
        return Err(AppError::WindowGone);
    }
    let title = winapi::title_of(hwnd);
    crate::log::write("cmd", format!("toggle_pin（来自界面）hwnd=0x{hwnd:X} title=「{title}」"));
    state.toggle_pin(hwnd, &title)
}

/// 取消全部置顶，返回取消的数量
#[tauri::command]
pub fn unpin_all(state: State<'_, AppState>) -> usize {
    crate::log::write("cmd", "unpin_all（来自界面）");
    state.unpin_all()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub pinned: Vec<isize>,
    pub hotkey: String,
    pub hotkey_active: bool,
    pub autostart: bool,
    /// 设置值：是否希望以管理员运行
    pub run_as_admin: bool,
    /// 实际值：本进程当前是否已经是管理员。与设置不一致时界面要提示重启
    pub is_admin: bool,
    /// true = 数据在 exe 同目录（真便携）
    pub portable: bool,
    pub data_dir: String,
    /// SQLite 数据文件路径
    pub db_path: String,
    /// 已记录的置顶事件条数
    pub pin_event_count: i64,
    pub version: String,
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        pinned: state.pins.ids(),
        hotkey: state.hotkey_display(),
        hotkey_active: state.hotkey_active(),
        autostart: crate::autostart::is_enabled(),
        run_as_admin: state.run_as_admin(),
        is_admin: state.is_admin,
        portable: state.paths.portable,
        data_dir: state.paths.data_dir.display().to_string(),
        db_path: state.db.path().display().to_string(),
        pin_event_count: state.db.pin_event_count(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 设置开机自启，返回生效后的状态（以注册表实际读到的为准，不假设写入一定成功）
#[tauri::command]
pub fn set_autostart(enabled: bool) -> AppResult<bool> {
    crate::autostart::set(enabled)?;
    Ok(crate::autostart::is_enabled())
}

/// 切换「是否以管理员运行」。
///
/// 进程令牌在运行时无法更改，所以这个开关必须靠**重启进程**落实：
/// 先把设置写进数据库，再以目标权限启动新实例，然后本进程退出。
/// 新实例启动时会读到新设置，发现权限已经匹配，就不会再重启（不会来回循环）。
#[tauri::command]
pub fn set_run_as_admin(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<bool> {
    state.db.set_bool(db::keys::RUN_AS_ADMIN, enabled)?;
    crate::log::write("cmd", format!("set_run_as_admin({enabled})"));

    if state.is_admin == enabled {
        // 当前权限已经就是目标权限，无需重启
        return Ok(enabled);
    }

    let marker = elevation::marker_path(&state.paths.data_dir);
    let _ = std::fs::write(&marker, if enabled { "admin" } else { "user" });
    let _ = state.db.set_i64(db::keys::LAST_SWITCH_AT, db::now_ms());

    match elevation::relaunch(enabled) {
        Ok(()) => {
            crate::log::write("cmd", "已发起权限切换重启，本进程退出");
            app.exit(0);
            Ok(enabled)
        }
        Err(e) => {
            // 重启没发起成功（典型：用户取消了 UAC），把标记清掉，
            // 否则下次启动会误以为「自己是一次重启的结果」
            let _ = std::fs::remove_file(&marker);
            crate::log::write("cmd", format!("权限切换失败：{e}"));
            Err(e)
        }
    }
}

/// 启动动画页跑完后调用：关掉动画窗、放出主窗口
#[tauri::command]
pub fn finish_splash(app: AppHandle) {
    crate::splash::finish(&app);
}

#[tauri::command]
pub fn open_data_dir(state: State<'_, AppState>) -> AppResult<()> {
    let dir = state.paths.data_dir.clone();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map_err(|e| AppError::msg(format!("无法打开资源管理器：{e}")))?;
    Ok(())
}

/// 关闭窗口的行为是「收进托盘」，由前端显式调用，便于以后加设置项
#[tauri::command]
pub fn hide_to_tray(app: AppHandle) -> AppResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide()?;
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
