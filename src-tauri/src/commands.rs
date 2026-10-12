//! Tauri 命令层：前端唯一能调到的入口。每个命令都只做转发与数据整形，
//! 业务规则在 pin.rs / db.rs / elevation.rs 里。

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

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
    /// 本进程当前是否已经是管理员。
    ///
    /// 程序固定尝试以管理员运行，但提权可能失败（UAC 被拒、账户无管理员权限），
    /// 那种情况下界面要说明「为什么有些窗口置顶不了」。
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

/// 修改全局热键，返回**实际生效**的文案。
///
/// 失败（组合已被占用、写法不合法）时错误信息直接给用户看，原热键保持不变。
#[tauri::command]
pub fn set_hotkey(state: State<'_, AppState>, combo: String) -> AppResult<String> {
    crate::log::write("cmd", format!("set_hotkey({combo})"));
    state.rebind_hotkey(&combo)
}

/// 前端画好第一帧后调用：收起启动动画、放出主窗口。
///
/// 由前端主动调用，而不是 Rust 定时显示 —— Rust 无从知道 WebView 什么时候
/// 才真正画出了内容，定时显示会让用户看到一个还没画好的界面。
#[tauri::command]
pub fn main_ready(app: AppHandle) {
    crate::splash::destroy();
    if let Some(m) = app.get_webview_window("main") {
        let _ = m.show();
        let _ = m.unminimize();
        let _ = m.set_focus();
    }
    crate::log::write("app", "前端已就绪，启动动画收尾、主窗口显示");
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
