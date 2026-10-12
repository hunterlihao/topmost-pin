//! Tauri 命令层：前端唯一能调到的入口。每个命令都只做转发与数据整形，
//! 业务规则在 pin.rs / winapi.rs 里。

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::hotkey::HOTKEY_DISPLAY;
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
        return Err(AppError::WindowGone);
    }
    let title = winapi::title_of(hwnd);
    state.pins.toggle(hwnd, &title)
}

/// 取消全部置顶，返回取消的数量
#[tauri::command]
pub fn unpin_all(state: State<'_, AppState>) -> usize {
    state.pins.unpin_all()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub pinned: Vec<isize>,
    pub hotkey: String,
    pub hotkey_active: bool,
    pub autostart: bool,
    /// true = 数据在 exe 同目录（真便携）
    pub portable: bool,
    pub data_dir: String,
    pub version: String,
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    Snapshot {
        pinned: state.pins.ids(),
        hotkey: HOTKEY_DISPLAY.to_string(),
        hotkey_active: state.hotkey_active(),
        autostart: crate::autostart::is_enabled(),
        portable: state.paths.portable,
        data_dir: state.paths.data_dir.display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 设置开机自启，返回生效后的状态（以注册表实际读到的为准，不假设写入一定成功）
#[tauri::command]
pub fn set_autostart(enabled: bool) -> AppResult<bool> {
    crate::autostart::set(enabled)?;
    Ok(crate::autostart::is_enabled())
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
