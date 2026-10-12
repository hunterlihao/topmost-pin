//! 错误类型。所有面向用户的文案集中在这里，命令层只做转发。

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// SetWindowPos 返回 ERROR_ACCESS_DENIED(5) —— 目标是提权进程的窗口。
    /// 非提权进程无法操作更高完整性级别的窗口，这是 Windows 的 UIPI 限制。
    #[error("无法置顶：目标窗口以管理员身份运行。以管理员身份重启本程序后即可置顶。")]
    AccessDenied,

    #[error("这个窗口已经关闭了。")]
    WindowGone,

    #[error("没有找到目标窗口。把光标移到要置顶的窗口上再按一次。")]
    NoTarget,

    #[error("置顶失败（系统错误码 {0}）。")]
    Win32(u32),

    #[error("{0}")]
    Msg(String),
}

impl AppError {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Msg(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self::Msg(e.to_string())
    }
}

/// Tauri 命令的 Err 分支要求可序列化，这里序列化为纯字符串，
/// 前端拿到即可直接展示。
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
