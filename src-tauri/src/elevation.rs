//! 管理员权限：判断当前是否提权，以及**在运行时以另一种权限重启自己**。
//!
//! 为什么需要「重启自己」：进程一旦启动，它的令牌就固定了，无法在运行中提权或降权。
//! 所以「是否以管理员运行」这个开关只能靠重启进程来落实。
//!
//! 两个方向的做法不同：
//!   · 提权：`ShellExecuteW(..., "runas", ...)` —— 由系统弹出 UAC，用户同意后
//!     以高完整性令牌启动新实例。
//!   · 降权：**没有对应的 API**。这里走一个通行做法：让 `explorer.exe` 去启动它。
//!     explorer 运行在标准用户的中等完整性级别上，它创建的子进程自然也是中等完整性。
//!
//! 重启的握手靠 `data\.relaunching` 标记文件（见 lib.rs），而不是命令行参数 ——
//! 因为经 explorer 启动时无法可靠地传递参数，而环境变量在跨提权边界时也不保证保留。

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

use crate::error::{AppError, AppResult};
use crate::winapi::{path_to_wide, to_wide};

/// ShellExecuteW 的返回值小于等于 32 表示失败（这是它的历史约定）
const SHELL_EXEC_SUCCESS_THRESHOLD: isize = 32;
const SW_SHOWNORMAL: i32 = 1;

/// 当前进程是否以管理员（高完整性）身份运行
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// 把 ShellExecuteW 的返回值翻译成人话
fn shell_result(r: isize, action: &str) -> AppResult<()> {
    if r > SHELL_EXEC_SUCCESS_THRESHOLD {
        return Ok(());
    }
    let hint = match r {
        5 => "（UAC 被拒绝或取消）",
        2 => "（找不到可执行文件）",
        31 => "（没有关联的应用程序）",
        _ => "",
    };
    Err(AppError::msg(format!(
        "{action}失败，ShellExecute 返回 {r}{hint}"
    )))
}

/// 以管理员身份重启自己。
///
/// ★ 这里**没有**「降权重启」那条路径了。曾经有过 —— 设置页有个权限开关，
/// 可以切回普通权限（走 explorer.exe，因为它运行在中完整性级别，子进程自然也是）。
/// 后来权限改成固定管理员运行、开关删掉，那条分支就成了永远走不到的死代码。
/// 死代码最坏的地方不是浪费，而是**让人以为那个能力还在**，所以一并删掉。
///
/// 调用方应当在成功后立即退出进程：单实例互斥体要尽早释放，新实例才能接手。
pub fn relaunch_elevated() -> AppResult<()> {
    let exe = std::env::current_exe()?;
    let exe_w = path_to_wide(&exe);
    let verb = to_wide("runas");
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            exe_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    shell_result(r, "以管理员身份重启")
}

/// 重启握手用的标记文件路径。
///
/// 它同时承担两个职责：
///   1. 让新实例知道「我是一次权限切换的结果」，从而可以等待旧实例释放互斥体；
///   2. 让新实例知道「不要再切换一次」，否则设置与实际不一致时会来回重启。
pub fn marker_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join(".relaunching")
}
