//! Win32 直调层 —— 本程序与操作系统之间唯一的边界。
//!
//! 为什么是裸 FFI：本机 cargo 无法联网，只能用离线缓存里已有的 crate。
//! `tauri-plugin-global-shortcut` / `global-hotkey` 都不在缓存中，因此窗口枚举、
//! 置顶、热键全部走 windows-sys 0.61 的原始声明。
//!
//! ★ 类型注意：windows-sys 里 `HWND` 是裸指针（`*mut c_void`），而裸指针不是
//! `Send`，不能直接放进跨线程的集合里。所以对外一律用 `HwndRaw = isize` 表示
//! 句柄，只在真正调用 API 的那一刻转回 `HWND`。

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, HWND, LPARAM, POINT, ERROR_ACCESS_DENIED,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, SetWindowPos, WindowFromPoint, GA_ROOT, GWL_EXSTYLE,
    HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, WS_EX_TOPMOST,
};

use crate::error::AppError;

/// 跨线程安全的窗口句柄表示
pub type HwndRaw = isize;

/// 把 Rust 字符串转成 Win32 需要的 UTF-16 宽字符（带结尾 NUL）
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 把路径转成 UTF-16。走 OsStr 而不是 to_string_lossy，
/// 避免非 UTF-8 路径被替换字符污染后打不开。
pub fn path_to_wide(p: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[inline]
fn to_hwnd(h: HwndRaw) -> HWND {
    h as HWND
}

/// 被忽略的窗口类名。
///
/// 这些是系统外壳的实现细节（桌面、任务栏、悬停预览），用户不把它们当「窗口」，
/// 列出来只会变成噪音。
const IGNORED_CLASSES: &[&str] = &[
    "Progman",                    // 桌面
    "WorkerW",                    // 桌面壁纸层
    "Shell_TrayWnd",              // 任务栏
    "Shell_SecondaryTrayWnd",     // 副屏任务栏
    "Windows.UI.Core.CoreWindow", // UWP 无边框外壳
    "ForegroundStaging",          // 前台切换暂存窗口
    "SysShadow",                  // 系统阴影
    "TaskListThumbnailWnd",       // 任务栏悬停预览
    "XamlExplorerHostIslandWindow", // Win11 桌面/任务视图外壳
];

fn is_ignored_class(class: &str) -> bool {
    IGNORED_CLASSES.iter().any(|c| c.eq_ignore_ascii_case(class))
}

/// 读窗口标题。
///
/// 注意：对**其他进程**的窗口，GetWindowTextW 读的是窗口对象里缓存的标题，
/// 不会发送 WM_GETTEXT 过去，所以不会因为目标进程卡死而把本程序拖住。
fn window_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        // ★ 必须夹紧再切片。理论返回值不会超过缓冲区，但这个函数是从
        // `extern "system"` 的枚举回调里调的 —— 越界切片会 panic，
        // 而 panic 穿过 FFI 边界是**未定义行为**，不是「崩一下就好」。
        // 一行 min 换掉一个 UB 入口，很划算。
        debug_slice(&buf, n)
    }
}

fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut buf = vec![0u16; 256];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        debug_slice(&buf, n)
    }
}

/// 按 API 返回的长度取有效部分，并夹紧到缓冲区范围内
#[inline]
fn debug_slice(buf: &[u16], n: i32) -> String {
    let len = (n.max(0) as usize).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

pub fn title_of(hwnd: HwndRaw) -> String {
    window_text(to_hwnd(hwnd))
}

pub fn class_of(hwnd: HwndRaw) -> String {
    class_name(to_hwnd(hwnd))
}

pub fn pid_of(hwnd: HwndRaw) -> u32 {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(to_hwnd(hwnd), &mut pid);
        pid
    }
}

pub fn is_alive(hwnd: HwndRaw) -> bool {
    hwnd != 0 && unsafe { IsWindow(to_hwnd(hwnd)) != 0 }
}

pub fn is_own_window(hwnd: HwndRaw) -> bool {
    pid_of(hwnd) == unsafe { GetCurrentProcessId() }
}

/// 是否是用户认知里「可以被置顶」的窗口：活着、不是自己、不是外壳
pub fn is_candidate(hwnd: HwndRaw) -> bool {
    is_alive(hwnd) && !is_own_window(hwnd) && !is_ignored_class(&class_of(hwnd))
}

const UNKNOWN_PROCESS: &str = "未知进程";

