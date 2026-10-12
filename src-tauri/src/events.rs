//! 前端事件：状态变化与错误提示的唯一下行通道。
//!
//! 事件名沿用「域://动作」的项目约定（与 menstruation 的 tray:// 一致）。

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/** 置顶集合发生变化 */
pub const PIN_CHANGED: &str = "pin://changed";
/** 一次操作失败，前端弹错误条 */
pub const PIN_ERROR: &str = "pin://error";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinChangedPayload {
    pub pinned: Vec<isize>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinErrorPayload {
    pub message: String,
}

/// 置顶集合变了 —— 前端据此重新取快照。
pub fn emit_changed(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let payload = PinChangedPayload {
            pinned: state.pins.ids(),
        };
        let _ = app.emit(PIN_CHANGED, payload);
    }
}

/// 操作失败。热键线程与托盘菜单都走这里，保证界面一定能看到原因。
pub fn emit_error(app: &AppHandle, message: impl Into<String>) {
    let _ = app.emit(
        PIN_ERROR,
        PinErrorPayload {
            message: message.into(),
        },
    );
}