/// 进程可执行文件名，如 `chrome.exe`。
///
/// 用 QUERY_LIMITED_INFORMATION 而非 QUERY_INFORMATION：前者对普通权限进程
/// 就能打开同权限级别的绝大多数进程，也不会被拒绝。
pub fn process_name(pid: u32) -> String {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return UNKNOWN_PROCESS.to_string();
        }
        // 260（MAX_PATH）不够：长路径（`\\?\` 前缀、深层 node_modules、
        // 某些安装器的临时目录）会让 QueryFullProcessImageNameW 直接失败，
        // 列表里就出现「未知进程」——而且看不出是长度问题。
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(h);
        if ok == 0 || size == 0 {
            return UNKNOWN_PROCESS.to_string();
        }
        let full = debug_slice(&buf, size as i32);
        full.rsplit('\\')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(UNKNOWN_PROCESS)
            .to_string()
    }
}

/// 一次枚举得到的原始窗口记录（进程名留给调用方按需补齐，避免重复 OpenProcess）
pub struct RawWindow {
    pub hwnd: HwndRaw,
    pub title: String,
    pub class_name: String,
    pub pid: u32,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    // lparam 是调用方传进来的 &mut Vec<RawWindow>
    let out = &mut *(lparam as *mut Vec<RawWindow>);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let class = class_name(hwnd);
    if is_ignored_class(&class) {
        return 1;
    }
    // 标题为空的窗口绝大多数是隐藏的辅助窗口（输入法、拖放、消息泵），
    // 列出来只会淹没真正想要的窗口。
    let title = window_text(hwnd);
    if title.trim().is_empty() {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == GetCurrentProcessId() {
        return 1;
    }

    out.push(RawWindow {
        hwnd: hwnd as HwndRaw,
        title,
        class_name: class,
        pid,
    });
    1 // 继续枚举
}

/// 枚举所有可见的、有标题的顶层窗口，已剔除本程序与系统外壳
pub fn enum_top_level_windows() -> Vec<RawWindow> {
    let mut out: Vec<RawWindow> = Vec::with_capacity(64);
    unsafe {
        EnumWindows(Some(enum_proc), &mut out as *mut _ as LPARAM);
    }
    out
}

/// 光标位置（屏幕坐标）。仅用于日志与诊断。
pub fn cursor_pos() -> Option<(i32, i32)> {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            None
        } else {
            Some((pt.x, pt.y))
        }
    }
}

/// 光标正下方的顶层窗口。
///
/// `WindowFromPoint` 返回的是最深的子窗口（可能是某个按钮），
/// 所以要用 `GetAncestor(GA_ROOT)` 提到顶层。
pub fn window_under_cursor() -> Option<HwndRaw> {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let h = WindowFromPoint(pt);
        if h.is_null() {
            return None;
        }
        let root = GetAncestor(h, GA_ROOT);
        let root = if root.is_null() { h } else { root };
        Some(root as HwndRaw)
    }
}

pub fn foreground_window() -> Option<HwndRaw> {
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            None
        } else {
            Some(h as HwndRaw)
        }
    }
}

/// 读 WS_EX_TOPMOST 位。
///
/// 只用于「重断言」时判断是否需要补一次，**绝不用于决定置顶还是取消置顶** ——
/// 那个判断依据是 pin 集合，原因见 pin.rs 顶部注释。
pub fn has_topmost_style(hwnd: HwndRaw) -> bool {
    unsafe { (GetWindowLongPtrW(to_hwnd(hwnd), GWL_EXSTYLE) & (WS_EX_TOPMOST as isize)) != 0 }
}

/// 置顶 / 取消置顶。
///
/// ★ 判定成败只看返回值，**不要**用「errno 是否非零」来判断：
/// Win32 的约定是 BOOL 为 0 才算失败，而成功的调用完全可能在 last-error 里
/// 留下一个陈旧值。反过来同样成立 —— 调用前必须 `SetLastError(0)` 清零，
/// 否则一次「失败但没写错误码」的调用会让我们读到**上一个 API 留下的**错误码，
/// 从而把一次普通失败误报成「管理员窗口」。这个坑本项目踩过。
pub fn set_topmost(hwnd: HwndRaw, on: bool) -> Result<(), AppError> {
    unsafe {
        let insert_after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        SetLastError(0);
        let ok = SetWindowPos(
            to_hwnd(hwnd),
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
        if ok == 0 {
            let code = GetLastError();
            crate::log::write(
                "win32",
                format!(
                    "SetWindowPos 失败 hwnd=0x{hwnd:X} on={on} alive={} err={code}",
                    is_alive(hwnd)
                ),
            );
            return Err(if code == ERROR_ACCESS_DENIED {
                AppError::AccessDenied
            } else {
                AppError::Win32(code)
            });
        }
        Ok(())
    }
}
